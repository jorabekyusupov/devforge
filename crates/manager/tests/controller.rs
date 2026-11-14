mod common;
use devforge_core::{ErrorCode, SecretStore};
use devforge_manager::{
    Client, Owner,
    auth::{DesktopAuth, Policy, TrustedDesktopAction},
    controller::{Controller, Error, StartIntent, Stores, local_owner_user},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Default)]
struct MemorySecrets {
    values: Mutex<BTreeMap<String, String>>,
    gets: AtomicUsize,
    delay_ms: AtomicU64,
}
impl SecretStore for MemorySecrets {
    fn get(&self, key: &str) -> Result<Option<String>, ErrorCode> {
        self.gets.fetch_add(1, Ordering::SeqCst);
        thread::sleep(Duration::from_millis(self.delay_ms.load(Ordering::SeqCst)));
        Ok(self.values.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &str, value: &str) -> Result<(), ErrorCode> {
        self.values.lock().unwrap().insert(key.into(), value.into());
        Ok(())
    }
    fn delete(&self, key: &str) -> Result<(), ErrorCode> {
        self.values.lock().unwrap().remove(key);
        Ok(())
    }
}
struct NoWorkspaceSecrets;
impl SecretStore for NoWorkspaceSecrets {
    fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
        panic!("controller metadata must not read native secrets")
    }
    fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
        panic!("controller metadata must not write native secrets")
    }
    fn delete(&self, _: &str) -> Result<(), ErrorCode> {
        panic!("controller metadata must not delete native secrets")
    }
}
fn stores(desktop: Arc<MemorySecrets>) -> Stores {
    Stores {
        workspace: Arc::new(NoWorkspaceSecrets),
        desktop,
    }
}
fn provision(root: &Path, store: &MemorySecrets) -> String {
    let owner = common::acquire(root).unwrap();
    let mut auth = DesktopAuth::new(
        owner.endpoint().clone(),
        local_owner_user().unwrap(),
        Policy::default(),
    )
    .unwrap();
    let key = auth
        .rotate_desktop_key(
            &TrustedDesktopAction::from_explicit_desktop_action(),
            store,
            Instant::now(),
        )
        .unwrap();
    key.expose_secret().to_owned()
}
fn running(
    root: &Path,
    desktop: Arc<MemorySecrets>,
) -> (Client, JoinHandle<devforge_manager::controller::Result<()>>) {
    let controller = Controller::start(root, StartIntent::AutoStart, stores(desktop)).unwrap();
    let client = Client::new(controller.endpoint().clone());
    let worker = thread::spawn(move || controller.serve(None));
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if client.request(&json!({"method":"health"})).is_ok() {
            break;
        }
        assert!(Instant::now() < until, "controller did not become ready");
        thread::sleep(Duration::from_millis(10));
    }
    (client, worker)
}
fn authenticate(client: &Client, key: &str) -> String {
    let response = client
        .request(&json!({"method":"authenticate","credential":key}))
        .unwrap();
    assert_eq!(response["ok"], true);
    response["data"]["session"].as_str().unwrap().to_owned()
}
fn quit(
    client: &Client,
    session: &str,
    worker: JoinHandle<devforge_manager::controller::Result<()>>,
) {
    let response = client
        .request(&json!({"method":"quit","session":session}))
        .unwrap();
    assert_eq!(response["ok"], true);
    assert_eq!(response["data"]["state"], "quitting");
    worker.join().unwrap().unwrap();
}

