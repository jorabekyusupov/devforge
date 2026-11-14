mod common;
use devforge_manager::{
    Client, Endpoint, Error, Owner, Server, StopSignal,
    launch::{self, LaunchSpec},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};

fn root() -> tempfile::TempDir {
    common::root("df-manager-")
}

#[cfg(windows)]
#[test]
fn disposable_root_security_probe_preserves_production_validation() {
    let root = root();
    let result = common::acquire(root.path());
    common::diagnose("root_after_acquire", root.path());
    if root.path().join("control").exists() {
        common::diagnose("control_after_acquire", &root.path().join("control"));
    }
    assert!(result.is_ok(), "MANAGER_FIXTURE_OWNER_ACQUIRE_OK=false");
}
fn fixture(mode: &str, root: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "subprocess_fixture", "--ignored", "--nocapture"])
        .env("DEVFORGE_MANAGER_FIXTURE_MODE", mode)
        .env("DEVFORGE_MANAGER_FIXTURE_ROOT", root);
    command
}
struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn ready(root: &Path) -> Client {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(endpoint) = Endpoint::existing(root) {
            let client = Client::new(endpoint);
            if client.request(&json!({"probe":"ping"})).is_ok() {
                return client;
            }
        }
        assert!(
            Instant::now() < until,
            "isolated manager did not become ready"
        );
        thread::sleep(Duration::from_millis(20));
    }
}
#[test]
fn owner_excludes_second_owner_and_blocks_installer_until_drop() {
    let root = root();
    let owner = common::acquire(root.path()).unwrap();
    assert!(matches!(Owner::acquire(root.path()), Err(Error::Busy)));
    assert!(devforge_platform::distribution::OperationLease::exclusive(root.path()).is_err());
    drop(owner);
    let installer =
        devforge_platform::distribution::OperationLease::exclusive(root.path()).unwrap();
    assert!(matches!(Owner::acquire(root.path()), Err(Error::Busy)));
    drop(installer);
    assert!(Owner::acquire(root.path()).is_ok());
}

#[cfg(unix)]
#[test]
fn owner_rejects_dangling_operation_lock_without_outside_write_and_releases_singleton() {
    use std::os::unix::fs::symlink;
    let root = root();
    let outside = tempfile::tempdir().unwrap();
    let forbidden = outside.path().join("must-not-be-created");
    let operation_lock = root.path().join("operations.lock");
    symlink(&forbidden, &operation_lock).unwrap();
    assert!(Owner::acquire(root.path()).is_err());
    assert!(!forbidden.exists());
    assert_eq!(fs::read_link(&operation_lock).unwrap(), forbidden);

    // OperationLease failed after ManagerLease was acquired. Removing only the
    // test-owned symlink must allow a fresh owner, proving the failure released
    // the singleton instead of leaving an unavailable endpoint behind.
    fs::remove_file(operation_lock).unwrap();
    let owner = common::acquire(root.path()).unwrap();
    assert!(devforge_platform::distribution::OperationLease::exclusive(root.path()).is_err());
    drop(owner);
    assert!(devforge_platform::distribution::OperationLease::exclusive(root.path()).is_ok());
}

