#![cfg(feature = "test-support")]
//! Only disposable JSON fixtures; no native credentials or application processes.
use devforge_core::{ErrorCode, SecretStore};
use devforge_workspace::testing::{
    FixtureSecrets, MAX_FILE_BYTES, MAX_KEY_BYTES, MAX_KEYS, MAX_VALUE_BYTES,
};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::Path,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

fn private_write(path: &Path, bytes: &[u8]) {
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).unwrap().write_all(bytes).unwrap();
}

#[test]
fn constructor_before_root_creation_and_independent_instances_observe_latest_merge_delete() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("not-created-yet");
    let path = data.join("e2e-secrets.json");
    let first = FixtureSecrets::open(path.clone());
    let second = FixtureSecrets::open(path.clone());
    assert!(!data.exists());
    assert_eq!(first.get("missing"), Err(ErrorCode::SecretUnavailable));
    fs::create_dir(&data).unwrap();
    first.set("project/first", "isolated-first").unwrap();
    second.set("control/native", "isolated-control").unwrap();
    assert_eq!(
        first.get("control/native").unwrap().as_deref(),
        Some("isolated-control")
    );
    assert_eq!(
        second.get("project/first").unwrap().as_deref(),
        Some("isolated-first")
    );
    first.set("project/first", "updated").unwrap();
    assert_eq!(
        second.get("project/first").unwrap().as_deref(),
        Some("updated")
    );
    second.delete("control/native").unwrap();
    assert_eq!(first.get("control/native").unwrap(), None);
    assert_eq!(
        first.get("project/first").unwrap().as_deref(),
        Some("updated")
    );
    assert!(path.with_file_name("e2e-secrets.json.lock").is_file());
}

