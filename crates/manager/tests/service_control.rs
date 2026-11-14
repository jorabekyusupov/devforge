mod common;
use devforge_control::{
    ErrorCode, Id, OperationState, Request, Scope,
    mutations::{MutationInput, MutationReceipt, ServiceActionInput},
};
use devforge_core::{ErrorCode as CoreError, SecretStore};
use devforge_manager::{
    Client,
    auth::{DesktopAuth, Policy, TrustedDesktopAction},
    controller::{Controller, StartIntent, Stores, local_owner_user},
    external_auth::{ExternalCredential, IssuedGrant},
    external_client::{ExternalClient, ServiceClient},
    external_control::{ExternalInput, ExternalOutput},
    native_client::NativeClient,
};
use devforge_projects::project::{Catalog, ProjectStore};
use devforge_runtime::package::Target;
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Condvar, Mutex},
    thread,
    time::{Duration, Instant},
};
fn id(v: &str) -> Id {
    Id::new(v).unwrap()
}
#[derive(Default)]
struct Memory {
    values: Mutex<BTreeMap<String, String>>,
    gate: Mutex<(bool, bool)>,
    ready: Condvar,
}
impl SecretStore for Memory {
    fn get(&self, k: &str) -> Result<Option<String>, CoreError> {
        Ok(self.values.lock().unwrap().get(k).cloned())
    }
    fn set(&self, k: &str, v: &str) -> Result<(), CoreError> {
        let mut gate = self.gate.lock().unwrap();
        if gate.0 {
            gate.1 = true;
            self.ready.notify_all();
            while gate.0 {
                gate = self.ready.wait(gate).unwrap();
            }
        }
        drop(gate);
        self.values.lock().unwrap().insert(k.into(), v.into());
        Ok(())
    }
    fn delete(&self, k: &str) -> Result<(), CoreError> {
        self.values.lock().unwrap().remove(k);
        Ok(())
    }
}
struct Release(Arc<Memory>);
impl Drop for Release {
    fn drop(&mut self) {
        let mut gate = self.0.gate.lock().unwrap();
        gate.0 = false;
        self.0.ready.notify_all();
    }
}
struct Rig {
    root: tempfile::TempDir,
    native: NativeClient,
    endpoint: devforge_manager::Endpoint,
    worker: Option<thread::JoinHandle<devforge_manager::controller::Result<()>>>,
    projects: Vec<devforge_projects::project::Project>,
    secrets: Arc<Memory>,
    blocks: u64,
}
impl Rig {
    fn new(cache: Option<&Path>) -> Self {
        Self::setup(cache, false)
    }
    fn setup(cache: Option<&Path>, legacy: bool) -> Self {
        let root = common::root("df-service-");
        let catalog = Catalog::bundled();
        let target = Target::current().unwrap();
        if let Some(source) = cache {
            copy_cache(source, root.path(), &catalog);
        }
        let select = |kind: &str| {
            catalog
                .packages
                .iter()
                .find(|(_, p)| {
                    p.kind == kind
                        && p.target == target
                        && (cache.is_none()
                            || if kind == "php" {
                                p.version.starts_with("8.4.27")
                                    && p.variant == "cli"
                                    && root
                                        .path()
                                        .join("runtimes/packages")
                                        .join(p.key().unwrap())
                                        .join(".devforge-receipt.json")
                                        .is_file()
                            } else {
                                p.version.contains("24.22.2")
                            })
                })
                .unwrap()
                .0
                .clone()
        };
        let store = ProjectStore::open(root.path().join("projects")).unwrap();
        let mut projects = vec![];
        for (folder, body) in [
            ("Alpha", "M13_SERVICE_ALPHA"),
            ("Bravo", "M13_SERVICE_BRAVO"),
        ] {
            let path = root.path().join(folder);
            std::fs::create_dir_all(path.join("public")).unwrap();
            std::fs::write(
                path.join("public/index.php"),
                format!("<?php echo '{body}';"),
            )
            .unwrap();
            std::fs::write(
                path.join("composer.json"),
                r#"{"require":{"php":"^8.4","laravel/framework":"^13"}}"#,
            )
            .unwrap();
            let p = store
                .import(&path, folder, &select("php"), &select("node"), &catalog)
                .unwrap();
            projects.push(p);
        }
        drop(store);
        if cache.is_some() {
            let mut setup =
                devforge_workspace::Workspace::open(root.path(), Arc::new(Memory::default()))
                    .unwrap();
            for p in &mut projects {
                setup
                    .execute(
                        devforge_workspace::Command::Configure {
                            id: p.id.clone(),
                            revision: p.revision,
                            name: p.name.clone(),
                            php: p.php.clone(),
                            node: p.node.clone(),
                            domain: format!("{}.devforge.test", p.name.to_lowercase()),
                            https: false,
                            vite: false,
                            queue: false,
                        },
                        &Default::default(),
                    )
                    .unwrap();
                p.revision += 1;
            }
            drop(setup);
        }
        let control = Arc::new(Memory::default());
        let secrets = Arc::new(Memory::default());
        let owner = common::acquire(root.path()).unwrap();
        let mut auth = DesktopAuth::new(
            owner.endpoint().clone(),
            local_owner_user().unwrap(),
            Policy::default(),
        )
        .unwrap();
        let credential = auth
            .rotate_desktop_key(
                &TrustedDesktopAction::from_explicit_desktop_action(),
                control.as_ref(),
                Instant::now(),
            )
            .unwrap();
        drop(owner);
        if legacy {
            let path = root.path().join("control/operations.sqlite3");
            common::credential_file(&path, b"");
            let db = rusqlite::Connection::open(path).unwrap();
            db.execute_batch("CREATE TABLE operations(operation_id TEXT PRIMARY KEY,client_id TEXT NOT NULL,project_id TEXT NOT NULL,idempotency_key TEXT NOT NULL,fingerprint TEXT NOT NULL,state TEXT NOT NULL,code TEXT,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,UNIQUE(client_id,project_id,idempotency_key)); PRAGMA user_version=1;").unwrap();
            for (op, key, state) in [
                ("old-success", "old-1", "succeeded"),
                ("old-queued", "old-2", "queued"),
                ("old-running", "old-3", "running"),
            ] {
                db.execute("INSERT INTO operations VALUES(?1,'old-client',?2,?3,'old-fingerprint',?4,NULL,1,1)",rusqlite::params![op,projects[0].id,key,state]).unwrap();
            }
        }
        let controller = Controller::start(
            root.path(),
            StartIntent::AutoStart,
            Stores {
                workspace: secrets.clone(),
                desktop: control,
            },
        )
        .unwrap();
        let endpoint = controller.endpoint().clone();
        let worker = thread::spawn(move || controller.serve(None));
        let until = Instant::now() + Duration::from_secs(5);
        while ExternalClient::observe_owner(&endpoint).is_err() {
            assert!(Instant::now() < until);
            thread::sleep(Duration::from_millis(10));
        }
        let native = NativeClient::authenticate_explicit(endpoint.clone(), &credential).unwrap();
        Self {
            root,
            native,
            endpoint,
            worker: Some(worker),
            projects,
            secrets,
            blocks: 0,
        }
    }
    fn grant(&mut self, index: usize, execution: bool) -> IssuedGrant {
        self.native
            .issue_external_grant(
                &id(&self.projects[index].id),
                if execution {
                    &[Scope::Observer, Scope::ProjectManage, Scope::ExecutionTest]
                } else {
                    &[Scope::Observer, Scope::ProjectManage]
                },
                Duration::from_secs(600),
            )
            .unwrap()
    }
    fn service(&self, grant: &IssuedGrant) -> ServiceClient {
        ServiceClient::connect(
            self.endpoint.clone(),
            ExternalCredential::from_wire(grant.credential.expose_secret()).unwrap(),
        )
        .unwrap()
    }
    fn external(&self, grant: &IssuedGrant) -> ExternalClient {
        ExternalClient::connect(
            self.endpoint.clone(),
            ExternalCredential::from_wire(grant.credential.expose_secret()).unwrap(),
        )
        .unwrap()
    }
    fn complete(&self, grant: &IssuedGrant, receipt: &MutationReceipt) -> OperationState {
        let mut client = self.external(grant);
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            let response = client
                .request(
                    id("observe-service"),
                    ExternalInput::OperationStatus {
                        project_id: receipt.project_id.clone(),
                        operation_id: receipt.operation_id.clone(),
                    },
                    None,
                )
                .unwrap();
            if response
                .error
                .as_ref()
                .is_some_and(|error| error.code == ErrorCode::Busy)
            {
                assert!(Instant::now() < until, "service remained busy");
                thread::sleep(Duration::from_millis(20));
                continue;
            }
            let Some(ExternalOutput::OperationStatus(output)) = response.data else {
                panic!("status denied: {:?}", response.error)
            };
            let state = output.items[0].state;
            if state.terminal() {
                return state;
            }
            assert!(Instant::now() < until, "service did not finish");
            thread::sleep(Duration::from_millis(20));
        }
    }
    fn block(&mut self) -> Release {
        self.blocks += 1;
        *self.secrets.gate.lock().unwrap() = (true, false);
        let release = Release(self.secrets.clone());
        self.native
            .desktop_access(
                &id(&format!("hold-owner-{}", self.blocks)),
                &devforge_manager::desktop::AccessRequest::ConfigurationSecret {
                    project_id: id(&self.projects[0].id),
                    reference: "gate".into(),
                    value: devforge_manager::desktop::SecretValue::new("in-memory-fixture")
                        .unwrap(),
                },
            )
            .unwrap();
        let gate = self.secrets.gate.lock().unwrap();
        let (_held, wait) = self
            .secrets
            .ready
            .wait_timeout_while(gate, Duration::from_secs(5), |g| !g.1)
            .unwrap();
        assert!(!wait.timed_out());
        release
    }
}
impl Drop for Rig {
    fn drop(&mut self) {
        let _ = self.native.quit();
        if let Some(worker) = self.worker.take() {
            let result = worker.join();
            if !std::thread::panicking() {
                result.unwrap().unwrap();
            }
        }
    }
}
fn request(project: &str, action: &str, key: &str) -> Request<MutationInput> {
    Request {
        api_version: Default::default(),
        request_id: id(key),
        project_id: id(project),
        expected_revision: Some(1),
        idempotency_key: Some(id(key)),
        input: match action {
            "start" => MutationInput::Start(ServiceActionInput { limits: None }),
            "restart" => MutationInput::Restart(ServiceActionInput { limits: None }),
            _ => MutationInput::Stop(ServiceActionInput { limits: None }),
        },
    }
}