#[test]
fn unauthenticated_health_is_sanitized_and_native_session_controls_real_actor() {
    let root = common::root("df-controller-");
    let secrets = Arc::new(MemorySecrets::default());
    let key = provision(root.path(), secrets.as_ref());
    let (client, worker) = running(root.path(), secrets.clone());
    assert_eq!(
        secrets.gets.load(Ordering::SeqCst),
        0,
        "startup/health cannot probe the credential store"
    );
    let health = client.request(&json!({"method":"health"})).unwrap();
    assert_eq!(
        health["data"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec!["api_version", "owner_pid", "state"]
    );
    assert_eq!(health["data"]["state"], "ready");
    for request in [
        json!({"method":"health","role":"desktop"}),
        json!({"method":"snapshot","session":"0".repeat(64),"role":"desktop"}),
        json!({"method":"explicit_user_start"}),
    ] {
        assert_eq!(
            client.request(&request).unwrap()["error"]["code"],
            "controller_invalid_request"
        );
    }
    assert_eq!(
        client
            .request(&json!({"method":"snapshot","session":"0".repeat(64)}))
            .unwrap()["error"]["code"],
        "manager_auth_unauthorized"
    );
    assert_eq!(secrets.gets.load(Ordering::SeqCst), 0);
    let session = authenticate(&client, &key);
    let snapshot = client
        .request(&json!({"method":"snapshot","session":session}))
        .unwrap();
    assert_eq!(snapshot["ok"], true);
    assert!(
        snapshot["data"]["snapshot"]["projects"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let submitted=client.request(&json!({"method":"submit","session":session,"project_id":"missing-project","idempotency_key":"controller-job","command":{"action":"visit","id":"missing-project"}})).unwrap();
    assert_eq!(submitted["ok"], true);
    let operation = submitted["data"]["operation_id"].as_str().unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let response=client.request(&json!({"method":"status","session":session,"project_id":"missing-project","operation_id":operation})).unwrap();
        if response["data"]["state"] == "failed" {
            break;
        }
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(client.request(&json!({"method":"failure_detail","session":session,"project_id":"missing-project","operation_id":operation})).unwrap()["data"]["code"],"not_found");
    assert_eq!(client.request(&json!({"method":"result","session":session,"project_id":"missing-project","operation_id":operation})).unwrap()["error"]["code"],"actor_operation_failed");
    let renewed = client
        .request(&json!({"method":"renew","session":session}))
        .unwrap();
    let next = renewed["data"]["session"].as_str().unwrap();
    assert!(next != session);
    assert_eq!(
        client
            .request(&json!({"method":"snapshot","session":session}))
            .unwrap()["error"]["code"],
        "manager_auth_unauthorized"
    );
    assert_eq!(
        secrets.gets.load(Ordering::SeqCst),
        1,
        "renewal/commands never re-probe native secrets"
    );
    quit(&client, next, worker);
    assert!(matches!(
        Controller::start(root.path(), StartIntent::AutoStart, stores(secrets.clone())),
        Err(Error::StartInhibited)
    ));
    let explicit = Controller::start(
        root.path(),
        StartIntent::ExplicitUserStart,
        stores(secrets.clone()),
    )
    .unwrap();
    drop(explicit);
    let state: Value =
        serde_json::from_slice(&fs::read(root.path().join("control/quit-inhibited.json")).unwrap())
            .unwrap();
    assert_eq!(state["inhibited"], false);
    assert_eq!(secrets.gets.load(Ordering::SeqCst), 1);
}

#[test]
fn native_store_wait_does_not_block_health_or_event_loop() {
    let root = common::root("df-controller-");
    let secrets = Arc::new(MemorySecrets::default());
    let key = provision(root.path(), secrets.as_ref());
    secrets.delay_ms.store(1000, Ordering::SeqCst);
    let (client, worker) = running(root.path(), secrets.clone());
    let authenticating = client.clone();
    let login = thread::spawn(move || authenticate(&authenticating, &key));
    let until = Instant::now() + Duration::from_secs(5);
    while secrets.gets.load(Ordering::SeqCst) == 0 {
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(5));
    }
    let started = Instant::now();
    let health = client.request(&json!({"method":"health"})).unwrap();
    assert_eq!(health["data"]["state"], "ready");
    assert!(started.elapsed() < Duration::from_millis(400));
    let started = Instant::now();
    let denied = client
        .request(&json!({"method":"snapshot","session":"0".repeat(64)}))
        .unwrap();
    assert_eq!(denied["error"]["code"], "controller_busy");
    assert!(started.elapsed() < Duration::from_millis(400));
    let session = login.join().unwrap();
    quit(&client, &session, worker);
}

#[test]
fn failed_quit_persistence_keeps_controller_authenticated_and_usable() {
    let root = common::root("df-controller-");
    let secrets = Arc::new(MemorySecrets::default());
    let key = provision(root.path(), secrets.as_ref());
    let (client, worker) = running(root.path(), secrets);
    let session = authenticate(&client, &key);
    let target = root.path().join("control/quit-inhibited.json");
    fs::create_dir(&target).unwrap();
    let failed = client
        .request(&json!({"method":"quit","session":session}))
        .unwrap();
    assert_eq!(
        failed["error"]["code"],
        "controller_persistence_unavailable"
    );
    assert_eq!(
        client.request(&json!({"method":"health"})).unwrap()["data"]["state"],
        "ready"
    );
    assert_eq!(
        client
            .request(&json!({"method":"snapshot","session":session}))
            .unwrap()["ok"],
        true
    );
    let accepted=client.request(&json!({"method":"submit","session":session,"project_id":"missing-project","idempotency_key":"after-failed-quit","command":{"action":"visit","id":"missing-project"}})).unwrap();
    assert_eq!(accepted["ok"], true);
    fs::remove_dir(target).unwrap();
    quit(&client, &session, worker);
}

#[test]
fn legacy_desktop_lease_excludes_controller_before_workspace_construction() {
    let root = common::root("df-controller-");
    let desktop = devforge_platform::distribution::AppLease::acquire(root.path()).unwrap();
    let secrets = Arc::new(MemorySecrets::default());
    assert!(matches!(
        Controller::start(root.path(), StartIntent::AutoStart, stores(secrets.clone())),
        Err(Error::LegacyDesktopActive)
    ));
    assert!(!root.path().join("projects").exists());
    assert_eq!(secrets.gets.load(Ordering::SeqCst), 0);
    drop(desktop);
    let controller =
        Controller::start(root.path(), StartIntent::AutoStart, stores(secrets)).unwrap();
    drop(controller);
    assert!(Owner::acquire(root.path()).is_ok());
}

#[test]
fn authenticated_subject_advice_is_typed_and_never_replaces_actual_admission_checks() {
    use devforge_manager::desktop::SubjectSelector;
    use devforge_projects::project::{Catalog, ProjectStore};
    use devforge_runtime::package::Target;
    let root = common::root("df-selector-");
    let source = root.path().join("fixture-project");
    fs::create_dir_all(source.join("public")).unwrap();
    fs::write(source.join("public/index.php"), "<?php").unwrap();
    fs::write(
        source.join("composer.json"),
        r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )
    .unwrap();
    let catalog = Catalog::bundled();
    let target = Target::current().unwrap();
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
    let registry = ProjectStore::open(root.path().join("projects")).unwrap();
    let project = registry
        .import(&source, "Selector fixture", &php, &node, &catalog)
        .unwrap();
    drop(registry);
    let secrets = Arc::new(MemorySecrets::default());
    let credential = provision(root.path(), secrets.as_ref());
    let (client, worker) = running(root.path(), secrets.clone());
    assert_eq!(client.request(&json!({"method":"resolve_subject","session":"0".repeat(64),"selector":{"kind":"root"}})).unwrap()["error"]["code"],"manager_auth_unauthorized");
    assert_eq!(secrets.gets.load(Ordering::SeqCst), 0);
    let session = authenticate(&client, &credential);
    let selector = SubjectSelector::from_command(&devforge_workspace::Command::Visit {
        id: project.id.clone(),
    })
    .unwrap();
    let advice = client
        .request(&json!({"method":"resolve_subject","session":session,"selector":selector}))
        .unwrap();
    assert_eq!(advice["data"]["advisory"], true);
    assert_eq!(advice["data"]["project_id"], project.id);
    assert_eq!(
        client
            .request(
                &json!({"method":"resolve_subject","session":session,"selector":{"kind":"root"}})
            )
            .unwrap()["data"]["project_id"],
        "desktop-root"
    );
    assert_eq!(client.request(&json!({"method":"resolve_subject","session":session,"selector":{"kind":"project","project_id":project.id,"raw_command":{"password":"not-sent"}}})).unwrap()["error"]["code"],"controller_invalid_request");
    let rejected=client.request(&json!({"method":"submit","session":session,"project_id":project.id,"idempotency_key":"advice-not-authority","command":{"action":"visit","id":"different-project"}})).unwrap();
    assert_eq!(rejected["error"]["code"], "actor_subject_mismatch");
    let forgotten=client.request(&json!({"method":"submit","session":session,"project_id":project.id,"idempotency_key":"forget-after-advice","command":{"action":"forget","id":project.id,"revision":project.revision}})).unwrap();
    assert_eq!(forgotten["ok"], true);
    let operation = forgotten["data"]["operation_id"].as_str().unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let status=client.request(&json!({"method":"status","session":session,"project_id":project.id,"operation_id":operation})).unwrap();
        if status["data"]["state"] == "succeeded" {
            break;
        }
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(client.request(&json!({"method":"resolve_subject","session":session,"selector":{"kind":"project","project_id":project.id}})).unwrap()["error"]["code"],"actor_subject_mismatch");
    let stale=client.request(&json!({"method":"submit","session":session,"project_id":project.id,"idempotency_key":"stale-advice","command":{"action":"visit","id":project.id}})).unwrap();
    assert_eq!(stale["ok"], true);
    let operation = stale["data"]["operation_id"].as_str().unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let status=client.request(&json!({"method":"status","session":session,"project_id":project.id,"operation_id":operation})).unwrap();
        if status["data"]["state"] == "failed" {
            break;
        }
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(5));
    }
    quit(&client, &session, worker);
}

