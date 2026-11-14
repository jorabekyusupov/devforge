//! Custom harness keeps Rust libtest banners out of the private child pipe.
//! Every fixture store is denied/in-memory; no native credential API is opened.
mod common;
use devforge_core::{ErrorCode, SecretStore};
use devforge_manager::{
    Client, Endpoint, Error,
    bootstrap::{BootstrapError, BootstrapWriter},
    controller::{Controller, StartIntent, Stores},
    launch::{self, LaunchSpec},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};
use zeroize::{Zeroize, Zeroizing};

struct DenyStore;
impl SecretStore for DenyStore {
    fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
        Err(ErrorCode::SecretUnavailable)
    }
    fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
        Err(ErrorCode::SecretUnavailable)
    }
    fn delete(&self, _: &str) -> Result<(), ErrorCode> {
        Err(ErrorCode::SecretUnavailable)
    }
}
struct NoProbeStore;
impl SecretStore for NoProbeStore {
    fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
        panic!("private bootstrap must not probe native store")
    }
    fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
        panic!("private bootstrap must not set native store")
    }
    fn delete(&self, _: &str) -> Result<(), ErrorCode> {
        panic!("private bootstrap must not delete native store")
    }
}
fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|a| a == "fixture") {
        fixture(args[1].to_str().unwrap(), Path::new(&args[2]));
        return;
    }
    real_private_channel_and_one_use_redemption();
    inherited_channel_rejects_files_and_null();
    mandatory_executable_digest_and_unready_cleanup();
    malformed_frame_is_bounded_and_cleaned();
    timeout_does_not_kill_an_observed_ready_owner();
    println!("bootstrap focused checks passed (5)");
}
fn fixture(mode: &str, root: &Path) {
    match mode {
        "sleep" => {
            std::thread::sleep(Duration::from_secs(60));
        }
        "unready" => {
            std::thread::sleep(Duration::from_secs(60));
        }
        "malformed" => {
            std::io::stdout().write_all(&[0; 139]).unwrap();
        }
        "reject" => {
            assert_eq!(
                BootstrapWriter::capture_inherited_stdout().unwrap_err(),
                BootstrapError::PipeRequired
            );
        }
        "ready" => {
            let writer = BootstrapWriter::capture_inherited_stdout().unwrap();
            // The retained writer MUST NOT survive exec in this fixture child.
            // Its lifetime would otherwise prevent the parent's frame+EOF read.
            let mut guard = Command::new(std::env::current_exe().unwrap())
                .args(["fixture", "sleep"])
                .arg(root)
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let controller = Controller::start(
                root,
                StartIntent::ExplicitUserStart,
                Stores {
                    workspace: Arc::new(DenyStore),
                    desktop: Arc::new(NoProbeStore),
                },
            )
            .unwrap();
            controller.serve(Some(writer)).unwrap();
            guard.kill().unwrap();
            guard.wait().unwrap();
            fs::write(root.join("stdout-guard"), b"untouched").unwrap();
            let _file = fs::OpenOptions::new()
                .append(true)
                .open(root.join("stdout-guard"))
                .unwrap();
            println!("this must go only to NUL, never the pipe or an opened data file");
            assert_eq!(fs::read(root.join("stdout-guard")).unwrap(), b"untouched");
        }
        "ready-no-frame" => {
            let _writer = BootstrapWriter::capture_inherited_stdout().unwrap();
            let controller = Controller::start(
                root,
                StartIntent::ExplicitUserStart,
                Stores {
                    workspace: Arc::new(DenyStore),
                    desktop: Arc::new(NoProbeStore),
                },
            )
            .unwrap();
            // Deliberate fixture self-exit only, never production owner cleanup.
            // The parent must leave an authenticated ready owner untouched.
            let marker = root.join("ready-survived");
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(2));
                fs::write(marker, b"alive").unwrap();
                std::process::exit(0);
            });
            controller.serve(None).unwrap();
        }
        _ => panic!("unknown isolated fixture"),
    }
}
fn root() -> tempfile::TempDir {
    common::root("df-boot-")
}
fn spec(root: &Path, mode: &str) -> LaunchSpec {
    let mut environment = BTreeMap::new();
    if let Some(value) = std::env::var_os("SystemRoot") {
        environment.insert("SystemRoot".into(), value);
    }
    LaunchSpec {
        executable: std::env::current_exe().unwrap(),
        arguments: vec!["fixture".into(), mode.into(), root.as_os_str().to_owned()],
        data_root: root.to_owned(),
        environment,
    }
}
fn digest(path: &Path) -> [u8; 32] {
    let mut file = fs::File::open(path).unwrap();
    let mut hash = Sha256::new();
    let mut bytes = [0; 65536];
    loop {
        let count = file.read(&mut bytes).unwrap();
        if count == 0 {
            break;
        }
        hash.update(&bytes[..count]);
    }
    hash.finalize().into()
}
fn spawn(root: &Path, mode: &str) -> Option<launch::PendingBootstrapChild> {
    let spec = spec(root, mode);
    match launch::spawn_with_private_bootstrap(&spec, &digest(&spec.executable)) {
        Ok(child) => Some(child),
        Err(Error::RestrictedJob) => {
            println!(
                "CAPABILITY_UNAVAILABLE: Windows ancestor job forbids private detached launch"
            );
            None
        }
        Err(error) => panic!("private fixture launch failed: {error}"),
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn request(client: &Client, mut request: Value) -> Value {
    let result = client.request(&request);
    if let Some(fields) = request.as_object_mut() {
        for key in ["token", "session"] {
            if let Some(Value::String(value)) = fields.get_mut(key) {
                value.zeroize();
            }
        }
    }
    result.unwrap()
}
fn real_private_channel_and_one_use_redemption() {
    let root = root();
    let Some(child) = spawn(root.path(), "ready") else {
        return;
    };
    let ready = child
        .wait_ready(Duration::from_secs(5), |_| Err(BootstrapError::NotReady))
        .unwrap();
    let credential = ready.credential();
    assert!(!format!("{credential:?}").contains(credential.expose_secret()));
    let client = Client::new(Endpoint::existing(root.path()).unwrap());
    let invalid = request(
        &client,
        json!({"method":"redeem_bootstrap","token":credential.expose_secret(),"root_fingerprint":"00".repeat(32),"manager_instance":hex(credential.manager_instance())}),
    );
    assert_eq!(invalid["ok"], false);
    let invalid = request(
        &client,
        json!({"method":"redeem_bootstrap","token":credential.expose_secret(),"root_fingerprint":hex(credential.root_fingerprint()),"manager_instance":"00".repeat(32)}),
    );
    assert_eq!(invalid["ok"], false);
    let mut auth = request(
        &client,
        json!({"method":"redeem_bootstrap","token":credential.expose_secret(),"root_fingerprint":hex(credential.root_fingerprint()),"manager_instance":hex(credential.manager_instance())}),
    );
    assert_eq!(auth["ok"], true);
    let session = Zeroizing::new(auth["data"]["session"].as_str().unwrap().to_owned());
    if let Some(Value::String(value)) = auth.get_mut("data").and_then(|v| v.get_mut("session")) {
        value.zeroize();
    }
    let reused = request(
        &client,
        json!({"method":"redeem_bootstrap","token":credential.expose_secret(),"root_fingerprint":hex(credential.root_fingerprint()),"manager_instance":hex(credential.manager_instance())}),
    );
    assert_eq!(reused["ok"], false);
    let reply = request(
        &client,
        json!({"method":"snapshot","session":session.as_str()}),
    );
    assert_eq!(reply["ok"], true);
    let reply = request(&client, json!({"method":"quit","session":session.as_str()}));
    assert_eq!(reply["ok"], true);
    assert!(ready.wait_for_exit().unwrap().success());
    assert_eq!(
        fs::read(root.path().join("stdout-guard")).unwrap(),
        b"untouched"
    );
}
fn inherited_channel_rejects_files_and_null() {
    let root = root();
    let path = root.path().join("regular-output");
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["fixture", "reject"])
        .arg(root.path())
        .stdin(Stdio::null())
        .stdout(fs::File::create(&path).unwrap())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
    assert!(fs::read(path).unwrap().is_empty());
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["fixture", "reject"])
        .arg(root.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success());
}
fn mandatory_executable_digest_and_unready_cleanup() {
    let root = root();
    let launch_spec = spec(root.path(), "unready");
    assert!(matches!(
        launch::spawn_with_private_bootstrap(&launch_spec, &[0; 32]),
        Err(Error::InvalidLaunch)
    ));
    let handles_before = open_handles();
    let Some(child) = spawn(root.path(), "unready") else {
        return;
    };
    assert!(matches!(
        child.wait_ready(Duration::from_millis(25), |_| Ok(false)),
        Err(BootstrapError::TimedOut)
    ));
    assert!(!root.path().join("control").exists());
    assert_eq!(
        open_handles(),
        handles_before,
        "timeout leaked a pipe/process handle"
    );
}
fn malformed_frame_is_bounded_and_cleaned() {
    let root = root();
    let handles_before = open_handles();
    let Some(child) = spawn(root.path(), "malformed") else {
        return;
    };
    assert!(matches!(
        child.wait_ready(Duration::from_secs(2), |_| Ok(false)),
        Err(BootstrapError::InvalidFrame)
    ));
    assert_eq!(
        open_handles(),
        handles_before,
        "malformed frame leaked a pipe/process handle"
    );
}
#[cfg(unix)]
fn open_handles() -> usize {
    fs::read_dir("/dev/fd").unwrap().count()
}
#[cfg(windows)]
fn open_handles() -> usize {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};
    let mut count = 0;
    assert_ne!(
        unsafe { GetProcessHandleCount(GetCurrentProcess(), &mut count) },
        0
    );
    count as usize
}
fn timeout_does_not_kill_an_observed_ready_owner() {
    let root = root();
    let Some(child) = spawn(root.path(), "ready-no-frame") else {
        return;
    };
    let result = child.wait_ready(Duration::from_millis(100), |expected_pid| {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(1) {
            if let Ok(endpoint) = Endpoint::existing(root.path())
                && let Ok((peer, reply)) =
                    Client::new(endpoint).request_with_peer(&json!({"method":"health"}))
                && peer.process_id == Some(expected_pid)
                && reply["ok"] == true
            {
                return Ok(true);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // Uncertain readiness is not proof of unready: never kill on unknown.
        Err(BootstrapError::NotReady)
    });
    assert!(matches!(result, Err(BootstrapError::TimedOut)));
    let start = Instant::now();
    while !root.path().join("ready-survived").exists() && start.elapsed() < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        fs::read(root.path().join("ready-survived")).unwrap(),
        b"alive"
    );
}
