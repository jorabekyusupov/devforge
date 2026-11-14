//! CLI routing tests never touch native credentials or user application data.
use devforge_manager::{LegacyExclusion, Owner};
use std::{
    fs,
    process::{Command, Stdio},
};
fn command(root: &std::path::Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_devforge"));
    command.env("DEVFORGE_DATA_DIR", root);
    command
}
fn short_root() -> tempfile::TempDir {
    #[cfg(unix)]
    {
        tempfile::Builder::new()
            .prefix("df-cli-")
            .tempdir_in("/tmp")
            .unwrap()
    }
    #[cfg(not(unix))]
    {
        tempfile::tempdir().unwrap()
    }
}
#[test]
fn help_has_no_filesystem_or_credential_bootstrap() {
    let temp = short_root();
    let root = temp.path().join("absent");
    for args in [vec!["--help"], vec!["manager", "--help"]] {
        let output = command(&root).args(args).output().unwrap();
        assert!(output.status.success());
        assert!(serde_json::from_slice::<serde_json::Value>(&output.stdout).is_ok());
        assert!(!root.exists());
    }
}
#[test]
fn missing_owner_health_does_not_create_data_root() {
    let temp = short_root();
    let root = temp.path().join("absent");
    let output = command(&root).args(["manager", "health"]).output().unwrap();
    assert!(!output.status.success());
    assert!(!root.exists());
}
#[test]
fn private_bootstrap_cannot_emit_credentials_to_disk_or_null() {
    let temp = short_root();
    let root = temp.path().join("absent");
    let target = temp.path().join("output");
    let status = command(&root)
        .args(["manager", "serve", "--private-desktop-bootstrap"])
        .stdout(fs::File::create(&target).unwrap())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!status.success());
    assert_eq!(fs::metadata(&target).unwrap().len(), 0);
    assert!(!root.exists());
    let status = command(&root)
        .args(["manager", "serve", "--private-desktop-bootstrap"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!status.success());
    assert!(!root.exists());
}
#[test]
fn invalid_manager_flags_fail_before_data_root_changes() {
    let temp = short_root();
    let root = temp.path().join("absent");
    for args in [
        vec!["manager", "serve", "--automatic", "--automatic"],
        vec!["manager", "serve", "--role", "desktop"],
        vec!["manager", "missing"],
    ] {
        let output = command(&root).args(args).output().unwrap();
        assert!(!output.status.success());
        let text = String::from_utf8(output.stderr).unwrap();
        assert!(text.contains("manager_invalid_arguments"));
        assert!(!root.exists());
    }
}
#[cfg(unix)]
#[test]
fn legacy_mutation_is_excluded_but_metadata_catalog_remains_available() {
    let temp = short_root();
    let _owner = Owner::acquire(temp.path()).unwrap();
    let blocked = command(temp.path())
        .args(["runtime", "remove", "absent"])
        .output()
        .unwrap();
    assert!(!blocked.status.success());
    assert!(
        String::from_utf8(blocked.stderr)
            .unwrap()
            .contains("manager_active_use_control_api")
    );
    let catalog = command(temp.path())
        .args(["runtime", "catalog"])
        .output()
        .unwrap();
    assert!(catalog.status.success());
}
#[cfg(unix)]
#[test]
fn legacy_guard_does_not_self_conflict_with_config_exclusive_lease() {
    let temp = short_root();
    let _legacy = LegacyExclusion::acquire(temp.path()).unwrap();
    assert!(devforge_platform::distribution::OperationLease::exclusive(temp.path()).is_ok());
    assert!(Owner::acquire(temp.path()).is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn actual_cli_private_cold_start_health_and_orderly_quit() {
    use devforge_manager::{
        Client, Endpoint,
        bootstrap::BootstrapError,
        launch::{self, LaunchSpec},
    };
    use sha2::{Digest, Sha256};
    use std::{collections::BTreeMap, io::Read, time::Duration};
    use zeroize::{Zeroize, Zeroizing};
    let temp = short_root();
    let exe = std::path::PathBuf::from(env!("CARGO_BIN_EXE_devforge"));
    let mut source = fs::File::open(&exe).unwrap();
    let mut hash = Sha256::new();
    let mut chunk = [0; 65536];
    loop {
        let n = source.read(&mut chunk).unwrap();
        if n == 0 {
            break;
        }
        hash.update(&chunk[..n]);
    }
    let expected: [u8; 32] = hash.finalize().into();
    let spec = LaunchSpec {
        executable: exe,
        arguments: vec![
            "manager".into(),
            "serve".into(),
            "--automatic".into(),
            "--private-desktop-bootstrap".into(),
        ],
        data_root: temp.path().to_owned(),
        environment: BTreeMap::new(),
    };
    let pending = launch::spawn_with_private_bootstrap(&spec, &expected).unwrap();
    let root = temp.path().to_owned();
    let boot = pending
        .wait_ready(Duration::from_secs(10), move |pid| {
            let endpoint = Endpoint::existing(&root).map_err(|_| BootstrapError::NotReady)?;
            let (peer, reply) = Client::new(endpoint)
                .request_with_peer(&serde_json::json!({"method":"health"}))
                .map_err(|_| BootstrapError::NotReady)?;
            if peer.process_id != Some(pid) {
                return Err(BootstrapError::NotReady);
            }
            Ok(reply["data"]["state"] == "ready")
        })
        .unwrap();
    let client = Client::new(Endpoint::existing(temp.path()).unwrap());
    let public = command(temp.path())
        .args(["manager", "health"])
        .output()
        .unwrap();
    assert!(public.status.success());
    let public: serde_json::Value = serde_json::from_slice(&public.stdout).unwrap();
    assert_eq!(public["data"]["state"], "ready");
    let credential = boot.credential();
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let mut request = serde_json::json!({"method":"redeem_bootstrap","token":credential.expose_secret(),"root_fingerprint":hex(credential.root_fingerprint()),"manager_instance":hex(credential.manager_instance())});
    let mut reply = client.request(&request).unwrap();
    if let Some(serde_json::Value::String(token)) = request.get_mut("token") {
        token.zeroize();
    }
    assert_eq!(reply["ok"], true);
    let session = Zeroizing::new(reply["data"]["session"].as_str().unwrap().to_owned());
    if let Some(serde_json::Value::String(token)) =
        reply.get_mut("data").and_then(|v| v.get_mut("session"))
    {
        token.zeroize();
    }
    let mut quit = serde_json::json!({"method":"quit","session":session.as_str()});
    let reply = client.request(&quit).unwrap();
    if let Some(serde_json::Value::String(token)) = quit.get_mut("session") {
        token.zeroize();
    }
    assert_eq!(reply["ok"], true);
    assert!(boot.wait_for_exit().unwrap().success());
    let output = command(temp.path())
        .args(["manager", "serve", "--automatic"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("controller_start_inhibited")
    );
}
