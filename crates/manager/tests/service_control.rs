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
    external_client::{
        DatabaseClient, DiagnosticsClient, DomainClient, EnvClient, ExternalClient, RuntimeClient,
        ServiceClient,
    },
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
        Self::runtime_setup(cache, legacy, false)
    }
    fn runtime_setup(cache: Option<&Path>, legacy: bool, alternative: bool) -> Self {
        let root = common::root("df-service-");
        let catalog = Catalog::bundled();
        let target = Target::current().unwrap();
        if let Some(source) = cache {
            copy_cache(source, root.path(), &catalog, alternative);
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
    fn runtime(&self, grant: &IssuedGrant) -> RuntimeClient {
        RuntimeClient::connect(
            self.endpoint.clone(),
            ExternalCredential::from_wire(grant.credential.expose_secret()).unwrap(),
        )
        .unwrap()
    }
    fn environment(&self, grant: &IssuedGrant) -> EnvClient {
        EnvClient::connect(
            self.endpoint.clone(),
            ExternalCredential::from_wire(grant.credential.expose_secret()).unwrap(),
        )
        .unwrap()
    }
    fn database(&self, grant: &IssuedGrant) -> DatabaseClient {
        DatabaseClient::connect(
            self.endpoint.clone(),
            ExternalCredential::from_wire(grant.credential.expose_secret()).unwrap(),
        )
        .unwrap()
    }
    fn domain(&self, grant: &IssuedGrant) -> DomainClient {
        DomainClient::connect(
            self.endpoint.clone(),
            ExternalCredential::from_wire(grant.credential.expose_secret()).unwrap(),
        )
        .unwrap()
    }
    fn diagnostics(&self, grant: &IssuedGrant) -> DiagnosticsClient {
        DiagnosticsClient::connect(
            self.endpoint.clone(),
            ExternalCredential::from_wire(grant.credential.expose_secret()).unwrap(),
        )
        .unwrap()
    }
    fn env_grant(&mut self, index: usize, execute: bool) -> IssuedGrant {
        self.native
            .issue_external_grant(
                &id(&self.projects[index].id),
                if execute {
                    &[
                        Scope::Observer,
                        Scope::ProjectManage,
                        Scope::SensitiveOperation,
                        Scope::ExecutionTest,
                    ]
                } else {
                    &[
                        Scope::Observer,
                        Scope::ProjectManage,
                        Scope::SensitiveOperation,
                    ]
                },
                Duration::from_secs(600),
            )
            .unwrap()
    }
    fn runtime_metadata(&mut self) {
        for index in 0..self.projects.len() {
            let p = &self.projects[index];
            let receipt = self
                .native
                .admit(
                    &id(&p.id),
                    &id(&format!("runtime-setup-{index}")),
                    &devforge_workspace::Command::Configure {
                        id: p.id.clone(),
                        revision: p.revision,
                        name: p.name.clone(),
                        php: p.php.clone(),
                        node: p.node.clone(),
                        domain: format!("{}-runtime.devforge.test", p.name.to_lowercase()),
                        https: false,
                        vite: false,
                        queue: false,
                    },
                )
                .unwrap();
            let until = Instant::now() + Duration::from_secs(5);
            loop {
                let state = self
                    .native
                    .status(&id(&p.id), &receipt.operation_id)
                    .unwrap()
                    .state;
                if state.terminal() {
                    assert_eq!(state, OperationState::Succeeded);
                    break;
                }
                assert!(Instant::now() < until);
                thread::sleep(Duration::from_millis(10));
            }
            self.projects[index].revision += 1;
        }
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
fn execution_policy_requires_execution_authority_and_keeps_retained_output_grant_scoped() {
    use devforge_control::mutations::{ExecutionArguments, ExecutionCommandInput};
    use devforge_manager::external_client::ExecutionClient;
    let mut rig = Rig::new(None);
    let ordinary = rig.grant(0, false);
    let full = rig.grant(0, true);
    let project = id(&rig.projects[0].id);
    let request = env_request(
        project.as_str(),
        1,
        "run-version",
        MutationInput::ExecutePhp(ExecutionCommandInput {
            args: ExecutionArguments::new(vec!["--version".into()]).unwrap(),
            limits: None,
        }),
    );
    let mut denied = ExecutionClient::connect(
        rig.endpoint.clone(),
        ExternalCredential::from_wire(ordinary.credential.expose_secret()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        denied.plan(&request).unwrap().error().unwrap().code,
        ErrorCode::Unauthorized
    );
    assert_eq!(
        denied
            .output(id("missing-output"), &project, &id("unknown-op"))
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::Unauthorized
    );
    let mut capable = ExecutionClient::connect(
        rig.endpoint.clone(),
        ExternalCredential::from_wire(full.credential.expose_secret()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        capable.plan(&request).unwrap().error().unwrap().code,
        ErrorCode::PreconditionFailed
    );
    assert_eq!(
        capable
            .output(id("missing-output"), &project, &id("unknown-op"))
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::Unauthorized
    );
    assert_eq!(
        capable
            .output(
                id("foreign-output"),
                &id(&rig.projects[1].id),
                &id("unknown-op")
            )
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::ProjectMismatch
    );
    rig.native
        .revoke_external_grant(&full.record.grant_id)
        .unwrap();
    assert_eq!(
        capable
            .output(id("revoked-output"), &project, &id("unknown-op"))
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::GrantRevoked
    );
}

#[test]
fn native_pending_review_and_grant_scoped_operation_controls_never_accept_external_authority() {
    use devforge_manager::service_control::{ReviewState, ReviewTarget};
    let mut rig = Rig::new(None);
    let grant = rig.grant(0, false);
    let other = rig.grant(1, false);
    assert!(grant.record.information.is_empty());
    let mut client = rig.service(&grant);
    let req = request(&rig.projects[0].id, "stop", "owner-visible-stop");
    let plan = client.plan(&req).unwrap().data().unwrap().clone();
    let summaries = rig.native.pending_reviews().unwrap();
    let summary = summaries
        .iter()
        .find(|summary| summary.digest == plan.digest)
        .unwrap();
    assert_eq!(summary.state, ReviewState::Pending);
    assert_eq!(summary.grant_id, grant.record.grant_id);
    assert_eq!(summary.client_id, grant.record.client_id);
    assert_eq!(summary.project_name, rig.projects[0].name);
    assert_eq!(Path::new(&summary.project_root), rig.projects[0].path);
    assert!(summary.revalidation_required);
    assert!(!summary.machine_confirmation_required);
    assert!(matches!(&summary.target, ReviewTarget::Service { action } if action == "stop"));
    assert!(!summary.effects.is_empty());
    let raw = Client::new(rig.endpoint.clone())
        .request(&json!({
            "method":"pending_reviews", "session":grant.credential.expose_secret()
        }))
        .unwrap();
    assert_eq!(raw["ok"], false);
    let raw = Client::new(rig.endpoint.clone())
        .request(&json!({
            "method":"external_operation_by_key", "session":grant.credential.expose_secret(),
            "grant_id":grant.record.grant_id,"idempotency_key":"owner-visible-stop"
        }))
        .unwrap();
    assert_eq!(raw["ok"], false);
    rig.native.approve_service_plan(&plan.digest).unwrap();
    assert_eq!(
        rig.native.pending_reviews().unwrap()[0].state,
        ReviewState::Approved
    );
    let release = rig.block();
    let receipt = client
        .apply(&req, &plan.digest)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    assert_eq!(
        rig.native.pending_reviews().unwrap()[0]
            .operation_id
            .as_ref(),
        Some(&receipt.operation_id)
    );
    assert_eq!(
        rig.native
            .external_operation_by_key(
                &grant.record.grant_id,
                req.idempotency_key.as_ref().unwrap()
            )
            .unwrap()
            .unwrap()
            .operation_id,
        receipt.operation_id
    );
    assert!(
        rig.native
            .external_operation_status(&other.record.grant_id, &receipt.operation_id)
            .is_err()
    );
    assert!(
        rig.native
            .external_operation_cancel(&other.record.grant_id, &receipt.operation_id)
            .is_err()
    );
    assert!(
        rig.native
            .external_operation_by_key(
                &other.record.grant_id,
                req.idempotency_key.as_ref().unwrap()
            )
            .unwrap()
            .is_none()
    );
    rig.native
        .revoke_external_grant(&grant.record.grant_id)
        .unwrap();
    assert_eq!(
        rig.native.pending_reviews().unwrap()[0].state,
        ReviewState::Revoked
    );
    assert_eq!(
        rig.native
            .external_operation_cancel(&grant.record.grant_id, &receipt.operation_id)
            .unwrap(),
        devforge_manager::actor::CancelOutcome::Requested
    );
    drop(release);
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let state = rig
            .native
            .external_operation_status(&grant.record.grant_id, &receipt.operation_id)
            .unwrap()
            .state;
        if state.terminal() {
            assert_eq!(state, OperationState::Cancelled);
            break;
        }
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(10));
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

#[test]
fn runtime_policy_keeps_nonselection_fields_authoritative_and_requires_native_review() {
    use devforge_control::{
        Response,
        mutations::{MutationLimits, PackageInput},
    };
    let mut rig = Rig::new(None);
    rig.runtime_metadata();
    let manage = rig.grant(0, false);
    let full = rig.grant(0, true);
    let mut pm = rig.runtime(&manage);
    let mut exec = rig.runtime(&full);
    assert_eq!(
        pm.selection(id("no-execution"), &id(&rig.projects[0].id))
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::Unauthorized
    );
    let current = exec
        .selection(id("selection"), &id(&rig.projects[0].id))
        .unwrap()
        .data()
        .unwrap()
        .clone();
    assert_eq!(current.revision.get(), rig.projects[0].revision);
    let make = |key: &str, input: MutationInput| Request {
        api_version: Default::default(),
        request_id: id(key),
        project_id: current.project_id.clone(),
        expected_revision: Some(current.revision.get()),
        idempotency_key: Some(id(key)),
        input,
    };
    let plain = make(
        "not-cached",
        MutationInput::Configure(current.input.clone()),
    );
    assert_eq!(
        exec.plan(&plain).unwrap().error().unwrap().code,
        ErrorCode::PreconditionFailed
    );
    for field in ["name", "domain", "https", "vite", "queue"] {
        let mut input = current.input.clone();
        match field {
            "name" => {
                input.name =
                    devforge_control::mutations::ProjectName::new("altered".into()).unwrap()
            }
            "domain" => {
                input.domain =
                    devforge_control::mutations::DomainName::new("altered.devforge.test".into())
                        .unwrap()
            }
            "https" => input.https = !input.https,
            "vite" => input.vite = !input.vite,
            _ => input.queue = !input.queue,
        };
        let req = make(field, MutationInput::Configure(input));
        assert_eq!(
            exec.plan(&req).unwrap().error().unwrap().code,
            ErrorCode::InvalidInput
        );
    }
    let catalog = Catalog::bundled();
    let package = catalog
        .packages
        .iter()
        .find(|(_, p)| p.kind == "caddy" && p.target == Target::current().unwrap())
        .unwrap()
        .0
        .clone();
    let install = make(
        "install-unapproved",
        MutationInput::RuntimeInstall(PackageInput {
            package_id: id(&package),
            limits: Some(MutationLimits::default()),
        }),
    );
    let plan = pm.plan(&install).unwrap();
    assert!(plan.is_ok());
    let digest = plan.data().unwrap().digest.clone();
    assert_eq!(
        pm.apply(&install, &digest).unwrap().error().unwrap().code,
        ErrorCode::Unauthorized
    );
    let transport = Client::new(rig.endpoint.clone());
    let denied=transport.request(&json!({"method":"service_plan","credential":manage.credential.expose_secret(),"request":install})).unwrap();
    let denied: Response<devforge_control::mutations::PlanReceipt> =
        serde_json::from_value(denied).unwrap();
    assert_eq!(
        denied.error().unwrap().code,
        ErrorCode::UnsupportedCapability
    );
    let foreign = exec
        .selection(id("foreign"), &id(&rig.projects[1].id))
        .unwrap();
    assert_eq!(foreign.error().unwrap().code, ErrorCode::ProjectMismatch);
    let before = rig.native.snapshot().unwrap();
    let row = before.snapshot["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["project"]["id"] == rig.projects[0].id)
        .unwrap();
    assert_eq!(row["project"]["php"], rig.projects[0].php);
    assert_eq!(row["project"]["composer"], rig.projects[0].composer);
}

fn environment_yaml(rig: &Rig, index: usize) -> String {
    let catalog = Catalog::bundled();
    let project = &rig.projects[index];
    let sites: serde_json::Value =
        serde_json::from_slice(&std::fs::read(rig.root.path().join("serving/sites.json")).unwrap())
            .unwrap();
    let domain = sites["sites"]
        .as_array()
        .unwrap()
        .iter()
        .find(|site| site["project_id"] == project.id)
        .unwrap()["domain"]
        .as_str()
        .unwrap();
    let php = catalog
        .get(&project.php)
        .unwrap()
        .version
        .split('-')
        .next()
        .unwrap();
    let node = catalog
        .get(&project.node)
        .unwrap()
        .version
        .trim_start_matches('v');
    let mut manifest:devforge_projects::manifest::Manifest=serde_json::from_value(json!({"schema":1,"project":{"name":project.name,"domain":domain,"https":false,"vite":false,"queue":false},"runtimes":{"php":php,"node":node,"composer":"2.11.2"},"pins":{}})).unwrap();
    manifest.pin_all(&catalog).unwrap();
    manifest.yaml().unwrap()
}
fn env_request(
    project: &str,
    revision: u64,
    key: &str,
    input: MutationInput,
) -> Request<MutationInput> {
    Request {
        api_version: Default::default(),
        request_id: id(key),
        project_id: id(project),
        expected_revision: Some(revision),
        idempotency_key: Some(id(key)),
        input,
    }
}
fn yaml_input(expected: Option<devforge_control::Digest>, text: String) -> MutationInput {
    MutationInput::YamlSave(devforge_control::mutations::YamlSaveInput {
        expected,
        text: devforge_control::mutations::YamlText::new(text).unwrap(),
        limits: None,
    })
}

#[test]
fn env_policy_opaque_versions_mask_invalid_yaml_and_keep_sensitive_fingerprints_out_of_journal() {
    let mut rig = Rig::new(None);
    rig.runtime_metadata();
    let writer = rig.env_grant(0, false);
    let mut env = rig.environment(&writer);
    let project = rig.projects[0].clone();
    let old = b"not: [valid YAML\n# PRIVATE_OLD=letmein\n";
    std::fs::write(project.path.join("devforge.yaml"), old).unwrap();
    let preview = env
        .plan(&env_request(
            &project.id,
            project.revision,
            "invalid-preview",
            MutationInput::EnvironmentPlan(devforge_control::mutations::EnvironmentPlanInput {
                write_env: false,
                limits: None,
            }),
        ))
        .unwrap();
    assert!(preview.is_ok());
    let preview = preview.data().unwrap();
    assert!(preview.receipt.is_none());
    assert!(preview.document.version.is_some());
    let public = serde_json::to_string(preview).unwrap();
    assert!(!public.contains("PRIVATE_OLD"));
    assert!(!public.contains(devforge_control_adapter::sha256(old).unwrap().as_str()));
    assert!(!public.contains("state_hash"));
    assert!(!public.contains("reference"));
    let text = format!("# YAML_CANARY=lowentropy\n{}", environment_yaml(&rig, 0));
    let req = env_request(
        &project.id,
        project.revision,
        "save-sensitive",
        yaml_input(preview.document.version.clone(), text.clone()),
    );
    let plan = env.plan(&req).unwrap();
    assert!(plan.is_ok(), "save preview denied: {:?}", plan.error());
    let plan = plan.data().unwrap().receipt.as_ref().unwrap().clone();
    let reviews = rig.native.pending_reviews().unwrap();
    let summary = reviews
        .iter()
        .find(|summary| summary.digest == plan.digest)
        .unwrap();
    assert!(
        matches!(&summary.target, devforge_manager::service_control::ReviewTarget::YamlSave { summary, .. }
        if summary.domain.ends_with(".devforge.test") && summary.pin_count > 0)
    );
    let safe_summary = serde_json::to_string(summary).unwrap();
    assert!(!safe_summary.contains("YAML_CANARY"));
    assert!(!safe_summary.contains("lowentropy"));
    assert!(
        !safe_summary.contains(
            devforge_control_adapter::sha256(text.as_bytes())
                .unwrap()
                .as_str()
        )
    );
    assert_eq!(
        env.apply(&req, &plan.digest).unwrap().error().unwrap().code,
        ErrorCode::Unauthorized
    );
    rig.native.approve_service_plan(&plan.digest).unwrap();
    let applied = env.apply(&req, &plan.digest).unwrap();
    assert!(applied.is_ok(), "save denied: {:?}", applied.error());
    let receipt = applied.data().unwrap().clone();
    assert_eq!(rig.complete(&writer, &receipt), OperationState::Succeeded);
    assert_eq!(
        std::fs::read_to_string(project.path.join("devforge.yaml")).unwrap(),
        text
    );
    let again = env.apply(&req, &plan.digest).unwrap();
    assert_eq!(again.data().unwrap().operation_id, receipt.operation_id);
    let binding = devforge_control::ProjectBinding::from_registered(
        id(&project.id),
        &project.path,
        project.revision,
    )
    .unwrap();
    let raw_fp = devforge_control_adapter::sha256(
        &devforge_control::mutations::fingerprint_material(&req, &binding).unwrap(),
    )
    .unwrap();
    assert_ne!(plan.request_fingerprint, raw_fp);
    let db =
        rusqlite::Connection::open(rig.root.path().join("control/operations.sqlite3")).unwrap();
    let stored: String = db
        .query_row(
            "SELECT fingerprint FROM operations WHERE operation_id=?1",
            [receipt.operation_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(stored.starts_with("volatile-secret-v1:"));
    assert_ne!(stored, raw_fp.as_str());
    drop(db);
    let bytes = std::fs::read(rig.root.path().join("control/operations.sqlite3")).unwrap();
    let journal = String::from_utf8_lossy(&bytes);
    assert!(!journal.contains("YAML_CANARY"));
    assert!(!journal.contains(raw_fp.as_str()));
    let stale = env_request(
        &project.id,
        project.revision,
        "stale-version",
        yaml_input(preview.document.version.clone(), environment_yaml(&rig, 0)),
    );
    assert_eq!(
        env.plan(&stale).unwrap().error().unwrap().code,
        ErrorCode::ReviewMismatch
    );
    let create = env_request(
        &project.id,
        project.revision,
        "create-only",
        yaml_input(None, environment_yaml(&rig, 0)),
    );
    assert_eq!(
        env.plan(&create).unwrap().error().unwrap().code,
        ErrorCode::ReviewMismatch
    );
    let changed = env_request(
        &project.id,
        project.revision,
        "save-sensitive",
        yaml_input(
            preview.document.version.clone(),
            format!("# changed\n{}", environment_yaml(&rig, 0)),
        ),
    );
    assert_eq!(
        env.apply(&changed, &plan.digest)
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::IdempotencyConflict
    );
    let oversized = env_request(
        &project.id,
        project.revision,
        "oversized",
        yaml_input(None, "x".repeat(16385)),
    );
    assert!(matches!(
        env.plan(&oversized),
        Err(ErrorCode::OutputLimitExceeded)
    ));
    let state = env
        .document_state(id("version"), &id(&project.id))
        .unwrap()
        .data()
        .unwrap()
        .clone();
    let other = rig.env_grant(0, false);
    let mut foreign = rig.environment(&other);
    let cross = env_request(
        &project.id,
        project.revision,
        "foreign-version",
        yaml_input(state.version, environment_yaml(&rig, 0)),
    );
    assert_eq!(
        foreign.plan(&cross).unwrap().error().unwrap().code,
        ErrorCode::ReviewMismatch
    );
    let apply = env_request(
        &project.id,
        project.revision,
        "no-execution",
        MutationInput::EnvironmentApply(devforge_control::mutations::EnvironmentApplyInput {
            write_env: true,
            limits: None,
        }),
    );
    assert_eq!(
        env.plan(&apply).unwrap().error().unwrap().code,
        ErrorCode::Unauthorized
    );
    rig.native
        .revoke_external_grant(&writer.record.grant_id)
        .unwrap();
    assert_eq!(
        env.document_state(id("revoked"), &id(&project.id))
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::GrantRevoked
    );
}

#[test]
fn database_policy_requires_sensitive_scope_and_keeps_choices_project_scoped_without_startup() {
    use devforge_control::{
        Response,
        mutations::{DatabaseBindInput, DatabaseRestoreInput},
    };
    let mut rig = Rig::new(None);
    let project = rig.projects[0].clone();
    let pm = rig.grant(0, false);
    let sensitive = rig.env_grant(0, false);
    let mut client = rig.database(&sensitive);
    let mut denied = rig.database(&pm);
    let choices = client.choices(id("choices"), &id(&project.id)).unwrap();
    assert!(choices.is_ok());
    let choices = choices.data().unwrap();
    assert_eq!(choices.project_id, id(&project.id));
    assert!(choices.stopped);
    assert!(choices.instance_ids.is_empty());
    assert!(choices.backup_ids.is_empty());
    let public = serde_json::to_string(choices).unwrap();
    for forbidden in ["secret_ref", "admin", "sha256", "port", "role", "path"] {
        assert!(!public.contains(forbidden));
    }
    let bind = env_request(
        &project.id,
        project.revision,
        "bind-denied",
        MutationInput::DatabaseBind(DatabaseBindInput {
            instance_id: id("missing-instance"),
            limits: None,
        }),
    );
    assert_eq!(
        denied.plan(&bind).unwrap().error().unwrap().code,
        ErrorCode::Unauthorized
    );
    assert_eq!(
        client.plan(&bind).unwrap().error().unwrap().code,
        ErrorCode::PreconditionFailed
    );
    let restore = env_request(
        &project.id,
        project.revision,
        "restore-denied",
        MutationInput::DatabaseRestore(DatabaseRestoreInput {
            backup_id: id("missing-backup"),
            limits: None,
        }),
    );
    assert_eq!(
        denied.plan(&restore).unwrap().error().unwrap().code,
        ErrorCode::Unauthorized
    );
    assert_eq!(
        client.plan(&restore).unwrap().error().unwrap().code,
        ErrorCode::PreconditionFailed
    );
    assert_eq!(
        client
            .choices(id("foreign"), &id(&rig.projects[1].id))
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::ProjectMismatch
    );
    let raw=Client::new(rig.endpoint.clone()).request(&json!({"method":"service_plan","credential":sensitive.credential.expose_secret(),"request":bind})).unwrap();
    let raw: Response<devforge_control::mutations::PlanReceipt> =
        serde_json::from_value(raw).unwrap();
    assert_eq!(raw.error().unwrap().code, ErrorCode::UnsupportedCapability);
    let raw=Client::new(rig.endpoint.clone()).request(&json!({"method":"db_plan","credential":sensitive.credential.expose_secret(),"request":request(&project.id,"stop","wrong-family")})).unwrap();
    let raw: Response<devforge_control::mutations::PlanReceipt> =
        serde_json::from_value(raw).unwrap();
    assert_eq!(raw.error().unwrap().code, ErrorCode::UnsupportedCapability);
    let db =
        rusqlite::Connection::open(rig.root.path().join("control/operations.sqlite3")).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM operations", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        0
    );
    rig.native
        .revoke_external_grant(&sensitive.record.grant_id)
        .unwrap();
    assert_eq!(
        client
            .choices(id("revoked"), &id(&project.id))
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::GrantRevoked
    );
}

#[test]
fn domain_policy_reads_only_own_facts_and_blocks_missing_native_preparation_and_json_elevation() {
    use devforge_manager::service_control::{DomainPlanKind, DomainReadKind};
    let mut rig = Rig::new(None);
    rig.runtime_metadata();
    let project = rig.projects[0].clone();
    let grant = rig
        .native
        .issue_external_grant(
            &id(&project.id),
            &[Scope::Observer],
            Duration::from_secs(600),
        )
        .unwrap();
    let mut observer = rig.domain(&grant);
    let authority = rig.root.path().join("serving/authority");
    assert!(!authority.exists());
    for operation in [
        DomainReadKind::Url,
        DomainReadKind::Ports,
        DomainReadKind::CertificateStatus,
    ] {
        let facts = observer
            .facts(id("facts"), &id(&project.id), operation)
            .unwrap();
        assert!(facts.is_ok());
        let facts = facts.data().unwrap();
        assert_eq!(facts.domain.as_str(), "alpha-runtime.devforge.test");
        assert!(!facts.ca_prepared);
        assert!(facts.ca_fingerprint.is_none());
        assert!(facts.system_trusted.is_none());
        let text = serde_json::to_string(facts).unwrap();
        assert!(!text.contains("bravo"));
        assert!(!text.contains("hosts_path"));
        assert!(!text.contains("/etc/hosts"));
    }
    for kind in [DomainPlanKind::Hosts, DomainPlanKind::Trust] {
        let prepared = observer
            .prepare(id("prepare"), &id(&project.id), kind)
            .unwrap();
        assert!(prepared.is_ok());
        let prepared = prepared.data().unwrap();
        assert!(prepared.ticket.is_none());
        assert_eq!(
            prepared.blocked.as_ref().unwrap().as_str(),
            "native_owner_preparation_required"
        );
        assert!(prepared.shared_ca_preserved);
    }
    assert!(!authority.exists());
    assert_eq!(
        observer
            .prepare(
                id("cleanup-denied"),
                &id(&project.id),
                DomainPlanKind::Cleanup
            )
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::Unauthorized
    );
    assert_eq!(
        observer
            .facts(id("foreign"), &id(&rig.projects[1].id), DomainReadKind::Url)
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::ProjectMismatch
    );
    let raw=Client::new(rig.endpoint.clone()).request(&json!({"method":"confirm_domain_change","session":grant.credential.expose_secret(),"digest":"0".repeat(64)})).unwrap();
    assert_eq!(raw["ok"], false);
    let request = env_request(
        &project.id,
        project.revision,
        "no-elevation",
        MutationInput::DomainApply(devforge_control::mutations::DomainApplyInput {
            plan: devforge_control::Digest::new("0".repeat(64)).unwrap(),
            limits: None,
        }),
    );
    let denied = observer.plan(&request).unwrap();
    assert_eq!(denied.error().unwrap().code, ErrorCode::Unauthorized);
    let mut forged = serde_json::to_value(&request).unwrap();
    forged["input"]["arguments"]["confirmed"] = json!(true);
    let raw=Client::new(rig.endpoint.clone()).request(&json!({"method":"domain_plan","credential":grant.credential.expose_secret(),"request":forged})).unwrap();
    assert_eq!(raw["error"]["code"], "invalid_input");
    assert!(!authority.exists());
}

#[test]
fn metadata_policy_masks_payloads_and_binds_sequence_cursors_to_grant_filter_and_epoch() {
    use devforge_control::{limits::*, tools::*};
    let mut rig = Rig::new(None);
    let project = rig.projects[0].clone();
    let grant = rig
        .native
        .issue_external_grant(
            &id(&project.id),
            &[Scope::Observer],
            Duration::from_secs(600),
        )
        .unwrap();
    let mut client = rig.diagnostics(&grant);
    let page = |cursor| PageRequest {
        limit: Bounded::new(2).unwrap(),
        max_bytes: Bounded::new(65536).unwrap(),
        timeout_ms: Bounded::new(5000).unwrap(),
        cursor,
    };
    let log = |cursor, level| Request {
        api_version: Default::default(),
        request_id: id("metadata-log"),
        project_id: id(&project.id),
        expected_revision: Some(project.revision),
        idempotency_key: None,
        input: ToolInput::Log(LogInput {
            page: Some(page(cursor)),
            content: None,
            since: None,
            query: None,
            level,
        }),
    };
    let missing = client.read(&log(None, None)).unwrap();
    assert!(missing.is_ok());
    let ToolOutput::Log(missing) = missing.data().unwrap() else {
        panic!("logoutput")
    };
    assert!(missing.items.is_empty());
    assert!(missing.freshness.window.as_ref().unwrap().source_missing);
    assert!(!rig.root.path().join("inspector").exists());
    let mail_request = Request {
        api_version: Default::default(),
        request_id: id("metadata-mail"),
        project_id: id(&project.id),
        expected_revision: Some(project.revision),
        idempotency_key: None,
        input: ToolInput::Mail(MailInput {
            page: None,
            subject: None,
            recipient: None,
        }),
    };
    let mail = client.read(&mail_request).unwrap();
    assert!(mail.is_ok());
    assert!(!rig.root.path().join("mail").exists());
    let mut forbidden = log(None, None);
    if let ToolInput::Log(input) = &mut forbidden.input {
        input.query = Some(Text::new(String::new()).unwrap());
    }
    match client.read(&forbidden) {
        Err(code) => assert_eq!(code, ErrorCode::Unauthorized),
        Ok(reply) => assert_eq!(reply.error().unwrap().code, ErrorCode::Unauthorized),
    }
    assert!(!rig.root.path().join("inspector").exists());
    let directory = rig.root.path().join("inspector");
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("events.sqlite3");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("PRAGMA journal_mode=DELETE; CREATE TABLE events(sequence INTEGER PRIMARY KEY AUTOINCREMENT,project TEXT NOT NULL,received INTEGER NOT NULL,body TEXT NOT NULL); PRAGMA user_version=1;").unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    for n in 0..4 {
        let event = json!({"kind":"request","context":format!("{:032x}",n+1),"name":"GET /PRIVATE_URL?token=CANARY","duration_ms":2.2,"data":{"status":201,"query_count":3,"sql":"PRIVATE_SQL_CANARY","headers":{"Authorization":"PRIVATE_HEADER_CANARY"}}});
        db.execute(
            "INSERT INTO events(project,received,body) VALUES(?1,?2,?3)",
            rusqlite::params![project.id, now, event.to_string()],
        )
        .unwrap();
    }
    for _ in 0..6 {
        let event = json!({"kind":"log","context":"c".repeat(32),"name":"info","duration_ms":0.0,"data":{"message":"PRIVATE_MESSAGE_CANARY","stack":"PRIVATE_STACK_CANARY"}});
        db.execute(
            "INSERT INTO events(project,received,body) VALUES(?1,?2,?3)",
            rusqlite::params![project.id, now, event.to_string()],
        )
        .unwrap();
    }
    let event = json!({"kind":"log","context":"d".repeat(32),"name":"info","duration_ms":0.0,"data":{"message":"FOREIGN_CANARY"}});
    db.execute(
        "INSERT INTO events(project,received,body) VALUES(?1,?2,?3)",
        rusqlite::params![rig.projects[1].id, now, event.to_string()],
    )
    .unwrap();
    let first = client.read(&log(None, None)).unwrap();
    assert!(first.is_ok());
    let text = serde_json::to_string(&first).unwrap();
    for canary in ["PRIVATE_", "FOREIGN_CANARY", "Authorization"] {
        assert!(!text.contains(canary));
    }
    let ToolOutput::Log(first) = first.data().unwrap() else {
        panic!("logoutput")
    };
    let cursor = first.next_cursor.clone().unwrap();
    let observed = first.freshness.observed_at.get();
    let high = first.freshness.window.as_ref().unwrap().high_water.get();
    let mut ids = first
        .items
        .iter()
        .map(|i| i.event_id.clone())
        .collect::<Vec<_>>();
    let mut tampered = cursor.clone();
    tampered.offset = SafeInt::new(tampered.offset.get() + 1).unwrap();
    assert_eq!(
        client
            .read(&log(Some(tampered), None))
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::CursorExpired
    );
    assert_eq!(
        client
            .read(&log(
                Some(cursor.clone()),
                Some(Text::new("error".into()).unwrap())
            ))
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::CursorExpired
    );
    let second_grant = rig
        .native
        .issue_external_grant(
            &id(&project.id),
            &[Scope::Observer],
            Duration::from_secs(600),
        )
        .unwrap();
    let mut other = rig.diagnostics(&second_grant);
    assert_eq!(
        other
            .read(&log(Some(cursor.clone()), None))
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::CursorExpired
    );
    // Clearing/appending the same store cannot change the already observed
    // frozen window; subsequent pages explicitly remain Cached/original-time.
    db.execute("DELETE FROM events WHERE project=?1", [project.id.as_str()])
        .unwrap();
    db.execute(
        "INSERT INTO events(project,received,body) VALUES(?1,?2,?3)",
        rusqlite::params![project.id, now, event.to_string()],
    )
    .unwrap();
    let mut next = Some(cursor.clone());
    while let Some(current) = next {
        let response = client.read(&log(Some(current), None)).unwrap();
        assert!(response.is_ok());
        let ToolOutput::Log(output) = response.data().unwrap() else {
            panic!("logoutput")
        };
        assert_eq!(output.freshness.origin, devforge_control::Origin::Cached);
        assert_eq!(output.freshness.observed_at.get(), observed);
        assert_eq!(
            output.freshness.window.as_ref().unwrap().high_water.get(),
            high
        );
        ids.extend(output.items.iter().map(|i| i.event_id.clone()));
        next = output.next_cursor.clone();
    }
    assert_eq!(ids.len(), 6);
    ids.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    ids.dedup();
    assert_eq!(ids.len(), 6);
    drop(db);
    std::fs::rename(&path, directory.join("old.sqlite3")).unwrap();
    let replacement = rusqlite::Connection::open(&path).unwrap();
    replacement.execute_batch("CREATE TABLE events(sequence INTEGER PRIMARY KEY AUTOINCREMENT,project TEXT NOT NULL,received INTEGER NOT NULL,body TEXT NOT NULL); PRAGMA user_version=1;").unwrap();
    drop(replacement);
    assert_eq!(
        client
            .read(&log(Some(cursor), None))
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::CursorExpired
    );
    let mailbox = rig.root.path().join("mail").join(&project.id);
    std::fs::create_dir_all(&mailbox).unwrap();
    std::fs::write(mailbox.join(".devforge-inbox"), b"devforge-inbox-v1\n").unwrap();
    let db = rusqlite::Connection::open(mailbox.join("inbox.sqlite3")).unwrap();
    db.execute_batch("CREATE TABLE messages(sequence INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT UNIQUE NOT NULL,received INTEGER NOT NULL,summary TEXT NOT NULL,search TEXT NOT NULL,raw BLOB NOT NULL); PRAGMA user_version=1;").unwrap();
    let summary = json!({"from":"PRIVATE_FROM_CANARY","subject":"PRIVATE_SUBJECT_CANARY","attachments":1,"attachment_name":"PRIVATE_FILE_CANARY"});
    db.execute("INSERT INTO messages(id,received,summary,search,raw) VALUES(?1,?2,?3,'PRIVATE_SEARCH_CANARY',X'50524956415445')",rusqlite::params!["e".repeat(32),now,summary.to_string()]).unwrap();
    drop(db);
    let mail = client.read(&mail_request).unwrap();
    assert!(mail.is_ok(), "mail metadata fault: {:?}", mail.error());
    let serialized = serde_json::to_string(&mail).unwrap();
    assert!(!serialized.contains("PRIVATE"));
    let ToolOutput::Mail(mail) = mail.data().unwrap() else {
        panic!("mailoutput")
    };
    assert_eq!(mail.items[0].attachments.get(), 1);
    rig.native
        .revoke_external_grant(&grant.record.grant_id)
        .unwrap();
    assert_eq!(
        client.read(&log(None, None)).unwrap().error().unwrap().code,
        ErrorCode::GrantRevoked
    );
}

#[test]
fn a04_inspector_details_require_project_scope_and_redact_queries() {
    use devforge_control::{InformationScope, limits::*, tools::*};
    let mut rig = Rig::new(None);
    let project = rig.projects[0].clone();
    let observer = rig
        .native
        .issue_external_grant(
            &id(&project.id),
            &[Scope::Observer],
            Duration::from_secs(600),
        )
        .unwrap();
    let grant = rig
        .native
        .issue_external_grant_with_information(
            &id(&project.id),
            &[Scope::Observer],
            &[InformationScope::InspectorQueries],
            Duration::from_secs(600),
        )
        .unwrap();
    let second = rig
        .native
        .issue_external_grant_with_information(
            &id(&project.id),
            &[Scope::Observer],
            &[InformationScope::InspectorQueries],
            Duration::from_secs(600),
        )
        .unwrap();
    let mut plain = rig.diagnostics(&observer);
    let mut client = rig.diagnostics(&grant);
    let mut other = rig.diagnostics(&second);
    let context = "c".repeat(32);
    let request = |project_id: &str, detail: bool, cursor| Request {
        api_version: Default::default(),
        request_id: id("a04-detail"),
        project_id: id(project_id),
        expected_revision: Some(project.revision),
        idempotency_key: None,
        input: ToolInput::Inspector(InspectorInput {
            page: Some(PageRequest {
                limit: Bounded::new(1).unwrap(),
                max_bytes: Bounded::new(65536).unwrap(),
                timeout_ms: Bounded::new(5000).unwrap(),
                cursor,
            }),
            context_id: Some(id(&context)),
            detail: Some(detail),
        }),
    };
    let denied = plain.read(&request(&project.id, true, None)).unwrap();
    assert_eq!(denied.error().unwrap().code, ErrorCode::Unauthorized);
    assert!(
        !rig.root.path().join("inspector").exists(),
        "denial before source IO"
    );
    let directory = rig.root.path().join("inspector");
    std::fs::create_dir(&directory).unwrap();
    let db = rusqlite::Connection::open(directory.join("events.sqlite3")).unwrap();
    db.execute_batch("PRAGMA journal_mode=DELETE; CREATE TABLE events(sequence INTEGER PRIMARY KEY AUTOINCREMENT,project TEXT NOT NULL,received INTEGER NOT NULL,body TEXT NOT NULL); PRAGMA user_version=1;").unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let events = [
        json!({"kind":"request","context":context,"name":"GET /PRIVATE_URL?token=PRIVATE_URL_CANARY","duration_ms":13.0,"data":{"status":200,"query_count":1,"sql_ms":12.0,"peak_memory_bytes":65536,"truncated":true,"headers":{"Authorization":"PRIVATE_HEADER_CANARY"}}}),
        json!({"kind":"query","context":context,"name":"select * from users where id=42 and email='SQL_SECRET_CANARY'","duration_ms":5.0,"data":{"connection":"sqlite","bindings":["PRIVATE_BINDING_CANARY"]}}),
        json!({"kind":"job","context":context,"name":"App\\Jobs\\Run","duration_ms":7.0,"data":{"status":"success","query_count":1}}),
        json!({"kind":"job","context":context,"name":"App\\Jobs\\Complete","duration_ms":7.0,"data":{"status":"success","query_count":0,"truncated":false}}),
        json!({"kind":"cache","context":context,"name":"hit","duration_ms":0.0,"data":{"key":"CACHE_SECRET_CANARY"}}),
        json!({"kind":"dump","context":context,"name":"array","duration_ms":0.0,"data":{"message":"safe diagnostic","token":"DUMP_SECRET_CANARY","nested":{"password":"NESTED_SECRET_CANARY","visible":"safe-child","raw_sql":"select 'DUMP_RAW_SQL_SECRET_CANARY'","query_string":"-- tag\nselect 'DUMP_ALIAS_SECRET_CANARY'","other":"/* trace */ SELECT 'DUMP_COMMENT_SECRET_CANARY'","sql":"select * from users where email='DUMP_SQL_SECRET_CANARY'","query":"select 'DUMP_QUERY_SECRET_CANARY'","statement":"select 'DUMP_STATEMENT_SECRET_CANARY'"}}}),
        json!({"kind":"dump","context":context,"name":"array","duration_ms":0.0,"data":{"oversize":"Z".repeat(9000)}}),
    ];
    let repeated_query = events[1].clone();
    for event in events {
        db.execute(
            "INSERT INTO events(project,received,body) VALUES(?1,?2,?3)",
            rusqlite::params![project.id, now, event.to_string()],
        )
        .unwrap();
    }
    for _ in 0..4 {
        db.execute(
            "INSERT INTO events(project,received,body) VALUES(?1,?2,?3)",
            rusqlite::params![project.id, now, repeated_query.to_string()],
        )
        .unwrap();
    }
    let foreign = json!({"kind":"query","context":context,"name":"select 'FOREIGN_SECRET_CANARY'","duration_ms":1.0,"data":{}});
    db.execute(
        "INSERT INTO events(project,received,body) VALUES(?1,?2,?3)",
        rusqlite::params![rig.projects[1].id, now, foreign.to_string()],
    )
    .unwrap();
    drop(db);
    let metadata = plain.read(&request(&project.id, false, None)).unwrap();
    assert!(metadata.is_ok());
    let metadata_text = serde_json::to_string(&metadata).unwrap();
    assert!(!metadata_text.contains("SQL_SECRET_CANARY") && !metadata_text.contains("PRIVATE_URL"));
    let ToolOutput::Inspector(metadata) = metadata.data().unwrap() else {
        panic!("inspector metadata")
    };
    assert!(metadata.items.iter().all(|item| item.detail.is_none()));
    let first = client.read(&request(&project.id, true, None)).unwrap();
    assert!(first.is_ok(), "{:?}", first.error());
    let ToolOutput::Inspector(first_page) = first.data().unwrap() else {
        panic!("inspector details")
    };
    let cursor = first_page
        .next_cursor
        .clone()
        .expect("multiple detail events");
    let foreign_cursor = other
        .read(&request(&project.id, true, Some(cursor.clone())))
        .unwrap();
    assert_eq!(
        foreign_cursor.error().unwrap().code,
        ErrorCode::CursorExpired
    );
    let metadata_cursor = client
        .read(&request(&project.id, false, Some(cursor.clone())))
        .unwrap();
    assert_eq!(
        metadata_cursor.error().unwrap().code,
        ErrorCode::CursorExpired,
        "cursor must bind content mode"
    );
    let mismatch = client
        .read(&request(&rig.projects[1].id, true, None))
        .unwrap();
    assert_eq!(mismatch.error().unwrap().code, ErrorCode::ProjectMismatch);
    let mut all_json = serde_json::to_string(&first).unwrap();
    let mut kinds = first_page
        .items
        .iter()
        .filter_map(|item| item.detail.as_ref().map(|d| d.kind.as_str().to_string()))
        .collect::<Vec<_>>();
    let mut repeated = first_page
        .items
        .iter()
        .filter_map(|item| {
            item.detail
                .as_ref()
                .and_then(|d| d.repeated_count.map(|n| n.get()))
        })
        .collect::<Vec<_>>();
    let mut details = first_page
        .items
        .iter()
        .filter_map(|item| item.detail.clone())
        .collect::<Vec<_>>();
    let mut next = first_page.next_cursor.clone();
    while let Some(cursor) = next {
        let page = client
            .read(&request(&project.id, true, Some(cursor)))
            .unwrap();
        assert!(page.is_ok(), "{:?}", page.error());
        all_json.push_str(&serde_json::to_string(&page).unwrap());
        let ToolOutput::Inspector(page) = page.data().unwrap() else {
            panic!("inspector details")
        };
        kinds.extend(
            page.items
                .iter()
                .filter_map(|item| item.detail.as_ref().map(|d| d.kind.as_str().to_string())),
        );
        repeated.extend(page.items.iter().filter_map(|item| {
            item.detail
                .as_ref()
                .and_then(|d| d.repeated_count.map(|n| n.get()))
        }));
        details.extend(page.items.iter().filter_map(|item| item.detail.clone()));
        next = page.next_cursor.clone();
    }
    for secret in [
        "PRIVATE_URL_CANARY",
        "PRIVATE_HEADER_CANARY",
        "SQL_SECRET_CANARY",
        "PRIVATE_BINDING_CANARY",
        "DUMP_SECRET_CANARY",
        "NESTED_SECRET_CANARY",
        "DUMP_SQL_SECRET_CANARY",
        "DUMP_RAW_SQL_SECRET_CANARY",
        "DUMP_ALIAS_SECRET_CANARY",
        "DUMP_COMMENT_SECRET_CANARY",
        "DUMP_QUERY_SECRET_CANARY",
        "DUMP_STATEMENT_SECRET_CANARY",
        "CACHE_SECRET_CANARY",
        "FOREIGN_SECRET_CANARY",
    ] {
        assert!(!all_json.contains(secret), "detail leaked {secret}");
    }
    assert!(
        all_json.contains("select") && all_json.contains('?'),
        "parameterized query detail should remain useful"
    );
    assert_eq!(
        repeated,
        vec![5; 5],
        "query repetition must be scoped to this captured context"
    );
    assert!(
        all_json.contains("\"sql_ms\":12") && all_json.contains("\"peak_memory_bytes\":65536"),
        "request profile metrics"
    );
    let captured_request = details
        .iter()
        .find(|d| d.kind.as_str() == "request")
        .unwrap();
    assert_eq!(
        captured_request.capture_truncated,
        Some(true),
        "request collector incompleteness must be explicit"
    );
    assert!(
        !captured_request.truncated,
        "short projected text itself is not truncated"
    );
    assert_eq!(
        captured_request.duration_ms.map(|n| n.get()),
        Some(13),
        "request duration is measured"
    );
    let cache = details.iter().find(|d| d.kind.as_str() == "cache").unwrap();
    assert!(
        cache.duration_ms.is_none(),
        "unmeasured cache duration must remain unknown"
    );
    assert!(
        cache.capture_truncated.is_none(),
        "cache has no capture-summary flag"
    );
    let dump = details
        .iter()
        .find(|d| d.kind.as_str() == "dump" && d.text.as_str().contains("safe diagnostic"))
        .unwrap();
    assert!(
        dump.duration_ms.is_none(),
        "unmeasured dump duration must remain unknown"
    );
    assert!(
        dump.text.as_str().contains("safe diagnostic") && dump.text.as_str().contains("safe-child"),
        "authorized user must receive non-sensitive dump values"
    );
    assert!(
        !dump.text.as_str().contains("DUMP_SECRET_CANARY")
            && !dump.text.as_str().contains("NESTED_SECRET_CANARY")
            && !dump.text.as_str().contains("DUMP_SQL_SECRET_CANARY")
            && !dump.text.as_str().contains("DUMP_RAW_SQL_SECRET_CANARY")
            && !dump.text.as_str().contains("DUMP_ALIAS_SECRET_CANARY")
            && !dump.text.as_str().contains("DUMP_COMMENT_SECRET_CANARY")
            && !dump.text.as_str().contains("DUMP_QUERY_SECRET_CANARY")
            && !dump.text.as_str().contains("DUMP_STATEMENT_SECRET_CANARY"),
        "dump secrets must be masked"
    );
    let withheld = details
        .iter()
        .find(|d| d.kind.as_str() == "dump" && d.truncated)
        .expect("oversize dump must be explicitly truncated");
    assert!(
        !withheld.text.as_str().contains(&"Z".repeat(2049)),
        "oversize payload must not exceed bounded output"
    );
    let jobs = details
        .iter()
        .filter(|d| d.kind.as_str() == "job")
        .collect::<Vec<_>>();
    assert!(
        jobs.iter().any(|d| d.capture_truncated == Some(false)),
        "explicit complete capture preserved"
    );
    assert!(
        jobs.iter().any(|d| d.capture_truncated.is_none()),
        "legacy absent capture state stays unknown"
    );
    assert!(
        details
            .iter()
            .filter(|d| d.kind.as_str() == "query")
            .all(|d| d.capture_truncated.is_none()),
        "query events must not invent a capture completeness state"
    );
    for expected in ["request", "query", "job", "dump", "cache"] {
        assert!(
            kinds.iter().any(|kind| kind == expected),
            "missing {expected}"
        );
    }
    rig.native
        .revoke_external_grant(&grant.record.grant_id)
        .unwrap();
    let revoked = client
        .read(&request(&project.id, true, Some(cursor)))
        .unwrap();
    assert_eq!(revoked.error().unwrap().code, ErrorCode::GrantRevoked);
}

#[test]
fn a03_log_content_requires_project_right_and_searches_redacted_projection() {
    use devforge_control::{InformationScope, limits::*, tools::*};
    let mut rig = Rig::new(None);
    let project = rig.projects[0].clone();
    let observer = rig
        .native
        .issue_external_grant(
            &id(&project.id),
            &[Scope::Observer],
            Duration::from_secs(600),
        )
        .unwrap();
    let privileged = rig
        .native
        .issue_external_grant_with_information(
            &id(&project.id),
            &[Scope::Observer],
            &[InformationScope::LogContent],
            Duration::from_secs(600),
        )
        .unwrap();
    let another = rig
        .native
        .issue_external_grant_with_information(
            &id(&project.id),
            &[Scope::Observer],
            &[InformationScope::LogContent],
            Duration::from_secs(600),
        )
        .unwrap();
    let mut plain = rig.diagnostics(&observer);
    let doctor = Request {
        api_version: Default::default(),
        request_id: id("a03-doctor"),
        project_id: id(&project.id),
        expected_revision: Some(project.revision),
        idempotency_key: None,
        input: ToolInput::Health(HealthInput { page: None }),
    };
    let health = plain.read(&doctor).unwrap();
    let ToolOutput::Health(health) = health.data().unwrap() else {
        panic!("health output")
    };
    assert_eq!(health.freshness.origin, devforge_control::Origin::Cached);
    assert!(health.items[0].evidence.is_some() && health.items[0].impact.is_some());
    let mut allowed = rig.diagnostics(&privileged);
    let mut other = rig.diagnostics(&another);
    let request = |project_id: &str, content: bool, query: Option<&str>, cursor| Request {
        api_version: Default::default(),
        request_id: id("a03-log"),
        project_id: id(project_id),
        expected_revision: Some(project.revision),
        idempotency_key: None,
        input: ToolInput::Log(LogInput {
            page: Some(PageRequest {
                limit: Bounded::new(1).unwrap(),
                max_bytes: Bounded::new(65536).unwrap(),
                timeout_ms: Bounded::new(5000).unwrap(),
                cursor,
            }),
            content: Some(content),
            since: None,
            query: query.map(|v| Text::new(v.to_string()).unwrap()),
            level: None,
        }),
    };
    let deny = |result: Result<devforge_control::Response<ToolOutput>, ErrorCode>| match result {
        Err(e) => assert_eq!(e, ErrorCode::Unauthorized),
        Ok(r) => assert_eq!(r.error().unwrap().code, ErrorCode::Unauthorized),
    };
    deny(plain.read(&request(&project.id, true, None, None)));
    deny(plain.read(&request(&project.id, false, Some("database"), None)));
    assert!(
        !rig.root.path().join("inspector").exists(),
        "denied content must not create/read a source"
    );
    let directory = rig.root.path().join("inspector");
    std::fs::create_dir(&directory).unwrap();
    let db = rusqlite::Connection::open(directory.join("events.sqlite3")).unwrap();
    db.execute_batch("PRAGMA journal_mode=DELETE; CREATE TABLE events(sequence INTEGER PRIMARY KEY AUTOINCREMENT,project TEXT NOT NULL,received INTEGER NOT NULL,body TEXT NOT NULL); PRAGMA user_version=1;").unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let exception = json!({"kind":"exception","context":"e".repeat(32),"name":"RuntimeException","duration_ms":0.0,"data":{"message":"Exceptional failure token=EXCEPTION_CANARY","stack":"SECRET_STACK_CANARY"}});
    db.execute(
        "INSERT INTO events(project,received,body) VALUES(?1,?2,?3)",
        rusqlite::params![project.id, now, exception.to_string()],
    )
    .unwrap();
    for _ in 0..3 {
        let event = json!({"kind":"log","context":"c".repeat(32),"name":"error","duration_ms":0.0,
            "data":{"message":"Database failed password=PRIVATE_CANARY Authorization: Bearer BEARER_CANARY","token":"PRIVATE_TOKEN_CANARY"}});
        db.execute(
            "INSERT INTO events(project,received,body) VALUES(?1,?2,?3)",
            rusqlite::params![project.id, now, event.to_string()],
        )
        .unwrap();
    }
    let event = json!({"kind":"log","context":"d".repeat(32),"name":"error","duration_ms":0.0,"data":{"message":"Database FOREIGN_CANARY"}});
    db.execute(
        "INSERT INTO events(project,received,body) VALUES(?1,?2,?3)",
        rusqlite::params![rig.projects[1].id, now, event.to_string()],
    )
    .unwrap();
    drop(db);
    let metadata = plain
        .read(&request(&project.id, false, None, None))
        .unwrap();
    assert!(metadata.is_ok());
    assert!(
        !serde_json::to_string(&metadata)
            .unwrap()
            .contains("Database failed")
    );
    let mut metadata_future = request(&project.id, false, None, None);
    if let ToolInput::Log(input) = &mut metadata_future.input {
        input.since = Some(SafeInt::new((now + 1) as u64).unwrap());
    }
    let metadata_future = plain.read(&metadata_future).unwrap();
    let ToolOutput::Log(metadata_future) = metadata_future.data().unwrap() else {
        panic!("log output")
    };
    assert!(
        metadata_future.items.is_empty(),
        "metadata since excludes earlier records"
    );
    let mut wrong_level = request(&project.id, true, Some("database"), None);
    if let ToolInput::Log(input) = &mut wrong_level.input {
        input.level = Some(Text::new("warning".into()).unwrap());
    }
    let wrong_level = allowed.read(&wrong_level).unwrap();
    let ToolOutput::Log(wrong_level) = wrong_level.data().unwrap() else {
        panic!("log output")
    };
    assert!(
        wrong_level.items.is_empty(),
        "level filters apply to scoped content"
    );
    let mut future = request(&project.id, true, Some("database"), None);
    if let ToolInput::Log(input) = &mut future.input {
        input.since = Some(SafeInt::new((now + 1) as u64).unwrap());
    }
    let future = allowed.read(&future).unwrap();
    let ToolOutput::Log(future) = future.data().unwrap() else {
        panic!("log output")
    };
    assert!(future.items.is_empty(), "since excludes earlier records");
    let mut matching_time = request(&project.id, true, Some("database"), None);
    if let ToolInput::Log(input) = &mut matching_time.input {
        input.since = Some(SafeInt::new(now as u64).unwrap());
    }
    let matching_time = allowed.read(&matching_time).unwrap();
    let ToolOutput::Log(matching_time) = matching_time.data().unwrap() else {
        panic!("log output")
    };
    assert!(
        !matching_time.items.is_empty(),
        "since includes boundary timestamp"
    );
    let first = allowed
        .read(&request(&project.id, true, Some("database"), None))
        .unwrap();
    assert!(first.is_ok(), "{first:?}");
    let json = serde_json::to_string(&first).unwrap();
    assert!(
        !json.contains("PRIVATE_CANARY")
            && !json.contains("PRIVATE_TOKEN_CANARY")
            && !json.contains("BEARER_CANARY")
            && !json.contains("FOREIGN_CANARY")
    );
    assert!(json.contains("Database failed"));
    let ToolOutput::Log(result) = first.data().unwrap() else {
        panic!("log output")
    };
    let cursor = result.next_cursor.clone().unwrap();
    let mut changed_since = request(&project.id, true, Some("database"), Some(cursor.clone()));
    if let ToolInput::Log(input) = &mut changed_since.input {
        input.since = Some(SafeInt::new((now + 1) as u64).unwrap());
    }
    let changed_since = allowed.read(&changed_since).unwrap();
    assert_eq!(
        changed_since.error().unwrap().code,
        ErrorCode::CursorExpired,
        "cursor must bind the time filter"
    );
    let secret = allowed
        .read(&request(&project.id, true, Some("PRIVATE_CANARY"), None))
        .unwrap();
    let ToolOutput::Log(secret) = secret.data().unwrap() else {
        panic!("log output")
    };
    assert!(
        secret.items.is_empty(),
        "matching secret against raw source leaks information"
    );
    let exception = allowed
        .read(&request(&project.id, true, Some("exceptional"), None))
        .unwrap();
    let exception_json = serde_json::to_string(&exception).unwrap();
    assert!(
        !exception_json.contains("EXCEPTION_CANARY")
            && !exception_json.contains("SECRET_STACK_CANARY")
    );
    let ToolOutput::Log(exception) = exception.data().unwrap() else {
        panic!("log output")
    };
    assert!(exception.items[0].exception.is_some() && exception.items[0].message.is_none());
    let response = other
        .read(&request(&project.id, true, Some("database"), Some(cursor)))
        .unwrap();
    assert_eq!(response.error().unwrap().code, ErrorCode::CursorExpired);
    let foreign = allowed
        .read(&request(&rig.projects[1].id, true, None, None))
        .unwrap();
    assert_eq!(foreign.error().unwrap().code, ErrorCode::ProjectMismatch);
    rig.native
        .revoke_external_grant(&privileged.record.grant_id)
        .unwrap();
    let revoked = allowed
        .read(&request(&project.id, true, None, None))
        .unwrap();
    assert_eq!(revoked.error().unwrap().code, ErrorCode::GrantRevoked);
}

#[cfg(target_os = "macos")]
fn copy_cache(source: &Path, root: &Path, catalog: &Catalog, alternative: bool) {
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
                && ((p.kind == "php"
                    && (p.version.starts_with("8.4.27")
                        || (alternative && p.version.starts_with("8.5.12"))))
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
fn copy_cache(_: &Path, _: &Path, _: &Catalog, _: bool) {
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

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires an explicitly approved immutable native runtime cache"]
fn runtime_native_cached_select_http_replay_and_install_preserve_configuration() {
    use devforge_control::mutations::PackageInput;
    use std::io::{Read, Write};
    let source = std::path::PathBuf::from(
        std::env::var_os("DEVFORGE_SERVICE_CACHE").expect("explicit approved cache required"),
    );
    let mut rig = Rig::runtime_setup(Some(&source), false, true);
    std::fs::write(
        rig.projects[0].path.join("public/index.php"),
        "<?php echo 'M13_RUNTIME_ALPHA:'.PHP_VERSION;",
    )
    .unwrap();
    let a = rig.grant(0, true);
    let b = rig.grant(1, true);
    let pm = rig.grant(0, false);
    let mut sa = rig.service(&a);
    let mut sb = rig.service(&b);
    let mut runtime = rig.runtime(&a);
    let mut installer = rig.runtime(&pm);
    let service = |rig: &mut Rig,
                   client: &mut ServiceClient,
                   grant: &IssuedGrant,
                   index: usize,
                   action: &str,
                   key: &str| {
        let mut req = request(&rig.projects[index].id, action, key);
        req.expected_revision = Some(rig.projects[index].revision);
        let plan = client.plan(&req).unwrap();
        assert!(plan.is_ok(), "service plan denied: {:?}", plan.error());
        let plan = plan.data().unwrap();
        rig.native.approve_service_plan(&plan.digest).unwrap();
        let reply = client.apply(&req, &plan.digest).unwrap();
        assert!(reply.is_ok(), "service apply denied: {:?}", reply.error());
        assert_eq!(
            rig.complete(grant, reply.data().unwrap()),
            OperationState::Succeeded
        );
    };
    let http = |rig: &mut Rig, index: usize, needle: &str| {
        let snapshot = rig.native.snapshot().unwrap();
        let row = snapshot.snapshot["projects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["project"]["id"] == rig.projects[index].id)
            .unwrap();
        let authority = row["serving"]["http_url"]
            .as_str()
            .unwrap()
            .strip_prefix("http://")
            .unwrap()
            .trim_end_matches('/');
        let port = authority
            .rsplit_once(':')
            .map(|(_, p)| p.parse::<u16>().unwrap())
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
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.contains("200 OK"));
        assert!(response.contains(needle));
        let pid = row["processes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p[0].as_str().is_some_and(|id| id.contains("php-web")))
            .unwrap()[1]["pid"]
            .as_u64()
            .unwrap();
        (pid, row.clone())
    };
    service(&mut rig, &mut sa, &a, 0, "start", "runtime-start-a");
    service(&mut rig, &mut sb, &b, 1, "start", "runtime-start-b");
    let (old_pid, before) = http(&mut rig, 0, "M13_RUNTIME_ALPHA:8.4.27");
    let (other_pid, _) = http(&mut rig, 1, "M13_SERVICE_BRAVO");
    let selection = runtime
        .selection(id("runtime-current"), &id(&rig.projects[0].id))
        .unwrap()
        .data()
        .unwrap()
        .clone();
    let candidate = Catalog::bundled()
        .packages
        .iter()
        .find(|(_, p)| {
            p.kind == "php"
                && p.variant == "cli"
                && p.target == Target::current().unwrap()
                && p.version.starts_with("8.5.12")
                && rig
                    .root
                    .path()
                    .join("runtimes/packages")
                    .join(p.key().unwrap())
                    .join(".devforge-receipt.json")
                    .is_file()
        })
        .unwrap()
        .0
        .clone();
    let make = |key: &str, input: MutationInput| Request {
        api_version: Default::default(),
        request_id: id(key),
        project_id: selection.project_id.clone(),
        expected_revision: Some(selection.revision.get()),
        idempotency_key: Some(id(key)),
        input,
    };
    let mut bad = selection.input.clone();
    bad.node = id("uncached-node");
    let denied = runtime
        .plan(&make("runtime-invalid-live", MutationInput::Configure(bad)))
        .unwrap();
    assert_eq!(denied.error().unwrap().code, ErrorCode::PreconditionFailed);
    assert_eq!(http(&mut rig, 0, "M13_RUNTIME_ALPHA:8.4.27").0, old_pid);
    let mut input = selection.input.clone();
    input.php = id(&candidate);
    let req = make("runtime-select-a", MutationInput::Configure(input));
    let plan = runtime.plan(&req).unwrap();
    assert!(plan.is_ok(), "runtime plan denied: {:?}", plan.error());
    let plan = plan.data().unwrap().clone();
    rig.native.approve_service_plan(&plan.digest).unwrap();
    let applied = runtime.apply(&req, &plan.digest).unwrap();
    assert!(
        applied.is_ok(),
        "runtime apply denied: {:?}",
        applied.error()
    );
    let receipt = applied.data().unwrap().clone();
    assert_eq!(rig.complete(&a, &receipt), OperationState::Succeeded);
    let (new_pid, after) = http(&mut rig, 0, "M13_RUNTIME_ALPHA:8.5.12");
    assert_ne!(new_pid, old_pid);
    assert_eq!(http(&mut rig, 1, "M13_SERVICE_BRAVO").0, other_pid);
    for field in ["name", "composer", "node"] {
        assert_eq!(after["project"][field], before["project"][field]);
    }
    assert_eq!(after["site"], before["site"]);
    assert_eq!(after["development"], before["development"]);
    let replay = runtime.apply(&req, &plan.digest).unwrap();
    assert_eq!(replay.data().unwrap().operation_id, receipt.operation_id);
    assert_eq!(http(&mut rig, 0, "M13_RUNTIME_ALPHA:8.5.12").0, new_pid);
    let lookup = runtime
        .lookup(
            id("runtime-lookup"),
            &req.project_id,
            req.idempotency_key.as_ref().unwrap(),
        )
        .unwrap();
    assert_eq!(
        lookup.data().unwrap().as_ref().unwrap().operation_id,
        receipt.operation_id
    );
    rig.projects[0].revision = after["project"]["revision"].as_u64().unwrap();
    let package = Catalog::bundled()
        .packages
        .iter()
        .find(|(_, p)| {
            p.kind == "caddy"
                && p.target == Target::current().unwrap()
                && p.version.contains("2.11.9")
        })
        .unwrap()
        .0
        .clone();
    let install = Request {
        api_version: Default::default(),
        request_id: id("runtime-install-cached"),
        project_id: id(&rig.projects[0].id),
        expected_revision: Some(rig.projects[0].revision),
        idempotency_key: Some(id("runtime-install-cached")),
        input: MutationInput::RuntimeInstall(PackageInput {
            package_id: id(&package),
            limits: None,
        }),
    };
    let plan = installer.plan(&install).unwrap().data().unwrap().clone();
    rig.native.approve_service_plan(&plan.digest).unwrap();
    let installed = installer.apply(&install, &plan.digest).unwrap();
    assert!(
        installed.is_ok(),
        "cached install denied: {:?}",
        installed.error()
    );
    assert_eq!(
        rig.complete(&pm, installed.data().unwrap()),
        OperationState::Succeeded
    );
    let (pid, installed_view) = http(&mut rig, 0, "M13_RUNTIME_ALPHA:8.5.12");
    assert_eq!(pid, new_pid);
    assert_eq!(installed_view["project"], after["project"]);
    assert_eq!(http(&mut rig, 1, "M13_SERVICE_BRAVO").0, other_pid);
    service(&mut rig, &mut sa, &a, 0, "stop", "runtime-stop-a");
    service(&mut rig, &mut sb, &b, 1, "stop", "runtime-stop-b");
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires an explicitly approved immutable native runtime cache"]
fn env_actual_cached_save_apply_injects_only_manager_secrets_and_replays_truthfully() {
    let source = std::path::PathBuf::from(
        std::env::var_os("DEVFORGE_SERVICE_CACHE").expect("explicit approved cache required"),
    );
    let mut rig = Rig::new(Some(&source));
    let project = rig.projects[0].clone();
    let grant = rig.env_grant(0, true);
    let mut env = rig.environment(&grant);
    let injected = "MANAGER_ONLY_lowentropy_value";
    let native = rig
        .native
        .desktop_access(
            &id("native-env-secret"),
            &devforge_manager::desktop::AccessRequest::ConfigurationSecret {
                project_id: id(&project.id),
                reference: "fixture_secret".into(),
                value: devforge_manager::desktop::SecretValue::new(injected).unwrap(),
            },
        )
        .unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    while !rig
        .native
        .status(&id(&project.id), &native.operation_id)
        .unwrap()
        .state
        .terminal()
    {
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(10));
    }
    std::fs::write(
        project.path.join(".env.example"),
        "APP_ENV=local\nAPP_KEY=\nTEST_PRIVATE=\n",
    )
    .unwrap();
    let mut manifest =
        devforge_projects::manifest::Manifest::parse(environment_yaml(&rig, 0).as_bytes()).unwrap();
    manifest
        .environment
        .insert("TEST_PRIVATE".into(), "fixture_secret".into());
    manifest.setup.create_env = true;
    manifest.setup.generate_key = true;
    let text = format!(
        "# PRIVATE_YAML_COMMENT=lowentropy\n{}",
        manifest.yaml().unwrap()
    );
    let save = env_request(
        &project.id,
        project.revision,
        "actual-env-save",
        yaml_input(None, text.clone()),
    );
    let preview = env.plan(&save).unwrap();
    assert!(
        preview.is_ok(),
        "save preview denied: {:?}",
        preview.error()
    );
    let preview = preview.data().unwrap();
    let public = serde_json::to_string(preview).unwrap();
    assert!(!public.contains("PRIVATE_YAML_COMMENT"));
    assert!(!public.contains(injected));
    let ticket = preview.receipt.as_ref().unwrap().digest.clone();
    rig.native.approve_service_plan(&ticket).unwrap();
    let saved = env.apply(&save, &ticket).unwrap();
    assert!(saved.is_ok(), "save denied: {:?}", saved.error());
    let saved = saved.data().unwrap().clone();
    let save_review = ticket.clone();
    assert_eq!(rig.complete(&grant, &saved), OperationState::Succeeded);
    assert_eq!(
        std::fs::read_to_string(project.path.join("devforge.yaml")).unwrap(),
        text
    );
    let preview_request = env_request(
        &project.id,
        project.revision,
        "actual-env-preview",
        MutationInput::EnvironmentPlan(devforge_control::mutations::EnvironmentPlanInput {
            write_env: true,
            limits: None,
        }),
    );
    let preview = env.plan(&preview_request).unwrap();
    assert!(preview.is_ok());
    let preview = preview.data().unwrap();
    assert!(
        preview.blockers.is_empty(),
        "masked blockers: {:?}",
        preview.blockers
    );
    assert!(
        preview.download_ids.is_empty(),
        "fixture must never download"
    );
    let public = serde_json::to_string(preview).unwrap();
    for forbidden in [
        injected,
        "fixture_secret",
        "PRIVATE_YAML_COMMENT",
        "state_hash",
        "environment_references",
    ] {
        assert!(!public.contains(forbidden));
    }
    let apply = env_request(
        &project.id,
        project.revision,
        "actual-env-apply",
        MutationInput::EnvironmentApply(devforge_control::mutations::EnvironmentApplyInput {
            write_env: true,
            limits: None,
        }),
    );
    let plan = env.plan(&apply).unwrap();
    assert!(plan.is_ok(), "apply preview denied: {:?}", plan.error());
    let ticket = plan
        .data()
        .unwrap()
        .receipt
        .as_ref()
        .unwrap()
        .digest
        .clone();
    rig.native.approve_service_plan(&ticket).unwrap();
    let applied = env.apply(&apply, &ticket).unwrap();
    assert!(applied.is_ok(), "apply denied: {:?}", applied.error());
    let applied = applied.data().unwrap().clone();
    assert_eq!(rig.complete(&grant, &applied), OperationState::Succeeded);
    let output = std::fs::read_to_string(project.path.join(".env")).unwrap();
    assert!(output.contains(injected));
    assert!(output.contains("APP_KEY=base64:"));
    let replay = env.apply(&save, &save_review).unwrap();
    assert_eq!(replay.data().unwrap().operation_id, saved.operation_id);
    // Reconciliation remains independent of body comparison, even after the
    // compound apply changes its own registry revision.
    let lookup = env
        .lookup(
            id("saved-key"),
            &id(&project.id),
            save.idempotency_key.as_ref().unwrap(),
        )
        .unwrap();
    assert_eq!(
        lookup.data().unwrap().as_ref().unwrap().operation_id,
        saved.operation_id
    );
    let journal = std::fs::read(rig.root.path().join("control/operations.sqlite3")).unwrap();
    let journal = String::from_utf8_lossy(&journal);
    assert!(!journal.contains("PRIVATE_YAML_COMMENT"));
    assert!(!journal.contains(injected));
    // Reopen the actual manager, preserving only its existing durable stores.
    // Volatile equality proof is intentionally gone, and no job is replayed.
    rig.native.quit().unwrap();
    rig.worker.take().unwrap().join().unwrap().unwrap();
    let controller = Controller::start(
        rig.root.path(),
        StartIntent::ExplicitUserStart,
        Stores {
            workspace: rig.secrets.clone(),
            desktop: Arc::new(Memory::default()),
        },
    )
    .unwrap();
    let stop = controller.quitter();
    let endpoint = controller.endpoint().clone();
    let worker = thread::spawn(move || controller.serve(None));
    let until = Instant::now() + Duration::from_secs(5);
    while ExternalClient::observe_owner(&endpoint).is_err() {
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(10));
    }
    let mut reopened = EnvClient::connect(
        endpoint,
        ExternalCredential::from_wire(grant.credential.expose_secret()).unwrap(),
    )
    .unwrap();
    let reply = reopened.apply(&save, &ticket).unwrap();
    assert_eq!(
        reply.error().unwrap().code,
        ErrorCode::IdempotencyUnverifiable
    );
    assert_eq!(
        reopened
            .lookup(
                id("reopened-key"),
                &id(&project.id),
                save.idempotency_key.as_ref().unwrap()
            )
            .unwrap()
            .data()
            .unwrap()
            .as_ref()
            .unwrap()
            .operation_id,
        saved.operation_id
    );
    stop.request_explicit().unwrap();
    worker.join().unwrap().unwrap();
}

fn a05_call(
    endpoint: &devforge_manager::Endpoint,
    credential: &ExternalCredential,
    project: &str,
    method: &str,
    input: serde_json::Value,
) -> serde_json::Value {
    Client::new(endpoint.clone())
        .request(&json!({
            "method":method,
            "credential":credential.expose_secret(),
            "request":{"api_version":1,"request_id":"a05-check","project_id":project,
                "expected_revision":null,"idempotency_key":null,"input":input}
        }))
        .unwrap()
}
fn a05_mark(
    endpoint: &devforge_manager::Endpoint,
    grant: &IssuedGrant,
    project: &str,
    stream: &str,
    detail: bool,
    context: Option<&str>,
) -> serde_json::Value {
    a05_call(
        endpoint,
        &grant.credential,
        project,
        "activity_mark",
        json!({
            "stream":stream,"detail":detail,"context_id":context,
        }),
    )
}
fn a05_wait(
    endpoint: &devforge_manager::Endpoint,
    grant: &IssuedGrant,
    project: &str,
    bookmark: &str,
    timeout_ms: u64,
) -> serde_json::Value {
    a05_call(
        endpoint,
        &grant.credential,
        project,
        "activity_wait",
        json!({
            "bookmark":bookmark,"timeout_ms":timeout_ms,
        }),
    )
}
fn a05_store(root: &Path) -> rusqlite::Connection {
    let directory = root.join("inspector");
    std::fs::create_dir_all(&directory).unwrap();
    let db = rusqlite::Connection::open(directory.join("events.sqlite3")).unwrap();
    db.execute_batch("PRAGMA journal_mode=DELETE; CREATE TABLE events(sequence INTEGER PRIMARY KEY AUTOINCREMENT,project TEXT NOT NULL,received INTEGER NOT NULL,body TEXT NOT NULL); PRAGMA user_version=1;").unwrap();
    db
}
fn a05_event(db: &rusqlite::Connection, project: &str, context: &str, kind: &str, message: &str) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let event = if kind == "request" {
        json!({"kind":"request","context":context,"name":"GET /PRIVATE_PATH","duration_ms":1.0,
            "data":{"status":200,"query_count":0,"headers":{"Authorization":"PRIVATE_HEADER_CANARY"}}})
    } else if kind == "query" {
        json!({"kind":"query","context":context,"name":"select * from users where email='PRIVATE_SQL_CANARY'","duration_ms":5.0,
            "data":{"bindings":["PRIVATE_BINDING_CANARY"]}})
    } else {
        json!({"kind":"log","context":context,"name":"error","duration_ms":0.0,
            "data":{"message":message,"token":"PRIVATE_TOKEN_CANARY"}})
    };
    db.execute(
        "INSERT INTO events(project,received,body) VALUES(?1,?2,?3)",
        rusqlite::params![project, now, event.to_string()],
    )
    .unwrap();
}

#[test]
fn a05_mark_wait_scopes_prior_events_and_revalidates_grants() {
    let mut rig = Rig::new(None);
    let project = rig.projects[0].clone();
    let observer = rig
        .native
        .issue_external_grant(
            &id(&project.id),
            &[Scope::Observer],
            Duration::from_secs(600),
        )
        .unwrap();
    let other = rig
        .native
        .issue_external_grant(
            &id(&project.id),
            &[Scope::Observer],
            Duration::from_secs(600),
        )
        .unwrap();
    let context = "c".repeat(32);
    let missing = a05_mark(
        &rig.endpoint,
        &observer,
        &project.id,
        "inspector",
        false,
        Some(&context),
    );
    assert_eq!(missing["error"]["code"], "precondition_failed", "{missing}");
    assert!(
        !rig.root.path().join("inspector").exists(),
        "missing-source mark must not create collector state"
    );
    let denied = a05_mark(
        &rig.endpoint,
        &observer,
        &project.id,
        "inspector",
        true,
        Some(&context),
    );
    assert_eq!(denied["error"]["code"], "unauthorized", "{denied}");
    assert!(
        !rig.root.path().join("inspector").exists(),
        "missing right must deny before source IO"
    );
    let db = a05_store(rig.root.path());
    a05_event(&db, &project.id, &context, "request", "");
    let marked = a05_mark(
        &rig.endpoint,
        &observer,
        &project.id,
        "inspector",
        false,
        Some(&context),
    );
    assert_eq!(marked["ok"], true, "{marked}");
    let bookmark = marked["data"]["bookmark"].as_str().unwrap().to_owned();
    // A 1 ms budget may expire before the first authorized source observation,
    // especially on loaded Windows. It must never invent an event or a
    // lossless result, whether it completes the observation or returns a
    // retryable TimedOut/Busy error.
    let short = a05_wait(&rig.endpoint, &observer, &project.id, &bookmark, 1);
    if short["ok"] == true {
        assert_eq!(short["data"]["timed_out"], true, "{short}");
        assert!(short["data"]["event"].is_null(), "{short}");
        assert_eq!(short["data"]["continuity"], "unverified", "{short}");
    } else {
        assert!(
            matches!(short["error"]["code"].as_str(), Some("timed_out" | "busy")),
            "{short}"
        );
        assert!(short["data"].is_null(), "{short}");
    }
    // Hold the actor before starting the watch: no source observation can
    // complete. A deadline under contention must fail explicitly rather than
    // fabricate a successful empty result. The existing gate waits until the
    // actor has entered SecretStore::set, so no timing guess is needed here.
    let release = rig.block();
    let unobserved = a05_wait(&rig.endpoint, &observer, &project.id, &bookmark, 100);
    assert_eq!(
        unobserved["ok"], false,
        "no source poll completed: {unobserved}"
    );
    assert!(
        matches!(
            unobserved["error"]["code"].as_str(),
            Some("timed_out" | "busy")
        ),
        "{unobserved}"
    );
    assert!(unobserved["data"].is_null(), "{unobserved}");
    drop(release);
    // Once the actor is released, a fresh source read and grant check can
    // establish an observed, explicitly unverified empty timeout.
    let timeout = a05_wait(&rig.endpoint, &observer, &project.id, &bookmark, 1500);
    assert_eq!(timeout["ok"], true, "{timeout}");
    assert_eq!(
        timeout["data"]["timed_out"], true,
        "prior event must never satisfy a new wait"
    );
    assert!(timeout["data"]["event"].is_null());
    assert_eq!(
        timeout["data"]["continuity"], "unverified",
        "timeout cannot assert lossless history"
    );
    // Foreign grant must be rejected before attempting to read the collector.
    // A malformed owned fixture makes any accidental source read observable.
    drop(db);
    let source = rig.root.path().join("inspector/events.sqlite3");
    let held = rig.root.path().join("inspector/held.sqlite3");
    std::fs::rename(&source, &held).unwrap();
    std::fs::write(&source, b"not-a-sqlite-collector").unwrap();
    let wrong = a05_wait(&rig.endpoint, &other, &project.id, &bookmark, 1500);
    assert!(
        matches!(
            wrong["error"]["code"].as_str(),
            Some("unauthorized" | "cursor_expired")
        ),
        "foreign grant must fail before malformed source read: {wrong}"
    );
    assert_eq!(std::fs::read(&source).unwrap(), b"not-a-sqlite-collector");
    std::fs::remove_file(&source).unwrap();
    std::fs::rename(&held, &source).unwrap();
    let db = rusqlite::Connection::open(&source).unwrap();
    let foreign = a05_wait(
        &rig.endpoint,
        &observer,
        &rig.projects[1].id,
        &bookmark,
        1500,
    );
    assert!(
        matches!(
            foreign["error"]["code"].as_str(),
            Some("project_mismatch" | "cursor_expired")
        ),
        "{foreign}"
    );
    let tampered = a05_wait(&rig.endpoint, &observer, &project.id, "watch-forged", 1500);
    assert_eq!(tampered["error"]["code"], "cursor_expired", "{tampered}");
    a05_event(&db, &project.id, &"d".repeat(32), "request", "");
    let no_match = a05_wait(&rig.endpoint, &observer, &project.id, &bookmark, 1500);
    assert_eq!(
        no_match["data"]["timed_out"], true,
        "unmatched contexts must not satisfy wait: {no_match}"
    );
    let endpoint = rig.endpoint.clone();
    let thread_project = project.id.clone();
    let thread_bookmark = bookmark.clone();
    let token = observer.credential.expose_secret().to_owned();
    let waiter = thread::spawn(move || {
        let credential = ExternalCredential::from_wire(&token).unwrap();
        a05_call(
            &endpoint,
            &credential,
            &thread_project,
            "activity_wait",
            json!({"bookmark":thread_bookmark,"timeout_ms":3000}),
        )
    });
    thread::sleep(Duration::from_millis(100));
    let started = Instant::now();
    let mut diagnostics = rig.diagnostics(&other);
    let health_request = Request {
        api_version: Default::default(),
        request_id: id("a05-health"),
        project_id: id(&project.id),
        expected_revision: None,
        idempotency_key: None,
        input: devforge_control::tools::ToolInput::Health(devforge_control::tools::HealthInput {
            page: None,
        }),
    };
    loop {
        match diagnostics.read(&health_request) {
            Ok(health) if health.is_ok() => break,
            Ok(health)
                if health.error().is_some_and(|e| e.code == ErrorCode::Busy)
                    && started.elapsed() < Duration::from_secs(1) =>
            {
                thread::sleep(Duration::from_millis(10))
            }
            Err(ErrorCode::Busy) if started.elapsed() < Duration::from_secs(1) => {
                thread::sleep(Duration::from_millis(10))
            }
            other => panic!("unrelated actor read must remain responsive during wait: {other:?}"),
        }
    }
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "wait must not occupy the actor"
    );
    a05_event(&db, &project.id, &context, "request", "");
    let result = waiter.join().unwrap();
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["data"]["timed_out"], false, "{result}");
    assert_eq!(
        result["data"]["continuity"], "unverified",
        "observed event does not prove lossless history"
    );
    let advanced = result["data"]["bookmark"].as_str().unwrap().to_owned();
    assert_ne!(
        advanced, bookmark,
        "a delivered event must advance the bookmark"
    );
    let no_replay = a05_wait(&rig.endpoint, &observer, &project.id, &advanced, 1500);
    assert_eq!(
        no_replay["data"]["timed_out"], true,
        "advanced bookmark must not replay the event: {no_replay}"
    );
    let text = serde_json::to_string(&result).unwrap();
    assert!(text.contains(&context), "matched context missing: {text}");
    for forbidden in [
        "PRIVATE_PATH",
        "PRIVATE_HEADER_CANARY",
        "PRIVATE_TOKEN_CANARY",
    ] {
        assert!(!text.contains(forbidden));
    }
    let detail_grant = rig
        .native
        .issue_external_grant_with_information(
            &id(&project.id),
            &[Scope::Observer],
            &[devforge_control::InformationScope::InspectorQueries],
            Duration::from_secs(600),
        )
        .unwrap();
    let detail_mark = a05_mark(
        &rig.endpoint,
        &detail_grant,
        &project.id,
        "inspector",
        true,
        Some(&context),
    );
    assert_eq!(detail_mark["ok"], true, "{detail_mark}");
    let detail_bookmark = detail_mark["data"]["bookmark"].as_str().unwrap().to_owned();
    a05_event(&db, &project.id, &context, "query", "");
    a05_event(&db, &project.id, &context, "request", "");
    let detail = a05_wait(
        &rig.endpoint,
        &detail_grant,
        &project.id,
        &detail_bookmark,
        1500,
    );
    assert_eq!(detail["ok"], true, "{detail}");
    assert_eq!(detail["data"]["timed_out"], false, "{detail}");
    assert_eq!(
        detail["data"]["event"]["data"]["detail"]["kind"], "query",
        "first buffered event must not be skipped: {detail}"
    );
    let after_first = detail["data"]["bookmark"].as_str().unwrap();
    let second = a05_wait(&rig.endpoint, &detail_grant, &project.id, after_first, 1500);
    assert_eq!(
        second["data"]["event"]["data"]["detail"]["kind"], "request",
        "second buffered event must be delivered next: {second}"
    );
    let detail_text = serde_json::to_string(&detail).unwrap();
    assert!(detail_text.contains("query"));
    assert!(
        !detail_text.contains("PRIVATE_SQL_CANARY")
            && !detail_text.contains("PRIVATE_BINDING_CANARY")
    );
    let next = a05_mark(
        &rig.endpoint,
        &observer,
        &project.id,
        "inspector",
        false,
        None,
    );
    assert_eq!(next["ok"], true, "{next}");
    let next = next["data"]["bookmark"].as_str().unwrap().to_owned();
    // Clearing the same SQLite file must not be mistaken for a normal timeout.
    db.execute("DELETE FROM events WHERE project=?1", [project.id.as_str()])
        .unwrap();
    let cleared = a05_wait(&rig.endpoint, &observer, &project.id, &next, 1500);
    assert_eq!(
        cleared["error"]["code"], "cursor_expired",
        "same-file rewind needs resync: {cleared}"
    );
    a05_event(&db, &project.id, &context, "request", "");
    let next = a05_mark(
        &rig.endpoint,
        &observer,
        &project.id,
        "inspector",
        false,
        None,
    );
    assert_eq!(next["ok"], true, "{next}");
    let next = next["data"]["bookmark"].as_str().unwrap().to_owned();
    drop(db);
    let source = rig.root.path().join("inspector/events.sqlite3");
    std::fs::rename(&source, rig.root.path().join("inspector/old.sqlite3")).unwrap();
    let db = rusqlite::Connection::open(&source).unwrap();
    db.execute_batch("CREATE TABLE events(sequence INTEGER PRIMARY KEY AUTOINCREMENT,project TEXT NOT NULL,received INTEGER NOT NULL,body TEXT NOT NULL); PRAGMA user_version=1;").unwrap();
    drop(db);
    let replaced = a05_wait(&rig.endpoint, &observer, &project.id, &next, 1500);
    assert_eq!(
        replaced["error"]["code"], "cursor_expired",
        "source replacement must invalidate marks: {replaced}"
    );
    rig.native
        .revoke_external_grant(&observer.record.grant_id)
        .unwrap();
    let revoked = a05_wait(&rig.endpoint, &observer, &project.id, &bookmark, 1500);
    assert_eq!(revoked["error"]["code"], "grant_revoked", "{revoked}");
}

#[test]
fn a05_log_wait_separates_stream_context_and_reports_retention_loss() {
    let mut rig = Rig::new(None);
    let project = rig.projects[0].clone();
    let grant = rig
        .native
        .issue_external_grant(
            &id(&project.id),
            &[Scope::Observer],
            Duration::from_secs(600),
        )
        .unwrap();
    let context = "c".repeat(32);
    let db = a05_store(rig.root.path());
    a05_event(&db, &project.id, &context, "log", "previous log");
    let mark = a05_mark(
        &rig.endpoint,
        &grant,
        &project.id,
        "logs",
        false,
        Some(&context),
    );
    assert_eq!(mark["ok"], true, "{mark}");
    let bookmark = mark["data"]["bookmark"].as_str().unwrap().to_owned();
    a05_event(&db, &project.id, &context, "request", "");
    a05_event(&db, &project.id, &"d".repeat(32), "log", "wrong context");
    let old = a05_wait(&rig.endpoint, &grant, &project.id, &bookmark, 1500);
    assert_eq!(
        old["data"]["timed_out"], true,
        "unrelated stream/context must not match: {old}"
    );
    a05_event(&db, &project.id, &context, "log", "latest private message");
    let result = a05_wait(&rig.endpoint, &grant, &project.id, &bookmark, 1500);
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["data"]["timed_out"], false, "{result}");
    let advanced = result["data"]["bookmark"].as_str().unwrap().to_owned();
    assert_ne!(advanced, bookmark);
    let no_replay = a05_wait(&rig.endpoint, &grant, &project.id, &advanced, 1500);
    assert_eq!(
        no_replay["data"]["timed_out"], true,
        "advanced log bookmark must not replay: {no_replay}"
    );
    let text = serde_json::to_string(&result).unwrap();
    assert!(text.contains("logs") && text.contains(&context), "{text}");
    assert!(!text.contains("latest private message") && !text.contains("PRIVATE_TOKEN_CANARY"));
    // Content is independently granted and reuses the A03 redacted projection.
    let content_grant = rig
        .native
        .issue_external_grant_with_information(
            &id(&project.id),
            &[Scope::Observer],
            &[devforge_control::InformationScope::LogContent],
            Duration::from_secs(600),
        )
        .unwrap();
    let content_mark = a05_mark(
        &rig.endpoint,
        &content_grant,
        &project.id,
        "logs",
        true,
        Some(&context),
    );
    assert_eq!(content_mark["ok"], true, "{content_mark}");
    let content_bookmark = content_mark["data"]["bookmark"].as_str().unwrap();
    a05_event(
        &db,
        &project.id,
        &context,
        "log",
        "Database failed token=PRIVATE_LOG_SECRET_CANARY",
    );
    let content = a05_wait(
        &rig.endpoint,
        &content_grant,
        &project.id,
        content_bookmark,
        1500,
    );
    assert_eq!(content["ok"], true, "{content}");
    assert_eq!(content["data"]["timed_out"], false, "{content}");
    let text = serde_json::to_string(&content).unwrap();
    assert!(text.contains("Database failed"));
    assert!(!text.contains("PRIVATE_LOG_SECRET_CANARY") && !text.contains("PRIVATE_TOKEN_CANARY"));
    // Inspect retention loss: more new events than the bounded source window.
    let mark = a05_mark(&rig.endpoint, &grant, &project.id, "inspector", false, None);
    assert_eq!(mark["ok"], true, "{mark}");
    let checkpoint = mark["data"]["bookmark"].as_str().unwrap().to_owned();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    for _ in 0..201 {
        a05_event(&db, &project.id, &context, "request", "");
    }
    db.execute_batch("COMMIT").unwrap();
    let lost = a05_wait(&rig.endpoint, &grant, &project.id, &checkpoint, 3000);
    assert_eq!(
        lost["error"]["code"], "output_limit_exceeded",
        "retention loss cannot be called timeout or complete: {lost}"
    );
    // A single new oversized source row can hide data too; report loss, not timeout.
    let mark = a05_mark(&rig.endpoint, &grant, &project.id, "inspector", false, None);
    assert_eq!(mark["ok"], true, "{mark}");
    let checkpoint = mark["data"]["bookmark"].as_str().unwrap().to_owned();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let oversized = json!({"kind":"request","context":context,"name":"GET /","duration_ms":1.0,
        "data":{"status":200,"query_count":0,"body":"x".repeat(270_000)}});
    db.execute(
        "INSERT INTO events(project,received,body) VALUES(?1,?2,?3)",
        rusqlite::params![project.id, now, oversized.to_string()],
    )
    .unwrap();
    let lost = a05_wait(&rig.endpoint, &grant, &project.id, &checkpoint, 3000);
    assert_eq!(
        lost["error"]["code"], "output_limit_exceeded",
        "byte-limit loss must be explicit: {lost}"
    );
}

#[test]
fn a05_cross_stream_correlation_narrows_to_new_inspector_context() {
    let mut rig = Rig::new(None);
    let project = rig.projects[0].clone();
    let grant = rig
        .native
        .issue_external_grant(
            &id(&project.id),
            &[Scope::Observer],
            Duration::from_secs(600),
        )
        .unwrap();
    let context = "c".repeat(32);
    let foreign = "d".repeat(32);
    let db = a05_store(rig.root.path());
    a05_event(&db, &project.id, &"e".repeat(32), "request", "");
    let inspector = a05_mark(&rig.endpoint, &grant, &project.id, "inspector", false, None);
    let logs = a05_mark(&rig.endpoint, &grant, &project.id, "logs", false, None);
    assert_eq!(inspector["ok"], true, "{inspector}");
    assert_eq!(logs["ok"], true, "{logs}");
    let inspector_mark = inspector["data"]["bookmark"].as_str().unwrap();
    let log_mark = logs["data"]["bookmark"].as_str().unwrap();
    // These events arise after both marks; the foreign context must not be correlated.
    a05_event(&db, &project.id, &context, "request", "");
    a05_event(
        &db,
        &project.id,
        &foreign,
        "log",
        "unrelated private content",
    );
    let discovered = a05_wait(&rig.endpoint, &grant, &project.id, inspector_mark, 1500);
    assert_eq!(discovered["ok"], true, "{discovered}");
    let discovered_context = discovered["data"]["event"]["data"]["context_id"]
        .as_str()
        .unwrap();
    assert_eq!(discovered_context, context);
    let restricted = a05_call(
        &rig.endpoint,
        &grant.credential,
        &project.id,
        "activity_wait",
        json!({"bookmark":log_mark,"timeout_ms":1500,"context_id":discovered_context}),
    );
    assert_eq!(restricted["ok"], true, "{restricted}");
    assert_eq!(
        restricted["data"]["timed_out"], true,
        "foreign log must not match narrowed wait: {restricted}"
    );
    let narrowed = restricted["data"]["bookmark"].as_str().unwrap().to_owned();
    assert_ne!(
        narrowed, log_mark,
        "narrowing must produce an independently bound bookmark"
    );
    a05_event(
        &db,
        &project.id,
        &context,
        "log",
        "matching private content",
    );
    let matched = a05_wait(&rig.endpoint, &grant, &project.id, &narrowed, 1500);
    assert_eq!(matched["ok"], true, "{matched}");
    assert_eq!(matched["data"]["timed_out"], false, "{matched}");
    assert_eq!(matched["data"]["event"]["stream"], "logs");
    assert_eq!(matched["data"]["event"]["context_id"], context);
    let text = serde_json::to_string(&matched).unwrap();
    assert!(!text.contains("private content") && !text.contains("PRIVATE_TOKEN_CANARY"));
    let advanced = matched["data"]["bookmark"].as_str().unwrap();
    let switched = a05_call(
        &rig.endpoint,
        &grant.credential,
        &project.id,
        "activity_wait",
        json!({"bookmark":advanced,"timeout_ms":1500,"context_id":foreign}),
    );
    assert_eq!(
        switched["error"]["code"], "cursor_expired",
        "a narrowed bookmark cannot switch contexts: {switched}"
    );
}

#[test]
fn a05_shutdown_interrupts_wait_without_blocking_or_inventing_an_event() {
    let mut rig = Rig::new(None);
    let project = rig.projects[0].clone();
    let grant = rig
        .native
        .issue_external_grant(
            &id(&project.id),
            &[Scope::Observer],
            Duration::from_secs(600),
        )
        .unwrap();
    let db = a05_store(rig.root.path());
    a05_event(&db, &project.id, &"c".repeat(32), "request", "");
    drop(db);
    let mark = a05_mark(&rig.endpoint, &grant, &project.id, "inspector", false, None);
    assert_eq!(mark["ok"], true, "{mark}");
    let bookmark = mark["data"]["bookmark"].as_str().unwrap().to_owned();
    let endpoint = rig.endpoint.clone();
    let project_id = project.id.clone();
    let token = grant.credential.expose_secret().to_owned();
    let waiter = thread::spawn(move || {
        Client::new(endpoint).request(&json!({
            "method":"activity_wait", "credential":token,
            "request":{"api_version":1,"request_id":"a05-shutdown","project_id":project_id,
                "expected_revision":null,"idempotency_key":null,
                "input":{"bookmark":bookmark,"timeout_ms":3000}}
        }))
    });
    thread::sleep(Duration::from_millis(100));
    let started = Instant::now();
    rig.native.quit().unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "shutdown must not wait for the full watch deadline"
    );
    if let Ok(response) = waiter.join().unwrap() {
        assert_eq!(
            response["ok"], false,
            "shutdown must not invent a successful event or timeout: {response}"
        );
        assert!(response["data"].is_null());
    }
}

#[test]
fn a05_sparse_other_project_sequences_and_same_file_clear_are_distinguished() {
    let mut rig = Rig::new(None);
    let project = rig.projects[0].clone();
    let other = rig.projects[1].clone();
    let grant = rig
        .native
        .issue_external_grant(
            &id(&project.id),
            &[Scope::Observer],
            Duration::from_secs(600),
        )
        .unwrap();
    let context = "c".repeat(32);
    let db = a05_store(rig.root.path());
    // Source exists because of B, but project A has no retained baseline.
    a05_event(&db, &other.id, &context, "request", "");
    let zero = a05_mark(&rig.endpoint, &grant, &project.id, "inspector", false, None);
    assert_eq!(zero["ok"], true, "{zero}");
    let zero = zero["data"]["bookmark"].as_str().unwrap().to_owned();
    a05_event(&db, &project.id, &context, "request", "");
    db.execute("DELETE FROM events WHERE project=?1", [project.id.as_str()])
        .unwrap();
    let transient = a05_wait(&rig.endpoint, &grant, &project.id, &zero, 1500);
    assert_eq!(transient["ok"], true, "{transient}");
    assert_eq!(transient["data"]["timed_out"], true, "{transient}");
    assert_eq!(
        transient["data"]["continuity"], "unverified",
        "insert-delete between polls cannot be claimed lossless"
    );
    a05_event(&db, &project.id, &context, "request", "");
    let mark = a05_mark(&rig.endpoint, &grant, &project.id, "inspector", false, None);
    assert_eq!(mark["ok"], true, "{mark}");
    let bookmark = mark["data"]["bookmark"].as_str().unwrap().to_owned();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    for _ in 0..12 {
        a05_event(&db, &other.id, &context, "request", "");
    }
    for _ in 0..200 {
        a05_event(&db, &project.id, &context, "request", "");
    }
    db.execute_batch("COMMIT").unwrap();
    let first = a05_wait(&rig.endpoint, &grant, &project.id, &bookmark, 3000);
    assert_eq!(
        first["ok"], true,
        "other-project global sequence gaps are not lost project events: {first}"
    );
    assert_eq!(first["data"]["timed_out"], false, "{first}");
    let anchor = a05_mark(&rig.endpoint, &grant, &project.id, "inspector", false, None);
    assert_eq!(anchor["ok"], true, "{anchor}");
    let anchor = anchor["data"]["bookmark"].as_str().unwrap().to_owned();
    // Preserve file identity but remove the marked anchor and immediately append.
    // A larger new sequence must not make a clear-and-repopulate look continuous.
    db.execute("DELETE FROM events WHERE project=?1", [project.id.as_str()])
        .unwrap();
    a05_event(&db, &project.id, &context, "request", "");
    let cleared = a05_wait(&rig.endpoint, &grant, &project.id, &anchor, 1500);
    assert_eq!(
        cleared["error"]["code"], "cursor_expired",
        "missing anchor requires resync even after new events: {cleared}"
    );
    // The collector's real retention is global; another project can evict A's
    // baseline even without an A write. A must then resync, not time out.
    let global = a05_mark(&rig.endpoint, &grant, &project.id, "inspector", false, None);
    assert_eq!(global["ok"], true, "{global}");
    let global = global["data"]["bookmark"].as_str().unwrap().to_owned();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    for _ in 0..4001 {
        a05_event(&db, &other.id, &context, "request", "");
    }
    db.execute("DELETE FROM events WHERE sequence NOT IN (SELECT sequence FROM events ORDER BY sequence DESC LIMIT 4000)", []).unwrap();
    db.execute_batch("COMMIT").unwrap();
    let pruned = a05_wait(&rig.endpoint, &grant, &project.id, &global, 1500);
    assert_eq!(
        pruned["error"]["code"], "cursor_expired",
        "globally pruned anchor must resync: {pruned}"
    );
}
