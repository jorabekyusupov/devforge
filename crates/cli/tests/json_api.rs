//! JSON CLI metadata/parse checks must not create a data root or probe secrets.
use serde_json::Value;
use std::process::{Command, Stdio};
fn invoke(
    args: &[&str],
) -> (
    tempfile::TempDir,
    std::path::PathBuf,
    std::process::Output,
    Value,
) {
    let container = tempfile::tempdir().unwrap();
    let absent = container.path().join("absent");
    let output = Command::new(env!("CARGO_BIN_EXE_devforge"))
        .env("DEVFORGE_DATA_DIR", &absent)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&output.stdout).expect("exact stdout JSON envelope");
    assert!(output.stderr.is_empty());
    assert_eq!(value["api_version"], 1);
    assert!(value["warnings"].is_array());
    assert!(
        !absent.exists(),
        "metadata/failed connect must not create state"
    );
    (container, absent, output, value)
}
#[test]
fn all_metadata_help_and_schema_routes_are_side_effect_free() {
    for args in [
        vec!["api"],
        vec!["api", "--help"],
        vec!["api", "health", "--help"],
        vec!["api", "status", "--help"],
        vec!["api", "project", "--help"],
        vec!["api", "project", "list", "--help"],
        vec!["api", "project", "inspect", "--help"],
        vec!["api", "operation", "--help"],
        vec!["api", "operation", "status", "--help"],
        vec!["api", "operation", "cancel", "--help"],
        vec!["api", "schema"],
        vec!["api", "capabilities"],
    ] {
        let (_, _, output, value) = invoke(&args);
        assert!(output.status.success(), "{args:?}");
        assert_eq!(value["ok"], true);
        assert!(value.get("error").is_none());
    }
    let (_, _, _, schema) = invoke(&["api", "schema"]);
    assert!(schema["data"]["response"]["oneOf"].is_array());
    let (_, _, _, capabilities) = invoke(&["api", "capabilities"]);
    assert_eq!(capabilities["data"]["owner_autostart"], false);
    assert_eq!(capabilities["data"]["planning_inventory_is_live"], false);
}
#[test]
fn parse_errors_are_versioned_redacted_and_have_stable_exit_codes() {
    for args in [
        vec!["api", "--api-version", "9", "health"],
        vec!["api", "--request-id", "invalid/path", "health"],
        vec![
            "api",
            "--request-id",
            "one",
            "--request-id",
            "two",
            "health",
        ],
        vec!["api", "--credential", "secret-token-canary", "status"],
        vec!["api", "schema", "--credential-stdin"],
    ] {
        let (_, _, output, value) = invoke(&args);
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(value["ok"], false);
        assert!(value.get("data").is_none());
        assert!(
            !String::from_utf8(output.stdout)
                .unwrap()
                .contains("secret-token-canary")
        );
    }
    let (_, _, _, value) = invoke(&["api", "--request-id", "correlation-42", "--json", "schema"]);
    assert_eq!(value["request_id"], "correlation-42");
}

#[test]
fn legacy_help_precedes_all_store_and_owner_initialization() {
    let container = tempfile::tempdir().unwrap();
    let absent = container.path().join("absent");
    for top in [
        "project",
        "runtime",
        "site",
        "inspect",
        "debug",
        "config",
        "workspace",
        "distribution",
        "doctor",
    ] {
        for tail in [vec!["--help"], vec!["list", "--help"]] {
            let output = Command::new(env!("CARGO_BIN_EXE_devforge"))
                .env("DEVFORGE_DATA_DIR", &absent)
                .arg(top)
                .args(tail)
                .output()
                .unwrap();
            assert!(output.status.success(), "{top}");
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["mode"], "legacy");
            assert!(!absent.exists());
        }
    }
}