#[test]
fn review_is_native_and_exact_and_retry_lookup_never_creates_a_second_effect() {
    let mut rig = Rig::new(None);
    let grant = rig.grant(0, false);
    let mut client = rig.service(&grant);
    let req = request(&rig.projects[0].id, "stop", "stop-once");
    let plan = client.plan(&req).unwrap().data().unwrap().clone();
    assert!(plan.blocked);
    assert_eq!(
        client
            .apply(&req, &plan.digest)
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::Unauthorized
    );
    let raw=Client::new(rig.endpoint.clone()).request(&json!({"method":"approve_service_plan","session":grant.credential.expose_secret(),"digest":plan.digest})).unwrap();
    assert_eq!(raw["ok"], false);
    rig.native.approve_service_plan(&plan.digest).unwrap();
    let receipt = client
        .apply(&req, &plan.digest)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    assert_eq!(rig.complete(&grant, &receipt), OperationState::Succeeded);
    let retry = client
        .apply(&req, &plan.digest)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    assert_eq!(retry.operation_id, receipt.operation_id);
    let lookup = client
        .lookup(
            id("lookup"),
            &req.project_id,
            req.idempotency_key.as_ref().unwrap(),
        )
        .unwrap();
    assert_eq!(
        lookup.data().unwrap().as_ref().unwrap().operation_id,
        receipt.operation_id
    );
    let mut changed = req.clone();
    changed.idempotency_key = Some(id("different-intent"));
    assert_eq!(
        client
            .apply(&changed, &plan.digest)
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::ReviewMismatch
    );
    let denied = client
        .plan(&request(&rig.projects[1].id, "stop", "foreign"))
        .unwrap();
    assert_eq!(denied.error().unwrap().code, ErrorCode::ProjectMismatch);
    assert_eq!(
        client
            .plan(&request(&rig.projects[0].id, "start", "no-execution"))
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::Unauthorized
    );
    let full = rig.grant(0, true);
    let mut full_client = rig.service(&full);
    assert_eq!(
        full_client
            .plan(&request(&rig.projects[0].id, "start", "missing-cache"))
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::PreconditionFailed
    );
}

