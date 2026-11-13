//! Reconcile immutable scan results with the registry. Filesystem walking and
//! native event collection live in the cancellable service below, outside IPC locks.
use super::*;
use devforge_projects::discovery::{Entry, Scan, identity};

#[derive(Clone, Default, Serialize)]
pub struct View {
    pub root: Option<PathBuf>,
    pub state: String,
    pub problem: Option<String>,
    pub skipped: usize,
    pub last_scan: u64,
    pub entries: Vec<Entry>,
}
impl Workspace {
    pub fn discovery_status(&mut self, root: Option<PathBuf>, scanning: bool, degraded: bool) {
        self.discovery.root = root;
        if scanning {
            self.discovery.state = "scanning".into();
        } else if degraded {
            self.discovery.state = "degraded".into();
        }
    }
    pub fn reconcile_discovery(&mut self, mut scan: Scan) -> Result<()> {
        self.discovery.root = Some(scan.root.clone());
        self.discovery.state = if scan.complete { "ready" } else { "degraded" }.into();
        self.discovery.problem = scan.problem.clone();
        self.discovery.skipped = scan.skipped;
        self.discovery.last_scan = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let mut entries = self
            .projects
            .discovery_entries()?
            .into_iter()
            .map(|e| (e.identity.clone(), e))
            .collect::<BTreeMap<_, _>>();
        let mut projects = self.projects.list()?;
        let mut seen = std::collections::HashSet::new();
        for candidate in scan.candidates {
            if identity(&candidate.path).ok().as_deref() != Some(&candidate.identity) {
                scan.complete = false;
                self.discovery.state = "degraded".into();
                self.discovery.problem = Some("partial".into());
                continue;
            }
            seen.insert(candidate.identity.clone());
            let mut entry = entries
                .remove(&candidate.identity)
                .unwrap_or_else(|| Entry {
                    identity: candidate.identity.clone(),
                    root: scan.root.clone(),
                    path: candidate.path.clone(),
                    name: candidate.name.clone(),
                    project_id: None,
                    state: "detected".into(),
                    problem: None,
                    ignored: false,
                });
            entry.root = scan.root.clone();
            entry.path = candidate.path.clone();
            entry.name = candidate.name.clone();
            entry.problem = candidate.problem.clone();
            if entry.ignored {
                entry.state = "ignored".into();
                self.projects.save_discovery(&entry)?;
                continue;
            }
            if projects.iter().any(|p| {
                p.path == candidate.path
                    && p.filesystem_identity
                        .as_ref()
                        .is_some_and(|id| id != &candidate.identity)
                    || p.path == candidate.path
                        && entries.values().any(|e| {
                            e.project_id.as_deref() == Some(&p.id)
                                && e.identity != candidate.identity
                        })
            }) {
                entry.project_id = None;
                entry.state = "conflict".into();
                entry.problem = Some("identity_changed".into());
                self.projects.save_discovery(&entry)?;
                continue;
            }
            let existing = entry
                .project_id
                .as_deref()
                .and_then(|id| projects.iter().find(|p| p.id == id))
                .or_else(|| {
                    projects.iter().find(|p| {
                        p.path == candidate.path
                            || identity(&p.path).ok().as_deref() == Some(&candidate.identity)
                    })
                })
                .cloned();
            if let Some(mut project) = existing {
                entry.project_id = Some(project.id.clone());
                if project.path != candidate.path {
                    // Do not rebind live processes or managed branch worktrees.
                    if self.sessions.contains_key(&project.id)
                        || project.worktree.is_some()
                        || candidate.identity.starts_with("path:")
                    {
                        entry.state = "relocation_pending".into();
                        self.projects.save_discovery(&entry)?;
                        continue;
                    }
                    match self
                        .sites
                        .project_mutation_guard(&project.id)
                        .and_then(|_guard| {
                            self.projects
                                .relocate(&project.id, project.revision, &candidate.path)
                                .map_err(|_| devforge_serving::ServingError::ProjectUnavailable)
                        }) {
                        Ok(p) => project = p,
                        Err(_) => {
                            entry.state = "relocation_pending".into();
                            self.projects.save_discovery(&entry)?;
                            continue;
                        }
                    }
                }
                entry.state = if entry.problem.is_some() {
                    "invalid"
                } else {
                    "ready"
                }
                .into();
                // Recover a project registration whose site write was interrupted.
                if !self
                    .sites
                    .sites()?
                    .iter()
                    .any(|s| s.project_id == project.id)
                {
                    match self.configure_discovered_site(&project, &candidate.name) {
                        Ok(()) => {}
                        Err(e) => {
                            entry.state = "conflict".into();
                            entry.problem = Some(e.code);
                        }
                    }
                }
            } else if entry.problem.is_some() {
                entry.state = "invalid".into();
                entry.project_id = None;
            } else if let Some((php, node)) = self.discovery_runtimes(&candidate.path) {
                match self.projects.import(
                    &candidate.path,
                    &candidate.name,
                    &php,
                    &node,
                    &self.catalog,
                ) {
                    Ok(p) => {
                        entry.project_id = Some(p.id.clone());
                        entry.state = "ready".into();
                        if let Err(e) = self.configure_discovered_site(&p, &candidate.name) {
                            entry.state = "conflict".into();
                            entry.problem = Some(e.code);
                        }
                        projects.push(p);
                    }
                    Err(e) => {
                        entry.state = "conflict".into();
                        entry.problem = Some(e.to_string());
                    }
                }
            } else {
                entry.state = "needs_runtime".into();
                entry.project_id = None;
            }
            self.projects.save_discovery(&entry)?;
        }
        for (_, mut entry) in entries {
            if entry.root != scan.root {
                entry.state = "detached".into();
            } else if scan.complete && !seen.contains(&entry.identity) && !entry.ignored {
                entry.state = "missing".into();
            }
            self.projects.save_discovery(&entry)?;
        }
        Ok(())
    }
    fn discovery_runtimes(&self, path: &Path) -> Option<(String, String)> {
        let checked = inspect(path).ok()?;
        let mut packages = self
            .runtime_cache
            .iter()
            .filter(|r| r.installed)
            .collect::<Vec<_>>();
        // Reviewed catalog defaults sort first; stable IDs break ties.
        packages.sort_by_key(|r| {
            (
                !self.catalog.defaults.values().any(|v| v == &r.id),
                r.id.clone(),
            )
        });
        for php in packages.iter().filter(|r| r.kind == "php") {
            let p = self.catalog.get(&php.id).ok()?;
            if !p.executables.contains_key("php") {
                continue;
            }
            let Some(server) = self.catalog.php_server(p).ok() else {
                continue;
            };
            if !packages
                .iter()
                .any(|r| self.catalog.get(&r.id).is_ok_and(|p| p == server))
            {
                continue;
            }
            for node in packages.iter().filter(|r| r.kind == "node") {
                if compatible(&checked, p, self.catalog.get(&node.id).ok()?).is_ok() {
                    return Some((php.id.clone(), node.id.clone()));
                }
            }
        }
        None
    }
    fn configure_discovered_site(&self, p: &Project, name: &str) -> Result<()> {
        let mut slug = name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect::<String>();
        slug = slug.trim_matches('-').chars().take(40).collect();
        if slug.is_empty() {
            slug = format!("project-{}", &p.id[..8]);
        }
        self.sites.configure(
            &self.projects,
            Site {
                project_id: p.id.clone(),
                domain: format!("{slug}.devforge.test"),
                https: false,
                vite: false,
            },
        )?;
        Ok(())
    }
}

