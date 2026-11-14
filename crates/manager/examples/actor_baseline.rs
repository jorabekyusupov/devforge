//! Isolated metadata actor measurement, not a GUI or managed-runtime benchmark.
use devforge_control::{Id, OperationState};
use devforge_core::{ErrorCode, SecretStore};
use devforge_manager::{
    Owner, PeerIdentity,
    actor::{ActorHandle, OperationIdentity, Receipt, WorkspaceActor},
    auth::{DesktopAuth, DesktopAuthority, Policy, TrustedDesktopAction},
};
use devforge_workspace::Command;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, &'static str>;
#[derive(Default)]
struct MemorySecrets(Mutex<BTreeMap<String, String>>);
impl SecretStore for MemorySecrets {
    fn get(&self, key: &str) -> std::result::Result<Option<String>, ErrorCode> {
        Ok(self
            .0
            .lock()
            .map_err(|_| ErrorCode::SecretUnavailable)?
            .get(key)
            .cloned())
    }
    fn set(&self, key: &str, value: &str) -> std::result::Result<(), ErrorCode> {
        self.0
            .lock()
            .map_err(|_| ErrorCode::SecretUnavailable)?
            .insert(key.into(), value.into());
        Ok(())
    }
    fn delete(&self, key: &str) -> std::result::Result<(), ErrorCode> {
        self.0
            .lock()
            .map_err(|_| ErrorCode::SecretUnavailable)?
            .remove(key);
        Ok(())
    }
}
#[derive(Default)]
struct DenyWorkspaceSecrets(AtomicUsize);
impl SecretStore for DenyWorkspaceSecrets {
    fn get(&self, _: &str) -> std::result::Result<Option<String>, ErrorCode> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Err(ErrorCode::SecretUnavailable)
    }
    fn set(&self, _: &str, _: &str) -> std::result::Result<(), ErrorCode> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Err(ErrorCode::SecretUnavailable)
    }
    fn delete(&self, _: &str) -> std::result::Result<(), ErrorCode> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Err(ErrorCode::SecretUnavailable)
    }
}
fn id(value: &str) -> Result<Id> {
    Id::new(value).map_err(|_| "invalid_fixture_identity")
}
fn identity(project: &str, key: &str) -> Result<OperationIdentity> {
    Ok(OperationIdentity {
        project_id: id(project)?,
        idempotency_key: id(key)?,
    })
}
fn await_operation(
    handle: &ActorHandle,
    proof: &DesktopAuthority<'_>,
    receipt: &Receipt,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let current = handle
            .status(proof, &receipt.project_id, &receipt.operation_id)
            .map_err(|_| "operation_status_failed")?;
        if current.state.terminal() {
            return if current.state == OperationState::Succeeded {
                Ok(())
            } else {
                Err("metadata_operation_failed")
            };
        }
        if Instant::now() >= deadline {
            return Err("metadata_operation_timeout");
        }
        thread::sleep(Duration::from_millis(1));
    }
}
fn measured_operation(
    handle: &ActorHandle,
    proof: &DesktopAuthority<'_>,
    subject: &str,
    key: &str,
    command: Command,
) -> Result<(Receipt, f64)> {
    let start = Instant::now();
    let receipt = handle
        .submit_desktop(proof, identity(subject, key)?, command)
        .map_err(|_| "operation_submit_failed")?;
    await_operation(handle, proof, &receipt)?;
    Ok((receipt, start.elapsed().as_secs_f64() * 1000.0))
}
fn distribution(samples: &mut [f64]) -> Value {
    samples.sort_by(f64::total_cmp);
    let percentile = |fraction: f64| {
        samples[((samples.len() as f64 * fraction).ceil() as usize).saturating_sub(1)]
    };
    json!({"samples":samples.len(), "min_ms":samples[0], "p50_ms":percentile(0.5), "p95_ms":percentile(0.95), "max_ms":samples[samples.len()-1]})
}
#[cfg(unix)]
fn resources() -> Option<(f64, Option<u64>)> {
    let mut value = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // getrusage fills this plain OS structure on success.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, value.as_mut_ptr()) } != 0 {
        return None;
    }
    let value = unsafe { value.assume_init() };
    let seconds = |v: libc::timeval| v.tv_sec as f64 + v.tv_usec as f64 / 1_000_000.0;
    #[cfg(target_os = "macos")]
    let peak_bytes = u64::try_from(value.ru_maxrss).ok()?;
    #[cfg(not(target_os = "macos"))]
    let peak_bytes = u64::try_from(value.ru_maxrss).ok()?.checked_mul(1024)?;
    Some((
        seconds(value.ru_utime) + seconds(value.ru_stime),
        Some(peak_bytes),
    ))
}
#[cfg(windows)]
fn resources() -> Option<(f64, Option<u64>)> {
    use windows_sys::Win32::{
        Foundation::FILETIME,
        System::Threading::{GetCurrentProcess, GetProcessTimes},
    };
    let mut creation = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let mut exit = creation;
    let mut kernel = creation;
    let mut user = creation;
    if unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return None;
    }
    let seconds = |v: FILETIME| {
        ((u64::from(v.dwHighDateTime) << 32) | u64::from(v.dwLowDateTime)) as f64 / 10_000_000.0
    };
    // Memory counters require an additional Windows API feature; keep unavailable
    // explicit instead of changing dependency/features for this harness.
    Some((seconds(kernel) + seconds(user), None))
}
fn os_version() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("/usr/bin/sw_vers")
            .arg("-productVersion")
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let value = String::from_utf8(output.stdout).ok()?;
        Some(value.trim().to_string())
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}
fn fixture(root: &Path, number: usize) -> Result<std::path::PathBuf> {
    let path = root.join("fixtures").join(format!("project-{number}"));
    fs::create_dir_all(path.join("public")).map_err(|_| "fixture_creation_failed")?;
    fs::write(path.join("public/index.php"), "<?php").map_err(|_| "fixture_creation_failed")?;
    fs::write(path.join("composer.json"), r#"{"require":{"php":"^8.3","laravel/framework":"^13"},"scripts":{"post-install-cmd":"@php -r \"exit(9);\""}}"#).map_err(|_| "fixture_creation_failed")?;
    fs::write(path.join(".env"), "BASELINE_CANARY=metadata-only\n")
        .map_err(|_| "fixture_creation_failed")?;
    Ok(path)
}
fn run_case(count: usize) -> Result<Value> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("df-perf-");
    #[cfg(unix)]
    let root = builder
        .tempdir_in("/tmp")
        .map_err(|_| "fixture_root_failed")?;
    #[cfg(not(unix))]
    let root = builder.tempdir().map_err(|_| "fixture_root_failed")?;
    // No owner/ACL normalization: fail closed if this ordinary user's fresh
    // TempDir does not satisfy production ownership rules.
    let owner = Owner::acquire(root.path()).map_err(|_| "owner_acquisition_failed")?;
    let peer = PeerIdentity {
        user: "actor-baseline-fixture".into(),
        process_id: Some(std::process::id()),
    };
    let mut auth = DesktopAuth::new(
        owner.endpoint().clone(),
        peer.user.clone(),
        Policy::default(),
    )
    .map_err(|_| "fixture_auth_failed")?;
    let auth_store = MemorySecrets::default();
    let key = auth
        .rotate_desktop_key(
            &TrustedDesktopAction::from_explicit_desktop_action(),
            &auth_store,
            Instant::now(),
        )
        .map_err(|_| "fixture_auth_failed")?;
    let session = auth
        .authenticate(&peer, &key, &auth_store, Instant::now())
        .map_err(|_| "fixture_auth_failed")?;
    let proof = auth
        .authorize(&peer, &session, Instant::now())
        .map_err(|_| "fixture_auth_failed")?;
    let denied = Arc::new(DenyWorkspaceSecrets::default());
    let start = Instant::now();
    let actor = WorkspaceActor::start(
        owner.claim_workspace().map_err(|_| "actor_permit_failed")?,
        denied.clone(),
    )
    .map_err(|_| "actor_start_failed")?;
    let actor_start_ms = start.elapsed().as_secs_f64() * 1000.0;
    let handle = actor.handle();
    let initial = handle
        .cached_snapshot(&proof)
        .map_err(|_| "snapshot_failed")?;
    let php = initial
        .snapshot
        .runtimes
        .iter()
        .find(|r| r.kind == "php" && (r.variant == "cli" || r.id.contains("windows")))
        .ok_or("metadata_php_candidate_missing")?
        .id
        .clone();
    let node = initial
        .snapshot
        .runtimes
        .iter()
        .find(|r| r.kind == "node")
        .ok_or("metadata_node_candidate_missing")?
        .id
        .clone();
    let mut imports = Vec::with_capacity(count);
    let mut projects = Vec::with_capacity(count);
    let setup = Instant::now();
    for number in 0..count {
        let path = fixture(root.path(), number)?;
        let (receipt, duration) = measured_operation(
            &handle,
            &proof,
            "desktop-root",
            &format!("import-{number}"),
            Command::Import {
                path: path.clone(),
                name: format!("Metadata {number}"),
                php: php.clone(),
                node: node.clone(),
                domain: format!("baseline-{number}.devforge.test"),
                https: false,
                vite: false,
                queue: false,
            },
        )?;
        let result = handle
            .result(&proof, &receipt.project_id, &receipt.operation_id)
            .map_err(|_| "import_result_missing")?;
        let project = result["project_id"]
            .as_str()
            .ok_or("import_identity_missing")?
            .to_string();
        if fs::read_to_string(path.join(".env")).map_err(|_| "fixture_read_failed")?
            != "BASELINE_CANARY=metadata-only\n"
        {
            return Err("fixture_data_changed");
        }
        imports.push(duration);
        projects.push(project);
    }
    let registration_wall_ms = setup.elapsed().as_secs_f64() * 1000.0;
    let mut favorites = Vec::with_capacity(20);
    for number in 0..20 {
        let project = &projects[number % count];
        let (_, duration) = measured_operation(
            &handle,
            &proof,
            project,
            &format!("favorite-{number}"),
            Command::Favorite {
                id: project.clone(),
                favorite: number % 2 == 0,
            },
        )?;
        favorites.push(duration);
    }
    let mut reads = Vec::with_capacity(2000);
    for _ in 0..2000 {
        let start = Instant::now();
        let snapshot = handle
            .cached_snapshot(&proof)
            .map_err(|_| "snapshot_failed")?;
        reads.push(start.elapsed().as_secs_f64() * 1000.0);
        if snapshot.snapshot.projects.len() != count || snapshot.error.is_some() {
            return Err("snapshot_count_or_health_failed");
        }
    }
    let mut refresh_waits = Vec::with_capacity(5);
    for _ in 0..5 {
        let before = handle
            .cached_snapshot(&proof)
            .map_err(|_| "snapshot_failed")?
            .observed_at;
        let start = Instant::now();
        loop {
            let snapshot = handle
                .cached_snapshot(&proof)
                .map_err(|_| "snapshot_failed")?;
            if snapshot.error.is_some() {
                return Err("snapshot_refresh_failed");
            }
            if snapshot.observed_at != before {
                break;
            }
            if start.elapsed() > Duration::from_secs(10) {
                return Err("snapshot_refresh_timeout");
            }
            thread::sleep(Duration::from_millis(1));
        }
        refresh_waits.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    let before = resources();
    let idle = Instant::now();
    thread::sleep(Duration::from_secs(1));
    let idle_seconds = idle.elapsed().as_secs_f64();
    let after = resources();
    let final_view = handle
        .cached_snapshot(&proof)
        .map_err(|_| "snapshot_failed")?;
    if final_view.snapshot.projects.len() != count
        || final_view.error.is_some()
        || final_view.busy
        || !final_view.snapshot.databases.is_empty()
        || !final_view.snapshot.caches.is_empty()
        || final_view.snapshot.runtimes.iter().any(|r| r.installed)
        || final_view
            .snapshot
            .projects
            .iter()
            .any(|r| !r.processes.is_empty() || r.serving.is_some() || r.state != "stopped")
        || denied.0.load(Ordering::Relaxed) != 0
    {
        return Err("metadata_only_invariant_failed");
    }
    let start = Instant::now();
    actor.shutdown().map_err(|_| "actor_shutdown_failed")?;
    let shutdown_ms = start.elapsed().as_secs_f64() * 1000.0;
    drop(handle);
    drop(owner);
    root.close().map_err(|_| "fixture_cleanup_failed")?;
    Ok(json!({
        "projects":count, "actor_start_ms":actor_start_ms,
        "runtime_selection":{"php":php,"node":node,"installed":false},
        "registration_wall_ms":registration_wall_ms,"import_end_to_end":distribution(&mut imports),
        "favorite_end_to_end_including_refresh":distribution(&mut favorites),
        "cached_snapshot_read":distribution(&mut reads),
        "periodic_refresh_observation_wait_including_250ms_schedule":distribution(&mut refresh_waits),
        "idle":{"wall_seconds":idle_seconds,"process_cpu_seconds":before.zip(after).map(|(b,a)|(a.0-b.0).max(0.0)),
            "one_core_percent":before.zip(after).map(|(b,a)|(a.0-b.0).max(0.0)/idle_seconds*100.0),
            "process_lifetime_peak_rss_bytes":after.and_then(|a|a.1),"native_resource_api_available":after.is_some()},
        "shutdown_ms":shutdown_ms,"fixture_cleaned":true,
        "metadata_invariants":{"no_installed_runtimes":true,"no_running_services":true,"workspace_secret_calls":0}
    }))
}
fn execute() -> Result<Value> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() > 5 {
        return Err("usage_expected_up_to_five_counts_1_to_200");
    }
    let counts = if args.is_empty() {
        vec![1, 10, 100]
    } else {
        args.iter()
            .map(|arg| {
                arg.parse::<usize>()
                    .ok()
                    .filter(|n| (1..=200).contains(n))
                    .ok_or("usage_expected_counts_1_to_200")
            })
            .collect::<Result<Vec<_>>>()?
    };
    let mut cases = Vec::new();
    for count in counts {
        cases.push(run_case(count)?);
    }
    Ok(
        json!({"schema":1,"measurement":"workspace_actor_metadata_only","profile":if cfg!(debug_assertions){"debug"}else{"release"},
        "package_version":env!("CARGO_PKG_VERSION"), "os":std::env::consts::OS,"os_version":os_version(),"arch":std::env::consts::ARCH,
        "logical_parallelism":thread::available_parallelism().ok().map(|n|n.get()),
        "auth":"in_memory_fixture_not_native_or_transport_acceptance",
        "resource_scope":"current_process_only; peak RSS cumulative across cases; Windows RSS unavailable in this build",
        "limitations":["no GUI/WebView/IPC benchmark","no runtime execution/download","periodic wait is not snapshot compute duration","sequential cached reads, not contention p95","idle sample is one second, not sustained idle budget","no cross-platform or performance budget claim"],
        "cases":cases}),
    )
}
fn main() {
    match execute() {
        Ok(report) => println!(
            "{}",
            serde_json::to_string_pretty(&report).expect("JSON value serialization")
        ),
        Err(code) => {
            eprintln!("{}", json!({"ok":false,"code":code}));
            std::process::exit(1);
        }
    }
}
