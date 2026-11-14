#![cfg(feature = "e2e")]
//! Actual debug CLI composition; no OS credential provider or native window.
use devforge_core::SecretStore;
use devforge_manager::{
    Client, Endpoint,
    auth::{DesktopAuth, Policy, TrustedDesktopAction},
    bootstrap::BootstrapError,
    controller::local_owner_user,
    launch::{self, LaunchSpec},
};
use devforge_workspace::testing::{FIXTURE_MARKER, FixtureRoot, FixtureSecrets};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};
use zeroize::{Zeroize, Zeroizing};
fn parent() -> tempfile::TempDir {
    #[cfg(unix)]
    {
        tempfile::Builder::new()
            .prefix("df-cli-fixture-")
            .tempdir_in("/tmp")
            .unwrap()
    }
    #[cfg(not(unix))]
    {
        tempfile::tempdir().unwrap()
    }
}
fn command(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_devforge"));
    command.env("DEVFORGE_DATA_DIR", root);
    command
}
fn scrub(request: &mut Value, key: &str) {
    if let Some(Value::String(value)) = request.get_mut(key) {
        value.zeroize();
    }
}
fn session(reply: &mut Value) -> Zeroizing<String> {
    assert_eq!(reply["ok"], true);
    let session = Zeroizing::new(reply["data"]["session"].as_str().unwrap().to_owned());
    if let Some(Value::String(value)) = reply
        .get_mut("data")
        .and_then(|data| data.get_mut("session"))
    {
        value.zeroize();
    }
    session
}

#[test]
fn explicit_fixture_manager_rejects_missing_wrong_copied_roots_without_writes() {
    let parent = parent();
    let absent = parent.path().join("absent");
    let noenv = Command::new(env!("CARGO_BIN_EXE_devforge"))
        .env_remove("DEVFORGE_DATA_DIR")
        .args(["manager", "serve", "--internal-fixture-store"])
        .output()
        .unwrap();
    assert!(!noenv.status.success());
    assert!(
        String::from_utf8(noenv.stderr)
            .unwrap()
            .contains("fixture_data_root_required")
    );
    let bad = command(&absent)
        .args(["manager", "serve", "--internal-fixture-store"])
        .output()
        .unwrap();
    assert!(!bad.status.success());
    assert!(!absent.exists());
    let empty = parent.path().join("empty");
    fs::create_dir(&empty).unwrap();
    let bad = command(&empty)
        .args(["manager", "serve", "--internal-fixture-store"])
        .output()
        .unwrap();
    assert!(!bad.status.success());
    assert_eq!(fs::read_dir(&empty).unwrap().count(), 0);
    let source = FixtureRoot::create_new(&parent.path().join("source")).unwrap();
    let unflagged = command(source.root())
        .args(["manager", "serve", "--automatic"])
        .output()
        .unwrap();
    assert!(!unflagged.status.success());
    assert!(
        String::from_utf8(unflagged.stderr)
            .unwrap()
            .contains("fixture_explicit_flag_required")
    );
    let copied = FixtureRoot::create_new(&parent.path().join("copied")).unwrap();
    fs::copy(
        source.root().join(FIXTURE_MARKER),
        copied.root().join(FIXTURE_MARKER),
    )
    .unwrap();
    let before = fs::read(copied.root().join(FIXTURE_MARKER)).unwrap();
    let bad = command(copied.root())
        .args(["manager", "serve", "--internal-fixture-store"])
        .output()
        .unwrap();
    assert!(!bad.status.success());
    assert_eq!(
        fs::read(copied.root().join(FIXTURE_MARKER)).unwrap(),
        before
    );
    assert_eq!(fs::read_dir(copied.root()).unwrap().count(), 1);
    let shell = command(copied.root())
        .args(["project", "shell", "missing", "--internal-fixture-store"])
        .output()
        .unwrap();
    assert!(!shell.status.success());
    assert_eq!(fs::read_dir(copied.root()).unwrap().count(), 1);
}

#[test]
fn internal_fixture_init_creates_only_a_new_leaf_never_adopts_existing_canary_directory() {
    let parent = parent();
    let root = parent.path().join("fresh");
    let initialized = command(&root)
        .args(["manager", "__fixture-root-init", "--internal-fixture-store"])
        .output()
        .unwrap();
    assert!(initialized.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&initialized.stdout).unwrap(),
        json!({"initialized":true})
    );
    assert!(FixtureRoot::validate(&root).is_ok());
    let existing = parent.path().join("existing");
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("canary"), b"preserve-existing-data").unwrap();
    let rejected = command(&existing)
        .args(["manager", "__fixture-root-init", "--internal-fixture-store"])
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert_eq!(
        fs::read(existing.join("canary")).unwrap(),
        b"preserve-existing-data"
    );
    assert!(!existing.join(FIXTURE_MARKER).exists());
    assert_eq!(fs::read_dir(existing).unwrap().count(), 1);
}

