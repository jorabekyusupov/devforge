//! Shared daily-workspace orchestration. No Tauri or UI types, no credentials in DTOs.
use devforge_core::SecretStore;
use devforge_projects::{
    database::{Binding, Instance, RunningPostgres},
    execution::{Context, Invocation, Tool},
    project::{Catalog, Project, ProjectStore, compatible, inspect},
    terminal,
};
use devforge_runtime::{
    Cancellation,
    package::{Limits, PackageStore, Target},
    process::{Observed, Readiness, Supervisor},
};
use devforge_serving::{
    manager::{Options, Serving, SiteStatus},
    site::{Site, SiteStore},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};
#[derive(Debug, Clone, Serialize)]
pub struct Error {
    pub scope: &'static str,
    pub code: String,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.scope, self.code)
    }
}
impl std::error::Error for Error {}
impl From<devforge_projects::ProjectError> for Error {
    fn from(e: devforge_projects::ProjectError) -> Self {
        Self {
            scope: "project",
            code: e.to_string(),
        }
    }
}
impl From<devforge_serving::ServingError> for Error {
    fn from(e: devforge_serving::ServingError) -> Self {
        Self {
            scope: "serving",
            code: e.to_string(),
        }
    }
}
impl From<devforge_runtime::RuntimeError> for Error {
    fn from(e: devforge_runtime::RuntimeError) -> Self {
        Self {
            scope: "runtime",
            code: e.to_string(),
        }
    }
}
impl From<std::io::Error> for Error {
    fn from(_: std::io::Error) -> Self {
        error("storage_unavailable")
    }
}
fn error(code: &str) -> Error {
    Error {
        scope: "workspace",
        code: code.into(),
    }
}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Development {
    pub queue: bool,
}
#[derive(Serialize, Deserialize, Default)]
struct Preferences {
    schema: u32,
    development: BTreeMap<String, Development>,
}
#[derive(Clone, Serialize)]
pub struct Runtime {
    pub problem: Option<String>,
    pub id: String,
    pub kind: String,
    pub version: String,
    pub variant: String,
    pub installed: bool,
}
#[derive(Clone, Serialize)]
pub struct Row {
    pub processes: Vec<(String, devforge_runtime::process::Snapshot)>,
    pub project: Project,
    pub site: Option<Site>,
    pub development: Development,
    pub state: String,
    pub serving: Option<SiteStatus>,
    pub queue_state: Option<String>,
    pub database: Option<Binding>,
}
#[derive(Clone, Serialize)]
pub struct Database {
    pub instance: Instance,
    pub state: String,
    pub users: usize,
}
#[derive(Clone, Serialize)]
pub struct Log {
    pub project_id: Option<String>,
    pub process: String,
    pub stream: String,
    pub text: String,
}
#[derive(Clone, Serialize, Default)]
pub struct Snapshot {
    pub projects: Vec<Row>,
    pub runtimes: Vec<Runtime>,
    pub databases: Vec<Database>,
    pub logs: Vec<Log>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    RefreshRuntimes,
    Import {
        path: PathBuf,
        name: String,
        php: String,
        node: String,
        domain: String,
        https: bool,
        vite: bool,
        queue: bool,
    },
    Configure {
        id: String,
        revision: u64,
        name: String,
        php: String,
        node: String,
        domain: String,
        https: bool,
        vite: bool,
        queue: bool,
    },
    Forget {
        id: String,
        revision: u64,
    },
    Start {
        id: String,
    },
    Stop {
        id: String,
    },
    Restart {
        id: String,
    },
    Install {
        package: String,
    },
    RemoveRuntime {
        package: String,
    },
    CreateDatabase {
        package: String,
    },
    BindDatabase {
        id: String,
        instance: String,
    },
    TestDatabase {
        id: String,
    },
    StopDatabase {
        instance: String,
    },
}
struct Session {
    web: Serving,
    queue: Option<Invocation>,
    supervisor: Supervisor,
}
impl Session {
    fn stop(&mut self) -> Result<()> {
        if let Some(q) = self.queue.take() {
            self.supervisor.stop(q.id())?;
        }
        self.web.stop()?;
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
pub struct Workspace {
    root: PathBuf,
    pub projects: ProjectStore,
    pub sites: SiteStore,
    pub packages: PackageStore,
    pub catalog: Catalog,
    secrets: Arc<dyn SecretStore + Send + Sync>,
    sessions: BTreeMap<String, Session>,
    databases: BTreeMap<String, RunningPostgres>,
    preferences: Preferences,
    runtime_cache: Vec<Runtime>,
    history: Vec<Log>,
}
impl Workspace {
    pub fn open(
        root: impl AsRef<Path>,
        secrets: Arc<dyn SecretStore + Send + Sync>,
    ) -> Result<Self> {
        fs::create_dir_all(root.as_ref())?;
        let root = fs::canonicalize(root)?;
        let pref = root.join("workspace.json");
        let preferences = if pref.exists() {
            let p: Preferences = serde_json::from_slice(&fs::read(&pref)?)
                .map_err(|_| error("storage_unavailable"))?;
            if p.schema != 1 {
                return Err(error("storage_unavailable"));
            }
            p
        } else {
            Preferences {
                schema: 1,
                ..Default::default()
            }
        };
        let mut w = Self {
            projects: ProjectStore::open(root.join("projects"))?,
            sites: SiteStore::open(root.join("serving"))?,
            packages: PackageStore::open(root.join("runtimes"), Limits::default())?,
            catalog: Catalog::bundled(),
            secrets,
            sessions: BTreeMap::new(),
            databases: BTreeMap::new(),
            preferences,
            root,
            runtime_cache: vec![],
            history: vec![],
        };
        w.refresh_runtimes()?;
        Ok(w)
    }
    fn save(&self) -> Result<()> {
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new_in(&self.root)?;
        file.write_all(&serde_json::to_vec_pretty(&self.preferences).unwrap())?;
        file.as_file().sync_all()?;
        file.persist(self.root.join("workspace.json"))
            .map_err(|_| error("storage_unavailable"))?;
        Ok(())
    }
    pub fn refresh_runtimes(&mut self) -> Result<()> {
        let target = Target::current()?;
        self.runtime_cache = self
            .catalog
            .packages
            .iter()
            .filter(|(_, p)| p.target == target)
            .map(|(id, p)| {
                let result = if let Some(name) = p.executables.keys().next() {
                    self.packages.executable(p, name).map(|_| ())
                } else if let Some(name) = p.resources.keys().next() {
                    self.packages.resource(p, name).map(|_| ())
                } else {
                    Err(devforge_runtime::RuntimeError::InvalidPackage)
                };
                let installed = result.is_ok();
                let problem = result
                    .err()
                    .filter(|e| *e != devforge_runtime::RuntimeError::NotInstalled)
                    .map(|e| e.to_string());
                Runtime {
                    id: id.clone(),
                    kind: p.kind.clone(),
                    version: p.version.clone(),
                    variant: p.variant.clone(),
                    installed,
                    problem,
                }
            })
            .collect();
        Ok(())
    }
    fn validate_site(&self, id: &str, domain: &str) -> Result<()> {
        devforge_serving::site::domain(domain)?;
        let site = Site {
            project_id: id.into(),
            domain: domain.into(),
            https: false,
            vite: false,
        };
        devforge_serving::site::domain(&site.vite_domain())?;
        if self
            .sites
            .sites()?
            .iter()
            .filter(|s| s.project_id != id)
            .any(|s| s.names().iter().any(|n| site.names().contains(n)))
        {
            return Err(devforge_serving::ServingError::DomainConflict.into());
        }
        Ok(())
    }
    pub fn execute(
        &mut self,
        command: Command,
        cancel: &Cancellation,
    ) -> Result<serde_json::Value> {
        match command {
            Command::RefreshRuntimes => {
                self.refresh_runtimes()?;
                Ok(serde_json::json!({"refreshed":true}))
            }
            Command::Import {
                path,
                name,
                php,
                node,
                domain,
                https,
                vite,
                queue,
            } => {
                self.validate_site("", &domain)?;
                let p = self
                    .projects
                    .import(&path, &name, &php, &node, &self.catalog)?;
                self.sites.configure(
                    &self.projects,
                    Site {
                        project_id: p.id.clone(),
                        domain,
                        https,
                        vite,
                    },
                )?;
                self.preferences
                    .development
                    .insert(p.id.clone(), Development { queue });
                self.save()?;
                Ok(serde_json::json!({"project_id":p.id}))
            }
            Command::Configure {
                id,
                revision,
                name,
                php,
                node,
                domain,
                https,
                vite,
                queue,
            } => {
                let p = self.projects.get(&id)?;
                if p.revision != revision {
                    return Err(devforge_projects::ProjectError::StaleRevision.into());
                }
                self.validate_site(&id, &domain)?;
                compatible(
                    &inspect(&p.path)?,
                    self.catalog.get(&php)?,
                    self.catalog.get(&node)?,
                )?;
                let running = self.sessions.contains_key(&id);
                self.stop(&id)?;
                self.projects
                    .configure(&id, revision, &name, &php, &node, &self.catalog)?;
                self.sites.configure(
                    &self.projects,
                    Site {
                        project_id: id.clone(),
                        domain,
                        https,
                        vite,
                    },
                )?;
                self.preferences
                    .development
                    .insert(id.clone(), Development { queue });
                self.save()?;
                if running {
                    self.start(&id)?;
                }
                Ok(serde_json::json!({"saved":true}))
            }
            Command::Forget { id, revision } => {
                self.stop(&id)?;
                self.projects.forget(&id, revision)?;
                self.sites.remove(&id)?;
                self.preferences.development.remove(&id);
                self.save()?;
                Ok(serde_json::json!({"data_preserved":true}))
            }
            Command::Start { id } => {
                self.start(&id)?;
                Ok(serde_json::json!({"started":true}))
            }
            Command::Stop { id } => {
                self.stop(&id)?;
                Ok(serde_json::json!({"stopped":true}))
            }
            Command::Restart { id } => {
                self.stop(&id)?;
                self.start(&id)?;
                Ok(serde_json::json!({"restarted":true}))
            }
            Command::Install { package } => {
                let p = self.catalog.get(&package)?.clone();
                self.packages.download(&p, cancel, |_| {})?;
                self.refresh_runtimes()?;
                Ok(serde_json::json!({"installed":true}))
            }
            Command::RemoveRuntime { package } => {
                self.packages.remove(self.catalog.get(&package)?)?;
                self.refresh_runtimes()?;
                Ok(serde_json::json!({"removed":true}))
            }
            Command::CreateDatabase { package } => {
                let i = self.projects.initialize_instance(
                    &package,
                    0,
                    &self.catalog,
                    &self.packages,
                    self.secrets.as_ref(),
                )?;
                Ok(serde_json::to_value(i).unwrap())
            }
            Command::BindDatabase { id, instance } => {
                if self.sessions.contains_key(&id) {
                    return Err(error("stop_before_database_change"));
                }
                self.ensure_database(&instance)?;
                let p = self.projects.get(&id)?;
                self.databases
                    .get_mut(&instance)
                    .ok_or_else(|| error("database_external"))?
                    .provision(&self.projects, &p, self.secrets.as_ref())?;
                Ok(serde_json::json!({"bound":true}))
            }
            Command::TestDatabase { id } => {
                let b = self
                    .projects
                    .binding(&id)?
                    .ok_or_else(|| error("database_unbound"))?;
                self.ensure_database(&b.instance_id)?;
                let connected = self
                    .projects
                    .credentials(&id, self.secrets.as_ref())?
                    .connection_test()?;
                Ok(serde_json::json!({"connected":connected}))
            }
            Command::StopDatabase { instance } => {
                if self.users(&instance) > 0 {
                    return Err(error("database_in_use"));
                }
                if let Some(mut server) = self.databases.remove(&instance) {
                    server.stop()?;
                }
                Ok(serde_json::json!({"stopped":true}))
            }
        }
    }
    fn users(&self, id: &str) -> usize {
        self.sessions
            .keys()
            .filter(|p| {
                self.projects
                    .binding(p)
                    .ok()
                    .flatten()
                    .is_some_and(|b| b.instance_id == id)
            })
            .count()
    }
    fn ensure_database(&mut self, id: &str) -> Result<()> {
        if !self.databases.contains_key(id) {
            let server = RunningPostgres::start(
                &self.projects,
                id,
                &self.catalog,
                &self.packages,
                self.secrets.as_ref(),
            )?;
            self.databases.insert(id.into(), server);
        }
        Ok(())
    }
    pub fn start(&mut self, id: &str) -> Result<()> {
        if self.sessions.contains_key(id) {
            return Err(error("already_running"));
        }
        self.projects.get(id)?;
        if let Some(binding) = self.projects.binding(id)? {
            self.ensure_database(&binding.instance_id)?;
        }
        let web = Serving::start_selected(
            &self.sites,
            &self.projects,
            &self.catalog,
            &self.packages,
            self.secrets.as_ref(),
            Options::default(),
            Some(&[id.into()]),
        )?;
        let mut session = Session {
            web,
            queue: None,
            supervisor: Supervisor::open(self.root.join("queue-logs").join(id))?,
        };
        if self
            .preferences
            .development
            .get(id)
            .is_some_and(|d| d.queue)
        {
            let dir = self.root.join("queue");
            fs::create_dir_all(&dir)?;
            let path = dir.join(format!("{}.php", devforge_serving::id()));
            fs::write(&path, QUEUE_WRAPPER)?;
            let p = self.projects.get(id)?;
            let nonce = devforge_serving::id();
            let origin = session.web.status().sites[0].preferred_url.clone();
            let invocation = Context::resolve(
                &self.projects,
                id,
                &self.catalog,
                &self.packages,
                self.secrets.as_ref(),
            )?
            .web_origin(&origin)?
            .invocation(
                Tool::Php,
                vec![
                    dunce::simplified(&path).as_os_str().into(),
                    dunce::simplified(&p.path).as_os_str().into(),
                    nonce.clone().into(),
                ],
                &self.catalog,
                &self.packages,
            )?;
            invocation.start_service(
                &mut session.supervisor,
                Readiness::Output {
                    stream: "stdout",
                    needle: format!("DEVFORGE_QUEUE_READY:{nonce}"),
                },
            )?;
            let queue_id = invocation.id().to_owned();
            session.queue = Some(invocation);
            let until = Instant::now() + Duration::from_secs(20);
            loop {
                let status = session.supervisor.snapshot(&queue_id)?;
                if status.observed == Observed::Ready {
                    break;
                }
                if status.observed == Observed::Failed || Instant::now() >= until {
                    self.history
                        .extend(
                            session
                                .supervisor
                                .logs(&queue_id)?
                                .0
                                .into_iter()
                                .map(|c| Log {
                                    project_id: Some(id.into()),
                                    process: "queue".into(),
                                    stream: c.stream.into(),
                                    text: c.text,
                                }),
                        );
                    return Err(error("queue_unavailable"));
                }
                thread::sleep(Duration::from_millis(25));
            }
        }
        self.sessions.insert(id.into(), session);
        Ok(())
    }
    pub fn stop(&mut self, id: &str) -> Result<()> {
        if let Some(mut session) = self.sessions.remove(id) {
            let mut logs = session
                .web
                .logs()
                .into_iter()
                .map(|(process, chunk)| Log {
                    project_id: Some(id.into()),
                    process,
                    stream: chunk.stream.into(),
                    text: chunk.text,
                })
                .collect::<Vec<_>>();
            if let Some(q) = &session.queue {
                logs.extend(session.supervisor.logs(q.id())?.0.into_iter().map(|c| Log {
                    project_id: Some(id.into()),
                    process: "queue".into(),
                    stream: c.stream.into(),
                    text: c.text,
                }));
            }
            self.history.extend(logs);
            if self.history.len() > 500 {
                self.history.drain(..self.history.len() - 500);
            }
            session.stop()?;
        }
        Ok(())
    }
    pub fn shutdown(&mut self) -> Result<()> {
        let ids: Vec<_> = self.sessions.keys().cloned().collect();
        for id in ids {
            self.stop(&id)?;
        }
        for (_, mut server) in std::mem::take(&mut self.databases) {
            server.stop()?;
        }
        Ok(())
    }
    pub fn snapshot(&self) -> Result<Snapshot> {
        let sites = self.sites.sites()?;
        let mut logs = self.history.clone();
        let mut rows = vec![];
        for project in self.projects.list()? {
            let session = self.sessions.get(&project.id);
            let serving = session.and_then(|s| s.web.status().sites.into_iter().next());
            let processes = session
                .map(|s| s.web.process_snapshots())
                .unwrap_or_default();
            let queue_state = session.and_then(|s| {
                s.queue.as_ref().map(|q| {
                    s.supervisor
                        .snapshot(q.id())
                        .map(|s| format!("{:?}", s.observed).to_lowercase())
                        .unwrap_or_else(|_| "failed".into())
                })
            });
            let state = if session.is_none() {
                "stopped"
            } else if processes
                .iter()
                .any(|(_, s)| s.observed == Observed::Failed)
                || queue_state.as_deref() == Some("failed")
            {
                "failed"
            } else if session.is_some_and(|s| s.web.status().running)
                && serving.as_ref().is_some_and(|s| {
                    s.php_ready
                        && (!sites
                            .iter()
                            .any(|site| site.project_id == project.id && site.vite)
                            || s.vite_ready)
                })
            {
                "running"
            } else {
                "starting"
            };
            if let Some(session) = session {
                for (process, chunk) in session.web.logs() {
                    logs.push(Log {
                        project_id: Some(project.id.clone()),
                        process,
                        stream: chunk.stream.into(),
                        text: chunk.text,
                    });
                }
                if let Some(q) = &session.queue {
                    for chunk in session.supervisor.logs(q.id())?.0 {
                        logs.push(Log {
                            project_id: Some(project.id.clone()),
                            process: "queue".into(),
                            stream: chunk.stream.into(),
                            text: chunk.text,
                        });
                    }
                }
            }
            let database = self.projects.binding(&project.id)?;
            rows.push(Row {
                processes,
                site: sites.iter().find(|s| s.project_id == project.id).cloned(),
                development: self
                    .preferences
                    .development
                    .get(&project.id)
                    .cloned()
                    .unwrap_or_default(),
                state: state.into(),
                serving,
                queue_state,
                database,
                project,
            });
        }
        let databases = self
            .projects
            .instances()?
            .into_iter()
            .map(|instance| {
                let server = self.databases.get(&instance.id);
                if let Some(server) = server {
                    for chunk in server.logs() {
                        logs.push(Log {
                            project_id: None,
                            process: format!("postgres:{}", instance.id),
                            stream: chunk.stream.into(),
                            text: chunk.text,
                        });
                    }
                }
                let state = server
                    .and_then(|s| s.status())
                    .map(|s| format!("{:?}", s.observed).to_lowercase())
                    .unwrap_or_else(|| "stopped".into());
                Database {
                    users: self.users(&instance.id),
                    instance,
                    state,
                }
            })
            .collect();
        // Per-process buffers are bounded; cap IPC output as well.
        let mut bytes = 0;
        logs.reverse();
        logs.retain(|log| {
            bytes += log.text.len();
            bytes <= 256 * 1024
        });
        logs.truncate(500);
        logs.reverse();
        Ok(Snapshot {
            projects: rows,
            runtimes: self.runtime_cache.clone(),
            databases,
            logs,
        })
    }
    pub fn open_target(&self, id: &str, target: &str) -> Result<()> {
        let project = self.projects.get(id)?;
        if target == "terminal" {
            let mut context = Context::resolve(
                &self.projects,
                id,
                &self.catalog,
                &self.packages,
                self.secrets.as_ref(),
            )?;
            terminal::prepare(&mut context, &self.catalog, &self.packages)?;
            return terminal::open(&self.projects, id, &self.root).map_err(Into::into);
        }
        let url = match target {
            "browser" => {
                self.sessions
                    .get(id)
                    .map(|s| s.web.status())
                    .filter(|s| s.running)
                    .and_then(|s| s.sites.into_iter().next())
                    .ok_or_else(|| error("not_running"))?
                    .preferred_url
            }
            "editor" => format!(
                "vscode://file{}",
                url::Url::from_directory_path(dunce::simplified(&project.path))
                    .map_err(|_| error("open_failed"))?
                    .path()
            ),
            "folder" => url::Url::from_directory_path(dunce::simplified(&project.path))
                .map_err(|_| error("open_failed"))?
                .into(),
            _ => return Err(error("open_failed")),
        };
        open_url(&url)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
fn open_url(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        if std::process::Command::new("/usr/bin/open")
            .arg(url)
            .status()?
            .success()
        {
            return Ok(());
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let u: Vec<u16> = std::ffi::OsStr::new(url)
            .encode_wide()
            .chain(Some(0))
            .collect();
        let verb: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
        let result = unsafe {
            windows_sys::Win32::UI::Shell::ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                u.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1,
            )
        };
        if result as isize > 32 {
            return Ok(());
        }
    }
    Err(error("open_failed"))
}
const QUEUE_WRAPPER: &str = r#"<?php
$root=$argv[1]; $token=$argv[2]; chdir($root);
require $root.'/vendor/autoload.php'; $app=require $root.'/bootstrap/app.php';
$kernel=$app->make(Illuminate\Contracts\Console\Kernel::class);$kernel->bootstrap();
$app->make('queue')->connection()->size();
fwrite(STDOUT,'DEVFORGE_QUEUE_READY:'.$token.PHP_EOL);fflush(STDOUT);
exit($kernel->handle(new Symfony\Component\Console\Input\ArgvInput(['artisan','queue:work','--sleep=1','--tries=1','--timeout=60','--no-interaction']),new Symfony\Component\Console\Output\ConsoleOutput()));
"#;
#[cfg(feature = "test-support")]
pub mod testing;
