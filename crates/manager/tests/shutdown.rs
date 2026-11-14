mod common;

use devforge_control::Id;
use devforge_core::{ErrorCode, SecretStore};
use devforge_manager::{
    Client,
    auth::{DesktopAuth, DesktopCredential, Policy, TrustedDesktopAction},
    controller::{Controller, Error as ControllerError, StartIntent, Stores, local_owner_user},
    desktop::{AccessRequest, SecretValue},
    native_client::{Error, NativeClient},
};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Default)]
struct MemoryStore(Mutex<BTreeMap<String, String>>);
impl SecretStore for MemoryStore {
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
struct PanicStore(AtomicBool);
impl SecretStore for PanicStore {
    fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
        Ok(None)
    }
    fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
        self.0.store(true, Ordering::Release);
        panic!("isolated worker failure: never a lifecycle payload")
    }
    fn delete(&self, _: &str) -> Result<(), ErrorCode> {
        Ok(())
    }
}
fn provision(root: &Path, store: &MemoryStore) -> DesktopCredential {
    let owner = common::acquire(root).unwrap();
    DesktopAuth::new(
        owner.endpoint().clone(),
        local_owner_user().unwrap(),
        Policy::default(),
    )
    .unwrap()
    .rotate_desktop_key(
        &TrustedDesktopAction::from_explicit_desktop_action(),
        store,
        Instant::now(),
    )
    .unwrap()
}
fn start(
    controller: Controller,
    key: &DesktopCredential,
) -> (
    NativeClient,
    thread::JoinHandle<devforge_manager::controller::Result<()>>,
) {
    let endpoint = controller.endpoint().clone();
    let probe = Client::new(endpoint.clone());
    let worker = thread::spawn(move || controller.serve(None));
    let deadline = Instant::now() + Duration::from_secs(5);
    while probe
        .request(&serde_json::json!({"method":"health"}))
        .is_err()
    {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    (
        NativeClient::authenticate_explicit(endpoint, key).unwrap(),
        worker,
    )
}
fn assert_code(result: Result<(), Error>, code: &str) {
    assert_eq!(result.unwrap_err().code(), code);
}

#[test]
fn successful_cleanup_is_owner_bound_and_a_later_owner_invalidates_old_success() {
    let root = common::root("df-shutdown-");
    let store = Arc::new(MemoryStore::default());
    let key = provision(root.path(), &store);
    let stores = || Stores {
        workspace: Arc::new(MemoryStore::default()),
        desktop: store.clone(),
    };
    let controller = Controller::start(root.path(), StartIntent::AutoStart, stores()).unwrap();
    let (mut client, worker) = start(controller, &key);
    assert_code(client.confirm_shutdown(), "controller_shutdown_unconfirmed");
    client.quit().unwrap();
    worker.join().unwrap().unwrap();
    client.confirm_shutdown().unwrap();

    let next = Controller::start(root.path(), StartIntent::ExplicitUserStart, stores()).unwrap();
    assert_code(client.confirm_shutdown(), "controller_busy");
    drop(next);
    assert_code(client.confirm_shutdown(), "controller_shutdown_unconfirmed");
}

#[test]
fn actual_worker_failure_is_not_mistaken_for_successful_cleanup_when_lease_releases() {
    use devforge_projects::project::{Catalog, ProjectStore};
    use devforge_runtime::package::Target;
    let root = common::root("df-shutdown-fail-");
    let source = root.path().join("project");
    std::fs::create_dir_all(source.join("public")).unwrap();
    std::fs::write(source.join("public/index.php"), "<?php").unwrap();
    std::fs::write(
        source.join("composer.json"),
        r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )
    .unwrap();
    let catalog = Catalog::bundled();
    let target = Target::current().unwrap();
    let select = |kind| {
        catalog
            .packages
            .iter()
            .find(|(_, package)| package.kind == kind && package.target == target)
            .unwrap()
            .0
            .clone()
    };
    let project = ProjectStore::open(root.path().join("projects"))
        .unwrap()
        .import(
            &source,
            "Shutdown fixture",
            &select("php"),
            &select("node"),
            &catalog,
        )
        .unwrap();
    let control = Arc::new(MemoryStore::default());
    let key = provision(root.path(), &control);
    let failure = Arc::new(PanicStore(AtomicBool::new(false)));
    let controller = Controller::start(
        root.path(),
        StartIntent::AutoStart,
        Stores {
            workspace: failure.clone(),
            desktop: control,
        },
    )
    .unwrap();
    let (mut client, worker) = start(controller, &key);
    client
        .desktop_access(
            &Id::new("shutdown-worker-failure").unwrap(),
            &AccessRequest::ConfigurationSecret {
                project_id: Id::new(project.id).unwrap(),
                reference: "fixture".into(),
                value: SecretValue::new("in-memory-only").unwrap(),
            },
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !failure.0.load(Ordering::Acquire) {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    client.quit().unwrap();
    assert_eq!(
        worker.join().unwrap().unwrap_err(),
        ControllerError::ActorUnavailable
    );
    assert!(devforge_platform::distribution::AppLease::acquire(root.path()).is_ok());
    assert_code(client.confirm_shutdown(), "controller_shutdown_failed");
    let marker = std::fs::read_to_string(root.path().join("control/quit-inhibited.json")).unwrap();
    assert!(!marker.contains("isolated worker failure"));
}