#[test]
fn actual_private_bootstrap_and_warm_auth_share_live_locked_fixture_stores() {
    let parent = parent();
    let fixture = FixtureRoot::create_new(&parent.path().join("data")).unwrap();
    let gui_control = FixtureSecrets::open(fixture.control_store_path());
    let gui_project = FixtureSecrets::open(fixture.project_store_path());
    let project_path = fixture.root().join("fixture-project");
    fs::create_dir_all(project_path.join("public")).unwrap();
    fs::write(project_path.join("public/index.php"), "<?php").unwrap();
    fs::write(
        project_path.join("composer.json"),
        r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )
    .unwrap();
    let catalog = devforge_projects::project::Catalog::bundled();
    let target = devforge_runtime::package::Target::current().unwrap();
    let php = catalog
        .packages
        .iter()
        .find(|(_, package)| {
            package.kind == "php"
                && package.target == target
                && package.executables.contains_key("php")
        })
        .unwrap()
        .0
        .clone();
    let node = catalog
        .packages
        .iter()
        .find(|(_, package)| package.kind == "node" && package.target == target)
        .unwrap()
        .0
        .clone();
    let registry =
        devforge_projects::project::ProjectStore::open(fixture.root().join("projects")).unwrap();
    let project = registry
        .import(&project_path, "Fixture app", &php, &node, &catalog)
        .unwrap();
    drop(registry);
    let executable = PathBuf::from(env!("CARGO_BIN_EXE_devforge"));
    let mut file = fs::File::open(&executable).unwrap();
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer).unwrap();
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    let spec = LaunchSpec {
        executable,
        arguments: vec![
            "manager".into(),
            "serve".into(),
            "--automatic".into(),
            "--private-desktop-bootstrap".into(),
            "--internal-fixture-store".into(),
        ],
        data_root: fixture.root().to_owned(),
        environment: BTreeMap::new(),
    };
    let pending = match launch::spawn_with_private_bootstrap(&spec, &hash.finalize().into()) {
        Ok(value) => value,
        Err(error) => {
            #[cfg(windows)]
            if matches!(error, devforge_manager::Error::RestrictedJob) {
                eprintln!(
                    "CAPABILITY_UNAVAILABLE: restricted Windows parent job requires independent fixture launch"
                );
                return;
            }
            panic!("private fixture launch failed: {error}");
        }
    };
    let root = fixture.root().to_owned();
    let boot = pending
        .wait_ready(Duration::from_secs(10), move |pid| {
            let endpoint = Endpoint::existing(&root).map_err(|_| BootstrapError::NotReady)?;
            let (peer, reply) = Client::new(endpoint)
                .request_with_peer(&json!({"method":"health"}))
                .map_err(|_| BootstrapError::NotReady)?;
            if peer.process_id != Some(pid) {
                return Err(BootstrapError::NotReady);
            }
            Ok(reply["data"]["state"] == "ready")
        })
        .unwrap();
    let endpoint = Endpoint::existing(fixture.root()).unwrap();
    let client = Client::new(endpoint.clone());
    let (peer, _) = client
        .request_with_peer(&json!({"method":"health"}))
        .unwrap();
    let (bootstrap, monitor) = boot.into_parts();
    assert!(!fixture.control_store_path().exists());
    assert!(!fixture.project_store_path().exists());
    let hex = |bytes: &[u8]| {
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let mut request = json!({"method":"redeem_bootstrap","token":bootstrap.expose_secret(),"root_fingerprint":hex(bootstrap.root_fingerprint()),"manager_instance":hex(bootstrap.manager_instance())});
    let mut reply = client.request_with_expected_peer(&peer, &request).unwrap();
    scrub(&mut request, "token");
    let cold_session = session(&mut reply);
    drop(bootstrap);
    let mut snapshot = json!({"method":"snapshot","session":cold_session.as_str()});
    let response = client.request_with_expected_peer(&peer, &snapshot).unwrap();
    scrub(&mut snapshot, "session");
    assert_eq!(response["ok"], true);
    // A separate parent/GUI-side policy writes AFTER the manager constructed its
    // store. Warm auth must read the latest file rather than a stale cached map.
    let mut parent_auth =
        DesktopAuth::new(endpoint, local_owner_user().unwrap(), Policy::default()).unwrap();
    let credential = parent_auth
        .rotate_desktop_key(
            &TrustedDesktopAction::from_explicit_desktop_action(),
            &gui_control,
            Instant::now(),
        )
        .unwrap();
    let mut request = json!({"method":"authenticate","credential":credential.expose_secret()});
    let mut reply = client.request_with_expected_peer(&peer, &request).unwrap();
    scrub(&mut request, "credential");
    let warm_session = session(&mut reply);
    assert!(fixture.control_store_path().is_file());
    // GUI-side project fixture instance existed before a manager write too.
    // The fixed helper reloads under lock across processes.
    gui_project.set("parent-canary", "fixture-only").unwrap();
    let mut request = json!({"method":"desktop_access","session":warm_session.as_str(),"idempotency_key":"fixture-cross-process","request":{"kind":"configuration_secret","project_id":project.id,"reference":"gui-fixture","value":"fixture-cross-process-value"}});
    let response = client.request_with_expected_peer(&peer, &request).unwrap();
    scrub(&mut request, "session");
    if let Some(Value::String(value)) = request
        .get_mut("request")
        .and_then(|request| request.get_mut("value"))
    {
        value.zeroize();
    }
    assert_eq!(response["ok"], true);
    let operation = response["data"]["operation_id"].as_str().unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let mut request = json!({"method":"status","session":warm_session.as_str(),"project_id":project.id,"operation_id":operation});
        let response = client.request_with_expected_peer(&peer, &request).unwrap();
        scrub(&mut request, "session");
        if response["data"]["state"] == "succeeded" {
            break;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        gui_project.get("parent-canary").unwrap().as_deref(),
        Some("fixture-only")
    );
    let project_values: BTreeMap<String, String> =
        serde_json::from_slice(&fs::read(fixture.project_store_path()).unwrap()).unwrap();
    let key = project_values
        .iter()
        .find(|(_, value)| value.as_str() == "fixture-cross-process-value")
        .unwrap()
        .0;
    assert!(
        gui_project
            .get(key)
            .unwrap()
            .is_some_and(|value| value == "fixture-cross-process-value")
    );
    let mut request = json!({"method":"quit","session":warm_session.as_str()});
    let response = client.request_with_expected_peer(&peer, &request).unwrap();
    scrub(&mut request, "session");
    assert_eq!(response["ok"], true);
    assert!(monitor.wait_for_exit().unwrap().success());
}