#[test]
fn two_process_clients_observe_same_authenticated_owner_and_graceful_shutdown() {
    let root = root();
    let mut owner = Process(fixture("owner", root.path()).spawn().unwrap());
    let client = ready(root.path());
    let owner_pid = owner.0.id();
    assert!(matches!(Owner::acquire(root.path()), Err(Error::Busy)));
    let mut a = fixture("client-a", root.path()).spawn().unwrap();
    let mut b = fixture("client-b", root.path()).spawn().unwrap();
    assert!(a.wait().unwrap().success());
    assert!(b.wait().unwrap().success());
    let a: Value =
        serde_json::from_slice(&fs::read(root.path().join("client-a.json")).unwrap()).unwrap();
    let b: Value =
        serde_json::from_slice(&fs::read(root.path().join("client-b.json")).unwrap()).unwrap();
    assert_eq!(a["owner"], owner_pid);
    assert_eq!(b["owner"], owner_pid);
    assert_ne!(a["client"], b["client"]);
    assert_eq!(a["peer_user"], b["peer_user"]);
    let (peer, response) = client.request_with_peer(&json!({"probe":"ping"})).unwrap();
    assert_eq!(response["peer_user"], peer.user);
    #[cfg(windows)]
    assert_eq!(peer.process_id, Some(owner_pid));
    client.request(&json!({"probe":"stop"})).unwrap();
    assert!(owner.0.wait().unwrap().success());
    assert!(Owner::acquire(root.path()).is_ok());
    assert!(devforge_platform::distribution::OperationLease::exclusive(root.path()).is_ok());
    assert!(client.request(&json!({"probe":"ping"})).is_err());
}

#[test]
fn crashed_owner_releases_lease_and_new_owner_replaces_only_owned_stale_endpoint() {
    let root = root();
    let mut first = Process(fixture("owner", root.path()).spawn().unwrap());
    let old_client = ready(root.path());
    let first_pid = first.0.id();
    assert_eq!(
        old_client.request(&json!({"probe":"ping"})).unwrap()["owner"],
        first_pid
    );
    first.0.kill().unwrap();
    first.0.wait().unwrap();
    assert!(old_client.request(&json!({"probe":"ping"})).is_err());
    let mut second = Process(fixture("owner", root.path()).spawn().unwrap());
    let current = ready(root.path());
    assert_ne!(second.0.id(), first_pid);
    assert_eq!(
        current.request(&json!({"probe":"ping"})).unwrap()["owner"],
        second.0.id()
    );
    current.request(&json!({"probe":"stop"})).unwrap();
    assert!(second.0.wait().unwrap().success());
}