#[cfg(feature = "test-support")]
#[test]
fn real_manager_grants_limit_json_cli_without_a_gui_or_native_store() {
    use devforge_control::{Id, ProjectBinding, Scope};
    use devforge_core::{ErrorCode, SecretStore};
    use devforge_manager::{
        Owner, PeerIdentity,
        auth::{DesktopAuth, Policy, TrustedDesktopAction},
        controller::{Controller, StartIntent, Stores, local_owner_user},
        external_auth::ExternalRegistry,
    };
    use devforge_projects::project::{Catalog, ProjectStore};
    use devforge_runtime::package::Target;
    use devforge_workspace::testing::FixtureRoot;
    use std::{
        collections::BTreeMap,
        fs,
        io::Write,
        sync::{Arc, Mutex},
        thread,
        time::{Duration, Instant},
    };
    #[derive(Default)]
    struct Memory(Mutex<BTreeMap<String, String>>);
    impl SecretStore for Memory {
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
    struct Deny;
    impl SecretStore for Deny {
        fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
            panic!("no native/workspace store reads")
        }
        fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
            panic!("no native/workspace store writes")
        }
        fn delete(&self, _: &str) -> Result<(), ErrorCode> {
            panic!("no native/workspace store deletes")
        }
    }
    let container = {
        #[cfg(unix)]
        {
            tempfile::Builder::new()
                .prefix("df-api-")
                .tempdir_in("/tmp")
                .unwrap()
        }
        #[cfg(not(unix))]
        {
            tempfile::tempdir().unwrap()
        }
    };
    let fixture = FixtureRoot::create_new(&container.path().join("data")).unwrap();
    let root = fixture.root();
    let catalog = Catalog::bundled();
    let target = Target::current().unwrap();
    let php = catalog
        .packages
        .iter()
        .find(|(id, p)| {
            p.kind == "php"
                && p.variant == "cli"
                && p.target == target
                && !catalog.superseded.contains_key(*id)
        })
        .unwrap()
        .0;
    let node = catalog
        .packages
        .iter()
        .find(|(id, p)| {
            p.kind == "node" && p.target == target && !catalog.superseded.contains_key(*id)
        })
        .unwrap()
        .0;
    let projects = ProjectStore::open(root.join("projects")).unwrap();
    let mut rows = vec![];
    for name in ["Allowed", "other-project-private-canary"] {
        let path = container.path().join(name);
        fs::create_dir_all(path.join("public")).unwrap();
        fs::write(path.join("public/index.php"), "<?php").unwrap();
        fs::write(
            path.join("composer.json"),
            br#"{"require":{"laravel/framework":"^13.0","php":">=8.0"}}"#,
        )
        .unwrap();
        rows.push(projects.import(&path, name, php, node, &catalog).unwrap());
    }
    drop(projects);
    let owner = Owner::acquire(root).unwrap();
    let peer = PeerIdentity {
        user: local_owner_user().unwrap(),
        process_id: Some(std::process::id()),
    };
    let mut auth = DesktopAuth::new(
        owner.endpoint().clone(),
        peer.user.clone(),
        Policy::default(),
    )
    .unwrap();
    let store = Memory::default();
    let key = auth
        .rotate_desktop_key(
            &TrustedDesktopAction::from_explicit_desktop_action(),
            &store,
            Instant::now(),
        )
        .unwrap();
    let session = auth
        .authenticate(&peer, &key, &store, Instant::now())
        .unwrap();
    let proof = auth.authorize(&peer, &session, Instant::now()).unwrap();
    let binding = ProjectBinding::from_registered(
        Id::new(&rows[0].id).unwrap(),
        &rows[0].path,
        rows[0].revision,
    )
    .unwrap();
    let mut grants = ExternalRegistry::open(&owner).unwrap();
    let allowed = grants
        .issue(
            &proof,
            &binding,
            &[Scope::Observer],
            &[],
            Duration::from_secs(60),
        )
        .unwrap();
    let revoked = grants
        .issue(
            &proof,
            &binding,
            &[Scope::Observer],
            &[],
            Duration::from_secs(60),
        )
        .unwrap();
    grants.revoke(&proof, &revoked.record.grant_id).unwrap();
    drop(grants);
    drop(auth);
    drop(owner);
    let controller = Controller::start(
        root,
        StartIntent::ExplicitUserStart,
        Stores {
            workspace: Arc::new(Deny),
            desktop: Arc::new(Deny),
        },
    )
    .unwrap();
    let quit = controller.quitter();
    let endpoint = controller.endpoint().clone();
    let worker = thread::spawn(move || controller.serve(None));
    let started = Instant::now();
    while devforge_manager::Client::new(endpoint.clone())
        .request(&serde_json::json!({"method":"health"}))
        .is_err()
    {
        assert!(started.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(10));
    }
    let run = |args: &[&str], token: Option<&str>| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_devforge"))
            .env("DEVFORGE_DATA_DIR", root)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if let Some(token) = token {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(token.as_bytes())
                .unwrap();
        } else {
            drop(child.stdin.take());
        }
        let output = child.wait_with_output().unwrap();
        assert!(output.stderr.is_empty());
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(!text.contains("other-project-private-canary"));
        assert!(!text.contains(root.to_str().unwrap()));
        if let Some(token) = token {
            assert!(!text.contains(token));
        }
        (
            output.status.code().unwrap(),
            serde_json::from_str::<Value>(&text).unwrap(),
        )
    };
    assert_eq!(run(&["api", "health"], None).0, 0);
    for args in [
        vec!["api", "status", "--credential-stdin"],
        vec!["api", "project", "list", "--credential-stdin"],
        vec![
            "api",
            "project",
            "inspect",
            &rows[0].id,
            "--credential-stdin",
        ],
    ] {
        let (code, value) = run(&args, Some(allowed.credential.expose_secret()));
        assert_eq!(code, 0);
        assert_eq!(value["ok"], true);
        if args[2] == "list" {
            assert_eq!(value["data"]["data"]["items"].as_array().unwrap().len(), 1);
        }
    }
    let (code, value) = run(
        &[
            "api",
            "project",
            "inspect",
            &rows[1].id,
            "--credential-stdin",
        ],
        Some(allowed.credential.expose_secret()),
    );
    assert_eq!(code, 3);
    assert_eq!(value["error"]["code"], "project_mismatch");
    let (code, value) = run(
        &["api", "status", "--credential-stdin"],
        Some(revoked.credential.expose_secret()),
    );
    assert_eq!(code, 3);
    assert_eq!(value["error"]["code"], "grant_revoked");
    for action in ["status", "cancel"] {
        let (code, value) = run(
            &[
                "api",
                "operation",
                action,
                "not-owned-operation",
                "--project",
                &rows[0].id,
                "--credential-stdin",
            ],
            Some(allowed.credential.expose_secret()),
        );
        assert_eq!(code, 3);
        assert_eq!(value["error"]["code"], "unauthorized");
    }
    quit.request_explicit().unwrap();
    worker.join().unwrap().unwrap();
}

