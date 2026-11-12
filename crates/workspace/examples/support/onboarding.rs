use super::*;
use devforge_workspace::setup::{CreateInput, PrepareOptions, Source};
fn options() -> PrepareOptions {
    PrepareOptions {
        composer: true,
        npm: true,
        create_env: true,
        generate_key: true,
        run_scripts: false,
        initialize_sqlite: true,
        trusted: true,
    }
}
fn get(w: &Workspace, id: &str, path: &str) -> Result<(u16, String)> {
    let status = w
        .snapshot()?
        .projects
        .into_iter()
        .find(|r| r.project.id == id)
        .unwrap()
        .serving
        .unwrap();
    let u = url::Url::parse(&status.preferred_url)?;
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .resolve(
            u.host_str().unwrap(),
            ([127, 0, 0, 1], u.port_or_known_default().unwrap()).into(),
        )
        .build()?;
    let r = client
        .get(format!(
            "{}{path}",
            status.preferred_url.trim_end_matches('/')
        ))
        .send()?;
    Ok((r.status().as_u16(), r.text()?))
}
fn git(
    w: &Workspace,
    secrets: &dyn devforge_core::SecretStore,
    id: &str,
    args: Vec<OsString>,
) -> Result<()> {
    let hooks = w.projects.root().join("empty-hooks");
    fs::create_dir_all(&hooks)?;
    let mut prefix = vec![
        "-c".into(),
        format!("core.hooksPath={}", dunce::simplified(&hooks).display()).into(),
        "-c".into(),
        "user.name=DevForge Native".into(),
        "-c".into(),
        "user.email=native@example.test".into(),
    ];
    prefix.extend(args);
    exec(w, secrets, id, Tool::Git, prefix)?;
    Ok(())
}
fn key(path: &Path) -> Result<String> {
    Ok(fs::read_to_string(path.join(".env"))?
        .lines()
        .find_map(|l| l.strip_prefix("APP_KEY="))
        .ok_or("APP_KEY missing")?
        .trim()
        .to_owned())
}
pub fn reset(root: &Path) -> Result<()> {
    let root = fs::canonicalize(root)?;
    let data = root.join("data");
    if !data.join("e2e-secrets.json").is_file() {
        return Err("not an isolated native fixture".into());
    }
    let secrets = Arc::new(devforge_workspace::testing::FixtureSecrets::open(
        data.join("e2e-secrets.json"),
    ));
    let mut w = Workspace::open(&data, secrets)?;
    let owned = root.join("M8 Ўзбек & ' projects");
    for p in w
        .projects
        .list()?
        .into_iter()
        .filter(|p| p.name.starts_with("M8 "))
    {
        if !p.path.starts_with(&owned) {
            return Err("refusing non-fixture project cleanup".into());
        }
        w.execute(
            Command::Forget {
                id: p.id,
                revision: p.revision,
            },
            &Cancellation::default(),
        )?;
    }
    w.shutdown()?;
    drop(w);
    if owned.is_dir() {
        fs::remove_dir_all(owned)?;
    }
    Ok(())
}
pub fn reset_ui(root: &Path) -> Result<()> {
    let root = fs::canonicalize(root)?;
    if !root.join("m8-native-result.json").is_file() {
        return Err("M8 native report required".into());
    }
    let data = root.join("data");
    let secrets = Arc::new(devforge_workspace::testing::FixtureSecrets::open(
        data.join("e2e-secrets.json"),
    ));
    let mut w = Workspace::open(&data, secrets)?;
    let owned = root.join("UI workspace");
    for p in w
        .projects
        .list()?
        .into_iter()
        .filter(|p| p.name.starts_with("M8 UI "))
    {
        if !p.path.starts_with(&owned) {
            return Err("refusing non-fixture UI cleanup".into());
        }
        w.execute(
            Command::Forget {
                id: p.id,
                revision: p.revision,
            },
            &Cancellation::default(),
        )?;
    }
    w.shutdown()?;
    drop(w);
    if owned.is_dir() {
        fs::remove_dir_all(owned)?;
    }
    Ok(())
}
pub fn run(root: &Path) -> Result<()> {
    let canonical_root = fs::canonicalize(root)?;
    let root = canonical_root.as_path();
    let data = root.join("data");
    let secrets = Arc::new(devforge_workspace::testing::FixtureSecrets::open(
        data.join("e2e-secrets.json"),
    ));
    let mut w = Workspace::open(&data, secrets.clone())?;
    let atlas = w
        .projects
        .list()?
        .into_iter()
        .find(|p| p.name == "Atlas")
        .ok_or("Atlas missing")?;
    let target = Target::current()?;
    let php = w
        .catalog
        .packages
        .iter()
        .find(|(_, p)| {
            p.target == target
                && p.kind == "php"
                && p.executables.contains_key("php")
                && p.version.starts_with("8.5.")
        })
        .unwrap()
        .0
        .clone();
    let node = w
        .catalog
        .packages
        .iter()
        .find(|(_, p)| {
            p.target == target
                && p.kind == "node"
                && p.version.trim_start_matches('v').starts_with("24.")
        })
        .unwrap()
        .0
        .clone();
    let parent = root.join("M8 Ўзбек & ' projects");
    fs::create_dir_all(&parent)?;
    let input = CreateInput {
        source: Source::Laravel {
            version: w.catalog.defaults["laravel"].clone(),
        },
        parent: parent.clone(),
        folder: "m8-fresh".into(),
        name: "M8 Fresh".into(),
        php: php.clone(),
        node: node.clone(),
        domain: "m8-fresh.devforge.test".into(),
        https: true,
        options: options(),
    };
    let hosts_before = fs::read(devforge_serving::system::hosts_path()?)?;
    eprintln!("M8_PHASE: create official Laravel using selected PHP Node Composer");
    let result = match w.execute(
        Command::CreateProject {
            input: input.clone(),
        },
        &Cancellation::default(),
    ) {
        Ok(value) => value,
        Err(e) => {
            let logs = w
                .snapshot()?
                .logs
                .into_iter()
                .filter(|l| l.process == "source")
                .rev()
                .take(12)
                .collect::<Vec<_>>();
            let diagnostic = json!({"stage":"source","error":e,"redacted_logs":logs});
            fs::write(
                root.join("m8-native-failure.json"),
                serde_json::to_vec_pretty(&diagnostic)?,
            )?;
            eprintln!("M8_NATIVE_FAILURE:{diagnostic}");
            return Err(e.into());
        }
    };
    let fresh_id = result["project_id"].as_str().unwrap().to_owned();
    let fresh = w.projects.get(&fresh_id)?;
    let initial_key = key(&fresh.path)?;
    assert!(initial_key.starts_with("base64:") && initial_key.len() > 40);
    assert!(fresh.path.join("vendor/autoload.php").is_file());
    assert!(fresh.path.join("node_modules").is_dir());
    assert!(fresh.path.join("database/database.sqlite").is_file());
    w.execute(
        Command::PrepareProject {
            id: fresh_id.clone(),
            options: options(),
        },
        &Cancellation::default(),
    )?;
    assert_eq!(key(&fresh.path)?, initial_key);
    assert!(
        w.execute(
            Command::CreateProject {
                input: input.clone()
            },
            &Cancellation::default()
        )
        .is_err()
    );
    assert_eq!(key(&fresh.path)?, initial_key);
    let mut composer: serde_json::Value =
        serde_json::from_slice(&fs::read(fresh.path.join("composer.json"))?)?;
    composer["scripts"]["post-install-cmd"] = json!(["@php m8-script.php"]);
    fs::write(
        fresh.path.join("composer.json"),
        serde_json::to_vec_pretty(&composer)?,
    )?;
    fs::write(fresh.path.join("m8-script.php"),br#"<?php foreach(file('.env') as $line){if(str_starts_with($line,'APP_KEY='))echo trim(substr($line,8)).PHP_EOL;}file_put_contents('.m8-script-ran','yes');"#)?;
    git(
        &w,
        secrets.as_ref(),
        &fresh_id,
        vec!["init".into(), "-b".into(), "main".into()],
    )?;
    git(
        &w,
        secrets.as_ref(),
        &fresh_id,
        vec!["add".into(), "--".into(), ".".into()],
    )?;
    git(
        &w,
        secrets.as_ref(),
        &fresh_id,
        vec!["commit".into(), "-m".into(), "Native fixture".into()],
    )?;
    let mut clone_input = input.clone();
    clone_input.folder = "m8-clone".into();
    clone_input.name = "M8 Clone".into();
    clone_input.domain = "m8-clone.devforge.test".into();
    clone_input.source = Source::Git {
        repository: dunce::simplified(&fresh.path)
            .to_string_lossy()
            .into_owned(),
        branch: Some("main".into()),
    };
    clone_input.options.initialize_sqlite = false;
    eprintln!("M8_PHASE: clone real local Git repo and prepare without running scripts");
    let result = w.execute(
        Command::CreateProject {
            input: clone_input.clone(),
        },
        &Cancellation::default(),
    )?;
    let clone_id = result["project_id"].as_str().unwrap().to_owned();
    let clone = w.projects.get(&clone_id)?;
    assert!(clone.path.join(".git").is_dir());
    assert!(!clone.path.join(".m8-script-ran").exists());
    let clone_key = key(&clone.path)?;
    assert_ne!(clone_key, initial_key);
    let mut trusted = clone_input.options.clone();
    trusted.run_scripts = true;
    w.execute(
        Command::PrepareProject {
            id: clone_id.clone(),
            options: trusted,
        },
        &Cancellation::default(),
    )?;
    assert!(clone.path.join(".m8-script-ran").is_file());
    assert_eq!(key(&clone.path)?, clone_key);
    assert!(
        !w.snapshot()?
            .logs
            .iter()
            .any(|l| l.text.contains(&clone_key))
    );
    assert!(
        w.snapshot()?
            .logs
            .iter()
            .any(|l| l.text.contains("[REDACTED]")),
        "script output must actually be captured and redacted"
    );
    git(
        &w,
        secrets.as_ref(),
        &fresh_id,
        vec!["checkout".into(), "-b".into(), "broken".into()],
    )?;
    fs::write(fresh.path.join("package.json"),br#"{"name":"m8-recovery","version":"1.0.0","scripts":{"postinstall":"node -e \"process.exit(7)\""}}"#)?;
    fs::remove_file(fresh.path.join("package-lock.json"))?;
    // The official Laravel template deliberately sets ignore-scripts=true.
    // This test branch explicitly enables its own lifecycle script policy.
    fs::write(
        fresh.path.join(".npmrc"),
        b"ignore-scripts=false\naudit=false\n",
    )?;
    git(
        &w,
        secrets.as_ref(),
        &fresh_id,
        vec![
            "add".into(),
            "--".into(),
            "package.json".into(),
            "package-lock.json".into(),
            ".npmrc".into(),
        ],
    )?;
    git(
        &w,
        secrets.as_ref(),
        &fresh_id,
        vec!["commit".into(), "-m".into(), "Recovery fixture".into()],
    )?;
    let mut broken = clone_input.clone();
    broken.folder = "m8-recovery".into();
    broken.name = "M8 Recovery".into();
    broken.domain = "m8-recovery.devforge.test".into();
    broken.source = Source::Git {
        repository: dunce::simplified(&fresh.path)
            .to_string_lossy()
            .into_owned(),
        branch: Some("broken".into()),
    };
    broken.options.composer = false;
    broken.options.run_scripts = true;
    eprintln!(
        "M8_PHASE: dependency failure preserves registered project and retries without recloning"
    );
    let failure = w
        .execute(
            Command::CreateProject {
                input: broken.clone(),
            },
            &Cancellation::default(),
        )
        .unwrap_err();
    let recovery_id = failure.project_id.ok_or("Recovery id missing")?;
    let recovery = w.projects.get(&recovery_id)?;
    assert!(recovery.path.join(".git").is_dir());
    let recovery_key = key(&recovery.path)?;
    fs::write(
        recovery.path.join("package.json"),
        br#"{"name":"m8-recovery","version":"1.0.0"}"#,
    )?;
    if recovery.path.join("package-lock.json").is_file() {
        fs::remove_file(recovery.path.join("package-lock.json"))?;
    }
    w.execute(
        Command::PrepareProject {
            id: recovery_id,
            options: broken.options,
        },
        &Cancellation::default(),
    )?;
    assert_eq!(key(&recovery.path)?, recovery_key);
    git(
        &w,
        secrets.as_ref(),
        &fresh_id,
        vec!["checkout".into(), "main".into()],
    )?;
    w.start(&atlas.id)?;
    w.start(&fresh_id)?;
    assert_eq!(get(&w, &fresh_id, "/")?.0, 200);
    let atlas_pids = w
        .snapshot()?
        .projects
        .into_iter()
        .find(|r| r.project.id == atlas.id)
        .unwrap()
        .processes
        .into_iter()
        .map(|(n, s)| (n, s.pid))
        .collect::<Vec<_>>();
    eprintln!("M8_PHASE: project PHP INI changes restart only selected project");
    let revision = w.projects.get(&fresh_id)?.revision;
    w.execute(
        Command::PhpSettings {
            id: fresh_id.clone(),
            revision,
            settings: devforge_projects::php_settings::PhpSettings {
                memory_limit: Some("512M".into()),
                upload_mb: Some(16),
                post_mb: Some(20),
                execution_seconds: Some(60),
                timezone: Some("Asia/Tashkent".into()),
            },
        },
        &Cancellation::default(),
    )?;
    assert_eq!(
        atlas_pids,
        w.snapshot()?
            .projects
            .into_iter()
            .find(|r| r.project.id == atlas.id)
            .unwrap()
            .processes
            .into_iter()
            .map(|(n, s)| (n, s.pid))
            .collect::<Vec<_>>()
    );
    let diagnostics = w.execute(
        Command::PhpDiagnostics {
            id: fresh_id.clone(),
        },
        &Cancellation::default(),
    )?;
    assert_eq!(diagnostics["settings"]["memory_limit"], "512M");
    assert_eq!(diagnostics["settings"]["date.timezone"], "Asia/Tashkent");
    assert_eq!(diagnostics["app_key_present"], true);
    assert_eq!(diagnostics["platform_check_available"], true);
    assert!(
        diagnostics["pdo_drivers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "sqlite")
    );
    let mut routes = fs::OpenOptions::new()
        .append(true)
        .open(fresh.path.join("routes/web.php"))?;
    use std::io::Write;
    routes.write_all(br#"\Illuminate\Support\Facades\Route::get('/m8-ini',fn()=>response()->json(['memory'=>ini_get('memory_limit'),'timezone'=>ini_get('date.timezone')]));"#)?;
    drop(routes);
    let (status, body) = get(&w, &fresh_id, "/m8-ini")?;
    assert_eq!(status, 200);
    let web: serde_json::Value = serde_json::from_str(&body)?;
    assert_eq!(web["memory"], "512M");
    let tls = w
        .snapshot()?
        .projects
        .into_iter()
        .find(|r| r.project.id == fresh_id)
        .unwrap()
        .serving
        .unwrap()
        .https_url
        .unwrap();
    let u = url::Url::parse(&tls)?;
    let ca = devforge_serving::authority::Authority::open(
        w.sites.root().join("authority"),
        &w.sites.owner()?,
        secrets.as_ref(),
    )?;
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .add_root_certificate(reqwest::Certificate::from_der(ca.der())?)
        .resolve(
            u.host_str().unwrap(),
            ([127, 0, 0, 1], u.port_or_known_default().unwrap()).into(),
        )
        .build()?;
    assert_eq!(
        client
            .get(format!("{}/m8-ini", tls.trim_end_matches('/')))
            .send()?
            .status(),
        200
    );
    let review = w.execute(
        Command::HttpsExport {
            id: fresh_id.clone(),
        },
        &Cancellation::default(),
    )?;
    assert_eq!(review["applied"], false);
    assert!(
        w.execute(
            Command::HttpsApply {
                id: fresh_id.clone(),
                ticket: review["ticket"].as_str().unwrap().into(),
                digest: review["digest"].as_str().unwrap().into(),
                consent: false
            },
            &Cancellation::default()
        )
        .is_err()
    );
    assert_eq!(
        hosts_before,
        fs::read(devforge_serving::system::hosts_path()?)?
    );
    w.execute(
        Command::Favorite {
            id: fresh_id.clone(),
            favorite: true,
        },
        &Cancellation::default(),
    )?;
    w.execute(
        Command::Visit {
            id: clone_id.clone(),
        },
        &Cancellation::default(),
    )?;
    w.shutdown()?;
    eprintln!("M8_PHASE: cancellation reaps actual descendant process");
    let port_file = root.join("m8-cancel-port");
    let _ = fs::remove_file(&port_file);
    let script = r#"const cp=require('node:child_process');cp.spawn(process.execPath,['-e',"const s=require('node:net').createServer();s.listen(0,'127.0.0.1',()=>require('node:fs').writeFileSync(process.argv[1],String(s.address().port)));",process.argv[1]],{stdio:'ignore'});setInterval(()=>{},1000);"#;
    let invocation = Context::resolve(
        &w.projects,
        &atlas.id,
        &w.catalog,
        &w.packages,
        secrets.as_ref(),
    )?
    .frontend()
    .invocation(
        Tool::Node,
        vec![
            "-e".into(),
            script.into(),
            dunce::simplified(&port_file).as_os_str().to_owned(),
        ],
        &w.catalog,
        &w.packages,
    )?;
    let cancel = Cancellation::default();
    let signal = cancel.clone();
    let observed = port_file.clone();
    let thread = std::thread::spawn(move || {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while !observed.is_file() && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        signal.cancel();
    });
    assert!(
        invocation
            .run_cancellable(root.join("cancel-logs"), &cancel)
            .is_err()
    );
    thread.join().unwrap();
    let port: u16 = fs::read_to_string(&port_file)?.parse()?;
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());
    drop(invocation);
    drop(w);
    let reopened = Workspace::open(&data, secrets)?;
    let fresh_row = reopened
        .snapshot()?
        .projects
        .into_iter()
        .find(|r| r.project.id == fresh_id)
        .unwrap();
    assert!(fresh_row.navigation.favorite);
    assert!(fresh_row.navigation.last_opened_ms > 0);
    assert_eq!(
        fresh_row.project.php_settings.memory_limit.as_deref(),
        Some("512M")
    );
    let report = json!({"passed":true,"host":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH},"real_laravel_create_composer_npm_app_key_sqlite":true,"git_clone_branch_unicode_parent":true,"scripts_opt_in_and_keys_redacted":true,"dependency_failure_preserves_project_and_retry":true,"existing_destination_and_keys_preserved":true,"php_settings_actual_cli_and_web":true,"php_change_preserves_other_project_pids":true,"actual_platform_requirements":true,"actual_https_owned_ca_client":true,"https_review_no_system_mutation_and_consent_guard":true,"cancellation_stops_descendant":true,"favorites_recent_php_persist":true,"source_branch":"main","source_relative":fresh.path.strip_prefix(fs::canonicalize(root)?)?,"fresh_id":fresh_id,"clone_id":clone_id});
    fs::write(
        root.join("m8-native-result.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("M8_NATIVE_REPORT:{report}");
    Ok(())
}