#[test]
fn detached_owner_survives_launcher_exit_or_reports_restricted_job() {
    let root = root();
    let status = fixture("launcher", root.path()).status().unwrap();
    assert!(status.success());
    let result = fs::read_to_string(root.path().join("launch-result")).unwrap();
    #[cfg(windows)]
    if result == "manager_restricted_parent_job" {
        // Explicit platform capability, not a claimed successful detach. A native
        // acceptance runner must record this branch and arrange an independent
        // ordinary-user launch before claiming headless durability.
        eprintln!("CAPABILITY_UNAVAILABLE: Windows parent job forbids detached launch");
        return;
    }
    let client = ready(root.path());
    let result: u32 = result.parse().unwrap();
    assert_eq!(
        client.request(&json!({"probe":"ping"})).unwrap()["owner"],
        result
    );
    client.request(&json!({"probe":"stop"})).unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    while Owner::acquire(root.path()).is_err() {
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn oversize_and_malformed_frames_never_dispatch_and_server_recovers() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let root = root();
    let owner = common::acquire(root.path()).unwrap();
    let endpoint = owner.endpoint().clone();
    let stop = StopSignal::default();
    let shutdown = stop.clone();
    let dispatched = Arc::new(AtomicUsize::new(0));
    let counter = dispatched.clone();
    let worker = thread::spawn(move || {
        Server::new(owner)
            .run(shutdown, move |_peer, request| {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(request)
            })
            .unwrap()
    });
    let client = ready(root.path());
    assert!(matches!(
        client.request(&json!({"large":"x".repeat(devforge_manager::MAX_REQUEST_BYTES)})),
        Err(Error::FrameTooLarge)
    ));
    #[cfg(unix)]
    {
        use std::io::Write;
        let mut connection = std::os::unix::net::UnixStream::connect(endpoint.address()).unwrap();
        connection
            .write_all(&((devforge_manager::MAX_REQUEST_BYTES + 1) as u32).to_be_bytes())
            .unwrap();
        drop(connection);
        let mut connection = std::os::unix::net::UnixStream::connect(endpoint.address()).unwrap();
        connection.write_all(&1u32.to_be_bytes()).unwrap();
        connection.write_all(b"{").unwrap();
        drop(connection);
    }
    #[cfg(windows)]
    let _ = endpoint;
    assert_eq!(
        client.request(&json!({"bounded":true})).unwrap(),
        json!({"bounded":true})
    );
    assert_eq!(dispatched.load(Ordering::SeqCst), 2);
    stop.request();
    worker.join().unwrap();
}

#[test]
fn oversized_response_closes_exchange_without_killing_owner() {
    let root = root();
    let owner = common::acquire(root.path()).unwrap();
    let stop = StopSignal::default();
    let shutdown = stop.clone();
    let worker = thread::spawn(move || {
        Server::new(owner)
            .run(shutdown, |_, input| {
                if input["oversized"] == true {
                    Ok(json!({"large":"x".repeat(devforge_manager::MAX_RESPONSE_BYTES)}))
                } else {
                    Ok(input)
                }
            })
            .unwrap()
    });
    let client = ready(root.path());
    assert!(client.request(&json!({"oversized":true})).is_err());
    assert_eq!(
        client.request(&json!({"still_alive":true})).unwrap(),
        json!({"still_alive":true})
    );
    stop.request();
    worker.join().unwrap();
}

#[test]
fn saturated_listener_rejects_excess_without_stopping_owner_and_idle_peers_expire() {
    use std::io::Write;
    let root = root();
    let owner = common::acquire(root.path()).unwrap();
    let endpoint = owner.endpoint().clone();
    let stop = StopSignal::default();
    let shutdown = stop.clone();
    let worker = thread::spawn(move || {
        Server::new(owner)
            .run(shutdown, |_, value| Ok(value))
            .unwrap()
    });
    let client = ready(root.path());
    let idle_peers = || {
        (0..devforge_manager::MAX_CONNECTIONS)
            .map(|_| {
                let mut connection = raw_connection(&endpoint);
                // A partial header makes the receiver await the framing
                // deadline without ever dispatching a handler.
                connection.write_all(&[0, 0]).unwrap();
                connection
            })
            .collect::<Vec<_>>()
    };
    let peers = idle_peers();
    thread::sleep(Duration::from_millis(100));
    assert!(client.request(&json!({"excess":true})).is_err());
    drop(peers);
    let recovered = ready(root.path());
    assert_eq!(
        recovered.request(&json!({"same_owner":true})).unwrap(),
        json!({"same_owner":true})
    );
    assert!(matches!(Owner::acquire(root.path()), Err(Error::Busy)));

    // Keep every peer handle open: capacity must recover from the server's
    // read deadline rather than from clients closing their connections.
    let peers = idle_peers();
    thread::sleep(devforge_manager::IO_TIMEOUT + Duration::from_millis(250));
    assert_eq!(
        client.request(&json!({"after_timeout":true})).unwrap(),
        json!({"after_timeout":true})
    );
    drop(peers);
    stop.request();
    worker.join().unwrap();
    assert!(Owner::acquire(root.path()).is_ok());
}

#[cfg(unix)]
fn raw_connection(endpoint: &Endpoint) -> std::os::unix::net::UnixStream {
    std::os::unix::net::UnixStream::connect(endpoint.address()).unwrap()
}
#[cfg(windows)]
fn raw_connection(endpoint: &Endpoint) -> fs::File {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        match fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(endpoint.address())
        {
            Ok(connection) => return connection,
            Err(error)
                if error.raw_os_error()
                    == Some(windows_sys::Win32::Foundation::ERROR_PIPE_BUSY as i32)
                    && Instant::now() < until =>
            {
                thread::sleep(Duration::from_millis(10))
            }
            Err(error) => panic!("isolated raw pipe connection failed: {error}"),
        }
    }
}