#[test]
fn invalid_or_missing_credentials_and_missing_owner_never_initialize_state() {
    use std::io::Write;
    for args in [
        vec!["api", "status"],
        vec!["api", "status", "--credential-stdin"],
    ] {
        let (_, _, output, value) = invoke(&args);
        assert_eq!(output.status.code(), Some(3));
        assert_eq!(value["error"]["code"], "unauthorized");
    }
    let (_, _, output, value) = invoke(&["api", "health"]);
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(value["error"]["code"], "storage_unavailable");
    let (_, _, output, value) = invoke(&["api", "unknown"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(value["error"]["code"], "invalid_input");
    let container = tempfile::tempdir().unwrap();
    let absent = container.path().join("absent");
    for input in [
        "a".repeat(63),
        "g".repeat(64),
        "a".repeat(67),
        "a".repeat(64) + "\n\nx",
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_devforge"))
            .env("DEVFORGE_DATA_DIR", &absent)
            .args(["api", "status", "--credential-stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.status.code(), Some(3));
        assert!(output.stderr.is_empty());
        assert!(!absent.exists());
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["error"]["code"], "unauthorized");
        assert!(!String::from_utf8(output.stdout).unwrap().contains(&input));
    }
}

#[test]
fn service_metadata_requires_exact_intent_and_never_treats_flags_as_approval() {
    let (_, _, help, value) = invoke(&["api", "service", "plan", "--help"]);
    assert!(help.status.success());
    assert_eq!(value["ok"], true);
    for args in [
        vec![
            "api",
            "service",
            "start",
            "project",
            "--revision",
            "1",
            "--idempotency-key",
            "key",
        ],
        vec![
            "api",
            "service",
            "plan",
            "install",
            "project",
            "--revision",
            "1",
            "--idempotency-key",
            "key",
        ],
        vec![
            "api",
            "service",
            "plan",
            "stop",
            "project",
            "--idempotency-key",
            "key",
        ],
        vec![
            "api",
            "service",
            "plan",
            "stop",
            "project",
            "--revision",
            "1",
        ],
        vec![
            "api",
            "service",
            "plan",
            "stop",
            "project",
            "--revision",
            "1",
            "--idempotency-key",
            "key",
            "--trusted",
        ],
        vec!["api", "status", "--revision", "1"],
        vec!["api", "operation", "lookup", "key"],
    ] {
        let (_, _, output, value) = invoke(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert_eq!(value["error"]["code"], "invalid_input");
    }
    let (_, _, _, capabilities) = invoke(&["api", "capabilities"]);
    let commands = capabilities["data"]["commands"].as_array().unwrap();
    let start = commands
        .iter()
        .find(|entry| entry["name"] == "service start")
        .unwrap();
    assert_eq!(
        start["required_scopes"],
        serde_json::json!(["project_manage", "execution_test"])
    );
    assert_eq!(start["execution_requires_native_approval"], true);
    let stop = commands
        .iter()
        .find(|entry| entry["name"] == "service stop")
        .unwrap();
    assert_eq!(
        stop["required_scopes"],
        serde_json::json!(["project_manage"])
    );
}

#[test]
fn runtime_metadata_is_finite_and_select_flags_cannot_modify_other_configuration() {
    for args in [
        vec!["api", "runtime", "--help"],
        vec!["api", "runtime", "plan", "--help"],
        vec!["api", "runtime", "install", "--help"],
        vec!["api", "runtime", "select", "--help"],
    ] {
        let (_, _, output, value) = invoke(&args);
        assert!(output.status.success());
        assert_eq!(value["ok"], true);
    }
    for args in [
        vec![
            "api",
            "runtime",
            "remove",
            "project",
            "--revision",
            "1",
            "--idempotency-key",
            "key",
        ],
        vec![
            "api",
            "runtime",
            "probe",
            "project",
            "--revision",
            "1",
            "--idempotency-key",
            "key",
        ],
        vec![
            "api",
            "runtime",
            "plan",
            "install",
            "project",
            "--php",
            "php",
            "--revision",
            "1",
            "--idempotency-key",
            "key",
        ],
        vec![
            "api",
            "runtime",
            "plan",
            "select",
            "project",
            "--php",
            "php",
            "--node",
            "node",
            "--name",
            "changed",
            "--revision",
            "1",
            "--idempotency-key",
            "key",
        ],
        vec![
            "api",
            "runtime",
            "plan",
            "select",
            "project",
            "--php",
            "php",
            "--node",
            "node",
            "--package",
            "package",
            "--revision",
            "1",
            "--idempotency-key",
            "key",
        ],
        vec![
            "api",
            "runtime",
            "select",
            "project",
            "--php",
            "php",
            "--node",
            "node",
            "--revision",
            "1",
            "--idempotency-key",
            "key",
        ],
        vec!["api", "status", "--package", "package"],
    ] {
        let (_, _, output, value) = invoke(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert_eq!(value["error"]["code"], "invalid_input");
    }
    let (_, _, _, capabilities) = invoke(&["api", "capabilities"]);
    let commands = capabilities["data"]["commands"].as_array().unwrap();
    let install = commands
        .iter()
        .find(|entry| entry["name"] == "runtime install")
        .unwrap();
    assert_eq!(
        install["required_scopes"],
        serde_json::json!(["project_manage"])
    );
    assert_eq!(
        install["effect"],
        "reviewed_shared_cache_write_without_pin_switch"
    );
    let select = commands
        .iter()
        .find(|entry| entry["name"] == "runtime select")
        .unwrap();
    assert_eq!(
        select["required_scopes"],
        serde_json::json!(["project_manage", "execution_test"])
    );
    assert_eq!(select["execution_requires_native_approval"], true);
}

#[test]
fn environment_metadata_keeps_yaml_off_argv_and_credentials_off_payload_stdin() {
    for args in [
        vec!["api", "env", "--help"],
        vec!["api", "env", "plan", "--help"],
        vec!["api", "env", "apply", "--help"],
        vec!["api", "env", "yaml-save", "--help"],
    ] {
        let (_, _, output, value) = invoke(&args);
        assert!(output.status.success());
        assert_eq!(value["ok"], true);
    }
    for args in [
        vec![
            "api",
            "env",
            "yaml-save",
            "project",
            "--revision",
            "1",
            "--idempotency-key",
            "key",
            "--review",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ],
        vec![
            "api",
            "env",
            "plan",
            "yaml-save",
            "project",
            "--revision",
            "1",
            "--idempotency-key",
            "key",
            "--input-stdin",
            "--credential-stdin",
        ],
        vec![
            "api",
            "env",
            "apply",
            "project",
            "--revision",
            "1",
            "--idempotency-key",
            "key",
            "--review",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--input-stdin",
        ],
        vec![
            "api",
            "env",
            "yaml-save",
            "project",
            "--yaml-file",
            "/tmp/not-accepted",
        ],
        vec![
            "api",
            "env",
            "yaml-save",
            "project",
            "--text",
            "secret-argv-canary",
        ],
        vec![
            "api",
            "service",
            "plan",
            "stop",
            "project",
            "--revision",
            "1",
            "--idempotency-key",
            "key",
            "--credential-file",
            "/tmp/not-read",
        ],
    ] {
        let (_, _, output, value) = invoke(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert_eq!(value["error"]["code"], "invalid_input");
        assert!(
            !String::from_utf8(output.stdout)
                .unwrap()
                .contains("secret-argv-canary")
        );
    }
    let (_, _, _, capabilities) = invoke(&["api", "capabilities"]);
    let commands = capabilities["data"]["commands"].as_array().unwrap();
    assert_eq!(
        commands
            .iter()
            .find(|entry| entry["name"] == "env apply")
            .unwrap()["required_scopes"],
        serde_json::json!(["project_manage", "sensitive_operation", "execution_test"])
    );
    assert_eq!(
        commands
            .iter()
            .find(|entry| entry["name"] == "env yaml-save")
            .unwrap()["required_scopes"],
        serde_json::json!(["project_manage", "sensitive_operation"])
    );
}

#[test]
fn database_metadata_and_parser_limit_actions_to_scoped_bind_restore() {
    for args in [
        vec!["api", "database", "--help"],
        vec!["api", "database", "plan", "--help"],
        vec!["api", "database", "bind", "--help"],
        vec!["api", "database", "restore", "--help"],
    ] {
        let (_, _, output, value) = invoke(&args);
        assert!(output.status.success());
        assert_eq!(value["ok"], true);
    }
    for args in [
        vec![
            "api",
            "database",
            "plan",
            "clone",
            "project",
            "--revision",
            "1",
            "--idempotency-key",
            "key",
        ],
        vec![
            "api",
            "database",
            "query",
            "project",
            "--sql",
            "private-sql-canary",
        ],
        vec![
            "api",
            "database",
            "plan",
            "bind",
            "project",
            "--backup",
            "backup",
            "--revision",
            "1",
            "--idempotency-key",
            "key",
        ],
        vec![
            "api",
            "database",
            "restore",
            "project",
            "--backup",
            "backup",
            "--revision",
            "1",
            "--idempotency-key",
            "key",
        ],
        vec![
            "api",
            "database",
            "plan",
            "restore",
            "project",
            "--backup",
            "backup",
            "--revision",
            "1",
            "--idempotency-key",
            "key",
            "--path",
            "/tmp/not-accepted",
        ],
        vec![
            "api",
            "database",
            "plan",
            "project",
            "--instance",
            "unscoped",
        ],
        vec!["api", "status", "--backup", "backup"],
    ] {
        let (_, _, output, value) = invoke(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert_eq!(value["error"]["code"], "invalid_input");
        assert!(
            !String::from_utf8(output.stdout)
                .unwrap()
                .contains("private-sql-canary")
        );
    }
    let (_, _, _, capabilities) = invoke(&["api", "capabilities"]);
    let commands = capabilities["data"]["commands"].as_array().unwrap();
    for name in ["database bind", "database restore"] {
        let entry = commands.iter().find(|entry| entry["name"] == name).unwrap();
        assert_eq!(
            entry["required_scopes"],
            serde_json::json!(["project_manage", "sensitive_operation"])
        );
        assert_eq!(entry["execution_requires_native_approval"], true);
    }
}

#[test]
fn domain_metadata_and_parser_never_accept_confirmation_elevation_or_new_configuration() {
    for args in [
        vec!["api", "domain", "--help"],
        vec!["api", "domain", "hosts-plan", "--help"],
        vec!["api", "domain", "trust-plan", "--help"],
        vec!["api", "domain", "cleanup", "--help"],
    ] {
        let (_, _, output, value) = invoke(&args);
        assert!(output.status.success());
        assert_eq!(value["ok"], true);
    }
    for args in [
        vec![
            "api",
            "domain",
            "configure",
            "project",
            "--domain",
            "other.devforge.test",
        ],
        vec!["api", "domain", "apply", "project", "--confirm"],
        vec!["api", "domain", "apply", "project", "--elevate"],
        vec!["api", "domain", "cleanup", "apply", "project", "--consent"],
        vec![
            "api",
            "domain",
            "hosts-plan",
            "project",
            "--helper",
            "/tmp/not-accepted",
        ],
        vec![
            "api",
            "domain",
            "cleanup",
            "project",
            "--plan",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ],
        vec![
            "api",
            "status",
            "--plan",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ],
    ] {
        let (_, _, output, value) = invoke(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert_eq!(value["error"]["code"], "invalid_input");
    }
    let (_, _, _, capabilities) = invoke(&["api", "capabilities"]);
    let commands = capabilities["data"]["commands"].as_array().unwrap();
    for name in ["domain apply", "domain cleanup apply"] {
        let entry = commands.iter().find(|entry| entry["name"] == name).unwrap();
        assert_eq!(
            entry["required_scopes"],
            serde_json::json!(["project_manage", "sensitive_operation"])
        );
        assert_eq!(entry["native_machine_confirmation_required"], true);
    }
}

#[test]
fn diagnostics_cli_metadata_filters_and_page_bounds_reject_before_state_or_io() {
    for args in [
        vec!["api", "health", "doctor", "--help"],
        vec!["api", "logs", "search", "--help"],
        vec!["api", "inspector", "request", "--help"],
        vec!["api", "mail", "list", "--help"],
    ] {
        let (_, _, output, value) = invoke(&args);
        assert!(output.status.success());
        assert_eq!(value["ok"], true);
    }
    for args in [
        vec!["api", "logs", "search", "project", "--query", ""],
        vec!["api", "mail", "list", "project", "--subject", ""],
        vec![
            "api",
            "mail",
            "list",
            "project",
            "--recipient",
            "private-filter-canary",
        ],
        vec!["api", "logs", "search", "project", "--limit", "0"],
        vec!["api", "logs", "search", "project", "--limit", "501"],
        vec!["api", "mail", "list", "project", "--max-bytes", "262145"],
        vec!["api", "mail", "list", "project", "--timeout-ms", "30001"],
        vec![
            "api",
            "logs",
            "search",
            "project",
            "--cursor",
            "{\"offset\":1}",
        ],
        vec!["api", "health", "--level", "info"],
        vec!["api", "status", "--limit", "1"],
    ] {
        let (_, _, output, value) = invoke(&args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert_eq!(value["error"]["code"], "invalid_input");
        assert!(
            !String::from_utf8(output.stdout)
                .unwrap()
                .contains("private-filter-canary")
        );
    }
    let (_, _, _, capabilities) = invoke(&["api", "capabilities"]);
    let commands = capabilities["data"]["commands"].as_array().unwrap();
    for name in [
        "health doctor",
        "logs search",
        "inspector request",
        "mail list",
    ] {
        let entry = commands.iter().find(|entry| entry["name"] == name).unwrap();
        assert_eq!(entry["required_scopes"], serde_json::json!(["observer"]));
        assert_eq!(entry["metadata_only"], true);
    }
}
