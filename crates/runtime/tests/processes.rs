mod support;
use devforge_runtime::{
    RuntimeError,
    process::{LogPolicy, Observed, Readiness, RestartPolicy, Supervisor},
};
use std::{
    fs, thread,
    time::{Duration, Instant},
};
use support::*;
fn ready(s: &Supervisor, id: &str) {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let state = s.snapshot(id).unwrap();
        if state.observed == Observed::Ready {
            return;
        }
        assert!(Instant::now() < until, "{state:?}");
        assert_ne!(state.observed, Observed::Failed, "{state:?}");
        thread::sleep(Duration::from_millis(10));
    }
}
#[test]
fn spawn_is_not_ready_and_duplicate_owners_are_rejected() {
    let f = Fixture::new();
    f.install();
    let root = f.temp.path().join("logs");
    let mut s = Supervisor::open(&root).unwrap();
    let mut other = Supervisor::open(&root).unwrap();
    let spec = f.spec("one", "delayed");
    s.start(spec.clone()).unwrap();
    assert_eq!(s.snapshot("one").unwrap().observed, Observed::Starting);
    assert_eq!(
        s.start(spec.clone()).err(),
        Some(RuntimeError::InvalidProcess)
    );
    assert_eq!(other.start(spec).err(), Some(RuntimeError::Busy));
    ready(&s, "one");
    assert_eq!(f.store.remove(&f.package).err(), Some(RuntimeError::Busy));
    assert_eq!(s.stop("one").unwrap().observed, Observed::Stopped);
    f.store.remove(&f.package).unwrap();
}
#[test]
fn argv_environment_and_working_directory_are_preserved_without_shell() {
    let f = Fixture::new();
    f.install();
    let mut s = Supervisor::open(f.temp.path().join("logs")).unwrap();
    let mut spec = f.spec("echo", "echo");
    spec.args.extend([
        "space and Ўзбек Русский".into(),
        "$(touch should-not-exist) ; & \"".into(),
    ]);
    spec.environment
        .insert("DEVFORGE_FIXTURE_MARKER".into(), "explicit".into());
    spec.readiness = Readiness::SuccessfulExit;
    s.start(spec).unwrap();
    let outcome = s.wait_terminal("echo", Duration::from_secs(5)).unwrap();
    assert_eq!(
        outcome.observed,
        Observed::Succeeded,
        "{outcome:?} logs={:?}",
        s.logs("echo").unwrap()
    );
    let output = s
        .logs("echo")
        .unwrap()
        .0
        .into_iter()
        .map(|l| l.text)
        .collect::<String>();
    let v: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
    assert_eq!(v["args"][0], "space and Ўзбек Русский");
    assert_eq!(v["args"][1], "$(touch should-not-exist) ; & \"");
    assert_eq!(v["marker"], "explicit");
    assert!(v["inherited"].is_null());
    assert_eq!(
        fs::canonicalize(v["cwd"].as_str().unwrap()).unwrap(),
        fs::canonicalize(f.temp.path()).unwrap()
    );
    assert!(!f.temp.path().join("should-not-exist").exists());
}
#[test]
fn readiness_timeout_and_startup_cancel_stop_attempt() {
    let f = Fixture::new();
    f.install();
    let mut s = Supervisor::open(f.temp.path().join("logs")).unwrap();
    let mut spec = f.spec("timeout", "quiet");
    spec.readiness_timeout = Duration::from_millis(50);
    s.start(spec).unwrap();
    let state = s.wait_terminal("timeout", Duration::from_secs(5)).unwrap();
    assert_eq!(state.failure, Some(RuntimeError::ReadinessTimeout));
    assert_ne!(state.observed, Observed::Ready);
    s.start(f.spec("cancel", "delayed")).unwrap();
    s.request_stop("cancel").unwrap();
    assert_eq!(
        s.wait_terminal("cancel", Duration::from_secs(5))
            .unwrap()
            .observed,
        Observed::Stopped
    );
}
#[test]
fn crashing_process_has_bounded_restart_and_cancellable_backoff() {
    let f = Fixture::new();
    f.install();
    let mut s = Supervisor::open(f.temp.path().join("logs")).unwrap();
    let mut spec = f.spec("crash", "crash");
    spec.restart = RestartPolicy {
        max_restarts: 2,
        backoff: Duration::from_millis(20),
    };
    s.start(spec).unwrap();
    let state = s.wait_terminal("crash", Duration::from_secs(5)).unwrap();
    assert_eq!(state.restarts, 2);
    assert_eq!(state.observed, Observed::Failed);
    assert_eq!(state.failure, Some(RuntimeError::ProcessExited));
    assert!(state.history.len() <= 32);
    let mut spec = f.spec("backoff", "crash");
    spec.restart = RestartPolicy {
        max_restarts: 10,
        backoff: Duration::from_secs(5),
    };
    s.start(spec).unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    while s.snapshot("backoff").unwrap().observed != Observed::Backoff {
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(10));
    }
    let start = Instant::now();
    s.stop("backoff").unwrap();
    assert!(start.elapsed() < Duration::from_secs(2));
}
#[test]
fn flooded_logs_are_bounded_rotated_and_drained_from_both_streams() {
    let f = Fixture::new();
    f.install();
    let root = f.temp.path().join("logs");
    let mut s = Supervisor::open(&root).unwrap();
    let mut spec = f.spec("flood", "flood");
    spec.logs = LogPolicy {
        memory_bytes: 8192,
        file_bytes: 8192,
        files: 3,
    };
    s.start(spec).unwrap();
    ready(&s, "flood");
    let until = Instant::now() + Duration::from_secs(5);
    while s.logs("flood").unwrap().1 < 100_000 {
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(10));
    }
    s.request_stop("flood").unwrap();
    s.wait_terminal("flood", Duration::from_secs(5)).unwrap();
    let (logs, dropped) = s.logs("flood").unwrap();
    assert!(logs.iter().map(|l| l.text.len()).sum::<usize>() <= 8192);
    assert!(dropped > 0);
    let files: Vec<_> = fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().contains(".log"))
        .collect();
    assert!(files.len() <= 3);
    for file in files {
        assert!(fs::metadata(file).unwrap().len() <= 8192);
    }
}
#[test]
fn owned_tree_cleanup_after_stop_crash_and_owner_drop_preserves_unrelated_child() {
    for mode in ["tree", "tree-crash", "drop"] {
        let f = Fixture::new();
        f.install();
        let mut unrelated =
            std::process::Command::new(env!("CARGO_BIN_EXE_devforge-process-fixture"))
                .arg("quiet")
                .spawn()
                .unwrap();
        let mut s = Supervisor::open(f.temp.path().join("logs")).unwrap();
        s.start(f.spec("tree", if mode == "drop" { "tree" } else { mode }))
            .unwrap();
        let until = Instant::now() + Duration::from_secs(5);
        let child_pid = loop {
            let text = s
                .logs("tree")
                .unwrap()
                .0
                .iter()
                .map(|l| l.text.clone())
                .collect::<String>();
            if let Some(id) = text.lines().find_map(|line| line.strip_prefix("CHILD:")) {
                break id.parse::<u32>().unwrap();
            }
            assert!(Instant::now() < until);
            thread::sleep(Duration::from_millis(10));
        };
        if mode == "tree-crash" {
            assert_eq!(
                s.wait_terminal("tree", Duration::from_secs(5))
                    .unwrap()
                    .observed,
                Observed::Failed
            );
        } else if mode == "tree" {
            s.stop("tree").unwrap();
        }
        drop(s);
        let until = Instant::now() + Duration::from_secs(3);
        while alive(child_pid) {
            assert!(
                Instant::now() < until,
                "descendant {child_pid} survived {mode}"
            );
            thread::sleep(Duration::from_millis(10));
        }
        assert!(unrelated.try_wait().unwrap().is_none());
        unrelated.kill().unwrap();
        unrelated.wait().unwrap();
    }
}
#[test]
fn uncooperative_process_is_forced_down_after_grace_and_can_restart() {
    let f = Fixture::new();
    f.install();
    let mut s = Supervisor::open(f.temp.path().join("logs")).unwrap();
    s.start(f.spec("stubborn", "stubborn")).unwrap();
    ready(&s, "stubborn");
    let start = Instant::now();
    let state = s.stop("stubborn").unwrap();
    assert_eq!(state.observed, Observed::Stopped);
    assert!(start.elapsed() >= Duration::from_millis(90));
    assert!(start.elapsed() < Duration::from_secs(3));
    s.start(f.spec("stubborn", "ready")).unwrap();
    ready(&s, "stubborn");
    s.stop("stubborn").unwrap();
}
#[cfg(unix)]
fn alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[test]
fn project_secret_output_is_redacted_before_memory_and_disk_and_exit_code_is_retained() {
    let f = Fixture::new();
    f.install();
    let root = f.temp.path().join("logs");
    let mut s = Supervisor::open(&root).unwrap();
    let mut spec = f.spec("secret", "secret");
    let value = "sensitive-cross-chunk-fixture-password";
    spec.environment
        .insert("APP_TEST_SECRET".into(), value.into());
    spec.readiness = Readiness::SuccessfulExit;
    s.start_redacted(spec, vec![value.into()]).unwrap();
    let state = s.wait_terminal("secret", Duration::from_secs(5)).unwrap();
    assert_eq!(state.exit_code, Some(17));
    let text = s
        .logs("secret")
        .unwrap()
        .0
        .into_iter()
        .map(|l| l.text)
        .collect::<String>();
    assert!(!text.contains(value));
    assert!(text.contains("[REDACTED]"));
    s.stop("secret").unwrap();
    let disk = fs::read_to_string(root.join("secret.log")).unwrap();
    assert!(!disk.contains(value));
    assert!(disk.contains("[REDACTED]"));
}
#[cfg(windows)]
fn alive(pid: u32) -> bool {
    use windows_sys::Win32::{Foundation::*, System::Threading::*};
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code = 0;
        let result = GetExitCodeProcess(handle, &mut code) != 0 && code == 259;
        CloseHandle(handle);
        result
    }
}