#[cfg(unix)]
#[test]
fn symlinks_weak_permissions_and_non_socket_endpoint_are_rejected_without_repair() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = root();
    let elsewhere = root.path().join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    symlink(&elsewhere, root.path().join("control")).unwrap();
    assert!(matches!(
        Owner::acquire(root.path()),
        Err(Error::UnsafeEndpoint)
    ));
    fs::remove_file(root.path().join("control")).unwrap();
    fs::create_dir(root.path().join("control")).unwrap();
    fs::set_permissions(
        root.path().join("control"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    assert!(matches!(
        Owner::acquire(root.path()),
        Err(Error::UnsafeEndpoint)
    ));
    assert_eq!(
        fs::metadata(root.path().join("control"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    fs::set_permissions(
        root.path().join("control"),
        fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let owner = common::acquire(root.path()).unwrap();
    fs::write(owner.endpoint().address(), b"unrelated").unwrap();
    let stop = StopSignal::default();
    let path = owner.endpoint().address().to_owned();
    assert!(matches!(
        Server::new(owner).run(stop, |_, value| Ok(value)),
        Err(Error::UnsafeEndpoint)
    ));
    assert_eq!(fs::read(path).unwrap(), b"unrelated");
}

#[test]
fn launch_rejects_relative_executable() {
    let root = root();
    assert!(matches!(
        launch::spawn_detached(&LaunchSpec {
            executable: PathBuf::from("devforge"),
            arguments: vec![],
            data_root: root.path().into(),
            environment: BTreeMap::new()
        }),
        Err(Error::InvalidLaunch)
    ));
}

/// Process-only transport fixture. These probe messages are not public product
/// tools and no fake Workspace, runtime or external capability is advertised.
#[test]
#[ignore = "spawned by isolated infrastructure tests"]
fn subprocess_fixture() {
    let mode = std::env::var("DEVFORGE_MANAGER_FIXTURE_MODE").unwrap();
    let root = PathBuf::from(std::env::var_os("DEVFORGE_MANAGER_FIXTURE_ROOT").unwrap());
    if mode == "owner" {
        let stop = StopSignal::default();
        let shutdown = stop.clone();
        Server::new(common::acquire(&root).unwrap()).run(stop, move |peer, input| {
            let stop = input["probe"] == "stop";
            let response = json!({"owner":std::process::id(),"client":input["client"],"peer_user":peer.user});
            if stop { shutdown.request(); }
            Ok(response)
        }).unwrap();
    } else if mode.starts_with("client-") {
        let client = Client::new(Endpoint::existing(&root).unwrap());
        let result = client
            .request(&json!({"probe":"ping","client":std::process::id()}))
            .unwrap();
        fs::write(
            root.join(format!("{mode}.json")),
            serde_json::to_vec(&result).unwrap(),
        )
        .unwrap();
    } else if mode == "launcher" {
        let mut environment = BTreeMap::new();
        environment.insert("DEVFORGE_MANAGER_FIXTURE_MODE".into(), "owner".into());
        environment.insert(
            "DEVFORGE_MANAGER_FIXTURE_ROOT".into(),
            root.as_os_str().to_owned(),
        );
        #[cfg(windows)]
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            environment.insert("SystemRoot".into(), system_root);
        }
        let result = launch::spawn_detached(&LaunchSpec {
            executable: std::env::current_exe().unwrap(),
            arguments: ["--exact", "subprocess_fixture", "--ignored", "--nocapture"]
                .map(Into::into)
                .to_vec(),
            data_root: root.clone(),
            environment,
        });
        let output = match result {
            Ok(child) => child.id().to_string(),
            Err(Error::RestrictedJob) => "manager_restricted_parent_job".into(),
            Err(error) => panic!("detached launch failed: {error}"),
        };
        fs::write(root.join("launch-result"), output).unwrap();
    } else {
        panic!("unknown fixture mode");
    }
}
