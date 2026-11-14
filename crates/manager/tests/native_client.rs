//! Custom harness supports a real private bootstrap subprocess, without libtest
//! banners in its stdout pipe. All credentials are disposable memory fixtures.
mod common;
use devforge_control::{Id, OperationState};
use devforge_core::{ErrorCode, SecretStore};
use devforge_manager::{
    Endpoint,
    actor::ROOT_SUBJECT,
    auth::{DesktopAuth, DesktopCredential, NativeGuiPolicy, Policy, TrustedDesktopAction},
    bootstrap::{BootstrapError, BootstrapWriter},
    controller::{Controller, Options, StartIntent, Stores, local_owner_user},
    launch::{self, LaunchSpec},
    native_client::{Cancellation, Error, NativeClient},
};
use devforge_projects::project::{Catalog, ProjectStore};
use devforge_runtime::package::Target;
use devforge_workspace::Command;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

#[derive(Default)]
struct MemoryStore {
    values: Mutex<BTreeMap<String, String>>,
    gets: AtomicUsize,
}
impl SecretStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<String>, ErrorCode> {
        self.gets.fetch_add(1, Ordering::SeqCst);
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
struct DenyStore;
impl SecretStore for DenyStore {
    fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
        panic!("SDK fixture must not probe native/workspace secrets")
    }
    fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
        panic!("SDK fixture must not write native/workspace secrets")
    }
    fn delete(&self, _: &str) -> Result<(), ErrorCode> {
        panic!("SDK fixture must not delete native/workspace secrets")
    }
}
fn main() {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.first().is_some_and(|v| v == "fixture") {
        cold_fixture(Path::new(&args[1]));
        return;
    }
    explicit_auth_operations_and_large_command();
    real_large_snapshot_is_complete_and_unicode_safe();
    explicit_renewal_expires_at_absolute_context_deadline();
    let cold = cold_bootstrap_and_owner_replacement();
    let unavailable = usize::from(cold != "passed");
    println!(
        "{}",
        serde_json::json!({"suite":"native_client","overall":if unavailable==0{"passed"}else{"partial"},"passed":4-unavailable,"unavailable":unavailable,
        "cases":[{"case":"explicit_auth_operations_large_command","status":"passed"},{"case":"actual_large_snapshot","status":"passed"},{"case":"renewal_absolute_deadline","status":"passed"},
            {"case":"detached_cold_bootstrap_owner_replacement","status":if cold=="passed"{"passed"}else{"unavailable"},"reason":if cold=="passed"{None}else{Some("restricted_ancestor_job")}}]})
    );
}
fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}
fn provision(root: &Path, store: &MemoryStore) -> DesktopCredential {
    let owner = common::acquire(root).unwrap();
    let mut auth = DesktopAuth::new(
        owner.endpoint().clone(),
        local_owner_user().unwrap(),
        Policy::default(),
    )
    .unwrap();
    auth.rotate_desktop_key(
        &TrustedDesktopAction::from_explicit_desktop_action(),
        store,
        Instant::now(),
    )
    .unwrap()
}
fn running(
    root: &Path,
    store: Arc<MemoryStore>,
    options: Options,
) -> (
    Endpoint,
    JoinHandle<devforge_manager::controller::Result<()>>,
    devforge_manager::controller::OsQuitHandle,
) {
    let controller = Controller::with_options(
        root,
        StartIntent::ExplicitUserStart,
        Stores {
            workspace: Arc::new(DenyStore),
            desktop: store,
        },
        options,
    )
    .unwrap();
    let endpoint = controller.endpoint().clone();
    let quit = controller.quitter();
    let handle = thread::spawn(move || controller.serve(None));
    let client = devforge_manager::Client::new(endpoint.clone());
    let started = Instant::now();
    while client
        .request(&serde_json::json!({"method":"health"}))
        .is_err()
    {
        assert!(started.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(5));
    }
    (endpoint, handle, quit)
}
fn complete(client: &mut NativeClient, project: &Id, operation: &Id) -> OperationState {
    let started = Instant::now();
    loop {
        let receipt = client.status(project, operation).unwrap();
        if receipt.state.terminal() {
            return receipt.state;
        }
        assert!(started.elapsed() < Duration::from_secs(10));
        thread::sleep(Duration::from_millis(5));
    }
}
fn laravel(path: &Path) {
    fs::create_dir_all(path.join("public")).unwrap();
    fs::write(path.join("public/index.php"), "<?php").unwrap();
    fs::write(
        path.join("composer.json"),
        r#"{"require":{"laravel/framework":"^13.0","php":">=8.0"}}"#,
    )
    .unwrap();
}
fn runtime_ids(catalog: &Catalog) -> (String, String) {
    let target = Target::current().unwrap();
    let php = catalog
        .packages
        .iter()
        .find(|(id, p)| {
            p.target == target
                && p.kind == "php"
                && p.executables.contains_key("php")
                && !catalog.superseded.contains_key(*id)
        })
        .unwrap()
        .0
        .clone();
    let node = catalog
        .packages
        .iter()
        .find(|(id, p)| {
            p.target == target && p.kind == "node" && !catalog.superseded.contains_key(*id)
        })
        .unwrap()
        .0
        .clone();
    (php, node)
}
fn explicit_auth_operations_and_large_command() {
    let root = common::root("df-sdk-");
    let store = Arc::new(MemoryStore::default());
    let key = provision(root.path(), &store);
    let (endpoint, worker, _quit) = running(root.path(), store.clone(), Options::default());
    assert_eq!(store.gets.load(Ordering::SeqCst), 0);
    let mut client = NativeClient::authenticate_explicit(endpoint, &key).unwrap();
    assert!(client.session_info().unwrap().renewable);
    assert!(!format!("{client:?}").contains(key.expose_secret()));
    let app = root.path().join("application");
    laravel(&app);
    let (php, node) = runtime_ids(&Catalog::bundled());
    let input = Command::Import {
        path: app,
        name: "SDK ўзбек Русский 🦀".into(),
        php,
        node,
        domain: "sdk.devforge.test".into(),
        https: false,
        vite: false,
        queue: false,
    };
    let subject = id(ROOT_SUBJECT);
    let receipt = client.admit(&subject, &id("sdk-import"), &input).unwrap();
    assert_eq!(
        complete(&mut client, &subject, &receipt.operation_id),
        OperationState::Succeeded
    );
    let project = client.result(&subject, &receipt.operation_id).unwrap();
    let project_id = id(project["project_id"].as_str().unwrap());
    let replay = client.admit(&subject, &id("sdk-import"), &input).unwrap();
    assert_eq!(replay.operation_id, receipt.operation_id);
    let favorite = client
        .admit(
            &project_id,
            &id("sdk-favorite"),
            &Command::Favorite {
                id: project_id.as_str().into(),
                favorite: true,
            },
        )
        .unwrap();
    assert_eq!(
        complete(&mut client, &project_id, &favorite.operation_id),
        OperationState::Succeeded
    );
    assert_eq!(
        client.cancel(&project_id, &favorite.operation_id).unwrap(),
        Cancellation::AlreadyTerminal
    );
    let command = Command::ConfigurationSave {
        id: project_id.as_str().into(),
        expected: None,
        text: "# 🦀 é SDK_BODY_NOT_A_SECRET\n".repeat(45_000),
        consent: true,
    };
    assert!(serde_json::to_vec(&command).unwrap().len() > 1_048_576);
    let failed = client
        .admit(&project_id, &id("sdk-large-command"), &command)
        .unwrap();
    assert_eq!(
        complete(&mut client, &project_id, &failed.operation_id),
        OperationState::Failed
    );
    assert!(matches!(
        client.result(&project_id, &failed.operation_id),
        Err(Error::Remote(_))
    ));
    let detail = client
        .failure_detail(&project_id, &failed.operation_id)
        .unwrap();
    assert_eq!(detail.project_id, project_id);
    assert!(!detail.code.contains("SDK_BODY"));
    let snapshot = client.snapshot().unwrap();
    assert_eq!(snapshot.snapshot["projects"].as_array().unwrap().len(), 1);
    assert_eq!(
        snapshot.snapshot["projects"][0]["navigation"]["favorite"],
        true
    );
    let _ = client.renew_explicit().unwrap();
    assert_eq!(store.gets.load(Ordering::SeqCst), 1);
    client.quit().unwrap();
    worker.join().unwrap().unwrap();
    assert_eq!(
        client.session_info().unwrap_err(),
        Error::ReauthenticationRequired
    );
}
fn real_large_snapshot_is_complete_and_unicode_safe() {
    let root = common::root("df-sdk-snap-");
    let store = Arc::new(MemoryStore::default());
    let key = provision(root.path(), &store);
    let catalog = Catalog::bundled();
    let (php, node) = runtime_ids(&catalog);
    let projects = ProjectStore::open(root.path().join("projects")).unwrap();
    const COUNT: usize = 1_000;
    for n in 0..COUNT {
        let path = root.path().join("fixtures").join(n.to_string());
        laravel(&path);
        projects
            .import(
                &path,
                &format!("Қатор Русский 🦀 {n}"),
                &php,
                &node,
                &catalog,
            )
            .unwrap();
    }
    drop(projects);
    let (endpoint, worker, _quit) = running(root.path(), store, Options::default());
    let mut client = NativeClient::authenticate_explicit(endpoint, &key).unwrap();
    let snapshot = client.snapshot().unwrap();
    assert_eq!(
        snapshot.snapshot["projects"].as_array().unwrap().len(),
        COUNT
    );
    assert!(serde_json::to_vec(&snapshot).unwrap().len() > devforge_manager::MAX_RESPONSE_BYTES);
    assert!(
        snapshot.snapshot["projects"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["project"]["name"].as_str().unwrap().contains("🦀"))
    );
    client.quit().unwrap();
    worker.join().unwrap().unwrap();
}
fn explicit_renewal_expires_at_absolute_context_deadline() {
    let root = common::root("df-sdk-renew-");
    let store = Arc::new(MemoryStore::default());
    let key = provision(root.path(), &store);
    let options = Options {
        auth: Policy {
            session_lifetime: Duration::from_millis(1_500),
            ..Policy::default()
        },
        native_gui: NativeGuiPolicy {
            context_lifetime: Duration::from_secs(3),
            ..NativeGuiPolicy::default()
        },
        ..Options::default()
    };
    let (endpoint, worker, quit) = running(root.path(), store.clone(), options);
    let mut client = NativeClient::authenticate_explicit(endpoint, &key).unwrap();
    let started = Instant::now();
    thread::sleep(Duration::from_millis(600));
    let context_before = client.session_info().unwrap().context_remaining_ms.unwrap();
    let renewed = client.renew_explicit().unwrap();
    assert!(renewed.context_remaining_ms.unwrap() <= context_before);
    thread::sleep(Duration::from_millis(1_100));
    assert!(started.elapsed() > Duration::from_millis(1_500));
    assert!(client.snapshot().is_ok());
    // Without an absolute bound, this second renewal would keep the token alive
    // beyond3s. It must instead be clamped to the original native context.
    let last = client.renew_explicit().unwrap();
    assert!(last.expires_in_ms <= last.context_remaining_ms.unwrap());
    while started.elapsed() < Duration::from_millis(3_100) {
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        client.renew_explicit().unwrap_err(),
        Error::ReauthenticationRequired
    );
    assert_eq!(store.gets.load(Ordering::SeqCst), 1);
    quit.request_explicit().unwrap();
    worker.join().unwrap().unwrap();
}
fn cold_fixture(root: &Path) {
    let writer = BootstrapWriter::capture_inherited_stdout().unwrap();
    let controller = Controller::start(
        root,
        StartIntent::ExplicitUserStart,
        Stores {
            workspace: Arc::new(DenyStore),
            desktop: Arc::new(DenyStore),
        },
    )
    .unwrap();
    let quit = controller.quitter();
    let stop = root.join("stop-fixture");
    let watcher = thread::spawn(move || {
        let started = Instant::now();
        while !stop.exists() && started.elapsed() < Duration::from_secs(30) {
            thread::sleep(Duration::from_millis(5));
        }
        let _ = quit.request_explicit();
    });
    controller.serve(Some(writer)).unwrap();
    watcher.join().unwrap();
}
fn executable_digest(path: &Path) -> [u8; 32] {
    let mut file = fs::File::open(path).unwrap();
    let mut hash = Sha256::new();
    let mut bytes = [0; 65_536];
    loop {
        let n = file.read(&mut bytes).unwrap();
        if n == 0 {
            break;
        }
        hash.update(&bytes[..n]);
    }
    hash.finalize().into()
}
fn cold(root: &Path) -> Option<launch::BootstrappedChild> {
    let exe = std::env::current_exe().unwrap();
    let mut environment = BTreeMap::new();
    if let Some(v) = std::env::var_os("SystemRoot") {
        environment.insert("SystemRoot".into(), v);
    }
    let spec = LaunchSpec {
        executable: exe.clone(),
        arguments: vec!["fixture".into(), root.as_os_str().to_owned()],
        data_root: root.to_owned(),
        environment,
    };
    match launch::spawn_with_private_bootstrap(&spec, &executable_digest(&exe)) {
        Ok(child) => Some(
            child
                .wait_ready(Duration::from_secs(5), |_| Err(BootstrapError::NotReady))
                .unwrap(),
        ),
        Err(devforge_manager::Error::RestrictedJob) => None,
        Err(error) => panic!("isolated SDK cold launch: {error}"),
    }
}
fn cold_bootstrap_and_owner_replacement() -> &'static str {
    let root = common::root("df-sdk-cold-");
    let Some(child) = cold(root.path()) else {
        return "unavailable_restricted_job";
    };
    let token = Zeroizing::new(child.credential().expose_secret().to_owned());
    let mut first =
        NativeClient::from_bootstrapped(Endpoint::existing(root.path()).unwrap(), child).unwrap();
    assert!(!format!("{first:?}").contains(token.as_str()));
    first.renew_explicit().unwrap();
    assert!(first.snapshot().is_ok());
    // Graceful trusted fixture shutdown preserves the first SDK's cached token,
    // so replacement proves kernel pinning before another credential is sent.
    fs::write(root.path().join("stop-fixture"), b"stop").unwrap();
    assert!(first.wait_for_launched_exit().unwrap().unwrap().success());
    fs::remove_file(root.path().join("stop-fixture")).unwrap();
    let child = cold(root.path()).unwrap();
    let mut second =
        NativeClient::from_bootstrapped(Endpoint::existing(root.path()).unwrap(), child).unwrap();
    assert_eq!(first.snapshot().unwrap_err(), Error::OwnerChanged);
    assert_eq!(
        first.session_info().unwrap_err(),
        Error::ReauthenticationRequired
    );
    second.quit().unwrap();
    fs::write(root.path().join("stop-fixture"), b"stop").unwrap();
    assert!(second.wait_for_launched_exit().unwrap().unwrap().success());
    "passed"
}
