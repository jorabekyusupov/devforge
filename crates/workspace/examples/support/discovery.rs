use super::*;
use devforge_core::{SettingsRepository, Theme};
use devforge_projects::discovery::scan;
use devforge_workspace::discovery::Service;
use std::time::{Duration, Instant};

fn wait_scan(
    service: &Service,
    root: &Path,
    count: usize,
    expected_name: Option<&str>,
) -> Result<devforge_projects::discovery::Scan> {
    let started = Instant::now();
    loop {
        if let Some(report) = service.take() {
            if report.root == root
                && report.complete
                && report.candidates.len() == count
                && expected_name.is_none_or(|name| report.candidates.iter().any(|c| c.name == name))
            {
                return Ok(report);
            }
        }
        if started.elapsed() > Duration::from_secs(8) {
            return Err(format!(
                "discovery timeout expected {count} event_count {}",
                service.event_count()
            )
            .into());
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}
fn tiny(path: &Path) -> Result<()> {
    fs::create_dir_all(path.join("public"))?;
    fs::write(path.join("artisan"), "<?php")?;
    fs::write(path.join("public/index.php"), "<?php")?;
    fs::write(
        path.join("composer.json"),
        r#"{"require":{"laravel/framework":"^13","php":"^8.4"}}"#,
    )?;
    Ok(())
}
pub fn run(root: &Path) -> Result<()> {
    let root = fs::canonicalize(root)?;
    let system_hosts_path = devforge_serving::system::hosts_path()?;
    let hosts_before = fs::read(&system_hosts_path)?;
    let data = root.join("data");
    let secrets = Arc::new(devforge_workspace::testing::FixtureSecrets::open(
        data.join("e2e-secrets.json"),
    ));
    let mut w = Workspace::open(&data, secrets.clone())?;
    let authority_before = w.sites.root().join("authority").exists();
    let manual = w
        .projects
        .list()?
        .into_iter()
        .filter(|p| ["Atlas", "Studio"].contains(&p.name.as_str()))
        .collect::<Vec<_>>();
    let before = serde_json::to_value(&manual)?;
    let folder = root.join("discovery projects Ўзбек");
    if folder.exists() {
        let owned = if root.join("discovery-fixture.json").exists() {
            serde_json::from_slice::<serde_json::Value>(&fs::read(
                root.join("discovery-fixture.json"),
            )?)?["prepared"]
                == true
        } else {
            serde_json::from_slice::<serde_json::Value>(&fs::read(
                root.join("m5-native-result.json"),
            )?)?["passed"]
                == true
        };
        if !owned {
            return Err("not a disposable native fixture".into());
        }
        for project in w
            .projects
            .list()?
            .into_iter()
            .filter(|p| p.path.starts_with(&folder))
        {
            w.execute(
                Command::Forget {
                    id: project.id,
                    revision: project.revision,
                },
                &Cancellation::default(),
            )?;
        }
        fs::remove_dir_all(&folder)?;
    }
    fs::create_dir_all(&folder)?;
    let folder = fs::canonicalize(folder)?;
    let no_runtime = root.join("no-runtime-root");
    fs::create_dir_all(&no_runtime)?;
    tiny(&no_runtime.join("Needs runtime"))?;
    {
        let mut empty = Workspace::open(root.join("no-runtime-data"), secrets.clone())?;
        empty.reconcile_discovery(scan(&no_runtime, &Cancellation::default()))?;
        assert!(empty.projects.list()?.is_empty());
        assert_eq!(
            empty.snapshot()?.discovery.entries[0].state,
            "needs_runtime"
        );
    }
    let service = Service::new();
    service.set_root(folder.clone());
    w.reconcile_discovery(wait_scan(&service, &folder, 0, None)?)?;
    // Add after the watcher is live. This catches incomplete-copy observation gaps.
    let added = folder.join("Auto Laravel");
    fs::create_dir(&added)?;
    std::thread::sleep(Duration::from_millis(700));
    #[cfg(windows)]
    {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(report) = service.take() {
                if report.watch_paths.contains(&added) {
                    break;
                }
            }
            if Instant::now() > deadline {
                return Err("child watcher was not armed".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let temporary = folder.join("Empty rename probe");
        fs::rename(&added, &temporary)
            .map_err(|e| format!("rename empty watched directory before any runtime: {e}"))?;
        fs::rename(&temporary, &added)
            .map_err(|e| format!("restore empty watched directory before any runtime: {e}"))?;
        eprintln!("DISCOVERY_PHASE: empty watched directory rename passed");
    }
    let source = manual
        .iter()
        .find(|p| p.name == "Studio")
        .ok_or("Studio fixture missing")?;
    let copy_started = Instant::now();
    eprintln!("DISCOVERY_PHASE: copy real Laravel into watched root");
    super::copy(&source.path, &added).map_err(|e| format!("copy Laravel fixture: {e}"))?;
    fs::write(
        added.join(".env"),
        "APP_NAME=Discovery\nAPP_ENV=local\nAPP_KEY=base64:MDEyMzQ1Njc4OWFiY2RlZjAxMjM0NTY3ODlhYmNkZWY=\nAPP_DEBUG=false\nSESSION_DRIVER=file\nCACHE_STORE=file\nQUEUE_CONNECTION=sync\nLOG_CHANNEL=single\n",
    ).map_err(|e|format!("write cloned fixture env: {e}"))?;
    fs::write(
        added.join("routes/web.php"),
        "<?php use Illuminate\\Support\\Facades\\Route; Route::get('/', fn()=>response()->json(['framework'=>app()->version(),'php'=>PHP_VERSION,'discovered'=>true]));",
    ).map_err(|e|format!("write cloned fixture route: {e}"))?;
    w.reconcile_discovery(wait_scan(&service, &folder, 1, Some("Auto Laravel"))?)?;
    let copy_ms = copy_started.elapsed().as_millis();
    let p = w
        .projects
        .list()?
        .into_iter()
        .find(|p| p.path == added)
        .ok_or("automatic registration missing")?;
    let domain = w
        .sites
        .sites()?
        .into_iter()
        .find(|s| s.project_id == p.id)
        .unwrap()
        .domain;
    assert!(domain.ends_with(".devforge.test"));
    #[cfg(windows)]
    {
        let temporary = folder.join("Complete rename probe");
        fs::rename(&added, &temporary)
            .map_err(|e| format!("rename complete watched Laravel before runtime: {e}"))?;
        fs::rename(&temporary, &added)
            .map_err(|e| format!("restore complete watched Laravel before runtime: {e}"))?;
        eprintln!("DISCOVERY_PHASE: complete watched Laravel rename before runtime passed");
    }
    eprintln!("DISCOVERY_PHASE: automatic registration ready; start HTTP");
    w.start(&p.id)?;
    let url = w
        .snapshot()?
        .projects
        .into_iter()
        .find(|r| r.project.id == p.id)
        .unwrap()
        .serving
        .unwrap()
        .http_url;
    let response = super::get(&url)?;
    assert_eq!(response["discovered"], true);
    assert!(
        response["framework"]
            .as_str()
            .is_some_and(|v| v.starts_with("13."))
    );
    eprintln!("DISCOVERY_PHASE: real Laravel HTTP passed; stop serving");
    w.stop(&p.id)?;
    let renamed = folder.join("Renamed Ўзбек");
    fs::rename(&added, &renamed).map_err(|e| format!("rename stopped Laravel directory: {e}"))?;
    w.reconcile_discovery(wait_scan(&service, &folder, 1, Some("Renamed Ўзбек"))?)?;
    assert_eq!(w.projects.get(&p.id)?.path, renamed);
    assert_eq!(
        w.sites
            .sites()?
            .into_iter()
            .find(|s| s.project_id == p.id)
            .unwrap()
            .domain,
        domain
    );
    // Manual project/runtime/DB identities remain byte-equivalent.
    assert_eq!(
        serde_json::to_value(
            w.projects
                .list()?
                .into_iter()
                .filter(|p| manual.iter().any(|old| old.id == p.id))
                .collect::<Vec<_>>()
        )?,
        before
    );
    eprintln!("DISCOVERY_PHASE: rename and manual metadata preserved; Forget/Restore");
    let p = w.projects.get(&p.id)?;
    w.execute(
        Command::Forget {
            id: p.id.clone(),
            revision: p.revision,
        },
        &Cancellation::default(),
    )?;
    w.reconcile_discovery(scan(&folder, &Cancellation::default()))?;
    assert!(w.projects.get(&p.id).is_err());
    let entry = w
        .projects
        .discovery_entries()?
        .into_iter()
        .find(|e| e.ignored && e.project_id.as_deref() == Some(&p.id))
        .unwrap();
    w.execute(
        Command::DiscoveryRestore {
            identity: entry.identity,
        },
        &Cancellation::default(),
    )?;
    w.reconcile_discovery(scan(&folder, &Cancellation::default()))?;
    let restored = w
        .projects
        .list()?
        .into_iter()
        .find(|p| p.path == renamed)
        .unwrap();
    assert_eq!(restored.id, p.id);
    assert_eq!(restored.php, p.php);
    assert_eq!(restored.node, p.node);
    // Leave a stable real Laravel project available for native UI Start checks.
    let mut settings = devforge_platform::SqliteSettings::open(&data)?;
    let mut chosen = settings.load()?;
    chosen.language = "en".into();
    chosen.theme = Theme::Light;
    chosen.project_folder = Some(dunce::simplified(&folder).to_string_lossy().into_owned());
    settings.save(&chosen)?;
    // Root unavailable does not manufacture missing projects.
    let bad = scan(&root.join("absent"), &Cancellation::default());
    assert!(!bad.complete);
    let mut hundred = ScanBudget::new(&root)?;
    let cold = hundred.measure()?;
    for n in 0..50 {
        service.set_root(if n % 2 == 0 {
            no_runtime.clone()
        } else {
            folder.clone()
        });
    }
    w.reconcile_discovery(wait_scan(&service, &folder, 1, Some("Renamed Ўзбек"))?)?;
    assert_eq!(w.projects.get(&restored.id)?.path, renamed);
    // Case-only rename and long Unicode parent paths exercise Windows identity
    // and verbatim-path handling on an actual Windows kernel in the CI lane.
    let case_path = folder.join("renamed Ўзбек");
    fs::rename(&renamed, &case_path).map_err(|e| format!("case-only rename: {e}"))?;
    w.reconcile_discovery(scan(&folder, &Cancellation::default()))?;
    assert_eq!(w.projects.get(&restored.id)?.path, case_path);
    fs::rename(&case_path, &renamed).map_err(|e| format!("case-only rename restore: {e}"))?;
    w.reconcile_discovery(scan(&folder, &Cancellation::default()))?;
    let long_parent = root
        .join("long-path-case")
        .join("parent-Русский-Ўзбек".repeat(4))
        .join("long-component".repeat(8))
        .join("nested-long-component".repeat(3));
    fs::create_dir_all(&long_parent)?;
    tiny(&long_parent.join("app"))?;
    assert_eq!(
        scan(&long_parent, &Cancellation::default())
            .candidates
            .len(),
        1
    );
    #[cfg(windows)]
    {
        let junction = folder.join("Junction child");
        let target = no_runtime.join("Needs runtime");
        let output = std::process::Command::new("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&target)
            .output()?;
        if !output.status.success() {
            return Err(format!(
                "fixture junction creation failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )
            .into());
        }
        let found = scan(&folder, &Cancellation::default());
        assert!(found.skipped > 0);
        assert!(found.candidates.iter().all(|c| c.path != junction));
        fs::remove_dir(&junction)?;
        assert!(target.join("composer.json").exists());
    }
    #[cfg(unix)]
    {
        let link = folder.join("Symlink child");
        let target = no_runtime.join("Needs runtime");
        std::os::unix::fs::symlink(&target, &link)?;
        let found = scan(&folder, &Cancellation::default());
        assert!(found.skipped > 0);
        assert!(found.candidates.iter().all(|c| c.path != link));
        fs::remove_file(&link)?;
        assert!(target.join("composer.json").exists());
    }
    // Native parent monitoring recovers after the configured directory is removed.
    eprintln!("DISCOVERY_PHASE: native root removal and recreation");
    let recovery_root = root.join("root-recovery");
    fs::create_dir_all(&recovery_root)?;
    let recovery_root = fs::canonicalize(recovery_root)?;
    let recovery = Service::new();
    recovery.set_root(recovery_root.clone());
    wait_scan(&recovery, &recovery_root, 0, None)?;
    fs::remove_dir(&recovery_root).map_err(|e| format!("remove watched empty root: {e}"))?;
    let recovery_started = Instant::now();
    loop {
        if let Some(report) = recovery.take() {
            if report.problem.as_deref() == Some("unavailable") {
                break;
            }
        }
        if recovery_started.elapsed() > Duration::from_secs(8) {
            return Err("root deletion was not observed".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    fs::create_dir(&recovery_root)?;
    wait_scan(&recovery, &recovery_root, 0, None)?;
    drop(recovery);
    // Force the production fallback branch, then rely on its real 30-second
    // timer and filesystem reads. No manufactured scan result or fake clock.
    eprintln!("DISCOVERY_PHASE: real periodic fallback and bounded event storm");
    let fallback_root = root.join("fallback-case");
    if fallback_root.exists() {
        fs::remove_dir_all(&fallback_root)?;
    }
    fs::create_dir_all(&fallback_root)?;
    let fallback = Service::polling_fixture();
    let fallback_root = fs::canonicalize(fallback_root)?;
    fallback.set_root(fallback_root.clone());
    wait_scan(&fallback, &fallback_root, 0, None)?;
    let fallback_started = Instant::now();
    tiny(&fallback_root.join("late"))?;
    loop {
        if let Some(report) = fallback.take() {
            if report.candidates.len() == 1 {
                break;
            }
        }
        if fallback_started.elapsed() > Duration::from_secs(35) {
            return Err("periodic fallback did not converge".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(fallback.status().degraded);
    let fallback_ms = fallback_started.elapsed().as_millis();
    drop(fallback);
    let stress_started = Instant::now();
    for n in 0..10_000 {
        fs::write(folder.join(format!("event-{n}")), "fixture")?;
    }
    w.reconcile_discovery(wait_scan(&service, &folder, 1, Some("Renamed Ўзбек"))?)?;
    for n in 0..10_000 {
        fs::remove_file(folder.join(format!("event-{n}")))?;
    }
    let storm_ms = stress_started.elapsed().as_millis();
    let count = service.event_count();
    drop(service);
    w.shutdown()?;
    assert_eq!(hosts_before, fs::read(system_hosts_path)?);
    assert_eq!(authority_before, w.sites.root().join("authority").exists());
    let report = json!({"target":Target::current()?,"no_runtime_candidate":true,"automatic_registration":true,"native_watch_incomplete_copy":true,"watch_copy_to_registration_ms":copy_ms,"native_event_count":count,"rename_preserves_id_domain":true,"forget_restore":true,"manual_metadata_preserved":true,"fifty_root_switches":true,"case_only_rename":true,"native_root_delete_recreate":true,"reparse_children_skipped":true,"long_unicode_path":true,"periodic_fallback_ms":fallback_ms,"event_storm_10000_ms":storm_ms,"real_laravel_http":response,"cold_100_scan_ms":cold,"system_hosts_unchanged":true,"authority_store_unchanged":true,"system_trust_operations_invoked":false,"restore_preserves_profile":true,"ui_project_id":restored.id,"ui_root":dunce::simplified(&folder),"passed":true});
    fs::write(
        root.join("discovery-native-result.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("DISCOVERY_NATIVE_REPORT:{report}");
    Ok(())
}
struct ScanBudget {
    path: PathBuf,
}
impl ScanBudget {
    fn new(root: &Path) -> Result<Self> {
        let path = root.join("hundred-fixture");
        fs::create_dir_all(&path)?;
        for n in 0..100 {
            tiny(&path.join(format!("app-{n}")))?;
        }
        Ok(Self { path })
    }
    fn measure(&mut self) -> Result<u128> {
        let start = Instant::now();
        let result = scan(&self.path, &Cancellation::default());
        assert_eq!(result.candidates.len(), 100);
        let ms = start.elapsed().as_millis();
        assert!(ms < 3000);
        Ok(ms)
    }
}

/// Focused acceptance setup: real Laravel and verified serving tools, without
/// unrelated queue/Vite/PostgreSQL workloads hiding discovery failures.
pub fn prepare(root: &Path, cache: &Path) -> Result<()> {
    fs::create_dir_all(root)?;
    let root = fs::canonicalize(root)?;
    let data = root.join("data");
    fs::create_dir_all(&data)?;
    let secrets = Arc::new(devforge_workspace::testing::FixtureSecrets::open(
        data.join("e2e-secrets.json"),
    ));
    let mut w = Workspace::open(&data, secrets.clone())?;
    let target = Target::current()?;
    let rename_probe = root.join("rename-probe");
    let source_probe = rename_probe.join("child");
    fs::create_dir_all(&source_probe)?;
    fs::rename(&source_probe, rename_probe.join("renamed"))
        .map_err(|e| format!("same-user directory rename before any process or watcher: {e}"))?;
    fs::remove_dir_all(&rename_probe)?;
    eprintln!("DISCOVERY_PREPARE: same-user directory mutation probe passed");
    let candidates: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../../../../runtime/m0-candidates.json"))?;
    for (id, p) in w.catalog.packages.iter().filter(|(_, p)| {
        p.target == target
            && (p.kind == "caddy"
                || p.kind == "composer"
                || p.kind == "php" && p.version.starts_with("8.5.")
                || p.kind == "node" && p.version.trim_start_matches('v').starts_with("24."))
    }) {
        eprintln!("DISCOVERY_PREPARE: verified runtime {id}");
        let source = candidates
            .iter()
            .find(|c| c["url"].as_str() == Some(&p.url))
            .ok_or("runtime source missing")?;
        let archive = cache.join(format!(
            "{}-{}",
            source["id"].as_str().unwrap(),
            p.url.rsplit('/').next().unwrap()
        ));
        if archive.exists() {
            w.packages
                .install_cached(p, &archive, &Cancellation::default(), |_| {})?;
        } else {
            w.packages.download(p, &Cancellation::default(), |_| {})?;
        }
    }
    w.refresh_runtimes()?;
    let choose = |kind: &str| {
        w.catalog
            .packages
            .iter()
            .find(|(_, p)| {
                p.target == target
                    && p.kind == kind
                    && (kind != "php"
                        || p.version.starts_with("8.5.") && p.executables.contains_key("php"))
                    && (kind != "node" || p.version.trim_start_matches('v').starts_with("24."))
            })
            .map(|(id, _)| id.clone())
            .unwrap()
    };
    let php = choose("php");
    let node = choose("node");
    let seed = root.join("tooling");
    tiny(&seed)?;
    let bootstrap = w
        .projects
        .import(&seed, "Tooling", &php, &node, &w.catalog)?;
    let studio = root.join("Studio Русский project");
    eprintln!("DISCOVERY_PREPARE: actual Laravel Composer installation");
    super::exec(
        &w,
        secrets.as_ref(),
        &bootstrap.id,
        Tool::Composer,
        vec![
            "create-project".into(),
            "laravel/laravel".into(),
            dunce::simplified(&studio).as_os_str().into(),
            "v13.11.0".into(),
            "--no-dev".into(),
            "--no-scripts".into(),
            "--no-plugins".into(),
            "--no-interaction".into(),
            "--prefer-dist".into(),
        ],
    )?;
    w.projects.forget(&bootstrap.id, bootstrap.revision)?;
    let atlas = root.join("Atlas Ўзбек project");
    super::copy(&studio, &atlas)?;
    for (path, name) in [(&studio, "Studio"), (&atlas, "Atlas")] {
        w.execute(
            Command::Import {
                path: path.clone(),
                name: name.into(),
                php: php.clone(),
                node: node.clone(),
                domain: format!(
                    "manual-{}-{}.devforge.test",
                    name.to_lowercase(),
                    &w.sites.owner()?[..8]
                ),
                https: false,
                vite: false,
                queue: false,
            },
            &Cancellation::default(),
        )?;
    }
    w.shutdown()?;
    fs::write(
        root.join("discovery-fixture.json"),
        serde_json::to_vec_pretty(
            &json!({"prepared":true,"real_laravel":true,"target":target,"manual_projects":w.projects.list()?.len()}),
        )?,
    )?;
    eprintln!("DISCOVERY_PREPARE: completed");
    Ok(())
}
