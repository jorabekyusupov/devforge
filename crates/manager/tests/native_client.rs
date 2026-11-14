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
    desktop::{AccessRequest, OpenTarget, ReadRequest, SecretValue},
    launch::{self, LaunchSpec},
    native_client::{AdmissionProgress, Cancellation, Error, NativeClient, ReadProgress},
};
use devforge_projects::project::{Catalog, ProjectStore};
use devforge_runtime::{Cancellation as AdmissionCancellation, package::Target};
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
use zeroize::{Zeroize, Zeroizing};

#[derive(Default)]
struct MemoryStore {
    values: Mutex<BTreeMap<String, String>>,
    gets: AtomicUsize,
    sets: AtomicUsize,
}
impl SecretStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<String>, ErrorCode> {
        self.gets.fetch_add(1, Ordering::SeqCst);
        Ok(self.values.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &str, value: &str) -> Result<(), ErrorCode> {
        self.sets.fetch_add(1, Ordering::SeqCst);
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
    if args.first().is_some_and(|v| v == "snapshot") {
        real_large_snapshot_is_complete_and_unicode_safe();
        println!(
            "{}",
            serde_json::json!({"suite":"native_client_incremental_snapshot","passed":1,"unavailable":0})
        );
        return;
    }
    if args.first().is_some_and(|v| v == "uncertain_access") {
        durable_access_lost_reply_is_reconciled_without_replay();
        println!(
            "{}",
            serde_json::json!({"suite":"native_client_uncertain_access","passed":1,"unavailable":0})
        );
        return;
    }
    if args.first().is_some_and(|v| v == "facade") {
        desktop_facade_large_reads_access_and_operations();
        println!(
            "{}",
            serde_json::json!({"suite":"native_client_facade","passed":1,"unavailable":0})
        );
        return;
    }
    if args.first().is_some_and(|v| v == "cancellable") {
        cancellable_admission_boundaries_and_exact_receipt_routing();
        println!(
            "{}",
            serde_json::json!({"suite":"native_client_cancellable","passed":1,"unavailable":0})
        );
        return;
    }
    explicit_auth_operations_and_large_command();
    real_large_snapshot_is_complete_and_unicode_safe();
    explicit_renewal_expires_at_absolute_context_deadline();
    cancellable_admission_boundaries_and_exact_receipt_routing();
    desktop_facade_large_reads_access_and_operations();
    durable_access_lost_reply_is_reconciled_without_replay();
    let cold = cold_bootstrap_and_owner_replacement();
    let unavailable = usize::from(cold != "passed");
    println!(
        "{}",
        serde_json::json!({"suite":"native_client","overall":if unavailable==0{"passed"}else{"partial"},"passed":7-unavailable,"unavailable":unavailable,
        "cases":[{"case":"durable_access_uncertainty","status":"passed"},{"case":"desktop_facade_large_read_access_list","status":"passed"},{"case":"cancellable_admission_boundaries","status":"passed"},{"case":"explicit_auth_operations_large_command","status":"passed"},{"case":"actual_large_snapshot","status":"passed"},{"case":"renewal_absolute_deadline","status":"passed"},
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
    running_with_workspace(root, store, Arc::new(DenyStore), options)
}
fn running_with_workspace(
    root: &Path,
    store: Arc<MemoryStore>,
    workspace: Arc<dyn SecretStore + Send + Sync>,
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
            workspace,
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
    let mut read = client.begin_snapshot().unwrap();
    let mut pages = 0;
    loop {
        pages += 1;
        let progress = client.snapshot_step(&mut read).unwrap();
        assert_eq!(
            client.health().unwrap().state,
            devforge_manager::native_client::OwnerState::Ready
        );
        if matches!(progress, ReadProgress::Complete) {
            break;
        }
    }
    assert!(pages > 1);
    let snapshot = client.finish_snapshot(read).unwrap();
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
    let convenience = client.snapshot().unwrap();
    assert_eq!(
        convenience.snapshot["projects"].as_array().unwrap().len(),
        COUNT
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
fn cancellable_admission_boundaries_and_exact_receipt_routing() {
    let root = common::root("df-sdk-cancel-");
    let store = Arc::new(MemoryStore::default());
    let key = provision(root.path(), &store);
    let catalog = Catalog::bundled();
    let (php, node) = runtime_ids(&catalog);
    let app = root.path().join("application");
    laravel(&app);
    let registry = ProjectStore::open(root.path().join("projects")).unwrap();
    let project = registry
        .import(&app, "Cancel fixture", &php, &node, &catalog)
        .unwrap();
    drop(registry);
    let project_id = id(&project.id);
    let (endpoint, worker, _quit) = running(root.path(), store.clone(), Options::default());
    let mut client = NativeClient::authenticate_explicit(endpoint, &key).unwrap();
    let favorite = Command::Favorite {
        id: project.id.clone(),
        favorite: true,
    };
    let before = AdmissionCancellation::default();
    before.cancel();
    assert!(matches!(
        client.admit_cancellable(&project_id, &id("cancel-before"), &favorite, &before),
        Err(Error::AdmissionCancelled)
    ));

    let during = AdmissionCancellation::default();
    let body = Command::ConfigurationSave {
        id: project.id.clone(),
        expected: None,
        text: "# 🦀 SDK cancellation body\n".repeat(5_000),
        consent: true,
    };
    let mut sent_chunk = false;
    let result =
        client.admit_observed(&project_id, &id("cancel-during"), &body, &during, |event| {
            if let AdmissionProgress::Uploading { sent, total } = event
                && sent > 0
                && sent < total
            {
                sent_chunk = true;
                during.cancel();
            }
        });
    assert!(sent_chunk);
    assert!(matches!(result, Err(Error::AdmissionCancelled)));

    let finalizing = AdmissionCancellation::default();
    let mut finalizing_seen = false;
    let result = client.admit_observed(
        &project_id,
        &id("cancel-finalizing"),
        &favorite,
        &finalizing,
        |event| {
            if matches!(event, AdmissionProgress::Finalizing) {
                finalizing_seen = true;
                finalizing.cancel();
            }
        },
    );
    assert!(finalizing_seen);
    assert!(matches!(result, Err(Error::AdmissionCancelled)));
    let ledger = rusqlite::Connection::open_with_flags(
        root.path().join("control/operations.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    assert_eq!(
        ledger
            .query_row("SELECT COUNT(*) FROM operations", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        client.snapshot().unwrap().snapshot["projects"][0]["navigation"]["favorite"],
        false
    );
    assert!(!app.join("devforge.yaml").exists());

    // This is the third begun upload (after two aborted ones); success also
    // proves abort released the backend's two-slot transient upload capacity.
    let accepted_cancel = AdmissionCancellation::default();
    let mut accepted = None;
    let receipt = client
        .admit_observed(
            &project_id,
            &id("cancel-accepted"),
            &favorite,
            &accepted_cancel,
            |event| {
                if let AdmissionProgress::Accepted(receipt) = event {
                    accepted = Some(receipt);
                    accepted_cancel.cancel();
                }
            },
        )
        .unwrap();
    assert!(accepted_cancel.is_cancelled());
    assert_eq!(accepted.unwrap().operation_id, receipt.operation_id);
    assert!(matches!(
        client.cancel(&project_id, &receipt.operation_id).unwrap(),
        Cancellation::Requested | Cancellation::AlreadyTerminal
    ));
    assert!(matches!(
        complete(&mut client, &project_id, &receipt.operation_id),
        OperationState::Cancelled | OperationState::Succeeded
    ));
    let other = client
        .admit(
            &project_id,
            &id("uncancelled-other"),
            &Command::Favorite {
                id: project.id,
                favorite: false,
            },
        )
        .unwrap();
    assert_eq!(
        complete(&mut client, &project_id, &other.operation_id),
        OperationState::Succeeded
    );
    assert_eq!(
        client.cancel(&project_id, &receipt.operation_id).unwrap(),
        Cancellation::AlreadyTerminal
    );
    assert_eq!(
        client
            .status(&project_id, &other.operation_id)
            .unwrap()
            .state,
        OperationState::Succeeded
    );
    assert_eq!(
        ledger
            .query_row("SELECT COUNT(*) FROM operations", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
    drop(ledger);
    assert_eq!(store.gets.load(Ordering::SeqCst), 1);
    client.quit().unwrap();
    worker.join().unwrap().unwrap();
}
fn read_value(client: &mut NativeClient, key: &str, request: &ReadRequest) -> serde_json::Value {
    let receipt = client.desktop_read(&id(key), request).unwrap();
    assert_eq!(
        complete(client, request.project_id(), &receipt.operation_id),
        OperationState::Succeeded
    );
    let mut image = client
        .begin_read_result(request.project_id(), &receipt.operation_id)
        .unwrap();
    loop {
        if matches!(
            client.read_result_step(&mut image).unwrap(),
            ReadProgress::Complete
        ) {
            break;
        }
    }
    client.finish_read_result(image).unwrap()
}
fn desktop_facade_large_reads_access_and_operations() {
    use devforge_core::{Settings, SettingsRepository, Theme};
    let root = common::root("df-sdk-facade-");
    let desktop_store = Arc::new(MemoryStore::default());
    let workspace_store = Arc::new(MemoryStore::default());
    let key = provision(root.path(), &desktop_store);
    let scan = root.path().join("scan-root");
    fs::create_dir(&scan).unwrap();
    let mut preferences = devforge_platform::SqliteSettings::open(root.path()).unwrap();
    preferences
        .save(&Settings {
            language: "ru".into(),
            theme: Theme::Dark,
            project_folder: Some(scan.to_string_lossy().into_owned()),
        })
        .unwrap();
    drop(preferences);
    let app = root.path().join("application");
    laravel(&app);
    let source = format!("<?php\n{}", "// \"🦀\" \"\\path\"\n".repeat(13_000));
    assert!(source.len() < 256 * 1024);
    fs::write(app.join("sdk-large.php"), &source).unwrap();
    let (endpoint, worker, _quit) = running_with_workspace(
        root.path(),
        desktop_store.clone(),
        workspace_store.clone(),
        Options::default(),
    );
    let mut client = NativeClient::authenticate_explicit(endpoint, &key).unwrap();
    let (php, node) = runtime_ids(&Catalog::bundled());
    let root_subject = id(ROOT_SUBJECT);
    let imported = client
        .admit(
            &root_subject,
            &id("facade-import"),
            &Command::Import {
                path: app.clone(),
                name: "Desktop facade".into(),
                php,
                node,
                domain: "sdk-facade.devforge.test".into(),
                https: false,
                vite: false,
                queue: false,
            },
        )
        .unwrap();
    assert_eq!(
        complete(&mut client, &root_subject, &imported.operation_id),
        OperationState::Succeeded
    );
    let project_id = id(client
        .result(&root_subject, &imported.operation_id)
        .unwrap()["project_id"]
        .as_str()
        .unwrap());
    let ledger = rusqlite::Connection::open_with_flags(
        root.path().join("control/operations.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let baseline = ledger
        .query_row("SELECT COUNT(*) FROM operations", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap();
    let request = ReadRequest::DebugSource {
        project_id: project_id.clone(),
        file: "sdk-large.php".into(),
    };
    let large = client
        .desktop_read(&id("facade-large-source"), &request)
        .unwrap();
    let replay = client
        .desktop_read(&id("facade-large-source"), &request)
        .unwrap();
    assert_eq!(large.operation_id, replay.operation_id);
    assert_eq!(
        complete(&mut client, &project_id, &large.operation_id),
        OperationState::Succeeded
    );
    let mut image = client
        .begin_read_result(&project_id, &large.operation_id)
        .unwrap();
    assert!(image.total_bytes() > devforge_manager::MAX_RESPONSE_BYTES as u64);
    // Two actual controllers can share a process PID in a native test, so root
    // binding must independently prevent routing a retained image to the other.
    let other_root = common::root("df-sdk-other-");
    let other_store = Arc::new(MemoryStore::default());
    let other_key = provision(other_root.path(), &other_store);
    let (other_endpoint, other_worker, _) =
        running(other_root.path(), other_store, Options::default());
    let mut other_client = NativeClient::authenticate_explicit(other_endpoint, &other_key).unwrap();
    assert_eq!(
        other_client.read_result_step(&mut image).unwrap_err(),
        Error::OwnerChanged
    );
    other_client.quit().unwrap();
    other_worker.join().unwrap().unwrap();
    let mut pages = 0;
    loop {
        pages += 1;
        let progress = client.read_result_step(&mut image).unwrap();
        // Each step releases the client borrow; unrelated native health/status
        // remains available while the image itself is retained outside the SDK.
        assert_eq!(
            client.health().unwrap().state,
            devforge_manager::native_client::OwnerState::Ready
        );
        if matches!(progress, ReadProgress::Complete) {
            break;
        }
    }
    assert!(pages > 1);
    let value = client.finish_read_result(image).unwrap();
    assert_eq!(value["code"], source);
    let document = read_value(
        &mut client,
        "facade-document",
        &ReadRequest::ConfigurationDocument {
            project_id: project_id.clone(),
        },
    );
    assert!(document["text"].as_str().unwrap().contains("schema:"));
    let inbox = read_value(
        &mut client,
        "facade-inbox",
        &ReadRequest::Inbox {
            project_id: project_id.clone(),
            query: String::new(),
            page: 0,
        },
    );
    assert_eq!(inbox["enabled"], false);
    let inspector = read_value(
        &mut client,
        "facade-inspector",
        &ReadRequest::Inspector {
            project_id: project_id.clone(),
            context: None,
        },
    );
    assert!(
        inspector["capture"]["events"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let logs = read_value(
        &mut client,
        "facade-logs",
        &ReadRequest::Logs {
            project_id: project_id.clone(),
            filter: Default::default(),
        },
    );
    assert!(logs["entries"].as_array().unwrap().is_empty());
    let failed_debug = client
        .desktop_read(
            &id("facade-missing-profile"),
            &ReadRequest::DebugProfile {
                project_id: project_id.clone(),
                profile: id(&"c".repeat(32)),
            },
        )
        .unwrap();
    assert_eq!(
        complete(&mut client, &project_id, &failed_debug.operation_id),
        OperationState::Failed
    );
    let failure = client
        .failure_detail(&project_id, &failed_debug.operation_id)
        .unwrap();
    assert!(matches!(
        failure.scope,
        devforge_manager::native_client::FailureScope::Debugger
    ));
    assert!(matches!(
        client.begin_read_result(&project_id, &large.operation_id),
        Err(Error::ReadExpired)
    ));
    assert_eq!(
        ledger
            .query_row("SELECT COUNT(*) FROM operations", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        baseline,
        "pure polling reads must not create SQL operation rows"
    );

    // A valid near-domain-limit YAML save returns more than the old64KiB result
    // cache once its hash/JSON wrapper is included. Keep Succeeded truthful and
    // retrieve the complete mutation value through the explicit image path.
    let mut yaml = document["text"].as_str().unwrap().to_owned();
    yaml.push_str("\n#");
    yaml.extend(std::iter::repeat_n('x', 65_530 - yaml.len()));
    let saved_yaml = client
        .admit(
            &project_id,
            &id("facade-large-save"),
            &Command::ConfigurationSave {
                id: project_id.as_str().into(),
                expected: document["expected"].as_str().map(str::to_owned),
                text: yaml.clone(),
                consent: true,
            },
        )
        .unwrap();
    assert_eq!(
        complete(&mut client, &project_id, &saved_yaml.operation_id),
        OperationState::Succeeded
    );
    assert!(matches!(
        client.result(&project_id, &saved_yaml.operation_id),
        Err(Error::Remote(_))
    ));
    let mut result_image = client
        .begin_result_image(&project_id, &saved_yaml.operation_id)
        .unwrap();
    assert!(result_image.total_bytes() > 65_536);
    loop {
        if matches!(
            client.read_result_step(&mut result_image).unwrap(),
            ReadProgress::Complete
        ) {
            break;
        }
    }
    let saved_document = client.finish_read_result(result_image).unwrap();
    assert_eq!(saved_document["text"], yaml);
    assert_eq!(fs::read_to_string(app.join("devforge.yaml")).unwrap(), yaml);

    let secret_marker = "SDK_DESKTOP_SECRET_NOT_REAL_🦀";
    let secret = AccessRequest::ConfigurationSecret {
        project_id: project_id.clone(),
        reference: "sdk_token".into(),
        value: SecretValue::new(secret_marker).unwrap(),
    };
    let saved = client
        .desktop_access(&id("facade-secret"), &secret)
        .unwrap();
    assert_eq!(
        complete(&mut client, &project_id, &saved.operation_id),
        OperationState::Succeeded
    );
    assert_eq!(
        client.result(&project_id, &saved.operation_id).unwrap()["saved"],
        true
    );
    assert!(
        workspace_store
            .values
            .lock()
            .unwrap()
            .values()
            .any(|v| v == secret_marker)
    );
    assert_eq!(
        client
            .desktop_access(&id("facade-secret"), &secret)
            .unwrap()
            .operation_id,
        saved.operation_id
    );
    let changed = AccessRequest::ConfigurationSecret {
        project_id: project_id.clone(),
        reference: "sdk_token".into(),
        value: SecretValue::new("different fixture value").unwrap(),
    };
    assert!(matches!(
        client.desktop_access(&id("facade-secret"), &changed),
        Err(Error::Remote(_))
    ));
    let stored_fingerprint: String = ledger
        .query_row(
            "SELECT fingerprint FROM operations WHERE operation_id=?1",
            [saved.operation_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(stored_fingerprint.starts_with("volatile-secret-v1:"));
    assert!(
        !String::from_utf8_lossy(
            &fs::read(root.path().join("control/operations.sqlite3")).unwrap()
        )
        .contains(secret_marker)
    );

    // Unknown registered-project ownership must fail before any OS terminal is
    // opened. This fixture deliberately does not launch a real desktop app.
    let unknown_project = id(&"a".repeat(32));
    let terminal = client
        .desktop_access(
            &id("facade-terminal"),
            &AccessRequest::OpenTarget {
                project_id: unknown_project.clone(),
                target: OpenTarget::Terminal,
            },
        )
        .unwrap();
    assert_eq!(
        complete(&mut client, &unknown_project, &terminal.operation_id),
        OperationState::Failed
    );
    let terminal_failure = client
        .failure_detail(&unknown_project, &terminal.operation_id)
        .unwrap();
    assert_eq!(terminal_failure.project_id, unknown_project);
    let scan2 = root.path().join("scan-root-two");
    fs::create_dir(&scan2).unwrap();
    let discovery = client
        .desktop_access(
            &id("facade-discovery"),
            &AccessRequest::DiscoveryRoot {
                path: scan2.to_string_lossy().into_owned(),
            },
        )
        .unwrap();
    assert_eq!(
        complete(&mut client, &root_subject, &discovery.operation_id),
        OperationState::Succeeded
    );
    client.discovery_request().unwrap();
    let loaded = devforge_platform::SqliteSettings::open(root.path())
        .unwrap()
        .load()
        .unwrap();
    assert_eq!(loaded.language, "ru");
    assert_eq!(loaded.theme, Theme::Dark);
    assert_eq!(
        loaded.project_folder.as_deref(),
        Some(fs::canonicalize(&scan2).unwrap().to_str().unwrap())
    );
    let first = client.operations(Some(&project_id), None, 1).unwrap();
    assert_eq!(first.items.len(), 1);
    let cursor = first
        .next_cursor
        .as_ref()
        .expect("second durable project access");
    let second = client
        .operations(Some(&project_id), Some(cursor), 1)
        .unwrap();
    assert_eq!(second.items.len(), 1);
    assert_ne!(first.items[0].operation_id, second.items[0].operation_id);
    assert!(first.active_reads.len() <= 2);
    assert_eq!(desktop_store.gets.load(Ordering::SeqCst), 1);
    drop(ledger);
    client.quit().unwrap();
    worker.join().unwrap().unwrap();
}
fn durable_access_lost_reply_is_reconciled_without_replay() {
    let root = common::root("df-sdk-unconfirmed-");
    let proxy_root = common::root("df-sdk-relay-");
    let desktop_store = Arc::new(MemoryStore::default());
    let workspace_store = Arc::new(MemoryStore::default());
    let key = provision(root.path(), &desktop_store);
    let app = root.path().join("application");
    laravel(&app);
    let catalog = Catalog::bundled();
    let (php, node) = runtime_ids(&catalog);
    let registry = ProjectStore::open(root.path().join("projects")).unwrap();
    let project = registry
        .import(&app, "Lost reply", &php, &node, &catalog)
        .unwrap();
    drop(registry);
    let project_id = id(&project.id);
    let (endpoint, worker, _quit) = running_with_workspace(
        root.path(),
        desktop_store,
        workspace_store.clone(),
        Options::default(),
    );
    let forward = devforge_manager::Client::new(endpoint);
    let (real_peer, _) = forward
        .request_with_peer(&serde_json::json!({"method":"health"}))
        .unwrap();
    let proxy_owner = common::acquire(proxy_root.path()).unwrap();
    let proxy_endpoint = proxy_owner.endpoint().clone();
    let stop = devforge_manager::StopSignal::default();
    let server_stop = stop.clone();
    let mode = Arc::new(AtomicUsize::new(1));
    let server_mode = mode.clone();
    let requests = Arc::new(AtomicUsize::new(0));
    let server_requests = requests.clone();
    let observed = Arc::new(Mutex::new(Vec::<Id>::new()));
    let server_observed = observed.clone();
    // The relay forwards to a REAL Controller. Only AFTER its actual durable
    // receipt is received does this fixture drop/corrupt the network reply.
    // It never fabricates a successful business result or reads native secrets.
    let proxy = thread::spawn(move || {
        devforge_manager::Server::new(proxy_owner).run(server_stop, move |_, mut request| {
            let access = request["method"] == "desktop_access";
            let mut response = forward.request_with_expected_peer(&real_peer, &request)?;
            if let Some(object) = request.as_object_mut() {
                for name in ["credential", "session", "token"] {
                    if let Some(serde_json::Value::String(value)) = object.get_mut(name) {
                        value.zeroize();
                    }
                }
                if let Some(serde_json::Value::Object(input)) = object.get_mut("request")
                    && let Some(serde_json::Value::String(value)) = input.get_mut("value")
                {
                    value.zeroize();
                }
            }
            if access {
                server_requests.fetch_add(1, Ordering::SeqCst);
                let action = server_mode.swap(0, Ordering::SeqCst);
                if response["ok"] == true {
                    server_observed
                        .lock()
                        .unwrap()
                        .push(id(response["data"]["operation_id"].as_str().unwrap()));
                    if action == 1 {
                        return Err(devforge_manager::Error::HandlerFailed);
                    }
                    if action == 2 {
                        response["data"]["project_id"] =
                            serde_json::Value::String("wrong-subject".into());
                    }
                }
            }
            Ok(response)
        })
    });
    let probe = devforge_manager::Client::new(proxy_endpoint.clone());
    let started = Instant::now();
    while probe
        .request(&serde_json::json!({"method":"health"}))
        .is_err()
    {
        assert!(started.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(5));
    }
    let mut client = NativeClient::authenticate_explicit(proxy_endpoint, &key).unwrap();
    let oversized = AccessRequest::ConfigurationSecret {
        project_id: project_id.clone(),
        reference: "r".repeat(70_000),
        value: SecretValue::new("fixture").unwrap(),
    };
    assert!(matches!(
        client.desktop_access(&id("access-presend-size"), &oversized),
        Err(Error::InputLimit)
    ));
    assert_eq!(
        requests.load(Ordering::SeqCst),
        0,
        "local known rejection must not send a request"
    );
    let intent = id("access-lost-reply");
    let request = AccessRequest::ConfigurationSecret {
        project_id: project_id.clone(),
        reference: "sdk_lost".into(),
        value: SecretValue::new("SDK_LOST_REPLY_FIXTURE").unwrap(),
    };
    assert!(matches!(
        client.desktop_access(&intent, &request),
        Err(Error::AdmissionUnconfirmed)
    ));
    assert_eq!(
        requests.load(Ordering::SeqCst),
        1,
        "SDK must not retry the access after a lost receipt"
    );
    let recovered = client
        .operation_by_key(&project_id, &intent)
        .unwrap()
        .expect("real admitted receipt");
    assert_eq!(recovered.operation_id, observed.lock().unwrap()[0]);
    assert_eq!(
        complete(&mut client, &project_id, &recovered.operation_id),
        OperationState::Succeeded
    );
    let effects = workspace_store.sets.load(Ordering::SeqCst);
    assert!(effects > 0);
    assert_eq!(
        client
            .operation_by_key(&project_id, &intent)
            .unwrap()
            .unwrap()
            .operation_id,
        recovered.operation_id
    );
    assert_eq!(workspace_store.sets.load(Ordering::SeqCst), effects);
    assert_eq!(
        requests.load(Ordering::SeqCst),
        1,
        "read-only reconciliation must not execute access"
    );
    assert!(
        client
            .operation_by_key(&project_id, &id("absent-at-this-instant"))
            .unwrap()
            .is_none()
    );

    mode.store(2, Ordering::SeqCst);
    let intent2 = id("access-malformed-receipt");
    assert!(matches!(
        client.desktop_access(&intent2, &request),
        Err(Error::AdmissionUnconfirmed)
    ));
    let recovered2 = client
        .operation_by_key(&project_id, &intent2)
        .unwrap()
        .unwrap();
    assert_eq!(recovered2.operation_id, observed.lock().unwrap()[1]);
    assert_eq!(
        complete(&mut client, &project_id, &recovered2.operation_id),
        OperationState::Succeeded
    );
    assert_eq!(requests.load(Ordering::SeqCst), 2);
    let invalid = AccessRequest::ConfigurationSecret {
        project_id: project_id.clone(),
        reference: "x".repeat(65),
        value: SecretValue::new("fixture").unwrap(),
    };
    assert!(matches!(
        client.desktop_access(&id("access-definite-denial"), &invalid),
        Err(Error::Remote(_))
    ));
    client.quit().unwrap();
    worker.join().unwrap().unwrap();
    stop.request();
    proxy.join().unwrap().unwrap();
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