#[test]
fn trusted_os_quit_uses_same_persistence_gate_and_stale_handle_cannot_change_later_owner() {
    let root = common::root("df-controller-");
    let secrets = Arc::new(MemorySecrets::default());
    let controller =
        Controller::start(root.path(), StartIntent::AutoStart, stores(secrets.clone())).unwrap();
    let signal = controller.quitter();
    let client = Client::new(controller.endpoint().clone());
    let worker = thread::spawn(move || controller.serve(None));
    let until = Instant::now() + Duration::from_secs(5);
    while client.request(&json!({"method":"health"})).is_err() {
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(10));
    }
    let marker = root.path().join("control/quit-inhibited.json");
    fs::create_dir(&marker).unwrap();
    assert_eq!(
        signal.request_explicit().unwrap_err(),
        Error::PersistenceUnavailable
    );
    assert_eq!(
        client.request(&json!({"method":"health"})).unwrap()["data"]["state"],
        "ready"
    );
    assert_eq!(
        secrets.gets.load(Ordering::SeqCst),
        0,
        "OS-signal quit cannot require native credentials"
    );
    fs::remove_dir(marker).unwrap();
    signal.request_explicit().unwrap();
    worker.join().unwrap().unwrap();
    assert!(matches!(
        Controller::start(root.path(), StartIntent::AutoStart, stores(secrets.clone())),
        Err(Error::StartInhibited)
    ));
    let later =
        Controller::start(root.path(), StartIntent::ExplicitUserStart, stores(secrets)).unwrap();
    assert_eq!(
        signal.request_explicit().unwrap_err(),
        Error::OwnerUnavailable
    );
    let state: Value =
        serde_json::from_slice(&fs::read(root.path().join("control/quit-inhibited.json")).unwrap())
            .unwrap();
    assert_eq!(state["inhibited"], false);
    drop(later);
}