use notify::{RecursiveMode, Watcher};
use std::sync::{
    Mutex as StdMutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc::{SyncSender, sync_channel},
};

#[derive(Clone, Default)]
pub struct ServiceStatus {
    pub root: Option<PathBuf>,
    pub generation: u64,
    pub scanning: bool,
    pub degraded: bool,
}
struct Shared {
    status: ServiceStatus,
    cancel: Cancellation,
    ready: Option<(u64, Scan)>,
}
/// One worker, one coalesced wake slot, one result slot. Drop cancels and joins.
/// The callback never scans, takes a workspace lock, or retains file events.
pub struct Service {
    shared: Arc<StdMutex<Shared>>,
    wake: SyncSender<()>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
    events: Arc<AtomicU64>,
}
impl Default for Service {
    fn default() -> Self {
        Self::new()
    }
}
impl Service {
    pub fn new() -> Self {
        Self::start(true)
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn polling_fixture() -> Self {
        Self::start(false)
    }
    fn start(native_enabled: bool) -> Self {
        let shared = Arc::new(StdMutex::new(Shared {
            status: ServiceStatus::default(),
            cancel: Cancellation::default(),
            ready: None,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (wake, rx) = sync_channel(1);
        let (shared_worker, stop_worker, waker) = (shared.clone(), stop.clone(), wake.clone());
        let events = Arc::new(AtomicU64::new(0));
        let event_counter = events.clone();
        let worker = std::thread::spawn(move || {
            let mut watched_generation = u64::MAX;
            let mut watcher: Option<notify::RecommendedWatcher> = None;
            let mut watched_paths = std::collections::HashSet::<PathBuf>::new();
            let backend_error = Arc::new(AtomicBool::new(false));
            let mut last_scan = Instant::now() - Duration::from_secs(30);
            loop {
                let _ =
                    rx.recv_timeout(Duration::from_secs(30).saturating_sub(last_scan.elapsed()));
                if stop_worker.load(Ordering::Relaxed) {
                    break;
                }
                // A bounded quiet debounce also bounds continuous event storms.
                let started = Instant::now();
                loop {
                    if started.elapsed() >= Duration::from_secs(2) {
                        break;
                    }
                    if rx.recv_timeout(Duration::from_millis(500)).is_err() {
                        break;
                    }
                    if stop_worker.load(Ordering::Relaxed) {
                        return;
                    }
                }
                let (status, cancel) = {
                    let s = shared_worker.lock().unwrap();
                    (s.status.clone(), s.cancel.clone())
                };
                let Some(root) = status.root.clone() else {
                    last_scan = Instant::now();
                    continue;
                };
                if watched_generation != status.generation {
                    watcher.take();
                    watched_paths.clear();
                    watched_generation = status.generation;
                }
                {
                    shared_worker.lock().unwrap().status.scanning = true;
                }
                let mut scan = devforge_projects::discovery::scan(&root, &cancel);
                if stop_worker.load(Ordering::Relaxed) {
                    break;
                }
                // Preserve the backend across scans. Replacing FSEvents streams
                // here creates an observation gap during incomplete directory copies.
                if backend_error.swap(false, Ordering::Relaxed) {
                    watcher.take();
                    watched_paths.clear();
                }
                if watcher.is_none() && native_enabled {
                    let event_wake = waker.clone();
                    let count = event_counter.clone();
                    let event_root = root.clone();
                    let errors = backend_error.clone();
                    watcher = notify::recommended_watcher(
                        move |result: notify::Result<notify::Event>| {
                            if result.is_err() {
                                errors.store(true, Ordering::Relaxed);
                            }
                            if result.as_ref().is_ok_and(|e| {
                                !e.paths.iter().any(|path| {
                                    let path = fs::canonicalize(path).unwrap_or_else(|_| {
                                        path.parent()
                                            .and_then(|p| fs::canonicalize(p).ok())
                                            .map(|p| p.join(path.file_name().unwrap_or_default()))
                                            .unwrap_or_else(|| path.clone())
                                    });
                                    path.starts_with(&event_root) || event_root.starts_with(&path)
                                })
                            }) {
                                return;
                            }
                            count.fetch_add(1, Ordering::Relaxed);
                            let _ = event_wake.try_send(());
                        },
                    )
                    .ok();
                }
                let mut desired = scan
                    .watch_paths
                    .iter()
                    .cloned()
                    .collect::<std::collections::HashSet<_>>();
                if root.is_dir() {
                    desired.insert(root.clone());
                }
                if let Some(parent) = root.parent() {
                    desired.insert(parent.to_path_buf());
                }
                for path in &scan.watch_paths {
                    if path.join("public").is_dir() {
                        desired.insert(path.join("public"));
                    }
                }
                let mut degraded = false;
                let topology_changed = desired != watched_paths;
                if let Some(watcher) = watcher.as_mut() {
                    for old in watched_paths.difference(&desired) {
                        let _ = watcher.unwatch(old);
                    }
                    watched_paths.retain(|p| desired.contains(p));
                    for path in desired
                        .difference(&watched_paths)
                        .cloned()
                        .collect::<Vec<_>>()
                    {
                        if watcher.watch(&path, RecursiveMode::NonRecursive).is_ok() {
                            watched_paths.insert(path);
                        } else {
                            degraded = true;
                        }
                    }
                } else {
                    degraded = true;
                }
                if degraded && scan.problem.is_none() {
                    scan.problem = Some("watch_unavailable".into());
                }
                let mut s = shared_worker.lock().unwrap();
                if s.status.generation == status.generation && !cancel.is_cancelled() {
                    s.status.scanning = false;
                    s.status.degraded = degraded;
                    s.ready = Some((status.generation, scan));
                }
                last_scan = Instant::now();
                if topology_changed && !degraded {
                    let _ = waker.try_send(());
                }
            }
        });
        Self {
            shared,
            wake,
            stop,
            worker: Some(worker),
            events,
        }
    }
    pub fn set_root(&self, root: PathBuf) {
        let mut s = self.shared.lock().unwrap();
        if s.status.root.as_ref() == Some(&root) {
            return;
        }
        s.cancel.cancel();
        s.cancel = Cancellation::default();
        s.status.generation = s.status.generation.wrapping_add(1);
        s.status.root = Some(root);
        s.status.scanning = true;
        s.ready = None;
        drop(s);
        self.request();
    }
    pub fn request(&self) {
        let _ = self.wake.try_send(());
    }
    pub fn status(&self) -> ServiceStatus {
        self.shared.lock().unwrap().status.clone()
    }
    pub fn take(&self) -> Option<Scan> {
        let mut s = self.shared.lock().unwrap();
        let (generation, scan) = s.ready.take()?;
        (generation == s.status.generation).then_some(scan)
    }
    pub fn event_count(&self) -> u64 {
        self.events.load(Ordering::Relaxed)
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.shared.lock().unwrap().cancel.cancel();
        self.request();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct NoSecrets;
    impl devforge_core::SecretStore for NoSecrets {
        fn get(&self, _: &str) -> std::result::Result<Option<String>, devforge_core::ErrorCode> {
            panic!("discovery accessed secrets")
        }
        fn set(&self, _: &str, _: &str) -> std::result::Result<(), devforge_core::ErrorCode> {
            panic!("discovery wrote secrets")
        }
        fn delete(&self, _: &str) -> std::result::Result<(), devforge_core::ErrorCode> {
            panic!("discovery deleted secrets")
        }
    }
    fn fixture(path: &Path) {
        fs::create_dir_all(path.join("public")).unwrap();
        fs::write(path.join("artisan"), "<?php").unwrap();
        fs::write(path.join("public/index.php"), "<?php").unwrap();
        fs::write(
            path.join("composer.json"),
            r#"{"require":{"laravel/framework":"^13","php":"^8.4"}}"#,
        )
        .unwrap();
    }
    fn scan_into(w: &mut Workspace, root: &Path) {
        w.reconcile_discovery(devforge_projects::discovery::scan(
            root,
            &Cancellation::default(),
        ))
        .unwrap();
    }
    #[test]
    fn no_runtime_candidates_missing_partial_and_detached_preserve_registry() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        fs::create_dir(&root).unwrap();
        fixture(&root.join("app"));
        let mut w = Workspace::open(temp.path().join("data"), Arc::new(NoSecrets)).unwrap();
        scan_into(&mut w, &root);
        assert!(w.projects.list().unwrap().is_empty());
        assert_eq!(
            w.snapshot().unwrap().discovery.entries[0].state,
            "needs_runtime"
        );
        let canonical = fs::canonicalize(&root).unwrap();
        w.reconcile_discovery(Scan {
            root: canonical.clone(),
            complete: false,
            problem: Some("partial".into()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            w.snapshot().unwrap().discovery.entries[0].state,
            "needs_runtime"
        );
        fs::remove_dir_all(root.join("app")).unwrap();
        scan_into(&mut w, &root);
        assert_eq!(w.snapshot().unwrap().discovery.entries[0].state, "missing");
        let other = temp.path().join("other");
        fs::create_dir(&other).unwrap();
        scan_into(&mut w, &other);
        assert_eq!(w.snapshot().unwrap().discovery.entries[0].state, "detached");
    }
    #[test]
    fn registration_rename_forget_restart_and_replacement_are_conservative() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root");
        fs::create_dir(&root).unwrap();
        fixture(&root.join("app"));
        let data = temp.path().join("data");
        let mut w = Workspace::open(&data, Arc::new(NoSecrets)).unwrap();
        // Unit fixture marks catalog runtimes available; actual executable/HTTP is
        // verified separately by native acceptance using real package receipts.
        for runtime in &mut w.runtime_cache {
            runtime.installed = true;
        }
        scan_into(&mut w, &root);
        let p = w.projects.list().unwrap().pop().unwrap();
        scan_into(&mut w, &root);
        assert_eq!(w.projects.list().unwrap().len(), 1);
        let domain = w.sites.sites().unwrap()[0].domain.clone();
        fs::rename(root.join("app"), root.join("renamed")).unwrap();
        scan_into(&mut w, &root);
        let renamed = w.projects.get(&p.id).unwrap();
        assert_eq!(
            renamed.path,
            fs::canonicalize(root.join("renamed")).unwrap()
        );
        assert_eq!(domain, w.sites.sites().unwrap()[0].domain);
        w.execute(
            Command::Forget {
                id: p.id.clone(),
                revision: renamed.revision,
            },
            &Cancellation::default(),
        )
        .unwrap();
        scan_into(&mut w, &root);
        assert!(w.projects.list().unwrap().is_empty());
        drop(w);
        let mut w = Workspace::open(&data, Arc::new(NoSecrets)).unwrap();
        scan_into(&mut w, &root);
        let e = w.projects.discovery_entries().unwrap().pop().unwrap();
        assert!(e.ignored);
        assert!(w.projects.list().unwrap().is_empty());
        w.projects.restore_discovery(&e.identity).unwrap();
        for r in &mut w.runtime_cache {
            r.installed = true;
        }
        scan_into(&mut w, &root);
        let restored = w.projects.list().unwrap().pop().unwrap();
        fs::rename(root.join("renamed"), temp.path().join("old-code")).unwrap();
        fixture(&root.join("renamed"));
        scan_into(&mut w, &root);
        assert_eq!(w.projects.list().unwrap().len(), 1);
        assert!(
            w.projects
                .discovery_entries()
                .unwrap()
                .iter()
                .any(|e| e.problem.as_deref() == Some("identity_changed"))
        );
        assert!(w.start(&restored.id).is_err());
    }
    #[test]
    fn native_watcher_converges_after_incomplete_copy_root_switch_and_quit() {
        let temp = tempfile::tempdir().unwrap();
        let a = temp.path().join("a");
        let b = temp.path().join("b");
        fs::create_dir(&a).unwrap();
        fs::create_dir(&b).unwrap();
        let service = Service::new();
        service.set_root(fs::canonicalize(&a).unwrap());
        let wait = |root: &Path, count: usize| {
            let start = Instant::now();
            let mut observed = String::from("no scan result");
            loop {
                if let Some(s) = service.take() {
                    observed = format!(
                        "root={:?} candidates={:?} complete={} problem={:?}",
                        s.root,
                        s.candidates.iter().map(|c| &c.name).collect::<Vec<_>>(),
                        s.complete,
                        s.problem
                    );
                    if s.root == root && s.candidates.len() == count {
                        return s;
                    }
                }
                assert!(
                    start.elapsed() < Duration::from_secs(8),
                    "watcher convergence timeout root={root:?} expected={count} events={} scanning={} degraded={} observed={observed}",
                    service.event_count(),
                    service.status().scanning,
                    service.status().degraded
                );
                std::thread::sleep(Duration::from_millis(30));
            }
        };
        let canonical = fs::canonicalize(&a).unwrap();
        wait(&canonical, 0);
        fs::create_dir(a.join("app")).unwrap();
        std::thread::sleep(Duration::from_millis(700));
        fixture(&a.join("app"));
        wait(&canonical, 1);
        assert!(service.event_count() > 0);
        let other = fs::canonicalize(&b).unwrap();
        service.set_root(other.clone());
        wait(&other, 0);
        fixture(&b.join("new"));
        wait(&other, 1);
        let now = Instant::now();
        drop(service);
        assert!(now.elapsed() < Duration::from_secs(3));
    }
}
