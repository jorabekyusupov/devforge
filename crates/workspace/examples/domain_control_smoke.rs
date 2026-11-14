//! Explicit owned Caddy/client-only-CA proof, never system hosts/trust or PHP.
#[cfg(not(feature = "test-support"))]
fn main() {
    eprintln!("requires explicit test-support fixture build");
    std::process::exit(2);
}

#[cfg(feature = "test-support")]
fn main() {
    if let Err(error) = run() {
        eprintln!("domain fixture: {error}");
        std::process::exit(1);
    }
}
#[cfg(feature = "test-support")]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use devforge_core::{ErrorCode, SecretStore};
    use devforge_projects::project::Catalog;
    use devforge_runtime::{
        Cancellation,
        package::Target,
        process::{LogPolicy, Observed, ProcessSpec, Readiness, RestartPolicy, Supervisor},
    };
    use devforge_serving::authority::Authority;
    use devforge_workspace::{Command, Error, Workspace, domain_control::DomainPlanKind};
    use std::{
        collections::BTreeMap,
        fs,
        net::TcpListener,
        path::{Path, PathBuf},
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };
    struct DenySecrets;
    impl SecretStore for DenySecrets {
        fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
            panic!("observer may not read native keys")
        }
        fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
            panic!("observer may not create native keys")
        }
        fn delete(&self, _: &str) -> Result<(), ErrorCode> {
            panic!("observer may not delete native keys")
        }
    }
    #[derive(Default)]
    struct FixtureSecrets(Mutex<BTreeMap<String, String>>);
    impl SecretStore for FixtureSecrets {
        fn get(&self, key: &str) -> Result<Option<String>, ErrorCode> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &str, value: &str) -> Result<(), ErrorCode> {
            self.0.lock().unwrap().insert(key.into(), value.into());
            Ok(())
        }
        fn delete(&self, key: &str) -> Result<(), ErrorCode> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }
    fn quoted(path: &Path) -> String {
        serde_json::to_string(&dunce::simplified(path).to_string_lossy().replace('\\', "/"))
            .unwrap()
    }
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err("usage: RUNTIME_CACHE REPORT_JSON".into());
    }
    let source = PathBuf::from(&args[0]);
    let report = PathBuf::from(&args[1]);
    let root = tempfile::tempdir()?;
    let mut workspace = Workspace::open(root.path().join("data"), Arc::new(DenySecrets))?;
    let target = Target::current()?;
    let catalog = Catalog::bundled();
    let caddy_version = catalog
        .defaults
        .get("caddy")
        .ok_or("missing Caddy default")?;
    let (package_id, descriptor) = catalog
        .packages
        .iter()
        .find(|(_, p)| p.kind == "caddy" && p.target == target && p.version == *caddy_version)
        .ok_or("missing native Caddy")?;
    let key = descriptor.key()?;
    let original = source.join("packages").join(&key);
    let receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(original.join(".devforge-receipt.json"))?)?;
    if serde_json::from_value::<devforge_runtime::package::Package>(receipt["package"].clone())?
        != *descriptor
    {
        return Err("source descriptor mismatch".into());
    }
    // Destination store marker exists before copying. Never open/mutate source.
    let destination = root.path().join("data/runtimes/packages").join(&key);
    fs::create_dir(&destination)?;
    fs::copy(
        original.join(".devforge-receipt.json"),
        destination.join(".devforge-receipt.json"),
    )?;
    let relative = descriptor
        .executables
        .get("caddy")
        .ok_or("missing Caddy executable")?;
    let target_file = destination.join(relative);
    fs::create_dir_all(target_file.parent().unwrap())?;
    fs::copy(original.join(relative), &target_file)?;
    let executable = workspace.packages.executable(descriptor, "caddy")?;
    let project = root.path().join("project");
    fs::create_dir_all(project.join("public"))?;
    fs::write(project.join("public/index.php"), "<?php")?;
    fs::write(
        project.join("composer.json"),
        r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )?;
    let php = catalog
        .packages
        .iter()
        .find(|(_, p)| p.kind == "php" && p.target == target && p.executables.contains_key("php"))
        .ok_or("missing PHP metadata")?
        .0
        .clone();
    let node = catalog
        .packages
        .iter()
        .find(|(_, p)| p.kind == "node" && p.target == target && p.executables.contains_key("node"))
        .ok_or("missing Node metadata")?
        .0
        .clone();
    let imported = workspace.execute(
        Command::Import {
            path: project,
            name: "Domain fixture".into(),
            php,
            node,
            domain: "a.devforge.test".into(),
            https: true,
            vite: false,
            queue: false,
        },
        &Cancellation::default(),
    )?;
    let id = imported["project_id"].as_str().ok_or("missing project")?;
    let before = workspace.domain_facts_guarded(id, &|| Ok(()))?;
    assert!(!before.ca_prepared && before.system_trusted.is_none());
    let ca = Authority::open(
        workspace.sites.root().join("authority"),
        &workspace.sites.owner()?,
        &FixtureSecrets::default(),
    )?;
    let leaf = ca.issue(&root.path().join("leaf"), &["a.localhost".into()])?;
    let facts = workspace.domain_facts_guarded(id, &|| Ok(()))?;
    assert!(facts.ca_prepared && facts.system_trusted.is_none());
    let plan =
        workspace.domain_fixture_plan(id, DomainPlanKind::Trust, b"127.0.0.1 localhost\n")?;
    let mut confirmation = plan.confirm_native_owner()?;
    let denied = workspace.execute_domain_plan(
        id,
        &plan,
        &mut confirmation,
        &Cancellation::default(),
        &|| {
            Err(Error {
                scope: "workspace",
                code: "cancelled".into(),
                project_id: Some(id.into()),
            })
        },
    );
    assert_eq!(denied.unwrap_err().code, "cancelled");
    let socket = TcpListener::bind(("127.0.0.1", 0))?;
    let port = socket.local_addr()?.port();
    let config = root.path().join("Caddyfile");
    fs::write(
        &config,
        format!(
            "{{\n admin off\n auto_https off\n skip_install_trust\n persist_config off\n storage file_system {{\n root {}\n }}\n}}\nhttps://a.localhost:{port} {{\n bind 127.0.0.1\n tls {} {}\n respond \"owned-domain-tls\" 200\n}}\n",
            quoted(&root.path().join("caddy-data")),
            quoted(&leaf.certificate),
            quoted(&leaf.key)
        ),
    )?;
    let mut environment = BTreeMap::new();
    for name in ["SystemRoot", "WINDIR", "TEMP", "TMP", "TMPDIR"] {
        if let Some(value) = std::env::var_os(name) {
            environment.insert(name.into(), value);
        }
    }
    for name in [
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
    ] {
        let private = root.path().join(name);
        fs::create_dir(&private)?;
        environment.insert(name.into(), private.into_os_string());
    }
    let mut supervisor = Supervisor::open(root.path().join("logs"))?;
    drop(socket);
    supervisor.start(ProcessSpec {
        input: None,
        id: "domain-caddy".into(),
        executable,
        args: vec![
            "run".into(),
            "--config".into(),
            dunce::simplified(&config).as_os_str().into(),
            "--adapter".into(),
            "caddyfile".into(),
        ],
        directory: root.path().into(),
        environment,
        readiness: Readiness::Tcp {
            address: ([127, 0, 0, 1], port).into(),
        },
        readiness_timeout: Duration::from_secs(10),
        stop_grace: Duration::from_secs(1),
        restart: RestartPolicy::default(),
        logs: LogPolicy::default(),
    })?;
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let state = supervisor.snapshot("domain-caddy")?;
        if state.observed == Observed::Ready {
            break;
        }
        if state.observed == Observed::Failed || Instant::now() > until {
            return Err("owned Caddy did not become ready".into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .resolve("a.localhost", ([127, 0, 0, 1], port).into())
        .add_root_certificate(reqwest::Certificate::from_der(ca.der())?)
        .build()?;
    let response = client.get(format!("https://a.localhost:{port}/")).send()?;
    assert_eq!(response.status(), 200);
    assert_eq!(response.text()?, "owned-domain-tls");
    supervisor.stop("domain-caddy")?;
    fs::write(
        report,
        serde_json::to_vec_pretty(
            &serde_json::json!({"passed":true,"host":{"os":std::env::consts::OS,"arch":std::env::consts::ARCH},"caddy_package":package_id,"caddy_archive_sha256":descriptor.sha256,
        "copied_executable_verified":true,"observer_no_store_or_ca_creation":true,"denied_before_helper":true,"client_only_ca_tls_loopback":true,"owned_child_stopped":true,
        "system_hosts_read_or_write":false,"system_trust_read_or_write":false,"native_credentials":false,"php_execution":false,"source_cache_mutated":false}),
        )?,
    )?;
    println!("owned domain TLS and pre-helper denial passed");
    Ok(())
}