#[test]
fn trusted_quit_before_listener_start_never_advertises_readiness() {
    let root = common::root("df-controller-");
    let secrets = Arc::new(MemorySecrets::default());
    let controller =
        Controller::start(root.path(), StartIntent::AutoStart, stores(secrets.clone())).unwrap();
    let client = Client::new(controller.endpoint().clone());
    controller.quitter().request_explicit().unwrap();
    controller.serve(None).unwrap();
    assert!(client.request(&json!({"method":"health"})).is_err());
    assert_eq!(secrets.gets.load(Ordering::SeqCst), 0);
    assert!(matches!(
        Controller::start(root.path(), StartIntent::AutoStart, stores(secrets)),
        Err(Error::StartInhibited)
    ));
}

#[cfg(unix)]
#[test]
fn quit_rejects_dangling_marker_without_outside_write_or_stopping_actor() {
    use std::os::unix::fs::symlink;
    let root = common::root("df-controller-");
    let outside = common::root("df-marker-");
    let forbidden = outside.path().join("must-not-create");
    let secrets = Arc::new(MemorySecrets::default());
    let key = provision(root.path(), secrets.as_ref());
    let (client, worker) = running(root.path(), secrets);
    let session = authenticate(&client, &key);
    let marker = root.path().join("control/quit-inhibited.json");
    symlink(&forbidden, &marker).unwrap();
    assert_eq!(
        client
            .request(&json!({"method":"quit","session":session}))
            .unwrap()["error"]["code"],
        "controller_persistence_unavailable"
    );
    assert!(!forbidden.exists());
    assert_eq!(fs::read_link(&marker).unwrap(), forbidden);
    assert_eq!(
        client
            .request(&json!({"method":"snapshot","session":session}))
            .unwrap()["ok"],
        true
    );
    fs::remove_file(marker).unwrap();
    quit(&client, &session, worker);
}