#[test]
fn invalid_map_and_all_write_limits_preserve_existing_bytes() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("e2e-secrets.json");
    let store = FixtureSecrets::open(path.clone());
    store.set("retained", "unchanged").unwrap();
    let before = fs::read(&path).unwrap();
    for (key, value) in [
        ("".to_owned(), "x".to_owned()),
        ("k".repeat(MAX_KEY_BYTES + 1), "x".to_owned()),
        ("valid".to_owned(), "x".repeat(MAX_VALUE_BYTES + 1)),
    ] {
        assert_eq!(store.set(&key, &value), Err(ErrorCode::SecretUnavailable));
        assert_eq!(fs::read(&path).unwrap(), before);
    }
    let many = (0..MAX_KEYS)
        .map(|n| (format!("k-{n}"), "value".to_owned()))
        .collect::<BTreeMap<_, _>>();
    private_write(&path, &serde_json::to_vec(&many).unwrap());
    let before = fs::read(&path).unwrap();
    assert_eq!(store.get("k-1023").unwrap().as_deref(), Some("value"));
    assert_eq!(
        store.set("one-too-many", "value"),
        Err(ErrorCode::SecretUnavailable)
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    let escaped = "\0".repeat(MAX_VALUE_BYTES);
    store.delete("k-0").unwrap();
    private_write(
        &path,
        &serde_json::to_vec(&BTreeMap::from([("first", &escaped), ("second", &escaped)])).unwrap(),
    );
    let before = fs::read(&path).unwrap();
    assert!(before.len() < MAX_FILE_BYTES);
    assert_eq!(
        store.set("third", &escaped),
        Err(ErrorCode::SecretUnavailable)
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    for bad in [
        br#"{"a":"first","a":"duplicate"}"#.as_slice(),
        b"not-json",
        br#"{"x":4}"#,
        b"[]",
    ] {
        private_write(&path, bad);
        assert_eq!(store.get("a"), Err(ErrorCode::SecretUnavailable));
        assert_eq!(
            store.set("replacement", "must-not-reset"),
            Err(ErrorCode::SecretUnavailable)
        );
        assert_eq!(fs::read(&path).unwrap(), bad);
    }
    private_write(&path, &vec![b' '; MAX_FILE_BYTES + 1]);
    let before = fs::read(&path).unwrap();
    assert_eq!(store.get("a"), Err(ErrorCode::SecretUnavailable));
    assert_eq!(store.delete("a"), Err(ErrorCode::SecretUnavailable));
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn replacement_root_and_hardlinked_data_or_lock_are_refused_without_mutating_target() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    fs::create_dir(&data).unwrap();
    let path = data.join("e2e-secrets.json");
    let store = FixtureSecrets::open(path.clone());
    store.set("retained", "fixture-only").unwrap();
    fs::rename(&data, root.path().join("old-data")).unwrap();
    fs::create_dir(&data).unwrap();
    assert_eq!(
        store.set("wrong-root", "no"),
        Err(ErrorCode::SecretUnavailable)
    );
    assert_eq!(fs::read_dir(&data).unwrap().count(), 0);
    let store = FixtureSecrets::open(path.clone());
    let outside = root.path().join("untouched.json");
    private_write(&outside, br#"{"retained":"safe"}"#);
    fs::hard_link(&outside, &path).unwrap();
    let before = fs::read(&outside).unwrap();
    assert_eq!(store.get("retained"), Err(ErrorCode::SecretUnavailable));
    assert_eq!(store.set("new", "no"), Err(ErrorCode::SecretUnavailable));
    assert_eq!(fs::read(&outside).unwrap(), before);
    fs::remove_file(&path).unwrap();
    fs::remove_file(path.with_file_name("e2e-secrets.json.lock")).unwrap();
    fs::hard_link(&outside, path.with_file_name("e2e-secrets.json.lock")).unwrap();
    assert_eq!(store.set("new", "no"), Err(ErrorCode::SecretUnavailable));
    assert!(!path.exists());
    assert_eq!(fs::read(&outside).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn symlink_roots_data_and_lock_never_create_or_change_external_targets() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let linked = root.path().join("linked");
    symlink(outside.path(), &linked).unwrap();
    let store = FixtureSecrets::open(linked.join("e2e-secrets.json"));
    assert_eq!(store.set("x", "no"), Err(ErrorCode::SecretUnavailable));
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    for name in ["e2e-secrets.json", "e2e-secrets.json.lock"] {
        let data = root.path().join(name.replace('.', "-"));
        fs::create_dir(&data).unwrap();
        let path = data.join("e2e-secrets.json");
        let target = outside.path().join("must-not-create");
        symlink(&target, data.join(name)).unwrap();
        let store = FixtureSecrets::open(path);
        assert_eq!(store.set("x", "no"), Err(ErrorCode::SecretUnavailable));
        assert!(!target.exists());
    }
}

#[cfg(windows)]
#[test]
fn junction_root_is_rejected_without_writing_fixture_files() {
    use std::os::windows::process::CommandExt;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let linked = root.path().join("linked-root");
    let quote = |path: &Path| {
        let value = path.to_str().unwrap();
        assert!(!value.contains(['%', '"', '\r', '\n']));
        format!("\"{value}\"")
    };
    let script = format!("mklink /J {} {}", quote(&linked), quote(outside.path()));
    let output = Command::new(
        std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/cmd.exe"),
    )
    .args(["/D", "/V:OFF", "/S", "/C"])
    .raw_arg(format!("\"{script}\""))
    .output()
    .unwrap();
    assert!(
        output.status.success(),
        "isolated junction fixture creation failed"
    );
    let store = FixtureSecrets::open(linked.join("e2e-secrets.json"));
    assert_eq!(
        store.set("fixture", "must-not-write"),
        Err(ErrorCode::SecretUnavailable)
    );
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
    fs::remove_dir(linked).unwrap();
}

struct OwnedChild(Child);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn two_processes_merge_keys_and_live_parent_observes_later_control_changes() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("e2e-secrets.json");
    let parent = FixtureSecrets::open(path.clone());
    parent.set("seed", "fixture-only").unwrap();
    let launch = |prefix: &str| {
        OwnedChild(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "fixture_child_process_worker", "--nocapture"])
                .env("DEVFORGE_FIXTURE_STORAGE_TEST_ROOT", root.path())
                .env("DEVFORGE_FIXTURE_STORAGE_TEST_WRITER", prefix)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        )
    };
    let mut left = launch("left");
    let mut right = launch("right");
    let until = Instant::now() + Duration::from_secs(10);
    while !root.path().join("left-ready").exists() || !root.path().join("right-ready").exists() {
        assert!(
            Instant::now() < until,
            "fixture children did not become ready"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    fs::write(root.path().join("go"), b"fixture barrier").unwrap();
    for child in [&mut left, &mut right] {
        loop {
            if let Some(status) = child.0.try_wait().unwrap() {
                assert!(status.success(), "owned fixture child failed");
                break;
            }
            assert!(Instant::now() < until, "fixture child exceeded deadline");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
    for prefix in ["left", "right"] {
        for n in 0..24 {
            assert_eq!(
                parent.get(&format!("{prefix}/{n}")).unwrap().as_deref(),
                Some("isolated-child-value")
            );
        }
        assert_eq!(parent.get(&format!("{prefix}/deleted")).unwrap(), None);
    }
    let late = FixtureSecrets::open(path);
    late.set("control/rotated", "later-visible").unwrap();
    assert_eq!(
        parent.get("control/rotated").unwrap().as_deref(),
        Some("later-visible")
    );
    parent.delete("control/rotated").unwrap();
    assert_eq!(late.get("control/rotated").unwrap(), None);
    assert_eq!(parent.get("seed").unwrap().as_deref(), Some("fixture-only"));
}

#[test]
fn fixture_child_process_worker() {
    // Only this integration-test executable reads these harness metadata vars;
    // they never enable a fixture provider in a production CLI/desktop build.
    let Some(root) = std::env::var_os("DEVFORGE_FIXTURE_STORAGE_TEST_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let prefix = std::env::var("DEVFORGE_FIXTURE_STORAGE_TEST_WRITER").unwrap();
    assert!(matches!(prefix.as_str(), "left" | "right"));
    let store = FixtureSecrets::open(root.join("e2e-secrets.json"));
    assert_eq!(store.get("seed").unwrap().as_deref(), Some("fixture-only"));
    fs::write(root.join(format!("{prefix}-ready")), b"ready").unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    while !root.join("go").exists() {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(2));
    }
    for n in 0..24 {
        store
            .set(&format!("{prefix}/{n}"), "isolated-child-value")
            .unwrap();
        std::thread::yield_now();
    }
    let transient = format!("{prefix}/deleted");
    store.set(&transient, "fixture-temporary").unwrap();
    store.delete(&transient).unwrap();
}