#[test]
fn queued_service_cancel_revoke_revision_and_deadline_are_rechecked_before_effects() {
    let mut rig = Rig::new(None);
    let grant = rig.grant(0, false);
    let mut client = rig.service(&grant);
    let release = rig.block();
    let req = request(&rig.projects[0].id, "stop", "queued-cancel");
    let plan = client.plan(&req).unwrap().data().unwrap().clone();
    rig.native.approve_service_plan(&plan.digest).unwrap();
    let receipt = client
        .apply(&req, &plan.digest)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    let mut external = rig.external(&grant);
    let cancel = external
        .request(
            id("cancel"),
            ExternalInput::OperationCancel {
                project_id: req.project_id.clone(),
                operation_id: receipt.operation_id.clone(),
            },
            None,
        )
        .unwrap();
    assert!(cancel.ok);
    drop(release);
    assert_eq!(rig.complete(&grant, &receipt), OperationState::Cancelled);
    let release = rig.block();
    let req = request(&rig.projects[0].id, "stop", "queued-stale");
    let plan = client.plan(&req).unwrap().data().unwrap().clone();
    rig.native.approve_service_plan(&plan.digest).unwrap();
    let stale = client
        .apply(&req, &plan.digest)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    // Isolated registry fixture changes revision while the one real worker is
    // blocked. This tests dequeue revalidation, not an additional production writer.
    let db = rusqlite::Connection::open(rig.root.path().join("projects/projects.sqlite3")).unwrap();
    db.execute(
        "UPDATE projects SET revision=2,record=json_set(record,'$.revision',2) WHERE id=?1",
        [rig.projects[0].id.as_str()],
    )
    .unwrap();
    drop(db);
    drop(release);
    assert_eq!(rig.complete(&grant, &stale), OperationState::Failed);
    let replay = client.apply(&req, &plan.digest).unwrap();
    assert_eq!(replay.data().unwrap().operation_id, stale.operation_id);
    assert_eq!(replay.data().unwrap().revision.get(), 1);
    let lookup = client
        .lookup(
            id("original-revision"),
            &req.project_id,
            req.idempotency_key.as_ref().unwrap(),
        )
        .unwrap();
    assert_eq!(lookup.data().unwrap().as_ref().unwrap().revision.get(), 1);
    let release = rig.block();
    let mut timed = request(&rig.projects[0].id, "stop", "queued-deadline");
    timed.expected_revision = Some(2);
    if let MutationInput::Stop(input) = &mut timed.input {
        input.limits = Some(
            serde_json::from_value(json!({"timeout_ms":3000,"max_output_bytes":65536})).unwrap(),
        );
    }
    let plan = client.plan(&timed).unwrap().data().unwrap().clone();
    rig.native.approve_service_plan(&plan.digest).unwrap();
    let timed_receipt = client
        .apply(&timed, &plan.digest)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    thread::sleep(Duration::from_millis(3100));
    drop(release);
    assert_eq!(rig.complete(&grant, &timed_receipt), OperationState::Failed);
    let mut req = request(&rig.projects[0].id, "stop", "queued-revoked");
    req.expected_revision = Some(2);
    let plan = client.plan(&req).unwrap().data().unwrap().clone();
    rig.native.approve_service_plan(&plan.digest).unwrap();
    let release = rig.block();
    let receipt = client
        .apply(&req, &plan.digest)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    rig.native
        .revoke_external_grant(&grant.record.grant_id)
        .unwrap();
    drop(release);
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let db =
            rusqlite::Connection::open(rig.root.path().join("control/operations.sqlite3")).unwrap();
        let state: String = db
            .query_row(
                "SELECT state FROM operations WHERE operation_id=?1",
                [receipt.operation_id.as_str()],
                |r| r.get(0),
            )
            .unwrap();
        if state == "cancelled" {
            break;
        }
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn schema_one_migration_preserves_old_receipts_and_small_output_budget_never_admits() {
    let mut rig = Rig::setup(None, true);
    let path = rig.root.path().join("control/operations.sqlite3");
    let db = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))
            .unwrap(),
        2
    );
    for (op, expected) in [
        ("old-success", "succeeded"),
        ("old-queued", "interrupted"),
        ("old-running", "unknown"),
    ] {
        let state: String = db
            .query_row(
                "SELECT state FROM operations WHERE operation_id=?1",
                [op],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(state, expected);
    }
    drop(db);
    let grant = rig.grant(0, false);
    let mut client = rig.service(&grant);
    let mut small = request(&rig.projects[0].id, "stop", "small-receipt");
    if let MutationInput::Stop(input) = &mut small.input {
        input.limits = Some(
            serde_json::from_value(json!({"timeout_ms":300000,"max_output_bytes":1})).unwrap(),
        );
    }
    let plan = client.plan(&small).unwrap().data().unwrap().clone();
    rig.native.approve_service_plan(&plan.digest).unwrap();
    assert_eq!(
        client
            .apply(&small, &plan.digest)
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::OutputLimitExceeded
    );
    assert!(
        client
            .lookup(
                id("no-admitted-row"),
                &small.project_id,
                small.idempotency_key.as_ref().unwrap()
            )
            .unwrap()
            .data()
            .unwrap()
            .is_none()
    );
    let normal = request(&rig.projects[0].id, "stop", "after-migration");
    let plan = client.plan(&normal).unwrap().data().unwrap().clone();
    rig.native.approve_service_plan(&plan.digest).unwrap();
    let receipt = client
        .apply(&normal, &plan.digest)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    assert_eq!(rig.complete(&grant, &receipt), OperationState::Succeeded);
    let db = rusqlite::Connection::open(&path).unwrap();
    let revision: i64 = db
        .query_row(
            "SELECT revision FROM service_operations WHERE operation_id=?1",
            [receipt.operation_id.as_str()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(revision, 1);
    assert_eq!(
        db.query_row("SELECT count(*) FROM operations", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        4
    );
}

#[cfg(target_os = "macos")]
fn copy_cache(source: &Path, root: &Path, catalog: &Catalog) {
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                copy(&entry.path(), &target);
            } else if kind.is_symlink() {
                std::os::unix::fs::symlink(std::fs::read_link(entry.path()).unwrap(), target)
                    .unwrap();
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    let selected = catalog
        .packages
        .values()
        .filter(|p| {
            p.target == Target::current().unwrap()
                && source
                    .join("packages")
                    .join(p.key().unwrap())
                    .join(".devforge-receipt.json")
                    .is_file()
                && ((p.kind == "php" && p.version.starts_with("8.4.27"))
                    || (p.kind == "node" && p.version.contains("24.22.2"))
                    || (p.kind == "caddy" && p.version.contains("2.11.9"))
                    || (p.kind == "composer" && p.version == "2.11.2"))
        })
        .collect::<Vec<_>>();
    let cache =
        devforge_runtime::package::PackageStore::open(root.join("runtimes"), Default::default())
            .unwrap();
    let destination = root.join("runtimes/packages");
    std::fs::create_dir_all(&destination).unwrap();
    for package in &selected {
        let key = package.key().unwrap();
        let package_source = source.join("packages").join(&key);
        let receipt: serde_json::Value = serde_json::from_slice(
            &std::fs::read(package_source.join(".devforge-receipt.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(receipt["package"], serde_json::to_value(package).unwrap());
        copy(&package_source, &destination.join(&key));
    }
    for package in selected {
        for name in package.executables.keys() {
            let _verified = cache.executable(package, name).unwrap();
        }
        for name in package.resources.keys() {
            cache.resource(package, name).unwrap().check().unwrap();
        }
    }
}
#[cfg(not(target_os = "macos"))]
fn copy_cache(_: &Path, _: &Path, _: &Catalog) {
    panic!("native cache fixture is macOS only")
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires an explicitly approved immutable native runtime cache"]
fn native_cached_start_http_retry_restart_stop_preserves_the_other_project() {
    use std::io::{Read, Write};
    let source = std::path::PathBuf::from(
        std::env::var_os("DEVFORGE_SERVICE_CACHE").expect("explicit approved cache required"),
    );
    let mut rig = Rig::new(Some(&source));
    let a = rig.grant(0, true);
    let b = rig.grant(1, true);
    let mut ca = rig.service(&a);
    let mut cb = rig.service(&b);
    let action = |rig: &mut Rig,
                  client: &mut ServiceClient,
                  grant: &IssuedGrant,
                  index: usize,
                  name: &str,
                  key: &str| {
        let mut req = request(&rig.projects[index].id, name, key);
        req.expected_revision = Some(rig.projects[index].revision);
        let plan = client.plan(&req).unwrap();
        assert!(
            plan.is_ok(),
            "native cached plan denied: {:?}",
            plan.error()
        );
        let plan = plan.data().unwrap().clone();
        rig.native.approve_service_plan(&plan.digest).unwrap();
        let applied = client.apply(&req, &plan.digest).unwrap();
        assert!(
            applied.is_ok(),
            "native cached apply denied: {:?}",
            applied.error()
        );
        let receipt = applied.data().unwrap().clone();
        assert_eq!(rig.complete(grant, &receipt), OperationState::Succeeded);
        (req, plan, receipt)
    };
    let http = |rig: &mut Rig, index: usize, body: &str| {
        let view = rig.native.snapshot().unwrap();
        let row = view.snapshot["projects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["project"]["id"] == rig.projects[index].id)
            .unwrap();
        let url = row["serving"]["http_url"].as_str().unwrap().to_owned();
        let authority = url.strip_prefix("http://").unwrap().trim_end_matches('/');
        let port = authority
            .rsplit_once(':')
            .map(|(_, port)| port.parse::<u16>().unwrap())
            .unwrap_or(80);
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(
            stream,
            "GET / HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        let mut bytes = String::new();
        stream.read_to_string(&mut bytes).unwrap();
        assert!(bytes.contains("200 OK"));
        assert!(bytes.contains(body));
        row["processes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p[0].as_str().is_some_and(|id| id.contains("php-web")))
            .unwrap()[1]["pid"]
            .as_u64()
            .unwrap()
    };
    let (req, plan, receipt) = action(&mut rig, &mut ca, &a, 0, "start", "native-start-a");
    let first = http(&mut rig, 0, "M13_SERVICE_ALPHA");
    let replay = ca.apply(&req, &plan.digest).unwrap();
    assert_eq!(replay.data().unwrap().operation_id, receipt.operation_id);
    assert_eq!(http(&mut rig, 0, "M13_SERVICE_ALPHA"), first);
    action(&mut rig, &mut cb, &b, 1, "start", "native-start-b");
    let other = http(&mut rig, 1, "M13_SERVICE_BRAVO");
    action(&mut rig, &mut ca, &a, 0, "restart", "native-restart-a");
    assert_ne!(http(&mut rig, 0, "M13_SERVICE_ALPHA"), first);
    assert_eq!(http(&mut rig, 1, "M13_SERVICE_BRAVO"), other);
    action(&mut rig, &mut ca, &a, 0, "stop", "native-stop-a");
    assert_eq!(http(&mut rig, 1, "M13_SERVICE_BRAVO"), other);
    action(&mut rig, &mut cb, &b, 1, "stop", "native-stop-b");
}
