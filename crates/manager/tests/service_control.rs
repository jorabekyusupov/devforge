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
    get_count: std::sync::atomic::AtomicUsize,
    gate: Mutex<(bool, bool)>,
    ready: Condvar,
}
impl SecretStore for Memory {
    fn get(&self, k: &str) -> Result<Option<String>, CoreError> {
        self.get_count
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
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
    native_credential: devforge_manager::auth::DesktopCredential,
    endpoint: devforge_manager::Endpoint,
    worker: Option<thread::JoinHandle<devforge_manager::controller::Result<()>>>,
    projects: Vec<devforge_projects::project::Project>,
    secrets: Arc<Memory>,
    blocks: u64,
    #[cfg(feature = "test-support")]
    debug_actor: devforge_manager::actor::ActorHandle,
}
impl Rig {
    fn new(cache: Option<&Path>) -> Self {
        Self::setup(cache, false)
    }
    fn setup(cache: Option<&Path>, legacy: bool) -> Self {
        Self::runtime_setup(cache, legacy, false)
    }
    fn runtime_setup(cache: Option<&Path>, legacy: bool, alternative: bool) -> Self {
        Self::runtime_setup_with(cache, legacy, alternative, |_, _| {})
    }
    fn runtime_setup_with<F>(cache: Option<&Path>, legacy: bool, alternative: bool, seed: F) -> Self
    where
        F: FnOnce(&Path, &mut Vec<devforge_projects::project::Project>),
    {
        Self::runtime_setup_with_policy(cache, legacy, alternative, Policy::default(), seed)
    }
    fn runtime_setup_with_policy<F>(
        cache: Option<&Path>,
        legacy: bool,
        alternative: bool,
        policy: Policy,
        seed: F,
    ) -> Self
    where
        F: FnOnce(&Path, &mut Vec<devforge_projects::project::Project>),
    {
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
        seed(root.path(), &mut projects);
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
        let controller = Controller::with_options(
            root.path(),
            StartIntent::AutoStart,
            Stores {
                workspace: secrets.clone(),
                desktop: control,
            },
            devforge_manager::controller::Options {
                auth: policy,
                ..Default::default()
            },
        )
        .unwrap();
        let endpoint = controller.endpoint().clone();
        #[cfg(feature = "test-support")]
        let debug_actor = controller.test_actor_handle();
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
            native_credential: credential,
            endpoint,
            worker: Some(worker),
            projects,
            secrets,
            blocks: 0,
            #[cfg(feature = "test-support")]
            debug_actor,
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
            sender: None,
            since: None,
            detail: None,
            mail_id: None,
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
    // An A credential must not reveal whether an unrelated project ID is
    // registered, and an invalid credential must fail before any source IO.
    for foreign_project in [&rig.projects[1].id, &"f".repeat(32)] {
        let denied = a05_mark(
            &rig.endpoint,
            &observer,
            foreign_project,
            "inspector",
            false,
            Some(&context),
        );
        assert_eq!(
            denied["error"]["code"], "project_mismatch",
            "unrelated project identity must not be disclosed: {denied}"
        );
    }
    let invalid = ExternalCredential::from_wire(&"0".repeat(64)).unwrap();
    let denied = a05_call(
        &rig.endpoint,
        &invalid,
        &project.id,
        "activity_mark",
        json!({
            "stream":"inspector", "detail":false, "context_id":context,
        }),
    );
    assert_eq!(denied["error"]["code"], "unauthorized", "{denied}");
    let expired = rig
        .native
        .issue_external_grant(
            &id(&project.id),
            &[Scope::Observer],
            Duration::from_millis(10),
        )
        .unwrap();
    thread::sleep(Duration::from_millis(30));
    let expired_result = a05_mark(
        &rig.endpoint,
        &expired,
        &project.id,
        "inspector",
        false,
        Some(&context),
    );
    assert_eq!(
        expired_result["error"]["code"], "grant_expired",
        "{expired_result}"
    );
    let revoked_grant = rig
        .native
        .issue_external_grant(
            &id(&project.id),
            &[Scope::Observer],
            Duration::from_secs(600),
        )
        .unwrap();
    rig.native
        .revoke_external_grant(&revoked_grant.record.grant_id)
        .unwrap();
    let revoked_result = a05_mark(
        &rig.endpoint,
        &revoked_grant,
        &project.id,
        "inspector",
        false,
        Some(&context),
    );
    assert_eq!(
        revoked_result["error"]["code"], "grant_revoked",
        "{revoked_result}"
    );
    assert!(
        !rig.root.path().join("inspector").exists(),
        "denials must occur before source IO"
    );
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

// A06 uses disposable mailbox SQLite only; it never sends mail or opens a real link.
fn a06_mailbox(root: &Path, project: &str) -> rusqlite::Connection {
    let dir = root.join("mail").join(project);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(".devforge-inbox"), b"devforge-inbox-v1\n").unwrap();
    let db = rusqlite::Connection::open(dir.join("inbox.sqlite3")).unwrap();
    db.execute_batch("PRAGMA journal_mode=DELETE; CREATE TABLE messages(sequence INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT UNIQUE NOT NULL,received INTEGER NOT NULL,summary TEXT NOT NULL,search TEXT NOT NULL,raw BLOB NOT NULL); PRAGMA user_version=1;").unwrap();
    db
}
fn a06_insert_mail(
    db: &rusqlite::Connection,
    message_id: &str,
    received: i64,
    sender: &str,
    recipient: &str,
    subject: &str,
    token: &str,
) {
    let raw = format!(
        "From: {sender}\r\nTo: {recipient}\r\nSubject: {subject}\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=fixture\r\n\r\n--fixture\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\nHello https://example.test/reset?token={token}\r\n--fixture\r\nContent-Type: text/plain; name=report.txt\r\nContent-Disposition: attachment; filename=report.txt\r\nContent-Transfer-Encoding: base64\r\n\r\nQVRUQUNITUVOVF9QUklWQVRFX0NBTkFSWQ==\r\n--fixture--\r\n"
    );
    let summary = json!({"id":message_id,"received_ms":received,"subject":subject,"from":sender,"to":recipient,"bytes":raw.len(),"attachments":1});
    db.execute(
        "INSERT INTO messages(id,received,summary,search,raw) VALUES(?1,?2,?3,?4,?5)",
        rusqlite::params![
            message_id,
            received,
            summary.to_string(),
            format!("{sender} {recipient} {subject}").to_lowercase(),
            raw.as_bytes()
        ],
    )
    .unwrap();
}
fn a06_read(
    endpoint: &devforge_manager::Endpoint,
    grant: &IssuedGrant,
    project: &str,
    arguments: serde_json::Value,
) -> serde_json::Value {
    a05_call(
        endpoint,
        &grant.credential,
        project,
        "diagnostics_read",
        json!({"tool":"mail.list","arguments":arguments}),
    )
}

#[test]
fn a06_mail_detail_filters_are_project_and_grant_scoped() {
    use devforge_control::InformationScope;
    use devforge_control::tools::{ActivityMarkInput, MailContentDetail, MailInput};
    use sha2::{Digest as _, Sha256};
    let input: MailInput = serde_json::from_value(json!({"subject":"PRIVATE_FILTER_CANARY","recipient":"PRIVATE_RECIPIENT_CANARY","sender":"PRIVATE_SENDER_CANARY","detail":true})).unwrap();
    let mark: ActivityMarkInput = serde_json::from_value(json!({"stream":"mail","detail":true,"subject":"PRIVATE_FILTER_CANARY","recipient":"PRIVATE_RECIPIENT_CANARY","sender":"PRIVATE_SENDER_CANARY"})).unwrap();
    assert!(
        !format!("{input:?} {mark:?}").contains("PRIVATE_"),
        "sensitive filters must be absent from Debug"
    );
    let mut rig = Rig::new(None);
    let project = rig.projects[0].clone();
    let other_project = rig.projects[1].clone();
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
            &[InformationScope::MailContent],
            Duration::from_secs(600),
        )
        .unwrap();
    let second = rig
        .native
        .issue_external_grant_with_information(
            &id(&project.id),
            &[Scope::Observer],
            &[InformationScope::MailContent],
            Duration::from_secs(600),
        )
        .unwrap();
    for args in [
        json!({"detail":true}),
        json!({"subject":""}),
        json!({"recipient":""}),
        json!({"sender":""}),
        json!({"mail_id":"a".repeat(32)}),
    ] {
        let denied = a06_read(&rig.endpoint, &observer, &project.id, args);
        assert_eq!(denied["error"]["code"], "unauthorized", "{denied}");
        assert!(
            !rig.root.path().join("mail").exists(),
            "unauthorized read must not create a mailbox"
        );
    }
    let denied = a06_read(
        &rig.endpoint,
        &observer,
        &project.id,
        json!({"subject":"PRIVATE_FILTER_CANARY"}),
    );
    assert_eq!(denied["error"]["code"], "unauthorized", "{denied}");
    assert!(
        !serde_json::to_string(&denied)
            .unwrap()
            .contains("PRIVATE_FILTER_CANARY")
    );
    let db = a06_mailbox(rig.root.path(), &project.id);
    let foreign_db = a06_mailbox(rig.root.path(), &other_project.id);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let first_id = "a".repeat(32);
    let second_id = "b".repeat(32);
    let foreign_id = "c".repeat(32);
    a06_insert_mail(
        &db,
        &first_id,
        now,
        "sender@example.test",
        "alpha@example.test",
        "Reset account",
        "ALPHA_TOKEN_CANARY",
    );
    a06_insert_mail(
        &db,
        &second_id,
        now + 1,
        "sender@example.test",
        "alpha@example.test",
        "Reset account",
        "SECOND_TOKEN_CANARY",
    );
    // Real reset emails often carry their signed link only in an HTML href.
    let html_id = "d".repeat(32);
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let local_url = format!(
        "http://127.0.0.1:{}/never-fetch?token=LOCAL_TOKEN_CANARY",
        listener.local_addr().unwrap().port()
    );
    let html_raw = format!(
        "From: sender@example.test\r\nTo: alpha@example.test\r\nSubject: HTML verify\r\nMIME-Version: 1.0\r\nContent-Type: text/html; charset=UTF-8\r\n\r\n<a href=\"https://example.test/verify?expires=123&amp;signature=HTML_SIGNATURE_CANARY\">Verify</a><a href=\"{local_url}\">Never fetch</a>"
    );
    let html_summary = json!({"id":html_id,"received_ms":now,"subject":"HTML verify","from":"sender@example.test","to":"alpha@example.test","bytes":html_raw.len(),"attachments":0});
    db.execute(
        "INSERT INTO messages(id,received,summary,search,raw) VALUES(?1,?2,?3,?4,?5)",
        rusqlite::params![
            html_id,
            now,
            html_summary.to_string(),
            "html verify",
            html_raw.as_bytes()
        ],
    )
    .unwrap();
    a06_insert_mail(
        &foreign_db,
        &foreign_id,
        now + 2,
        "foreign@example.test",
        "bravo@example.test",
        "Reset account",
        "FOREIGN_TOKEN_CANARY",
    );
    let metadata = a06_read(&rig.endpoint, &observer, &project.id, json!({}));
    assert_eq!(metadata["ok"], true, "{metadata}");
    let public = serde_json::to_string(&metadata).unwrap();
    for private in [
        "sender@example.test",
        "alpha@example.test",
        "Reset account",
        "ALPHA_TOKEN_CANARY",
        "SECOND_TOKEN_CANARY",
        "HTML_SIGNATURE_CANARY",
        "HTML verify",
        "report.txt",
        "ATTACHMENT_PRIVATE_CANARY",
    ] {
        assert!(!public.contains(private), "Observer leaked {private}");
    }
    let since_metadata = a06_read(
        &rig.endpoint,
        &observer,
        &project.id,
        json!({"since":now+2}),
    );
    assert_eq!(since_metadata["ok"], true, "{since_metadata}");
    assert_eq!(
        since_metadata["data"]["data"]["items"]
            .as_array()
            .unwrap()
            .len(),
        0,
        "since alone must remain Observer-safe"
    );
    let page = json!({"limit":1,"max_bytes":65536,"timeout_ms":5000});
    let first = a06_read(
        &rig.endpoint,
        &grant,
        &project.id,
        json!({"detail":true,"subject":"reset","recipient":"alpha@","sender":"sender@","page":page}),
    );
    assert_eq!(first["ok"], true, "{first}");
    let serialized = serde_json::to_string(&first).unwrap();
    assert!(
        !serialized.contains("FOREIGN_TOKEN_CANARY")
            && !serialized.contains("ATTACHMENT_PRIVATE_CANARY")
            && !serialized.contains("QVRUQUNITUVOVF9QUklWQVRFX0NBTkFSWQ==")
    );
    let items = first["data"]["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    let cursor = first["data"]["data"]["next_cursor"].clone();
    assert!(!cursor.is_null(), "{first}");
    let cursor_text = serde_json::to_string(&cursor).unwrap();
    for private in ["reset", "alpha@", "sender@", "ALPHA_TOKEN_CANARY"] {
        assert!(
            !cursor_text.contains(private),
            "cursor leaked private filter or token"
        );
    }
    let canonical = serde_json::to_vec(&(
        "mail.list",
        Some("reset"),
        Some("alpha@"),
        Some("sender@"),
        Option::<u64>::None,
        Some(true),
        Option::<String>::None,
    ))
    .unwrap();
    let guessable_hash: String = Sha256::digest(canonical)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_ne!(
        cursor["context"]["filter"].as_str(),
        Some(guessable_hash.as_str()),
        "public cursor must not expose deterministic filter hash"
    );
    let fresh = a06_read(
        &rig.endpoint,
        &grant,
        &project.id,
        json!({"detail":true,"subject":"reset","recipient":"alpha@","sender":"sender@","page":{"limit":1,"max_bytes":65536,"timeout_ms":5000}}),
    );
    assert_eq!(fresh["ok"], true, "{fresh}");
    assert_ne!(
        cursor["context"]["filter"], fresh["data"]["data"]["next_cursor"]["context"]["filter"],
        "filter binding must be opaque per window"
    );
    let wrong = a06_read(
        &rig.endpoint,
        &second,
        &project.id,
        json!({"detail":true,"subject":"reset","recipient":"alpha@","sender":"sender@","page":{"limit":1,"max_bytes":65536,"timeout_ms":5000,"cursor":cursor}}),
    );
    assert_eq!(wrong["error"]["code"], "cursor_expired", "{wrong}");
    assert!(
        !serde_json::to_string(&wrong)
            .unwrap()
            .contains("ALPHA_TOKEN_CANARY")
    );
    let changed = a06_read(
        &rig.endpoint,
        &grant,
        &project.id,
        json!({"detail":true,"subject":"different","recipient":"alpha@","sender":"sender@","page":{"limit":1,"max_bytes":65536,"timeout_ms":5000,"cursor":cursor}}),
    );
    assert_eq!(changed["error"]["code"], "cursor_expired", "{changed}");
    // The cursor represents the original frozen observation, not live state.
    // A message deleted after page 1 may still appear until the window expires,
    // and the response must identify that fact and preserve observation time.
    let observed = first["data"]["data"]["freshness"]["observed_at"].clone();
    db.execute("DELETE FROM messages WHERE id=?1", [&first_id])
        .unwrap();
    let cached = a06_read(
        &rig.endpoint,
        &grant,
        &project.id,
        json!({"detail":true,"subject":"reset","recipient":"alpha@","sender":"sender@","page":{"limit":1,"max_bytes":65536,"timeout_ms":5000,"cursor":cursor}}),
    );
    assert_eq!(cached["ok"], true, "{cached}");
    assert_eq!(
        cached["data"]["data"]["freshness"]["origin"], "cached",
        "{cached}"
    );
    assert_eq!(
        cached["data"]["data"]["freshness"]["observed_at"], observed,
        "{cached}"
    );
    assert_eq!(
        cached["data"]["data"]["items"][0]["mail_id"], first_id,
        "{cached}"
    );
    // Restore this disposable synthetic mail for subsequent exact-ID tests.
    a06_insert_mail(
        &db,
        &first_id,
        now,
        "sender@example.test",
        "alpha@example.test",
        "Reset account",
        "ALPHA_TOKEN_CANARY",
    );
    let foreign = a06_read(
        &rig.endpoint,
        &grant,
        &other_project.id,
        json!({"detail":true,"mail_id":foreign_id}),
    );
    assert!(
        matches!(
            foreign["error"]["code"].as_str(),
            Some("project_mismatch" | "unauthorized")
        ),
        "{foreign}"
    );
    assert!(
        !serde_json::to_string(&foreign)
            .unwrap()
            .contains("FOREIGN_TOKEN_CANARY")
    );
    let foreign_id_in_own_project = a06_read(
        &rig.endpoint,
        &grant,
        &project.id,
        json!({"detail":true,"mail_id":foreign_id}),
    );
    assert_eq!(
        foreign_id_in_own_project["ok"], true,
        "{foreign_id_in_own_project}"
    );
    assert!(
        foreign_id_in_own_project["data"]["data"]["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        !serde_json::to_string(&foreign_id_in_own_project)
            .unwrap()
            .contains("FOREIGN_TOKEN_CANARY")
    );
    let html = a06_read(
        &rig.endpoint,
        &grant,
        &project.id,
        json!({"detail":true,"mail_id":html_id}),
    );
    assert_eq!(html["ok"], true, "{html}");
    assert!(
        html["data"]["data"]["items"][0]["detail"]["links"]
            .as_array()
            .unwrap()
            .iter()
            .any(|link| link.as_str()
                == Some("https://example.test/verify?expires=123&signature=HTML_SIGNATURE_CANARY")),
        "HTML entity must decode into usable authorized URL: {html}"
    );
    assert!(
        html["data"]["data"]["items"][0]["detail"]["links"]
            .as_array()
            .unwrap()
            .iter()
            .any(|link| link.as_str() == Some(local_url.as_str())),
        "{html}"
    );
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock,
        "mail links must never be fetched automatically"
    );
    let show = a06_read(
        &rig.endpoint,
        &grant,
        &project.id,
        json!({"detail":true,"mail_id":first_id}),
    );
    assert_eq!(show["ok"], true, "{show}");
    let text = serde_json::to_string(&show).unwrap();
    let detail = &show["data"]["data"]["items"][0]["detail"];
    let typed: MailContentDetail = serde_json::from_value(detail.clone()).unwrap();
    let debug = format!("{typed:?}");
    for private in [
        "ALPHA_TOKEN_CANARY",
        "sender@example.test",
        "alpha@example.test",
        "Reset account",
    ] {
        assert!(!debug.contains(private), "Debug leaked {private}");
    }
    assert!(
        detail["links"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str() == Some("https://example.test/reset?token=ALPHA_TOKEN_CANARY")),
        "authorized verification URL remains usable: {show}"
    );
    assert!(
        detail["attachment_metadata"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["name"] == "report.txt" && v["size"].as_u64().unwrap_or(0) > 0),
        "authorized attachment metadata: {show}"
    );
    assert!(
        !text.contains("ATTACHMENT_PRIVATE_CANARY")
            && !text.contains("QVRUQUNITUVOVF9QUklWQVRFX0NBTkFSWQ==")
    );
    // A direct authorized show must locate an older exact ID even if newer
    // large MIME messages exceed the regular 16 MiB source-window budget.
    let large = vec![b'x'; 1_900_000];
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    for n in 0..9 {
        let large_id = format!("{:032x}", n + 1000);
        let received = now + 10 + n;
        let summary = json!({"id":large_id,"received_ms":received,"subject":"newer","from":"sender@example.test","to":"alpha@example.test","bytes":large.len(),"attachments":0});
        db.execute(
            "INSERT INTO messages(id,received,summary,search,raw) VALUES(?1,?2,?3,'newer',?4)",
            rusqlite::params![large_id, received, summary.to_string(), &large],
        )
        .unwrap();
    }
    db.execute_batch("COMMIT").unwrap();
    let older = a06_read(
        &rig.endpoint,
        &grant,
        &project.id,
        json!({"detail":true,"mail_id":first_id}),
    );
    assert_eq!(older["ok"], true, "{older}");
    assert_eq!(
        older["data"]["data"]["items"][0]["mail_id"], first_id,
        "older exact ID must not be lost behind newer large bodies"
    );
    assert!(
        older["data"]["data"]["items"][0]["detail"]["links"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_str() == Some("https://example.test/reset?token=ALPHA_TOKEN_CANARY"))
    );
    rig.native
        .revoke_external_grant(&grant.record.grant_id)
        .unwrap();
    let revoked = a06_read(
        &rig.endpoint,
        &grant,
        &project.id,
        json!({"detail":true,"mail_id":first_id}),
    );
    assert_eq!(revoked["error"]["code"], "grant_revoked", "{revoked}");
    assert!(
        !serde_json::to_string(&revoked)
            .unwrap()
            .contains("ALPHA_TOKEN_CANARY")
    );
}

#[test]
fn a06_mail_wait_uses_mark_filters_and_returns_only_new_authorized_mail() {
    use devforge_control::InformationScope;
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
            &[InformationScope::MailContent],
            Duration::from_secs(600),
        )
        .unwrap();
    let rejected = a05_call(
        &rig.endpoint,
        &observer.credential,
        &project.id,
        "activity_mark",
        json!({"stream":"mail","detail":true,"subject":"reset"}),
    );
    assert_eq!(rejected["error"]["code"], "unauthorized", "{rejected}");
    let filtered = a05_call(
        &rig.endpoint,
        &observer.credential,
        &project.id,
        "activity_mark",
        json!({"stream":"mail","detail":false,"subject":""}),
    );
    assert_eq!(filtered["error"]["code"], "unauthorized", "{filtered}");
    let missing = a05_call(
        &rig.endpoint,
        &grant.credential,
        &project.id,
        "activity_mark",
        json!({"stream":"mail","detail":true}),
    );
    assert_eq!(missing["error"]["code"], "precondition_failed", "{missing}");
    assert!(!rig.root.path().join("mail").exists());
    let db = a06_mailbox(rig.root.path(), &project.id);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    a06_insert_mail(
        &db,
        &"d".repeat(32),
        now,
        "sender@example.test",
        "alpha@example.test",
        "Reset account",
        "OLD_TOKEN_CANARY",
    );
    let metadata_mark = a05_call(
        &rig.endpoint,
        &observer.credential,
        &project.id,
        "activity_mark",
        json!({"stream":"mail","detail":false,"since":now}),
    );
    assert_eq!(metadata_mark["ok"], true, "{metadata_mark}");
    let metadata_bookmark = metadata_mark["data"]["bookmark"]
        .as_str()
        .unwrap()
        .to_owned();
    let marked = a05_call(
        &rig.endpoint,
        &grant.credential,
        &project.id,
        "activity_mark",
        json!({"stream":"mail","detail":true,"subject":"reset","recipient":"alpha@","sender":"sender@","since":now}),
    );
    assert_eq!(marked["ok"], true, "{marked}");
    let bookmark = marked["data"]["bookmark"].as_str().unwrap().to_owned();
    a06_insert_mail(
        &db,
        &"e".repeat(32),
        now + 1,
        "other@example.test",
        "other@example.test",
        "Receipt",
        "UNMATCHED_TOKEN_CANARY",
    );
    // Metadata-only wait and a nonmatching filtered mail must not parse MIME.
    db.execute(
        "UPDATE messages SET raw=X'00' WHERE id=?1",
        ["e".repeat(32)],
    )
    .unwrap();
    a06_insert_mail(
        &db,
        &"f".repeat(32),
        now + 2,
        "sender@example.test",
        "alpha@example.test",
        "Reset account",
        "NEW_TOKEN_CANARY",
    );
    let metadata_wait = a05_wait(
        &rig.endpoint,
        &observer,
        &project.id,
        &metadata_bookmark,
        2000,
    );
    assert_eq!(metadata_wait["ok"], true, "{metadata_wait}");
    assert_eq!(metadata_wait["data"]["event"]["stream"], "mail");
    let metadata_text = serde_json::to_string(&metadata_wait).unwrap();
    for secret in [
        "OLD_TOKEN_CANARY",
        "NEW_TOKEN_CANARY",
        "UNMATCHED_TOKEN_CANARY",
        "other@example.test",
        "Receipt",
    ] {
        assert!(
            !metadata_text.contains(secret),
            "Observer mail wait leaked {secret}"
        );
    }
    let result = a05_wait(&rig.endpoint, &grant, &project.id, &bookmark, 2000);
    assert_eq!(result["ok"], true, "{result}");
    assert_eq!(result["data"]["timed_out"], false, "{result}");
    assert_eq!(result["data"]["continuity"], "unverified");
    assert_eq!(result["data"]["event"]["stream"], "mail");
    let text = serde_json::to_string(&result).unwrap();
    assert!(
        text.contains("https://example.test/reset?token=NEW_TOKEN_CANARY"),
        "{result}"
    );
    assert!(!text.contains("OLD_TOKEN_CANARY") && !text.contains("UNMATCHED_TOKEN_CANARY"));
    let advanced = result["data"]["bookmark"].as_str().unwrap();
    let timeout = a05_wait(&rig.endpoint, &grant, &project.id, advanced, 1500);
    assert_eq!(timeout["ok"], true, "{timeout}");
    assert_eq!(timeout["data"]["timed_out"], true, "{timeout}");
    // A burst larger than the 16-MIME display cap must not be mistaken for
    // lost activity: a wait returns the earliest new matching mail, then the next.
    for n in 0..17 {
        a06_insert_mail(
            &db,
            &format!("{:032x}", n + 100),
            now + 3 + n,
            "sender@example.test",
            "alpha@example.test",
            "Reset account",
            &format!("BURST_TOKEN_{n}"),
        );
    }
    let burst_first = a05_wait(&rig.endpoint, &grant, &project.id, advanced, 2500);
    assert_eq!(burst_first["ok"], true, "{burst_first}");
    assert_eq!(
        burst_first["data"]["event"]["data"]["mail_id"],
        format!("{:032x}", 100)
    );
    let next_bookmark = burst_first["data"]["bookmark"].as_str().unwrap();
    let burst_next = a05_wait(&rig.endpoint, &grant, &project.id, next_bookmark, 2500);
    assert_eq!(burst_next["ok"], true, "{burst_next}");
    assert_eq!(
        burst_next["data"]["event"]["data"]["mail_id"],
        format!("{:032x}", 101)
    );
    rig.native
        .revoke_external_grant(&grant.record.grant_id)
        .unwrap();
    let revoked = a05_wait(&rig.endpoint, &grant, &project.id, advanced, 1500);
    assert_eq!(revoked["error"]["code"], "grant_revoked", "{revoked}");
}

#[test]
fn a07_support_preview_is_scoped_stable_and_never_contains_private_sources() {
    use devforge_control::tools::{SupportPreviewInput, SupportReadInput};

    let mut rig = Rig::new(None);
    let a = rig.projects[0].clone();
    let b = rig.projects[1].clone();
    let issue = |rig: &mut Rig, project: &str| {
        rig.native
            .issue_external_grant(&id(project), &[Scope::Observer], Duration::from_secs(600))
            .unwrap()
    };
    let owner = issue(&mut rig, &a.id);
    let other_client = issue(&mut rig, &a.id);
    let foreign = issue(&mut rig, &b.id);
    let revocable = issue(&mut rig, &a.id);
    let mut client = rig.diagnostics(&owner);
    let mut other = rig.diagnostics(&other_client);
    let mut foreign_client = rig.diagnostics(&foreign);
    let mut revoke_client = rig.diagnostics(&revocable);
    let preview = |project: &str, revision| Request {
        api_version: Default::default(),
        request_id: id("a07-preview"),
        project_id: id(project),
        expected_revision: revision,
        idempotency_key: None,
        input: SupportPreviewInput {},
    };
    let read = |project: &str, artifact: Id, revision| Request {
        api_version: Default::default(),
        request_id: id("a07-read"),
        project_id: id(project),
        expected_revision: revision,
        idempotency_key: None,
        input: SupportReadInput {
            artifact_id: artifact,
        },
    };
    let env = a.path.join(".env");
    let private_file = a.path.join("PRIVATE_FILENAME_CANARY.txt");
    std::fs::write(
        &env,
        "APP_KEY=PRIVATE_ENV_CANARY\nDB_PASSWORD=PRIVATE_PASSWORD_CANARY\n",
    )
    .unwrap();
    std::fs::write(&private_file, "PRIVATE_FILE_CONTENT_CANARY").unwrap();
    let inspector = rig.root.path().join("inspector");
    std::fs::create_dir_all(&inspector).unwrap();
    let bad_collector = inspector.join("events.sqlite3");
    std::fs::write(&bad_collector, "PRIVATE_LOG_CANARY not sqlite").unwrap();
    let mail = rig.root.path().join("mail").join(&a.id);
    std::fs::create_dir_all(&mail).unwrap();
    let bad_mail = mail.join("inbox.sqlite3");
    std::fs::write(&bad_mail, "PRIVATE_MAIL_CANARY not sqlite").unwrap();
    let initial_env = std::fs::read(&env).unwrap();
    let initial_file = std::fs::read(&private_file).unwrap();

    // Bad collectors prove support preview does not read raw log/mail stores.
    let first = client
        .support_preview(&preview(&a.id, Some(a.revision)))
        .unwrap();
    assert!(first.is_ok(), "{:?}", first.error());
    let output = first.data().unwrap();
    let artifact = output.artifact_id.clone();
    let first_preview = serde_json::to_value(&output.preview).unwrap();
    assert_eq!(first_preview["schema_version"], 1);
    assert_eq!(first_preview["doctor"]["origin"], "cached");
    assert!(first_preview["runbook"]["evidence"].is_string());
    assert!(first_preview["runbook"]["impact"].is_string());
    for omitted in [
        "environment_values",
        "private_keys",
        "database_dumps",
        "logs_and_mail_content",
        "arbitrary_files",
    ] {
        assert!(
            first_preview["omitted"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_str() == Some(omitted)),
            "missing omitted section {omitted}"
        );
    }
    let serialized = serde_json::to_string(&first).unwrap();
    for secret in [
        "PRIVATE_ENV_CANARY",
        "PRIVATE_PASSWORD_CANARY",
        "PRIVATE_FILENAME_CANARY",
        "PRIVATE_FILE_CONTENT_CANARY",
        "PRIVATE_LOG_CANARY",
        "PRIVATE_MAIL_CANARY",
        owner.credential.expose_secret(),
        a.path.to_str().unwrap(),
    ] {
        assert!(
            !serialized.contains(secret),
            "support preview exposed private source: {secret}"
        );
    }
    let again = client
        .support_read(&read(&a.id, artifact.clone(), Some(a.revision)))
        .unwrap();
    assert!(again.is_ok(), "{:?}", again.error());
    assert_eq!(again.data().unwrap().artifact_id, artifact);
    assert_eq!(
        serde_json::to_value(&again.data().unwrap().preview).unwrap(),
        first_preview,
        "read must use the exact preview, not recompute a different snapshot"
    );
    assert_eq!(again.data().unwrap().expires_at, output.expires_at);

    let other_result = other
        .support_read(&read(&a.id, artifact.clone(), Some(a.revision)))
        .unwrap();
    assert_eq!(
        other_result.error().unwrap().code,
        ErrorCode::CursorExpired,
        "same-project foreign client/grant cannot read artifact"
    );
    let mismatch = client
        .support_read(&read(&b.id, artifact.clone(), None))
        .unwrap();
    assert_eq!(mismatch.error().unwrap().code, ErrorCode::ProjectMismatch);
    // Existing and nonexistent foreign project IDs are indistinguishable to
    // an A-bound credential; an unknown token cannot read any artifact.
    let nonexistent = "f".repeat(32);
    for foreign_project in [&b.id, &nonexistent] {
        let denied = client
            .support_preview(&preview(foreign_project, None))
            .unwrap();
        assert_eq!(denied.error().unwrap().code, ErrorCode::ProjectMismatch);
    }
    let unknown_credential = ExternalCredential::from_wire(&"0".repeat(64)).unwrap();
    let mut unknown_client =
        DiagnosticsClient::connect(rig.endpoint.clone(), unknown_credential).unwrap();
    let denied = unknown_client
        .support_read(&read(&a.id, artifact.clone(), None))
        .unwrap();
    assert_eq!(denied.error().unwrap().code, ErrorCode::Unauthorized);
    let mismatch = foreign_client
        .support_read(&read(&b.id, artifact.clone(), None))
        .unwrap();
    assert_eq!(mismatch.error().unwrap().code, ErrorCode::CursorExpired);
    let unknown = client
        .support_read(&read(&a.id, id("unknown-artifact"), None))
        .unwrap();
    assert_eq!(unknown.error().unwrap().code, ErrorCode::CursorExpired);
    let revision = client
        .support_read(&read(&a.id, artifact.clone(), Some(a.revision + 1)))
        .unwrap();
    assert_eq!(revision.error().unwrap().code, ErrorCode::RevisionConflict);

    let revoke_preview = revoke_client
        .support_preview(&preview(&a.id, Some(a.revision)))
        .unwrap();
    assert!(revoke_preview.is_ok());
    let revoke_id = revoke_preview.data().unwrap().artifact_id.clone();
    rig.native
        .revoke_external_grant(&revocable.record.grant_id)
        .unwrap();
    let revoked = revoke_client
        .support_read(&read(&a.id, revoke_id, None))
        .unwrap();
    assert_eq!(revoked.error().unwrap().code, ErrorCode::GrantRevoked);
    assert_eq!(std::fs::read(&env).unwrap(), initial_env);
    assert_eq!(std::fs::read(&private_file).unwrap(), initial_file);
    assert_eq!(
        std::fs::read(&bad_collector).unwrap(),
        b"PRIVATE_LOG_CANARY not sqlite"
    );
    assert_eq!(
        std::fs::read(&bad_mail).unwrap(),
        b"PRIVATE_MAIL_CANARY not sqlite"
    );

    // A valid existing artifact cannot silently rebind to a changed revision.
    rig.runtime_metadata();
    let stale = client
        .support_read(&read(&a.id, artifact.clone(), None))
        .unwrap();
    assert_eq!(
        stale.error().unwrap().code,
        ErrorCode::CursorExpired,
        "artifact must be bound to its original project revision"
    );
    let stale_revision = client
        .support_read(&read(&a.id, artifact, Some(a.revision)))
        .unwrap();
    assert_eq!(
        stale_revision.error().unwrap().code,
        ErrorCode::RevisionConflict
    );
}

#[test]
fn a07_support_export_requires_native_review_and_preserves_exact_preview() {
    use devforge_control::{mutations::SupportExportInput, tools::SupportPreviewInput};
    use devforge_manager::service_control::ReviewTarget;
    let mut rig = Rig::new(None);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(rig.root.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let project = rig.projects[0].clone();
    let grant = rig
        .native
        .issue_external_grant(
            &id(&project.id),
            &[
                Scope::Observer,
                Scope::ProjectManage,
                Scope::SensitiveOperation,
            ],
            Duration::from_secs(600),
        )
        .unwrap();
    let observer = rig
        .native
        .issue_external_grant(
            &id(&project.id),
            &[Scope::Observer],
            Duration::from_secs(600),
        )
        .unwrap();
    let mut client = rig.diagnostics(&grant);
    let mut denied = rig.diagnostics(&observer);
    let preview_request = Request {
        api_version: Default::default(),
        request_id: id("export-preview"),
        project_id: id(&project.id),
        expected_revision: Some(project.revision),
        idempotency_key: None,
        input: SupportPreviewInput {},
    };
    let preview = client
        .support_preview(&preview_request)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    let bytes = serde_json::to_vec(&preview.preview).unwrap();
    let request = |key: &str, artifact_id: Id| {
        env_request(
            &project.id,
            project.revision,
            key,
            MutationInput::SupportExport(SupportExportInput {
                artifact_id,
                limits: None,
            }),
        )
    };
    let req = request("a07-export-one", preview.artifact_id.clone());
    assert!(!denied.support_export_plan(&req).unwrap().is_ok());
    let plan = client
        .support_export_plan(&req)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    let pending = rig.native.pending_reviews().unwrap();
    let review = pending
        .iter()
        .find(|review| review.digest == plan.digest)
        .unwrap();
    let destination = match &review.target {
        ReviewTarget::SupportExport {
            destination,
            byte_count,
            ..
        } => {
            assert_eq!(*byte_count, bytes.len() as u64);
            std::path::PathBuf::from(destination)
        }
        _ => panic!("wrong review target"),
    };
    assert_eq!(
        destination.parent(),
        Some(
            std::fs::canonicalize(rig.root.path().join("control"))
                .unwrap()
                .as_path()
        )
    );
    assert!(!destination.exists());
    assert!(
        !client
            .support_export_apply(&req, &plan.digest)
            .unwrap()
            .is_ok()
    );
    assert!(!destination.exists());
    let changed = request("a07-export-one", id("different-artifact"));
    assert!(
        !client
            .support_export_apply(&changed, &plan.digest)
            .unwrap()
            .is_ok()
    );
    assert!(!destination.exists());
    rig.native.approve_service_plan(&plan.digest).unwrap();
    let applied = client.support_export_apply(&req, &plan.digest).unwrap();
    let receipt = applied
        .data()
        .unwrap_or_else(|| panic!("apply: {:?}", applied.error()))
        .clone();
    assert_eq!(rig.complete(&grant, &receipt), OperationState::Succeeded);
    assert_eq!(std::fs::read(&destination).unwrap(), bytes);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&destination)
                .unwrap()
                .permissions()
                .mode()
                & 0o077,
            0
        );
    }
    // Replaying the same idempotency key is a lookup, never another write.
    std::fs::write(&destination, b"existing-content").unwrap();
    let replay = client.support_export_apply(&req, &plan.digest).unwrap();
    assert!(replay.is_ok(), "{:?}", replay.error());
    assert_eq!(std::fs::read(&destination).unwrap(), b"existing-content");
    // A newly approved plan for this artifact still must not overwrite a file.
    let second = request("a07-export-two", preview.artifact_id.clone());
    let second_plan = client
        .support_export_plan(&second)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    rig.native
        .approve_service_plan(&second_plan.digest)
        .unwrap();
    let second_result = client
        .support_export_apply(&second, &second_plan.digest)
        .unwrap();
    if let Some(receipt) = second_result.data() {
        assert_ne!(rig.complete(&grant, receipt), OperationState::Succeeded);
    }
    assert_eq!(std::fs::read(&destination).unwrap(), b"existing-content");

    let revoke = rig
        .native
        .issue_external_grant(
            &id(&project.id),
            &[
                Scope::Observer,
                Scope::ProjectManage,
                Scope::SensitiveOperation,
            ],
            Duration::from_secs(600),
        )
        .unwrap();
    let mut revoke_client = rig.diagnostics(&revoke);
    let p = revoke_client
        .support_preview(&preview_request)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    let r = request("a07-export-revoked", p.artifact_id);
    let planned = revoke_client
        .support_export_plan(&r)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    let revoked_destination = rig
        .native
        .pending_reviews()
        .unwrap()
        .into_iter()
        .find_map(|v| {
            if v.digest == planned.digest {
                if let ReviewTarget::SupportExport { destination, .. } = v.target {
                    Some(std::path::PathBuf::from(destination))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .unwrap();
    rig.native.approve_service_plan(&planned.digest).unwrap();
    rig.native
        .revoke_external_grant(&revoke.record.grant_id)
        .unwrap();
    assert!(
        !revoke_client
            .support_export_apply(&r, &planned.digest)
            .unwrap()
            .is_ok()
    );
    assert!(!revoked_destination.exists());

    let expiring = rig
        .native
        .issue_external_grant(
            &id(&project.id),
            &[
                Scope::Observer,
                Scope::ProjectManage,
                Scope::SensitiveOperation,
            ],
            Duration::from_secs(3),
        )
        .unwrap();
    let mut expiring_client = rig.diagnostics(&expiring);
    let p = expiring_client
        .support_preview(&preview_request)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    let r = request("a07-export-expired", p.artifact_id);
    let planned = expiring_client
        .support_export_plan(&r)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    let expired_destination = rig
        .native
        .pending_reviews()
        .unwrap()
        .into_iter()
        .find_map(|v| {
            if v.digest == planned.digest {
                if let ReviewTarget::SupportExport { destination, .. } = v.target {
                    Some(std::path::PathBuf::from(destination))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .unwrap();
    rig.native.approve_service_plan(&planned.digest).unwrap();
    thread::sleep(Duration::from_millis(3200));
    assert!(
        !expiring_client
            .support_export_apply(&r, &planned.digest)
            .unwrap()
            .is_ok()
    );
    assert!(!expired_destination.exists());
}

#[test]
fn a08_site_probe_requires_project_execution_grant_and_current_binding() {
    use devforge_control::{limits::Text, tools::SiteProbeInput};
    let mut rig = Rig::new(None);
    let a = rig.projects[0].clone();
    let b = rig.projects[1].clone();
    let observer = rig
        .native
        .issue_external_grant(&id(&a.id), &[Scope::Observer], Duration::from_secs(600))
        .unwrap();
    let execution = rig.grant(0, true);
    let mut denied = rig.diagnostics(&observer);
    let mut client = rig.diagnostics(&execution);
    let req = |project: &str, revision| Request {
        api_version: Default::default(),
        request_id: id("a08-probe"),
        project_id: id(project),
        expected_revision: revision,
        idempotency_key: None,
        input: SiteProbeInput {
            path: Text::new("/".into()).unwrap(),
        },
    };
    let before = std::fs::read_dir(rig.root.path()).unwrap().count();
    assert_eq!(
        denied
            .site_probe(&req(&a.id, None))
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::Unauthorized
    );
    assert_eq!(
        client
            .site_probe(&req(&b.id, None))
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::ProjectMismatch
    );
    assert_eq!(
        client
            .site_probe(&req("missing-project", None))
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::ProjectMismatch
    );
    assert_eq!(
        client
            .site_probe(&req(&a.id, Some(a.revision + 1)))
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::RevisionConflict
    );
    // A valid grant cannot cause a request to a stopped/nonexistent site.
    assert_eq!(
        client
            .site_probe(&req(&a.id, Some(a.revision)))
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::PreconditionFailed
    );
    rig.native
        .revoke_external_grant(&execution.record.grant_id)
        .unwrap();
    assert_eq!(
        client
            .site_probe(&req(&a.id, None))
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::GrantRevoked
    );
    assert_eq!(std::fs::read_dir(rig.root.path()).unwrap().count(), before);
}

#[test]
fn a09_debugger_reads_require_explicit_project_content_rights_before_io() {
    use devforge_control::InformationScope;
    let mut rig = Rig::new(None);
    let a = rig.projects[0].clone();
    let b = rig.projects[1].clone();
    let observer = rig
        .native
        .issue_external_grant(&id(&a.id), &[Scope::Observer], Duration::from_secs(600))
        .unwrap();
    // A grant made with every pre-A09 information right must not inherit
    // the new ability to read live debugger values.
    let old = rig
        .native
        .issue_external_grant_with_information(
            &id(&a.id),
            &[Scope::Observer],
            &[
                InformationScope::LogContent,
                InformationScope::InspectorQueries,
                InformationScope::MailContent,
                InformationScope::SourceArtifacts,
                InformationScope::DatabaseQuery,
            ],
            Duration::from_secs(600),
        )
        .unwrap();
    let values = rig
        .native
        .issue_external_grant_with_information(
            &id(&a.id),
            &[Scope::Observer],
            &[InformationScope::DebuggerValues],
            Duration::from_secs(600),
        )
        .unwrap();
    let both = rig
        .native
        .issue_external_grant_with_information(
            &id(&a.id),
            &[Scope::Observer],
            &[
                InformationScope::SourceArtifacts,
                InformationScope::DebuggerValues,
            ],
            Duration::from_secs(600),
        )
        .unwrap();
    let endpoint = rig.endpoint.clone();
    let inspect = |grant: &IssuedGrant, project: &str, detail, locals| {
        a05_call(
            &endpoint,
            &grant.credential,
            project,
            "debugger_inspect",
            json!({"detail":detail,"locals":locals}),
        )
    };
    let root = rig.root.path().join("projects/debugging");
    assert!(
        !root.exists(),
        "fixture unexpectedly contains debugger storage"
    );
    let metadata = inspect(&observer, &a.id, false, false);
    assert_eq!(metadata["ok"], true, "{metadata}");
    for (grant, detail, locals) in [
        (&observer, true, false),
        (&observer, false, true),
        (&old, false, true),
        (&values, true, false),
        (&values, true, true),
    ] {
        let denied = inspect(grant, &a.id, detail, locals);
        assert_eq!(denied["error"]["code"], "unauthorized", "{denied}");
        assert!(
            !root.exists(),
            "denied request must not create debugger storage"
        );
    }
    for (grant, detail, locals) in [
        (&old, true, false),
        (&values, false, true),
        (&both, true, true),
    ] {
        let allowed = inspect(grant, &a.id, detail, locals);
        assert_eq!(allowed["ok"], true, "{allowed}");
        assert!(
            !root.exists(),
            "read must not create or trim debugger storage"
        );
    }
    let profile = |grant: &IssuedGrant| {
        a05_call(
            &endpoint,
            &grant.credential,
            &a.id,
            "debugger_profile",
            json!({"profile_id":"unknown-profile"}),
        )
    };
    for grant in [&observer, &values] {
        let denied = profile(grant);
        assert_eq!(denied["error"]["code"], "unauthorized", "{denied}");
        assert!(
            !root.exists(),
            "denied profile read must not create storage"
        );
    }
    let unavailable = profile(&old);
    assert_eq!(unavailable["ok"], false, "{unavailable}");
    assert!(!root.exists(), "missing profile must not create storage");
    for project in [&b.id[..], "unknown-project"] {
        let response = inspect(&both, project, true, true);
        assert_eq!(response["error"]["code"], "project_mismatch", "{response}");
    }
    let stale = Client::new(endpoint.clone())
        .request(&json!({
            "method":"debugger_inspect", "credential":both.credential.expose_secret(),
            "request":{"api_version":1,"request_id":"a09-stale","project_id":a.id,
                "expected_revision":a.revision + 1,"idempotency_key":null,
                "input":{"detail":true,"locals":true}}
        }))
        .unwrap();
    assert_eq!(stale["error"]["code"], "revision_conflict", "{stale}");
    let bad = ExternalCredential::from_wire(&"a".repeat(64)).unwrap();
    let unknown = a05_call(
        &rig.endpoint,
        &bad,
        &a.id,
        "debugger_inspect",
        json!({"detail":false,"locals":false}),
    );
    assert_eq!(unknown["error"]["code"], "unauthorized", "{unknown}");
    drop(bad);
    rig.native
        .revoke_external_grant(&both.record.grant_id)
        .unwrap();
    let revoked = inspect(&both, &a.id, true, true);
    assert_eq!(revoked["error"]["code"], "grant_revoked", "{revoked}");
    assert!(!root.exists());
}

#[test]
fn a09_debugger_mutations_are_scoped_and_bound_to_revision_and_owner() {
    use devforge_control::{
        limits::{SafeInt, Text},
        mutations::{
            DebuggerBreakpoint, DebuggerBreakpointsInput, DebuggerCommand, DebuggerControlInput,
        },
    };
    let mut rig = Rig::new(None);
    let a = rig.projects[0].clone();
    let b = rig.projects[1].clone();
    let observer = rig
        .native
        .issue_external_grant(&id(&a.id), &[Scope::Observer], Duration::from_secs(600))
        .unwrap();
    let manager = rig.grant(0, false);
    let runner = rig.grant(0, true);
    let another = rig.grant(0, true);
    let mut no_right = rig.diagnostics(&observer);
    let mut writer = rig.diagnostics(&manager);
    let mut client = rig.diagnostics(&runner);
    let mut foreign = rig.diagnostics(&another);
    let breakpoint = |project: &str, revision, key| {
        env_request(
            project,
            revision,
            key,
            MutationInput::DebuggerBreakpoints(DebuggerBreakpointsInput {
                breakpoints: vec![DebuggerBreakpoint {
                    file: Text::new("public/index.php".into()).unwrap(),
                    line: SafeInt::new(1).unwrap(),
                }],
                limits: None,
            }),
        )
    };
    let request = breakpoint(&a.id, a.revision, "a09-breakpoint");
    assert_eq!(
        no_right
            .debugger_plan(&request)
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::Unauthorized
    );
    assert_eq!(
        writer
            .debugger_plan(&breakpoint(&b.id, b.revision, "foreign-breakpoint"))
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::ProjectMismatch
    );
    assert_eq!(
        writer
            .debugger_plan(&breakpoint(&a.id, a.revision + 1, "stale-breakpoint"))
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::RevisionConflict
    );
    let ticket = writer
        .debugger_plan(&request)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    let response = foreign.debugger_apply(&request, &ticket.digest).unwrap();
    assert!(
        response.error().is_some(),
        "another grant must not apply a plan"
    );
    let receipt = writer
        .debugger_apply(&request, &ticket.digest)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    assert_eq!(rig.complete(&manager, &receipt), OperationState::Succeeded);
    let replay = writer
        .debugger_apply(&request, &ticket.digest)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    assert_eq!(receipt.operation_id, replay.operation_id);
    let control = |revision| {
        env_request(
            &a.id,
            revision,
            "a09-control",
            MutationInput::DebuggerControl(DebuggerControlInput {
                command: DebuggerCommand::StepOver,
                session_id: id("00000000000000000000000000000000"),
                session_revision: SafeInt::new(0).unwrap(),
                depth: None,
                limits: None,
            }),
        )
    };
    assert_eq!(
        writer
            .debugger_plan(&control(a.revision + 1))
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::Unauthorized
    );
    assert_eq!(
        client
            .debugger_plan(&control(a.revision))
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::RevisionConflict
    );
    assert!(
        !client
            .debugger_plan(&control(a.revision + 1))
            .unwrap()
            .is_ok(),
        "a stopped debugger cannot plan a step"
    );
}

#[cfg(feature = "test-support")]
#[test]
fn a09_primitive_assignment_requires_exact_native_review_and_keeps_secret_out_of_ledger() {
    use devforge_control::{
        InformationScope,
        limits::{SafeInt, Text},
        mutations::{DebuggerAssignInput, DebuggerPrimitive, DebuggerSecret},
    };
    use devforge_manager::service_control::ReviewTarget;
    let mut rig = Rig::new(None);
    let project = rig.projects[0].clone();
    let (session, session_revision) = rig
        .debug_actor
        .test_synthetic_paused_debugger(&project.id)
        .unwrap();
    let scopes = &[
        Scope::Observer,
        Scope::ProjectManage,
        Scope::ExecutionTest,
        Scope::SensitiveOperation,
    ];
    let plain = rig
        .native
        .issue_external_grant(&id(&project.id), scopes, Duration::from_secs(600))
        .unwrap();
    let writer = rig
        .native
        .issue_external_grant_with_information(
            &id(&project.id),
            scopes,
            &[InformationScope::DebuggerValues],
            Duration::from_secs(600),
        )
        .unwrap();
    let another = rig
        .native
        .issue_external_grant_with_information(
            &id(&project.id),
            scopes,
            &[InformationScope::DebuggerValues],
            Duration::from_secs(600),
        )
        .unwrap();
    let mut no_content = rig.diagnostics(&plain);
    let mut client = rig.diagnostics(&writer);
    let mut other = rig.diagnostics(&another);
    let req = |value: &str| {
        env_request(
            &project.id,
            project.revision,
            "a09-assign",
            MutationInput::DebuggerAssign(DebuggerAssignInput {
                name: Text::new("$value".into()).unwrap(),
                kind: DebuggerPrimitive::Int,
                value: DebuggerSecret::new(value.into()).unwrap(),
                session_id: id(&session),
                session_revision: SafeInt::new(session_revision).unwrap(),
                limits: None,
            }),
        )
    };
    let one = req("1");
    assert_eq!(
        no_content
            .debugger_plan(&one)
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::Unauthorized
    );
    let plan = client.debugger_plan(&one).unwrap().data().unwrap().clone();
    let binding = devforge_control::ProjectBinding::from_registered(
        id(&project.id),
        &project.path,
        project.revision,
    )
    .unwrap();
    let raw_fp = devforge_control_adapter::sha256(
        &devforge_control::mutations::fingerprint_material(&one, &binding).unwrap(),
    )
    .unwrap();
    assert_ne!(
        plan.request_fingerprint, raw_fp,
        "low-entropy value fingerprint must be opaque"
    );
    let pending = rig.native.pending_reviews().unwrap();
    let review = pending
        .iter()
        .find(|item| item.digest == plan.digest)
        .unwrap();
    assert!(
        matches!(&review.target, ReviewTarget::DebuggerAssign {name,value_type,value,session_id,session_revision: reviewed_revision} if name == "$value" && value_type == "int" && value == "1" && session_id == &session && *reviewed_revision == session_revision)
    );
    assert_eq!(
        client
            .debugger_apply(&one, &plan.digest)
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::Unauthorized
    );
    assert!(
        other
            .debugger_apply(&one, &plan.digest)
            .unwrap()
            .error()
            .is_some(),
        "foreign grant cannot use the plan"
    );
    rig.native.approve_service_plan(&plan.digest).unwrap();
    assert!(
        client
            .debugger_apply(&req("2"), &plan.digest)
            .unwrap()
            .error()
            .is_some(),
        "changed value cannot use approval"
    );
    let receipt = client
        .debugger_apply(&one, &plan.digest)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    assert_eq!(
        rig.complete(&writer, &receipt),
        OperationState::Succeeded,
        "queue admission succeeds for the synthetic paused session"
    );
    let replay = client
        .debugger_apply(&one, &plan.digest)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    assert_eq!(receipt.operation_id, replay.operation_id);
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
    let journal = std::fs::read(rig.root.path().join("control/operations.sqlite3")).unwrap();
    assert!(!String::from_utf8_lossy(&journal).contains(raw_fp.as_str()));
    rig.native
        .revoke_external_grant(&writer.record.grant_id)
        .unwrap();
    assert_eq!(
        client
            .debugger_apply(&one, &plan.digest)
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::GrantRevoked
    );
}

#[test]
fn a09_reviewed_profile_export_is_scoped_exact_and_no_clobber() {
    use devforge_control::{InformationScope, mutations::DebuggerExportInput};
    use devforge_manager::service_control::ReviewTarget;
    let mut rig = Rig::new(None);
    let project = rig.projects[0].clone();
    let directory = rig
        .root
        .path()
        .join("projects/debugging")
        .join(&project.id)
        .join("profiles");
    std::fs::create_dir_all(&directory).unwrap();
    let source = directory.join("cachegrind.a.out");
    std::fs::write(&source, b"events: Time_(10ns) Memory_(bytes)\nfl=(1) /outside/private.php\nfn=(1) main\n1 100000 8\nsummary: 100000 8\n").unwrap();
    let scopes = &[
        Scope::Observer,
        Scope::ProjectManage,
        Scope::ExecutionTest,
        Scope::SensitiveOperation,
    ];
    let no_content = rig
        .native
        .issue_external_grant(&id(&project.id), scopes, Duration::from_secs(600))
        .unwrap();
    let writer = rig
        .native
        .issue_external_grant_with_information(
            &id(&project.id),
            scopes,
            &[InformationScope::SourceArtifacts],
            Duration::from_secs(600),
        )
        .unwrap();
    let mut denied = rig.diagnostics(&no_content);
    let mut client = rig.diagnostics(&writer);
    let inspect = a05_call(
        &rig.endpoint,
        &writer.credential,
        &project.id,
        "debugger_inspect",
        json!({"detail":true,"locals":false}),
    );
    let profile_id = id(inspect["data"]["profiles"][0]["id"]
        .as_str()
        .expect("synthetic profile present"));
    let preview = a05_call(
        &rig.endpoint,
        &writer.credential,
        &project.id,
        "debugger_profile",
        json!({"profile_id":profile_id}),
    );
    assert_eq!(preview["ok"], true, "{preview}");
    let bytes = serde_json::to_vec(&preview["data"]).unwrap();
    let request = |key: &str| {
        env_request(
            &project.id,
            project.revision,
            key,
            MutationInput::DebuggerExport(DebuggerExportInput {
                profile_id: profile_id.clone(),
                limits: None,
            }),
        )
    };
    let req = request("a09-profile-export");
    assert_eq!(
        denied.debugger_plan(&req).unwrap().error().unwrap().code,
        ErrorCode::Unauthorized
    );
    let plan = client.debugger_plan(&req).unwrap().data().unwrap().clone();
    let pending = rig.native.pending_reviews().unwrap();
    let review = pending.iter().find(|v| v.digest == plan.digest).unwrap();
    let destination = match &review.target {
        ReviewTarget::DebuggerExport {
            destination,
            content_sha256,
            byte_count,
        } => {
            assert_eq!(*byte_count, bytes.len() as u64);
            assert_eq!(
                content_sha256.as_str(),
                devforge_control_adapter::sha256(&bytes).unwrap().as_str()
            );
            std::path::PathBuf::from(destination)
        }
        _ => panic!("wrong review target"),
    };
    assert!(!destination.exists());
    assert_eq!(
        client
            .debugger_apply(&req, &plan.digest)
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::Unauthorized
    );
    rig.native.approve_service_plan(&plan.digest).unwrap();
    let receipt = client
        .debugger_apply(&req, &plan.digest)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    assert_eq!(rig.complete(&writer, &receipt), OperationState::Succeeded);
    assert_eq!(std::fs::read(&destination).unwrap(), bytes);
    let duplicate = request("a09-profile-export-second");
    let second = client
        .debugger_plan(&duplicate)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    rig.native.approve_service_plan(&second.digest).unwrap();
    let receipt = client
        .debugger_apply(&duplicate, &second.digest)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    assert_eq!(rig.complete(&writer, &receipt), OperationState::Failed);
    assert_eq!(
        std::fs::read(&destination).unwrap(),
        bytes,
        "existing export must not be overwritten"
    );
}

#[cfg(feature = "test-support")]
#[test]
fn a09_pending_reviews_hide_auto_actions_and_mask_revoked_or_stale_assignments() {
    use devforge_control::{
        InformationScope,
        limits::{SafeInt, Text},
        mutations::{
            DebuggerAssignInput, DebuggerBreakpoint, DebuggerBreakpointsInput, DebuggerCommand,
            DebuggerControlInput, DebuggerMode, DebuggerModeInput, DebuggerPrimitive,
            DebuggerSecret,
        },
    };
    use devforge_manager::service_control::{ReviewState, ReviewTarget};
    let mut rig = Rig::new(None);
    let project = rig.projects[0].clone();
    let (session, session_revision) = rig
        .debug_actor
        .test_synthetic_paused_debugger(&project.id)
        .unwrap();
    let scopes = &[
        Scope::Observer,
        Scope::ProjectManage,
        Scope::ExecutionTest,
        Scope::SensitiveOperation,
    ];
    let first = rig
        .native
        .issue_external_grant_with_information(
            &id(&project.id),
            scopes,
            &[InformationScope::DebuggerValues],
            Duration::from_secs(600),
        )
        .unwrap();
    let second = rig
        .native
        .issue_external_grant_with_information(
            &id(&project.id),
            scopes,
            &[InformationScope::DebuggerValues],
            Duration::from_secs(600),
        )
        .unwrap();
    let mut one = rig.diagnostics(&first);
    let mut two = rig.diagnostics(&second);
    let assign = |key: &str, value: &str| {
        env_request(
            &project.id,
            project.revision,
            key,
            MutationInput::DebuggerAssign(DebuggerAssignInput {
                name: Text::new("$value".into()).unwrap(),
                kind: DebuggerPrimitive::String,
                value: DebuggerSecret::new(value.into()).unwrap(),
                session_id: id(&session),
                session_revision: SafeInt::new(session_revision).unwrap(),
                limits: None,
            }),
        )
    };
    let first_plan = one
        .debugger_plan(&assign("a09-revoke-review", "REVOKED_PRIVATE_CANARY"))
        .unwrap()
        .data()
        .unwrap()
        .clone();
    let second_plan = two
        .debugger_plan(&assign("a09-stale-review", "STALE_PRIVATE_CANARY"))
        .unwrap()
        .data()
        .unwrap()
        .clone();
    let bp = env_request(
        &project.id,
        project.revision,
        "a09-auto-bp",
        MutationInput::DebuggerBreakpoints(DebuggerBreakpointsInput {
            breakpoints: vec![DebuggerBreakpoint {
                file: Text::new("public/index.php".into()).unwrap(),
                line: SafeInt::new(1).unwrap(),
            }],
            limits: None,
        }),
    );
    let control = env_request(
        &project.id,
        project.revision,
        "a09-auto-control",
        MutationInput::DebuggerControl(DebuggerControlInput {
            command: DebuggerCommand::StepOver,
            session_id: id(&session),
            session_revision: SafeInt::new(session_revision).unwrap(),
            depth: None,
            limits: None,
        }),
    );
    let bp_plan = two.debugger_plan(&bp).unwrap().data().unwrap().clone();
    let control_plan = two.debugger_plan(&control).unwrap().data().unwrap().clone();
    let listed = rig.native.pending_reviews().unwrap();
    assert!(listed.iter().any(|v| v.digest == first_plan.digest));
    assert!(listed.iter().any(|v| v.digest == second_plan.digest));
    assert!(
        !listed
            .iter()
            .any(|v| v.digest == bp_plan.digest || v.digest == control_plan.digest)
    );
    rig.native
        .revoke_external_grant(&first.record.grant_id)
        .unwrap();
    let listed = rig.native.pending_reviews().unwrap();
    let revoked = listed
        .iter()
        .find(|v| v.digest == first_plan.digest)
        .unwrap();
    assert!(matches!(revoked.state, ReviewState::Revoked));
    assert!(
        matches!(&revoked.target,ReviewTarget::DebuggerAssign{value,..} if value == "[REDACTED]")
    );
    // An active session intentionally refuses breakpoint writes. Switching
    // the stopped project's debugger mode off is a separate legitimate action
    // that advances its project revision and invalidates the old review.
    let change = env_request(
        &project.id,
        project.revision,
        "a09-stale-mode",
        MutationInput::DebuggerMode(DebuggerModeInput {
            mode: DebuggerMode::Off,
            runtime_id: None,
            limits: None,
        }),
    );
    let mode_plan = two.debugger_plan(&change).unwrap().data().unwrap().clone();
    let receipt = two
        .debugger_apply(&change, &mode_plan.digest)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    assert_eq!(rig.complete(&second, &receipt), OperationState::Succeeded);
    let listed = rig.native.pending_reviews().unwrap();
    let stale = listed
        .iter()
        .find(|v| v.digest == second_plan.digest)
        .unwrap();
    assert!(matches!(stale.state, ReviewState::Stale));
    assert!(
        matches!(&stale.target,ReviewTarget::DebuggerAssign{value,..} if value == "[REDACTED]")
    );
}

#[cfg(feature = "test-support")]
#[test]
fn a09_assignment_review_rejects_new_session_with_same_pause_revision() {
    use devforge_control::{
        InformationScope,
        limits::{SafeInt, Text},
        mutations::{DebuggerAssignInput, DebuggerPrimitive, DebuggerSecret},
    };
    use devforge_manager::service_control::{ReviewState, ReviewTarget};
    let mut rig = Rig::new(None);
    let project = rig.projects[0].clone();
    let (first_session, first_revision) = rig
        .debug_actor
        .test_synthetic_paused_debugger(&project.id)
        .unwrap();
    let grant = rig
        .native
        .issue_external_grant_with_information(
            &id(&project.id),
            &[
                Scope::Observer,
                Scope::ProjectManage,
                Scope::ExecutionTest,
                Scope::SensitiveOperation,
            ],
            &[InformationScope::DebuggerValues],
            Duration::from_secs(600),
        )
        .unwrap();
    let mut client = rig.diagnostics(&grant);
    let req = env_request(
        &project.id,
        project.revision,
        "a09-aba-assignment",
        MutationInput::DebuggerAssign(DebuggerAssignInput {
            name: Text::new("$value".into()).unwrap(),
            kind: DebuggerPrimitive::Int,
            value: DebuggerSecret::new("2".into()).unwrap(),
            session_id: id(&first_session),
            session_revision: SafeInt::new(first_revision).unwrap(),
            limits: None,
        }),
    );
    let plan = client.debugger_plan(&req).unwrap().data().unwrap().clone();
    let list = rig.native.pending_reviews().unwrap();
    let review = list
        .iter()
        .find(|review| review.digest == plan.digest)
        .unwrap();
    assert!(
        matches!(&review.target, ReviewTarget::DebuggerAssign{session_id,session_revision,value,..} if session_id==&first_session && *session_revision==first_revision && value=="2")
    );
    rig.native.approve_service_plan(&plan.digest).unwrap();
    let (replacement, replacement_revision) = rig
        .debug_actor
        .test_synthetic_paused_debugger(&project.id)
        .unwrap();
    assert_ne!(first_session, replacement);
    assert_eq!(
        first_revision, replacement_revision,
        "exercise ABA with same pause revision"
    );
    let result = client.debugger_apply(&req, &plan.digest).unwrap();
    assert!(
        !result.is_ok(),
        "approval for first session must not mutate replacement"
    );
    let list = rig.native.pending_reviews().unwrap();
    let review = list
        .iter()
        .find(|review| review.digest == plan.digest)
        .unwrap();
    assert!(matches!(review.state, ReviewState::Stale));
    assert!(
        matches!(&review.target, ReviewTarget::DebuggerAssign {value,..} if value == "[REDACTED]")
    );
}

#[cfg(feature = "test-support")]
#[test]
fn a09_breakpoints_reject_active_session_without_changing_project() {
    use devforge_control::{
        limits::{SafeInt, Text},
        mutations::{DebuggerBreakpoint, DebuggerBreakpointsInput},
    };
    let mut rig = Rig::new(None);
    let project = rig.projects[0].clone();
    let _paused = rig
        .debug_actor
        .test_synthetic_paused_debugger(&project.id)
        .unwrap();
    let grant = rig.grant(0, false);
    let mut client = rig.diagnostics(&grant);
    let before = rig.native.snapshot().unwrap();
    let before = before.snapshot["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["project"]["id"] == project.id)
        .unwrap()["project"]
        .clone();
    let request = env_request(
        &project.id,
        project.revision,
        "a09-active-breakpoint-refusal",
        MutationInput::DebuggerBreakpoints(DebuggerBreakpointsInput {
            breakpoints: vec![DebuggerBreakpoint {
                file: Text::new("public/index.php".into()).unwrap(),
                line: SafeInt::new(1).unwrap(),
            }],
            limits: None,
        }),
    );
    let plan = client
        .debugger_plan(&request)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    let receipt = client
        .debugger_apply(&request, &plan.digest)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    assert_eq!(rig.complete(&grant, &receipt), OperationState::Failed);
    let after = rig.native.snapshot().unwrap();
    let after = after.snapshot["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["project"]["id"] == project.id)
        .unwrap()["project"]
        .clone();
    assert_eq!(after["revision"], before["revision"]);
    assert_eq!(
        after["debugger"]["breakpoints"],
        before["debugger"]["breakpoints"]
    );
}

// A10 cases each start an independent manager and several Git processes.
// Serialize just these expensive fixtures to avoid runner resource starvation;
// no test assertions, product deadline, or non-A10 test concurrency changes.
fn a10_fixture_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[test]
fn a10_workspace_list_and_preparation_are_project_bound_and_noncreating() {
    let _fixture_guard = a10_fixture_guard();
    use devforge_control::tools::{
        WorkspaceListInput, WorkspacePrepareAction, WorkspacePrepareInput,
    };
    let mut rig = Rig::new(None);
    let source = rig.projects[0].clone();
    let foreign = rig.projects[1].clone();
    let owner = rig
        .native
        .issue_external_grant(
            &id(&source.id),
            &[Scope::Observer],
            Duration::from_secs(600),
        )
        .unwrap();
    let mut client = rig.diagnostics(&owner);
    let list = |project: &str, revision| Request {
        api_version: Default::default(),
        request_id: id("a10-list"),
        project_id: id(project),
        expected_revision: revision,
        idempotency_key: None,
        input: WorkspaceListInput {},
    };
    let root = rig.root.path().join("worktrees");
    let before = root.exists();
    let result = client
        .workspace_list(&list(&source.id, Some(source.revision)))
        .unwrap();
    assert!(result.data().is_some());
    assert_eq!(
        root.exists(),
        before,
        "read must not create worktree storage"
    );
    for project in [&foreign.id[..], "missing-project"] {
        let result = client.workspace_list(&list(project, None)).unwrap();
        assert_eq!(result.error().unwrap().code, ErrorCode::ProjectMismatch);
    }
    let stale = client
        .workspace_list(&list(&source.id, Some(source.revision + 1)))
        .unwrap();
    assert_eq!(stale.error().unwrap().code, ErrorCode::RevisionConflict);
    let prep = Request {
        api_version: Default::default(),
        request_id: id("a10-prepare"),
        project_id: id(&source.id),
        expected_revision: Some(source.revision),
        idempotency_key: None,
        input: WorkspacePrepareInput {
            action: WorkspacePrepareAction::Archive,
            workspace_id: Some(id("unowned")),
            name: None,
            branch: None,
            base: None,
            new_branch: None,
            database: None,
            dependencies: None,
            copy_environment: None,
        },
    };
    assert!(client.workspace_prepare(&prep).unwrap().data().is_none());
    assert_eq!(root.exists(), before);
    rig.native
        .revoke_external_grant(&owner.record.grant_id)
        .unwrap();
    let revoked = client
        .workspace_list(&list(&source.id, Some(source.revision)))
        .unwrap();
    assert_eq!(revoked.error().unwrap().code, ErrorCode::GrantRevoked);
    assert_eq!(root.exists(), before);
}

#[test]
fn a10_workspace_create_preview_ticket_is_bound_and_requires_exact_review() {
    let _fixture_guard = a10_fixture_guard();
    use devforge_control::{
        limits::Text,
        mutations::WorkspaceCreateInput,
        tools::{WorkspaceDatabase, WorkspacePrepareAction, WorkspacePrepareInput},
    };
    use devforge_manager::service_control::{ReviewState, ReviewTarget};
    use std::process::Command;
    let mut rig = Rig::new(None);
    rig.runtime_metadata();
    let project = rig.projects[0].clone();
    let source = project.path.clone();
    std::fs::write(source.join("devforge.yaml"), environment_yaml(&rig, 0)).unwrap();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .arg("-C")
            .arg(&source)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_string()
    };
    git(&["init", "-q"]);
    git(&["config", "user.name", "Fixture"]);
    git(&["config", "user.email", "fixture@example.test"]);
    git(&["add", "."]);
    git(&["commit", "-qm", "base"]);
    let head = git(&["rev-parse", "HEAD"]);
    let observer = rig
        .native
        .issue_external_grant(
            &id(&project.id),
            &[Scope::Observer],
            Duration::from_secs(600),
        )
        .unwrap();
    let rights = [Scope::Observer, Scope::ProjectManage, Scope::ExecutionTest];
    let info = [devforge_control::InformationScope::WorkspaceContentScan];
    let full = rig
        .native
        .issue_external_grant_with_information(
            &id(&project.id),
            &rights,
            &info,
            Duration::from_secs(600),
        )
        .unwrap();
    let other = rig
        .native
        .issue_external_grant_with_information(
            &id(&project.id),
            &rights,
            &info,
            Duration::from_secs(600),
        )
        .unwrap();
    let mut client = rig.diagnostics(&full);
    let mut unprivileged = rig.diagnostics(&observer);
    let mut alien = rig.diagnostics(&other);
    let input = WorkspacePrepareInput {
        action: WorkspacePrepareAction::Create,
        workspace_id: None,
        name: Some(Text::new("A10 child".into()).unwrap()),
        branch: Some(Text::new("a10-child".into()).unwrap()),
        base: Some(Text::new("HEAD".into()).unwrap()),
        new_branch: Some(true),
        database: Some(WorkspaceDatabase::Empty),
        dependencies: Some(false),
        copy_environment: Some(false),
    };
    let req = Request {
        api_version: Default::default(),
        request_id: id("a10-preview"),
        project_id: id(&project.id),
        expected_revision: Some(project.revision),
        idempotency_key: None,
        input,
    };
    // An old Observer grant must be denied before inspecting a broken source.
    let gitdir = source.join(".git");
    let held = source.join(".git-fixture-held");
    std::fs::rename(&gitdir, &held).unwrap();
    let denied_before_io = unprivileged.workspace_prepare(&req).unwrap();
    assert_eq!(
        denied_before_io.error().unwrap().code,
        ErrorCode::Unauthorized
    );
    std::fs::rename(&held, &gitdir).unwrap();
    // Secret copy and database clone are independent sensitive authorities.
    // A broken .git makes any attempted source scan fail before a permission gate.
    // The secret-store counter additionally proves no credential lookup happened.
    let mut copy_req = req.clone();
    copy_req.input.copy_environment = Some(true);
    assert!(
        !full
            .record
            .information
            .contains(&devforge_control::InformationScope::WorkspaceSecretCopy)
    );
    let sensitive_scopes = [
        Scope::Observer,
        Scope::ProjectManage,
        Scope::ExecutionTest,
        Scope::SensitiveOperation,
    ];
    let secret_info = [
        devforge_control::InformationScope::WorkspaceContentScan,
        devforge_control::InformationScope::WorkspaceSecretCopy,
    ];
    let sensitive_only = rig
        .native
        .issue_external_grant_with_information(
            &id(&project.id),
            &sensitive_scopes,
            &info,
            Duration::from_secs(600),
        )
        .unwrap();
    let secret_only = rig
        .native
        .issue_external_grant_with_information(
            &id(&project.id),
            &rights,
            &secret_info,
            Duration::from_secs(600),
        )
        .unwrap();
    let authorized = rig
        .native
        .issue_external_grant_with_information(
            &id(&project.id),
            &sensitive_scopes,
            &secret_info,
            Duration::from_secs(600),
        )
        .unwrap();
    let mut sensitive_client = rig.diagnostics(&sensitive_only);
    let mut secret_client = rig.diagnostics(&secret_only);
    let mut authorized_client = rig.diagnostics(&authorized);
    std::fs::rename(&gitdir, &held).unwrap();
    let secret_gets = rig
        .secrets
        .get_count
        .load(std::sync::atomic::Ordering::SeqCst);
    for denied in [&mut client, &mut sensitive_client, &mut secret_client] {
        let result = denied.workspace_prepare(&copy_req).unwrap();
        assert_eq!(result.error().unwrap().code, ErrorCode::Unauthorized);
    }
    assert_eq!(
        rig.secrets
            .get_count
            .load(std::sync::atomic::Ordering::SeqCst),
        secret_gets,
        "denied copy must not read secret store"
    );
    let mut no_sensitive_clone = req.clone();
    no_sensitive_clone.input.database = Some(WorkspaceDatabase::Clone);
    let denied = client.workspace_prepare(&no_sensitive_clone).unwrap();
    assert_eq!(
        denied.error().unwrap().code,
        ErrorCode::Unauthorized,
        "clone denied before Git or DB source inspection"
    );
    std::fs::rename(&held, &gitdir).unwrap();
    let allowed_copy = authorized_client.workspace_prepare(&copy_req).unwrap();
    assert!(
        allowed_copy.data().is_some(),
        "both explicit rights should admit preview: {:?}",
        allowed_copy.error()
    );
    let copy_plan = env_request(
        &project.id,
        project.revision,
        "a10-copy-secret",
        MutationInput::WorkspaceCreate(WorkspaceCreateInput {
            plan: allowed_copy.data().unwrap().plan.clone().unwrap(),
            limits: None,
        }),
    );
    let exact = authorized_client
        .workspace_plan(&copy_plan)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    let review = rig
        .native
        .pending_reviews()
        .unwrap()
        .into_iter()
        .find(|v| v.digest == exact.digest)
        .unwrap();
    assert!(
        matches!(&review.target,ReviewTarget::Workspace{action,copy_environment:Some(true),..} if action=="create")
    );
    assert_eq!(review.state, ReviewState::Pending);
    rig.native.approve_workspace_plan(&exact.digest).unwrap();
    let mut clone_req = req.clone();
    clone_req.input.database = Some(WorkspaceDatabase::Clone);
    let clone_preview = sensitive_client.workspace_prepare(&clone_req).unwrap();
    if let Some(clone_data) = clone_preview.data() {
        assert!(
            clone_data
                .blockers
                .iter()
                .any(|b| b.as_str() == "worktree_sqlite_scope")
        );
        assert!(
            clone_data.plan.is_none(),
            "unowned SQLite must not receive an executable ticket"
        );
    } else {
        assert!(clone_preview.error().is_some());
    }
    let preview = client.workspace_prepare(&req).unwrap();
    assert!(preview.error().is_none(), "{:?}", preview.error());
    let preview = preview.data().unwrap();
    assert!(preview.blockers.is_empty(), "{:?}", preview.blockers);
    let ticket = preview.plan.clone().unwrap();
    let raw = serde_json::to_string(preview).unwrap();
    assert!(!raw.contains(source.to_str().unwrap()));
    let mutation = env_request(
        &project.id,
        project.revision,
        "a10-create",
        MutationInput::WorkspaceCreate(WorkspaceCreateInput {
            plan: ticket.clone(),
            limits: None,
        }),
    );
    let denied = unprivileged.workspace_plan(&mutation).unwrap();
    assert!(matches!(
        denied.error().unwrap().code,
        ErrorCode::Unauthorized
    ));
    let denied = alien.workspace_plan(&mutation).unwrap();
    assert!(matches!(
        denied.error().unwrap().code,
        ErrorCode::Unauthorized | ErrorCode::ReviewMismatch | ErrorCode::ReviewExpired
    ));
    let plan = client
        .workspace_plan(&mutation)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    let pending = rig.native.pending_reviews().unwrap();
    let review = pending.iter().find(|v| v.digest == plan.digest).unwrap();
    assert_eq!(review.state, ReviewState::Pending);
    assert!(matches!(&review.target,ReviewTarget::Workspace{action,..} if action=="create"));
    assert!(
        rig.native.approve_service_plan(&plan.digest).is_err(),
        "generic service approval must not approve workspace plans"
    );
    let still_pending = rig.native.pending_reviews().unwrap();
    assert_eq!(
        still_pending
            .iter()
            .find(|r| r.digest == plan.digest)
            .unwrap()
            .state,
        ReviewState::Pending
    );
    let denied = client.workspace_apply(&mutation, &plan.digest).unwrap();
    assert!(denied.data().is_none());
    assert_eq!(git(&["rev-parse", "HEAD"]), head);
    rig.native.approve_workspace_plan(&plan.digest).unwrap();
    let worktree_root = rig.root.path().join("worktrees");
    let count_receipts = || {
        std::fs::read_dir(&worktree_root)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter(|d| d.path().join("receipt.json").is_file())
            .count()
    };
    let receipts_before = count_receipts();
    std::fs::write(source.join("dirty-canary"), "untouched source").unwrap();
    let stale = client.workspace_apply(&mutation, &plan.digest).unwrap();
    if let Some(receipt) = stale.data() {
        assert_ne!(
            rig.complete(&full, receipt),
            OperationState::Succeeded,
            "stale admitted work must fail before effects"
        );
    } else {
        assert!(stale.error().is_some());
    }
    assert_eq!(
        count_receipts(),
        receipts_before,
        "stale request must not reserve a child"
    );
    assert_eq!(git(&["rev-parse", "HEAD"]), head);
    assert_eq!(
        std::fs::read_to_string(source.join("dirty-canary")).unwrap(),
        "untouched source"
    );
}

#[test]
fn a10_workspace_mutation_rejects_foreign_ids_before_project_lookup() {
    let _fixture_guard = a10_fixture_guard();
    use devforge_control::{Digest, mutations::WorkspaceCreateInput};
    let mut rig = Rig::new(None);
    let own = rig.projects[0].clone();
    let foreign = rig.projects[1].clone();
    let grant = rig.grant(0, true);
    let mut client = rig.diagnostics(&grant);
    let before = rig.root.path().join("worktrees").exists();
    let ticket = Digest::new("a".repeat(64)).unwrap();
    for project in [&foreign.id[..], "missing-project"] {
        let request = env_request(
            project,
            own.revision,
            "a10-foreign",
            MutationInput::WorkspaceCreate(WorkspaceCreateInput {
                plan: ticket.clone(),
                limits: None,
            }),
        );
        let plan = client.workspace_plan(&request).unwrap();
        assert_eq!(
            plan.error().unwrap().code,
            ErrorCode::ProjectMismatch,
            "existing and missing foreign IDs must not be distinguishable"
        );
        let applied = client.workspace_apply(&request, &ticket).unwrap();
        assert_eq!(applied.error().unwrap().code, ErrorCode::ProjectMismatch);
        let found = client
            .workspace_lookup(id("a10-foreign-lookup"), &id(project), &id("a10-foreign"))
            .unwrap();
        assert_eq!(found.error().unwrap().code, ErrorCode::ProjectMismatch);
    }
    assert_eq!(rig.root.path().join("worktrees").exists(), before);
}

#[test]
fn a10_lifecycle_rejects_symlinked_workspace_outside_owned_root() {
    let _fixture_guard = a10_fixture_guard();
    use devforge_control::{
        InformationScope,
        tools::{WorkspacePrepareAction, WorkspacePrepareInput},
    };
    let mut rig = Rig::new(None);
    let project = rig.projects[0].clone();
    let grant = rig
        .native
        .issue_external_grant_with_information(
            &id(&project.id),
            &[
                Scope::Observer,
                Scope::ProjectManage,
                Scope::SensitiveOperation,
            ],
            &[InformationScope::WorkspaceContentScan],
            Duration::from_secs(600),
        )
        .unwrap();
    let worktrees = rig.root.path().join("worktrees");
    std::fs::create_dir_all(&worktrees).unwrap();
    let outside = rig.root.path().join("outside-canary");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("receipt.json"), b"A10_UNTRUSTED_CANARY").unwrap();
    let name = "f".repeat(32);
    let child = worktrees.join(&name);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &child).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Paths are supplied only as environment values, never interpolated in
        // PowerShell source; this creates a real, unprivileged directory junction.
        const CREATE: &str = "$ErrorActionPreference='Stop'; Microsoft.PowerShell.Management\\New-Item -ItemType Junction -Path $env:DEVFORGE_TEST_JUNCTION_LINK -Target $env:DEVFORGE_TEST_JUNCTION_TARGET -ErrorAction Stop | Out-Null";
        let powershell =
            std::path::PathBuf::from(std::env::var_os("SystemRoot").expect("SystemRoot"))
                .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let output = std::process::Command::new(powershell)
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                CREATE,
            ])
            .env("DEVFORGE_TEST_JUNCTION_LINK", &child)
            .env("DEVFORGE_TEST_JUNCTION_TARGET", &outside)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "junction fixture creation failed: {:?}",
            output.status.code()
        );
        assert_ne!(
            std::fs::symlink_metadata(&child).unwrap().file_attributes() & 0x400,
            0
        );
        assert_eq!(
            std::fs::canonicalize(&child).unwrap(),
            std::fs::canonicalize(&outside).unwrap()
        );
    }
    #[cfg(not(any(unix, windows)))]
    std::fs::create_dir(&child).unwrap();
    let mut client = rig.diagnostics(&grant);
    for action in [
        WorkspacePrepareAction::Archive,
        WorkspacePrepareAction::Restore,
        WorkspacePrepareAction::Purge,
    ] {
        let req = Request {
            api_version: Default::default(),
            request_id: id("a10-no-unsafe-scan"),
            project_id: id(&project.id),
            expected_revision: Some(project.revision),
            idempotency_key: None,
            input: WorkspacePrepareInput {
                action,
                workspace_id: Some(id(&name)),
                name: None,
                branch: None,
                base: None,
                new_branch: None,
                database: None,
                dependencies: None,
                copy_environment: None,
            },
        };
        let response = client.workspace_prepare(&req).unwrap();
        let fault = response
            .error()
            .expect("unowned or symlinked workspace must be refused");
        assert_ne!(
            fault.code,
            ErrorCode::UnsupportedCapability,
            "lifecycle is implemented"
        );
    }
    assert_eq!(
        std::fs::read(outside.join("receipt.json")).unwrap(),
        b"A10_UNTRUSTED_CANARY"
    );
    #[cfg(unix)]
    assert!(
        std::fs::symlink_metadata(&child)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        assert_ne!(
            std::fs::symlink_metadata(&child).unwrap().file_attributes() & 0x400,
            0
        );
        // Remove the junction itself, never the target directory.
        std::fs::remove_dir(&child).unwrap();
        assert_eq!(
            std::fs::read(outside.join("receipt.json")).unwrap(),
            b"A10_UNTRUSTED_CANARY"
        );
    }
}

#[test]
fn a10_delayed_workspace_preview_returns_no_ticket_and_does_not_mutate() {
    let _fixture_guard = a10_fixture_guard();
    use devforge_control::{
        InformationScope,
        limits::Text,
        tools::{WorkspaceDatabase, WorkspacePrepareAction, WorkspacePrepareInput},
    };
    let mut rig = Rig::new(None);
    let project = rig.projects[0].clone();
    let source = project.path.clone();
    let marker = source.join("a10-delayed-canary");
    std::fs::write(&marker, b"unchanged").unwrap();
    let grant = rig
        .native
        .issue_external_grant_with_information(
            &id(&project.id),
            &[Scope::Observer],
            &[InformationScope::WorkspaceContentScan],
            Duration::from_secs(600),
        )
        .unwrap();
    let request = Request {
        api_version: Default::default(),
        request_id: id("a10-delayed-preview"),
        project_id: id(&project.id),
        expected_revision: Some(project.revision),
        idempotency_key: None,
        input: WorkspacePrepareInput {
            action: WorkspacePrepareAction::Create,
            workspace_id: None,
            name: Some(Text::new("Child".into()).unwrap()),
            branch: Some(Text::new("a10-delayed".into()).unwrap()),
            base: Some(Text::new("HEAD".into()).unwrap()),
            new_branch: Some(true),
            database: Some(WorkspaceDatabase::Empty),
            dependencies: Some(false),
            copy_environment: Some(false),
        },
    };
    let barrier_grant = rig
        .native
        .issue_external_grant(
            &id(&project.id),
            &[Scope::Observer],
            Duration::from_secs(600),
        )
        .unwrap();
    let root = rig.root.path().join("worktrees");
    let existed = root.exists();
    let release = rig.block();
    let endpoint = rig.endpoint.clone();
    let token = grant.credential.expose_secret().to_owned();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let caller = thread::spawn(move || {
        let credential = ExternalCredential::from_wire(&token).unwrap();
        let mut client = DiagnosticsClient::connect(endpoint, credential).unwrap();
        ready_tx.send(()).unwrap();
        client.workspace_prepare(&request)
    });
    ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    // Revoke before releasing the blocked actor; a delayed request must not
    // obtain a ticket or resume with the previously valid authority.
    rig.native
        .revoke_external_grant(&grant.record.grant_id)
        .unwrap();
    drop(release);
    let result = caller.join().unwrap();
    match result {
        Ok(reply) => assert!(
            reply.data().is_none(),
            "revoked delayed request must not issue a ticket"
        ),
        Err(error) => assert!(matches!(
            error,
            ErrorCode::GrantRevoked
                | ErrorCode::Unauthorized
                | ErrorCode::Busy
                | ErrorCode::Interrupted
        )),
    }
    // A subsequent actor request is a barrier after the delayed item.
    let mut client = rig.diagnostics(&barrier_grant);
    let list = Request {
        api_version: Default::default(),
        request_id: id("a10-preview-barrier"),
        project_id: id(&project.id),
        expected_revision: Some(project.revision),
        idempotency_key: None,
        input: devforge_control::tools::WorkspaceListInput {},
    };
    let barrier = client.workspace_list(&list).unwrap();
    assert!(
        barrier.data().is_some(),
        "actor must recover after delayed preview: {:?}",
        barrier.error()
    );
    assert_eq!(std::fs::read(&marker).unwrap(), b"unchanged");
    assert_eq!(
        root.exists(),
        existed,
        "revoked delayed preview must not create a worktree"
    );
}

// Seed an owned Git worktree before starting the manager; no test-only IPC or
// production authority bypass is required. All paths belong to this temporary Rig.
fn seed_a10_owned_worktree(root: &Path, projects: &mut Vec<devforge_projects::project::Project>) {
    use devforge_workspace::{
        Workspace,
        worktrees::{DatabaseMode, Input, Record},
    };
    use std::process::Command;
    let mut w = Workspace::open(root, Arc::new(Memory::default())).unwrap();
    let original = projects[0].clone();
    w.execute(
        devforge_workspace::Command::Configure {
            id: original.id.clone(),
            revision: original.revision,
            name: original.name.clone(),
            php: original.php.clone(),
            node: original.node.clone(),
            domain: "alpha.devforge.test".into(),
            https: false,
            vite: false,
            queue: false,
        },
        &Default::default(),
    )
    .unwrap();
    let source = w.projects.get(&original.id).unwrap();
    projects[0] = source.clone();
    let git = |path: &Path, args: &[&str]| {
        let out = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    std::fs::create_dir_all(source.path.join("database")).unwrap();
    std::fs::write(source.path.join("database/.gitkeep"), "").unwrap();
    std::fs::write(
        source.path.join(".env.example"),
        "APP_KEY=\nDB_CONNECTION=sqlite\n",
    )
    .unwrap();
    std::fs::write(
        source.path.join(".gitignore"),
        ".env\n/database/*.sqlite\n/vendor\n/node_modules\n",
    )
    .unwrap();
    git(&source.path, &["init", "-q"]);
    git(&source.path, &["config", "user.name", "Fixture"]);
    git(
        &source.path,
        &["config", "user.email", "fixture@example.test"],
    );
    let php = w
        .catalog
        .get(&source.php)
        .unwrap()
        .version
        .split('-')
        .next()
        .unwrap();
    let node = w
        .catalog
        .get(&source.node)
        .unwrap()
        .version
        .trim_start_matches('v');
    w.manifest_init(&source.path, php, node, "2.11.2").unwrap();
    git(&source.path, &["add", "."]);
    git(&source.path, &["commit", "-qm", "fixture base"]);
    let input = Input {
        source_id: source.id.clone(),
        name: "A10 owned".into(),
        branch: "a10-owned".into(),
        base: "HEAD".into(),
        new_branch: true,
        database: DatabaseMode::Empty,
        dependencies: false,
        copy_environment: false,
    };
    let plan = w.worktree_plan(&input).unwrap();
    assert!(plan.blockers.is_empty(), "{:?}", plan.blockers);
    let wid = "a".repeat(32);
    let dir = root.join("worktrees").join(&wid);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tree");
    git(
        &source.path,
        &[
            "worktree",
            "add",
            "-qb",
            &input.branch,
            path.to_str().unwrap(),
        ],
    );
    let child = w
        .projects
        .import(&path, &input.name, &source.php, &source.node, &w.catalog)
        .unwrap();
    w.projects
        .claim_worktree(&child.id, child.revision, &wid)
        .unwrap();
    let child = w.projects.get(&child.id).unwrap();
    let record = Record {
        isolated: false,
        schema: 1,
        id: wid,
        input,
        source_path: source.path.clone(),
        common_dir: std::fs::canonicalize(source.path.join(".git")).unwrap(),
        git_dir: Some(
            std::fs::canonicalize(git(&path, &["rev-parse", "--absolute-git-dir"])).unwrap(),
        ),
        commit: plan.commit.clone(),
        domain: plan.domain,
        review_digest: plan.digest,
        project_id: Some(child.id.clone()),
        stage: "ready".into(),
        last_error: None,
        configuration: plan.configuration,
    };
    std::fs::write(
        dir.join("receipt.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    // New committed source change creates an exact fast-forward opportunity.
    std::fs::write(source.path.join("a10-new.txt"), "committed source\n").unwrap();
    git(&source.path, &["add", "a10-new.txt"]);
    git(&source.path, &["commit", "-qm", "source change"]);
    std::fs::write(source.path.join("dirty-canary"), "source remains untouched").unwrap();
    projects.push(child);
}

fn a10_lifecycle_action(
    rig: &mut Rig,
    grant: &IssuedGrant,
    workspace: &str,
    action: devforge_control::tools::WorkspacePrepareAction,
    key: &str,
) -> OperationState {
    use devforge_control::{
        mutations::WorkspaceLifecycleInput,
        tools::{WorkspacePrepareAction as A, WorkspacePrepareInput},
    };
    let project = rig.projects[0].clone();
    let mut client = rig.diagnostics(grant);
    let prepare = Request {
        api_version: Default::default(),
        request_id: id(&format!("{key}-prepare")),
        project_id: id(&project.id),
        expected_revision: Some(project.revision),
        idempotency_key: None,
        input: WorkspacePrepareInput {
            action,
            workspace_id: Some(id(workspace)),
            name: None,
            branch: None,
            base: None,
            new_branch: None,
            database: None,
            dependencies: None,
            copy_environment: None,
        },
    };
    let reply = client.workspace_prepare(&prepare).unwrap();
    assert!(
        reply.error().is_none(),
        "{key} prepare: {:?}",
        reply.error()
    );
    let preview = reply.data().unwrap();
    assert!(
        preview.blockers.is_empty(),
        "{key} blockers {:?}",
        preview.blockers
    );
    let param = WorkspaceLifecycleInput {
        workspace_id: id(workspace),
        plan: preview.plan.clone().unwrap(),
        limits: None,
    };
    let input = match action {
        A::Archive => MutationInput::WorkspaceArchive(param),
        A::Restore => MutationInput::WorkspaceRestore(param),
        A::Purge => MutationInput::WorkspacePurge(param),
        A::Merge => MutationInput::WorkspaceMerge(param),
        _ => panic!("unexpected action"),
    };
    let request = env_request(&project.id, project.revision, key, input);
    let reply = client.workspace_plan(&request).unwrap();
    assert!(reply.error().is_none(), "{key} plan: {:?}", reply.error());
    let plan = reply.data().unwrap().clone();
    let pending = rig.native.pending_reviews().unwrap();
    let summary = pending
        .iter()
        .find(|s| s.digest == plan.digest)
        .expect("exact native review");
    assert_eq!(
        summary.state,
        devforge_manager::service_control::ReviewState::Pending
    );
    let no_approval = client.workspace_apply(&request, &plan.digest).unwrap();
    assert!(
        no_approval.data().is_none(),
        "{key} must require native review"
    );
    rig.native.approve_workspace_plan(&plan.digest).unwrap();
    let applied = client.workspace_apply(&request, &plan.digest).unwrap();
    assert!(
        applied.error().is_none(),
        "{key} apply: {:?}",
        applied.error()
    );
    rig.complete(grant, applied.data().unwrap())
}

#[test]
fn a10_owned_archive_restore_purge_preserves_source_and_ignored_data() {
    let _fixture_guard = a10_fixture_guard();
    use devforge_control::{InformationScope, tools::WorkspacePrepareAction as A};
    let mut rig = Rig::runtime_setup_with(None, false, false, seed_a10_owned_worktree);
    let source = rig.projects[0].clone();
    let child = rig.projects[2].clone();
    let wid = child.worktree.as_ref().unwrap().clone();
    let target = rig.root.path().join("worktrees").join(&wid).join("tree");
    let retained = rig
        .root
        .path()
        .join("worktrees")
        .join(&wid)
        .join("retained");
    let git = |path: &Path, args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let source_head = git(&source.path, &["rev-parse", "HEAD"]);
    let source_status = git(&source.path, &["status", "--porcelain"]);
    std::fs::write(target.join(".env"), "PRIVATE=kept").unwrap();
    let grant = rig
        .native
        .issue_external_grant_with_information(
            &id(&source.id),
            &[
                Scope::Observer,
                Scope::ProjectManage,
                Scope::ExecutionTest,
                Scope::SensitiveOperation,
            ],
            &[InformationScope::WorkspaceContentScan],
            Duration::from_secs(600),
        )
        .unwrap();
    assert_eq!(
        a10_lifecycle_action(&mut rig, &grant, &wid, A::Archive, "a10-archive-1"),
        OperationState::Succeeded
    );
    assert!(!target.exists() && retained.exists());
    assert_eq!(
        std::fs::read_to_string(retained.join(".env")).unwrap(),
        "PRIVATE=kept"
    );
    assert_eq!(
        a10_lifecycle_action(&mut rig, &grant, &wid, A::Restore, "a10-restore"),
        OperationState::Succeeded
    );
    assert!(target.exists() && !retained.exists());
    assert_eq!(
        std::fs::read_to_string(target.join(".env")).unwrap(),
        "PRIVATE=kept"
    );
    assert_eq!(
        a10_lifecycle_action(&mut rig, &grant, &wid, A::Archive, "a10-archive-2"),
        OperationState::Succeeded
    );
    assert_eq!(
        a10_lifecycle_action(&mut rig, &grant, &wid, A::Purge, "a10-purge"),
        OperationState::Succeeded
    );
    assert!(!target.exists() && !retained.exists());
    assert_eq!(git(&source.path, &["rev-parse", "HEAD"]), source_head);
    assert_eq!(git(&source.path, &["status", "--porcelain"]), source_status);
    assert_eq!(
        std::fs::read_to_string(source.path.join("dirty-canary")).unwrap(),
        "source remains untouched"
    );
}

#[test]
fn a10_merge_fast_forwards_owned_worktree_and_preserves_source() {
    let _fixture_guard = a10_fixture_guard();
    use devforge_control::{
        InformationScope,
        mutations::WorkspaceLifecycleInput,
        tools::{WorkspaceMergeEffect, WorkspacePrepareAction as A, WorkspacePrepareInput},
    };
    let mut rig = Rig::runtime_setup_with(None, false, false, seed_a10_owned_worktree);
    let source = rig.projects[0].clone();
    let child = rig.projects[2].clone();
    let wid = child.worktree.as_ref().unwrap().clone();
    let target = rig.root.path().join("worktrees").join(&wid).join("tree");
    let git = |path: &Path, args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let source_head = git(&source.path, &["rev-parse", "HEAD"]);
    let target_head = git(&target, &["rev-parse", "HEAD"]);
    assert_ne!(source_head, target_head);
    let source_ref = git(&source.path, &["symbolic-ref", "HEAD"]);
    let source_status = git(&source.path, &["status", "--porcelain"]);
    let index = git(&source.path, &["write-tree"]);
    let rights = [
        Scope::Observer,
        Scope::ProjectManage,
        Scope::ExecutionTest,
        Scope::SensitiveOperation,
    ];
    let grant = rig
        .native
        .issue_external_grant_with_information(
            &id(&source.id),
            &rights,
            &[InformationScope::WorkspaceContentScan],
            Duration::from_secs(600),
        )
        .unwrap();
    let missing = rig
        .native
        .issue_external_grant(&id(&source.id), &rights, Duration::from_secs(600))
        .unwrap();
    let mut client = rig.diagnostics(&grant);
    let mut denied = rig.diagnostics(&missing);
    let prepare = Request {
        api_version: Default::default(),
        request_id: id("a10-merge-preview"),
        project_id: id(&source.id),
        expected_revision: Some(source.revision),
        idempotency_key: None,
        input: WorkspacePrepareInput {
            action: A::Merge,
            workspace_id: Some(id(&wid)),
            name: None,
            branch: None,
            base: None,
            new_branch: None,
            database: None,
            dependencies: None,
            copy_environment: None,
        },
    };
    assert_eq!(
        denied
            .workspace_prepare(&prepare)
            .unwrap()
            .error()
            .unwrap()
            .code,
        ErrorCode::Unauthorized
    );
    let dirty = target.join("untracked-guard");
    std::fs::write(&dirty, b"dirty").unwrap();
    let blocked = client.workspace_prepare(&prepare).unwrap();
    assert!(
        blocked.data().is_some(),
        "dirty preview: {:?}",
        blocked.error()
    );
    let blocked = blocked.data().unwrap();
    assert!(
        blocked
            .blockers
            .iter()
            .any(|v| v.as_str() == "worktree_dirty")
    );
    assert!(blocked.plan.is_none());
    std::fs::remove_file(&dirty).unwrap();
    let preview = client.workspace_prepare(&prepare).unwrap();
    assert!(preview.error().is_none(), "{:?}", preview.error());
    let preview = preview.data().unwrap();
    assert!(preview.blockers.is_empty(), "{:?}", preview.blockers);
    let merge = preview.merge.as_ref().unwrap();
    assert_eq!(merge.effect, WorkspaceMergeEffect::FastForward);
    assert_eq!(merge.source_head.as_str(), source_head);
    assert_eq!(merge.target_head.as_str(), target_head);
    let request = env_request(
        &source.id,
        source.revision,
        "a10-merge-approved",
        MutationInput::WorkspaceMerge(WorkspaceLifecycleInput {
            workspace_id: id(&wid),
            plan: preview.plan.clone().unwrap(),
            limits: None,
        }),
    );
    let plan = client
        .workspace_plan(&request)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    assert!(
        client
            .workspace_apply(&request, &plan.digest)
            .unwrap()
            .data()
            .is_none(),
        "native review mandatory"
    );
    rig.native.approve_workspace_plan(&plan.digest).unwrap();
    let reply = client.workspace_apply(&request, &plan.digest).unwrap();
    assert!(reply.error().is_none(), "{:?}", reply.error());
    assert_eq!(
        rig.complete(&grant, reply.data().unwrap()),
        OperationState::Succeeded
    );
    assert_eq!(git(&target, &["rev-parse", "HEAD"]), source_head);
    assert_eq!(git(&source.path, &["rev-parse", "HEAD"]), source_head);
    assert_eq!(git(&source.path, &["symbolic-ref", "HEAD"]), source_ref);
    assert_eq!(git(&source.path, &["status", "--porcelain"]), source_status);
    assert_eq!(git(&source.path, &["write-tree"]), index);
    assert_eq!(
        std::fs::read_to_string(source.path.join("dirty-canary")).unwrap(),
        "source remains untouched"
    );
}

#[test]
fn a10_merge_refuses_overwriting_ignored_file_and_reports_incomplete() {
    let _fixture_guard = a10_fixture_guard();
    use devforge_control::{
        InformationScope,
        mutations::WorkspaceLifecycleInput,
        tools::{WorkspacePrepareAction as A, WorkspacePrepareInput},
    };
    let mut rig = Rig::runtime_setup_with(None, false, false, seed_a10_owned_worktree);
    let source = rig.projects[0].clone();
    let child = rig.projects[2].clone();
    let wid = child.worktree.as_ref().unwrap().clone();
    let target = rig.root.path().join("worktrees").join(&wid).join("tree");
    let git = |path: &Path, args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    // Source explicitly commits an ignored name; target has a private ignored
    // file at that name. Git must not overwrite the target when fast-forwarding.
    std::fs::write(source.path.join(".env"), "FIXTURE_SOURCE=1").unwrap();
    git(&source.path, &["add", "-f", ".env"]);
    git(&source.path, &["commit", "-qm", "track fixture env"]);
    std::fs::write(target.join(".env"), "TARGET_PRIVATE=preserve").unwrap();
    let source_head = git(&source.path, &["rev-parse", "HEAD"]);
    let target_head = git(&target, &["rev-parse", "HEAD"]);
    let grant = rig
        .native
        .issue_external_grant_with_information(
            &id(&source.id),
            &[
                Scope::Observer,
                Scope::ProjectManage,
                Scope::ExecutionTest,
                Scope::SensitiveOperation,
            ],
            &[InformationScope::WorkspaceContentScan],
            Duration::from_secs(600),
        )
        .unwrap();
    let mut client = rig.diagnostics(&grant);
    let req = Request {
        api_version: Default::default(),
        request_id: id("a10-ignored-preview"),
        project_id: id(&source.id),
        expected_revision: Some(source.revision),
        idempotency_key: None,
        input: WorkspacePrepareInput {
            action: A::Merge,
            workspace_id: Some(id(&wid)),
            name: None,
            branch: None,
            base: None,
            new_branch: None,
            database: None,
            dependencies: None,
            copy_environment: None,
        },
    };
    let preview = client.workspace_prepare(&req).unwrap();
    assert!(preview.error().is_none(), "{:?}", preview.error());
    let preview = preview.data().unwrap();
    assert!(preview.blockers.is_empty(), "{:?}", preview.blockers);
    let request = env_request(
        &source.id,
        source.revision,
        "a10-ignored-merge",
        MutationInput::WorkspaceMerge(WorkspaceLifecycleInput {
            workspace_id: id(&wid),
            plan: preview.plan.clone().unwrap(),
            limits: None,
        }),
    );
    let plan = client
        .workspace_plan(&request)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    rig.native.approve_workspace_plan(&plan.digest).unwrap();
    let applied = client.workspace_apply(&request, &plan.digest).unwrap();
    assert!(applied.error().is_none(), "{:?}", applied.error());
    let receipt = applied.data().unwrap().clone();
    assert_eq!(rig.complete(&grant, &receipt), OperationState::Failed);
    let status = rig
        .native
        .external_operation_status(&grant.record.grant_id, &receipt.operation_id)
        .unwrap();
    assert_eq!(status.code.as_deref(), Some("worktree_merge_incomplete"));
    assert_eq!(
        std::fs::read_to_string(target.join(".env")).unwrap(),
        "TARGET_PRIVATE=preserve"
    );
    assert_eq!(git(&target, &["rev-parse", "HEAD"]), target_head);
    assert_eq!(git(&source.path, &["rev-parse", "HEAD"]), source_head);
}

#[test]
fn a10_merge_cancel_and_revoke_before_worker_preserve_both_checkouts() {
    let _fixture_guard = a10_fixture_guard();
    use devforge_control::{
        InformationScope,
        mutations::WorkspaceLifecycleInput,
        tools::{WorkspacePrepareAction as A, WorkspacePrepareInput},
    };
    let mut rig = Rig::runtime_setup_with(None, false, false, seed_a10_owned_worktree);
    let source = rig.projects[0].clone();
    let child = rig.projects[2].clone();
    let wid = child.worktree.as_ref().unwrap().clone();
    let target = rig.root.path().join("worktrees").join(&wid).join("tree");
    let git = |path: &Path, args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let source_head = git(&source.path, &["rev-parse", "HEAD"]);
    let target_head = git(&target, &["rev-parse", "HEAD"]);
    let source_status = git(&source.path, &["status", "--porcelain"]);
    let grant = rig
        .native
        .issue_external_grant_with_information(
            &id(&source.id),
            &[
                Scope::Observer,
                Scope::ProjectManage,
                Scope::ExecutionTest,
                Scope::SensitiveOperation,
            ],
            &[InformationScope::WorkspaceContentScan],
            Duration::from_secs(600),
        )
        .unwrap();
    let mut client = rig.diagnostics(&grant);
    let prepare = Request {
        api_version: Default::default(),
        request_id: id("a10-cancel-preview"),
        project_id: id(&source.id),
        expected_revision: Some(source.revision),
        idempotency_key: None,
        input: WorkspacePrepareInput {
            action: A::Merge,
            workspace_id: Some(id(&wid)),
            name: None,
            branch: None,
            base: None,
            new_branch: None,
            database: None,
            dependencies: None,
            copy_environment: None,
        },
    };
    let preview = client.workspace_prepare(&prepare).unwrap();
    assert!(preview.error().is_none(), "{:?}", preview.error());
    let request = env_request(
        &source.id,
        source.revision,
        "a10-cancel-merge",
        MutationInput::WorkspaceMerge(WorkspaceLifecycleInput {
            workspace_id: id(&wid),
            plan: preview.data().unwrap().plan.clone().unwrap(),
            limits: None,
        }),
    );
    let plan = client
        .workspace_plan(&request)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    rig.native.approve_workspace_plan(&plan.digest).unwrap();
    let release = rig.block();
    let reply = client.workspace_apply(&request, &plan.digest).unwrap();
    assert!(reply.error().is_none(), "{:?}", reply.error());
    let receipt = reply.data().unwrap().clone();
    let cancel = rig
        .external(&grant)
        .request(
            id("a10-cancel"),
            ExternalInput::OperationCancel {
                project_id: id(&source.id),
                operation_id: receipt.operation_id.clone(),
            },
            None,
        )
        .unwrap();
    assert!(cancel.ok);
    drop(release);
    assert_eq!(rig.complete(&grant, &receipt), OperationState::Cancelled);
    assert_eq!(git(&target, &["rev-parse", "HEAD"]), target_head);
    assert_eq!(git(&source.path, &["rev-parse", "HEAD"]), source_head);
    assert_eq!(git(&source.path, &["status", "--porcelain"]), source_status);
    let preview = client
        .workspace_prepare(&prepare)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    let request = env_request(
        &source.id,
        source.revision,
        "a10-revoke-merge",
        MutationInput::WorkspaceMerge(WorkspaceLifecycleInput {
            workspace_id: id(&wid),
            plan: preview.plan.unwrap(),
            limits: None,
        }),
    );
    let plan = client
        .workspace_plan(&request)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    rig.native.approve_workspace_plan(&plan.digest).unwrap();
    let release = rig.block();
    let reply = client.workspace_apply(&request, &plan.digest).unwrap();
    assert!(reply.error().is_none(), "{:?}", reply.error());
    let receipt = reply.data().unwrap().clone();
    rig.native
        .revoke_external_grant(&grant.record.grant_id)
        .unwrap();
    drop(release);
    let until = Instant::now() + Duration::from_secs(8);
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
        assert!(Instant::now() < until, "revoked operation remained {state}");
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(git(&target, &["rev-parse", "HEAD"]), target_head);
    assert_eq!(git(&source.path, &["rev-parse", "HEAD"]), source_head);
    assert_eq!(git(&source.path, &["status", "--porcelain"]), source_status);
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires explicit DEVFORGE_SERVICE_CACHE immutable macOS runtime cache"]
fn a10_native_reviewed_create_and_sqlite_clone_preserve_source_canaries() {
    let _fixture_guard = a10_fixture_guard();
    use devforge_control::{
        InformationScope,
        limits::Text,
        mutations::WorkspaceCreateInput,
        tools::{WorkspaceDatabase, WorkspacePrepareAction as A, WorkspacePrepareInput},
    };
    let cache = std::path::PathBuf::from(
        std::env::var_os("DEVFORGE_SERVICE_CACHE").expect("explicit immutable cache"),
    );
    let mut rig = Rig::new(Some(&cache));
    let source = rig.projects[0].clone();
    std::fs::create_dir_all(source.path.join("database")).unwrap();
    std::fs::write(source.path.join("database/.gitkeep"), "").unwrap();
    std::fs::write(
        source.path.join(".env.example"),
        "APP_KEY=\nDB_CONNECTION=sqlite\n",
    )
    .unwrap();
    std::fs::write(
        source.path.join(".gitignore"),
        ".env\n/database/*.sqlite\n/vendor\n/node_modules\n",
    )
    .unwrap();
    std::fs::write(source.path.join("devforge.yaml"), environment_yaml(&rig, 0)).unwrap();
    std::fs::write(
        source.path.join(".env"),
        "APP_KEY=base64:SOURCE_CANARY\nDB_CONNECTION=sqlite\n",
    )
    .unwrap();
    let source_db = source.path.join("database/database.sqlite");
    let db = rusqlite::Connection::open(&source_db).unwrap();
    db.execute_batch("CREATE TABLE fixture_canary(value TEXT); INSERT INTO fixture_canary VALUES('SOURCE_DB_CANARY');").unwrap();
    drop(db);
    let source_db_bytes = std::fs::read(&source_db).unwrap();
    std::fs::write(source.path.join("database/other.sqlite"), "OTHER_DB_CANARY").unwrap();
    let git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&source.path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    git(&["init", "-q"]);
    git(&["config", "user.name", "Fixture"]);
    git(&["config", "user.email", "fixture@example.test"]);
    git(&["add", "."]);
    git(&["commit", "-qm", "source base"]);
    std::fs::write(source.path.join("dirty-canary"), "SOURCE_DIRTY_CANARY").unwrap();
    let head = git(&["rev-parse", "HEAD"]);
    let status = git(&["status", "--porcelain"]);
    let grant = rig
        .native
        .issue_external_grant_with_information(
            &id(&source.id),
            &[Scope::Observer, Scope::ProjectManage, Scope::ExecutionTest],
            &[InformationScope::WorkspaceContentScan],
            Duration::from_secs(600),
        )
        .unwrap();
    let mut client = rig.diagnostics(&grant);
    let preview = Request {
        api_version: Default::default(),
        request_id: id("a10-native-preview"),
        project_id: id(&source.id),
        expected_revision: Some(source.revision),
        idempotency_key: None,
        input: WorkspacePrepareInput {
            action: A::Create,
            workspace_id: None,
            name: Some(Text::new("Native child".into()).unwrap()),
            branch: Some(Text::new("a10-native-child".into()).unwrap()),
            base: Some(Text::new("HEAD".into()).unwrap()),
            new_branch: Some(true),
            database: Some(WorkspaceDatabase::Empty),
            dependencies: Some(false),
            copy_environment: Some(false),
        },
    };
    let result = client.workspace_prepare(&preview).unwrap();
    assert!(result.error().is_none(), "{:?}", result.error());
    let preview = result.data().unwrap();
    assert!(preview.blockers.is_empty(), "{:?}", preview.blockers);
    let request = env_request(
        &source.id,
        source.revision,
        "a10-native-create",
        MutationInput::WorkspaceCreate(WorkspaceCreateInput {
            plan: preview.plan.clone().unwrap(),
            limits: None,
        }),
    );
    let result = client.workspace_plan(&request).unwrap();
    assert!(result.error().is_none(), "{:?}", result.error());
    let plan = result.data().unwrap().clone();
    rig.native.approve_workspace_plan(&plan.digest).unwrap();
    let result = client.workspace_apply(&request, &plan.digest).unwrap();
    assert!(result.error().is_none(), "{:?}", result.error());
    let receipt = result.data().unwrap().clone();
    let state = rig.complete(&grant, &receipt);
    if state != OperationState::Succeeded {
        let summaries = std::fs::read_dir(rig.root.path().join("worktrees"))
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|d| std::fs::read(d.path().join("receipt.json")).ok())
            .filter_map(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .map(|v| {
                (
                    v["stage"].as_str().unwrap_or("unknown").to_owned(),
                    v["last_error"].as_str().unwrap_or("none").to_owned(),
                )
            })
            .collect::<Vec<_>>();
        panic!("native fixture operation {state:?}; safe stages: {summaries:?}");
    }
    let root = rig.root.path().join("worktrees");
    let records = std::fs::read_dir(root)
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|d| std::fs::read(d.path().join("receipt.json")).ok())
        .filter_map(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["stage"], "ready");
    assert_eq!(records[0]["input"]["database"], "empty");
    assert_eq!(git(&["rev-parse", "HEAD"]), head);
    assert_eq!(git(&["status", "--porcelain"]), status);
    assert_eq!(
        std::fs::read_to_string(source.path.join(".env")).unwrap(),
        "APP_KEY=base64:SOURCE_CANARY\nDB_CONNECTION=sqlite\n"
    );
    assert_eq!(std::fs::read(&source_db).unwrap(), source_db_bytes);
    assert_eq!(
        std::fs::read_to_string(source.path.join("database/other.sqlite")).unwrap(),
        "OTHER_DB_CANARY"
    );
    assert_eq!(
        std::fs::read_to_string(source.path.join("dirty-canary")).unwrap(),
        "SOURCE_DIRTY_CANARY"
    );
    // Mark only this fixture's conventional source SQLite as DevForge-owned.
    let setup = rig.root.path().join("projects/setup-owned");
    std::fs::create_dir_all(&setup).unwrap();
    std::fs::write(
        setup.join(format!("{}.json", source.id)),
        r#"{"sqlite_complete":true}"#,
    )
    .unwrap();
    let clone_grant = rig
        .native
        .issue_external_grant_with_information(
            &id(&source.id),
            &[
                Scope::Observer,
                Scope::ProjectManage,
                Scope::ExecutionTest,
                Scope::SensitiveOperation,
            ],
            &[InformationScope::WorkspaceContentScan],
            Duration::from_secs(600),
        )
        .unwrap();
    let mut clone_client = rig.diagnostics(&clone_grant);
    let clone_preview = Request {
        api_version: Default::default(),
        request_id: id("a10-native-clone-preview"),
        project_id: id(&source.id),
        expected_revision: Some(source.revision),
        idempotency_key: None,
        input: WorkspacePrepareInput {
            action: A::Create,
            workspace_id: None,
            name: Some(Text::new("Clone child".into()).unwrap()),
            branch: Some(Text::new("a10-native-clone".into()).unwrap()),
            base: Some(Text::new("HEAD".into()).unwrap()),
            new_branch: Some(true),
            database: Some(WorkspaceDatabase::Clone),
            dependencies: Some(false),
            copy_environment: Some(false),
        },
    };
    let response = clone_client.workspace_prepare(&clone_preview).unwrap();
    assert!(
        response.error().is_none(),
        "clone preview: {:?}",
        response.error()
    );
    let preview = response.data().unwrap();
    assert!(
        preview.blockers.is_empty(),
        "clone blockers: {:?}",
        preview.blockers
    );
    let request = env_request(
        &source.id,
        source.revision,
        "a10-native-clone",
        MutationInput::WorkspaceCreate(WorkspaceCreateInput {
            plan: preview.plan.clone().unwrap(),
            limits: None,
        }),
    );
    let response = clone_client.workspace_plan(&request).unwrap();
    assert!(
        response.error().is_none(),
        "clone plan: {:?}",
        response.error()
    );
    let plan = response.data().unwrap().clone();
    rig.native.approve_workspace_plan(&plan.digest).unwrap();
    let response = clone_client
        .workspace_apply(&request, &plan.digest)
        .unwrap();
    assert!(
        response.error().is_none(),
        "clone apply: {:?}",
        response.error()
    );
    let receipt = response.data().unwrap().clone();
    let state = rig.complete(&clone_grant, &receipt);
    if state != OperationState::Succeeded {
        let summaries = std::fs::read_dir(rig.root.path().join("worktrees"))
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|d| std::fs::read(d.path().join("receipt.json")).ok())
            .filter_map(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .map(|v| {
                (
                    v["stage"].as_str().unwrap_or("unknown").to_owned(),
                    v["last_error"].as_str().unwrap_or("none").to_owned(),
                )
            })
            .collect::<Vec<_>>();
        panic!("native clone operation {state:?}; safe stages: {summaries:?}");
    }
    let records = std::fs::read_dir(rig.root.path().join("worktrees"))
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|d| std::fs::read(d.path().join("receipt.json")).ok())
        .filter_map(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .collect::<Vec<_>>();
    let clone = records
        .iter()
        .find(|r| r["input"]["branch"] == "a10-native-clone")
        .unwrap();
    assert_eq!(clone["stage"], "ready");
    let clone_id = clone["id"].as_str().unwrap();
    let cloned_db = rig
        .root
        .path()
        .join("worktrees")
        .join(clone_id)
        .join("tree/database/database.sqlite");
    let db = rusqlite::Connection::open_with_flags(
        cloned_db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let value: String = db
        .query_row("SELECT value FROM fixture_canary", [], |r| r.get(0))
        .unwrap();
    assert_eq!(value, "SOURCE_DB_CANARY");
    assert_eq!(std::fs::read(&source_db).unwrap(), source_db_bytes);
    assert_eq!(
        std::fs::read_to_string(source.path.join("database/other.sqlite")).unwrap(),
        "OTHER_DB_CANARY"
    );
    assert_eq!(
        std::fs::read_to_string(source.path.join(".env")).unwrap(),
        "APP_KEY=base64:SOURCE_CANARY\nDB_CONNECTION=sqlite\n"
    );
    assert_eq!(git(&["rev-parse", "HEAD"]), head);
    assert_eq!(git(&["status", "--porcelain"]), status);

    // A real committed submodule selects isolated creation. Nested dirty state
    // remains unverified without executing the untrusted nested clean filter.
    std::fs::remove_file(source.path.join("dirty-canary")).unwrap();
    let module = rig.root.path().join("native-module-source");
    std::fs::create_dir_all(&module).unwrap();
    let module_git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&module)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "module git {:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    };
    module_git(&["init", "-q"]);
    module_git(&["config", "user.name", "Fixture"]);
    module_git(&["config", "user.email", "fixture@example.test"]);
    std::fs::write(module.join("nested.txt"), "AAAA").unwrap();
    module_git(&["add", "."]);
    module_git(&["commit", "-qm", "nested base"]);
    git(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        "-q",
        module.to_str().unwrap(),
        "module",
    ]);
    git(&["add", ".gitmodules", "module"]);
    git(&["commit", "-qm", "add fixture module"]);
    let isolated_head = git(&["rev-parse", "HEAD"]);
    let nested = source.path.join("module");
    let nested_git = |args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(&nested)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "nested config failed");
    };
    nested_git(&[
        "config",
        "filter.a10nested.clean",
        "sh -c 'printf clean > a10-nested-marker; cat'",
    ]);
    std::fs::write(
        nested.join(".gitattributes"),
        "nested.txt filter=a10nested\n",
    )
    .unwrap();
    std::fs::write(nested.join("nested.txt"), "BBBB").unwrap();
    let isolated_preview = Request {
        api_version: Default::default(),
        request_id: id("a10-native-isolated-preview"),
        project_id: id(&source.id),
        expected_revision: Some(source.revision),
        idempotency_key: None,
        input: WorkspacePrepareInput {
            action: A::Create,
            workspace_id: None,
            name: Some(Text::new("Isolated child".into()).unwrap()),
            branch: Some(Text::new("a10-native-isolated".into()).unwrap()),
            base: Some(Text::new("HEAD".into()).unwrap()),
            new_branch: Some(true),
            database: Some(WorkspaceDatabase::Empty),
            dependencies: Some(false),
            copy_environment: Some(false),
        },
    };
    let response = client.workspace_prepare(&isolated_preview).unwrap();
    assert!(
        response.error().is_none(),
        "isolated preview: {:?}",
        response.error()
    );
    let preview = response.data().unwrap();
    assert!(
        preview.blockers.is_empty(),
        "isolated blockers: {:?}",
        preview.blockers
    );
    let details = preview.create.as_ref().expect("create details");
    assert!(details.isolated);
    assert!(details.source_dirty.is_none());
    assert_eq!(details.source_commit.as_str(), isolated_head);
    let request = env_request(
        &source.id,
        source.revision,
        "a10-native-isolated",
        MutationInput::WorkspaceCreate(WorkspaceCreateInput {
            plan: preview.plan.clone().unwrap(),
            limits: None,
        }),
    );
    let response = client.workspace_plan(&request).unwrap();
    assert!(
        response.error().is_none(),
        "isolated plan: {:?}",
        response.error()
    );
    let plan = response.data().unwrap().clone();
    let pending = rig.native.pending_reviews().unwrap();
    let review = pending.iter().find(|v| v.digest == plan.digest).unwrap();
    assert!(matches!(
        &review.target,
        devforge_manager::service_control::ReviewTarget::Workspace {
            create_source_dirty: None,
            create_isolated: Some(true),
            ..
        }
    ));
    rig.native.approve_workspace_plan(&plan.digest).unwrap();
    let response = client.workspace_apply(&request, &plan.digest).unwrap();
    assert!(
        response.error().is_none(),
        "isolated apply: {:?}",
        response.error()
    );
    let receipt = response.data().unwrap().clone();
    let state = rig.complete(&grant, &receipt);
    if state != OperationState::Succeeded {
        let summaries = std::fs::read_dir(rig.root.path().join("worktrees"))
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .filter_map(|d| std::fs::read(d.path().join("receipt.json")).ok())
            .filter_map(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .map(|v| {
                (
                    v["stage"].as_str().unwrap_or("unknown").to_owned(),
                    v["last_error"].as_str().unwrap_or("none").to_owned(),
                )
            })
            .collect::<Vec<_>>();
        panic!("native isolated create {state:?}; safe stages: {summaries:?}");
    }
    let records = std::fs::read_dir(rig.root.path().join("worktrees"))
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|d| std::fs::read(d.path().join("receipt.json")).ok())
        .filter_map(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .collect::<Vec<_>>();
    let isolated = records
        .iter()
        .find(|r| r["input"]["branch"] == "a10-native-isolated")
        .unwrap();
    assert_eq!(isolated["stage"], "ready");
    assert_eq!(isolated["isolated"], true);
    let isolated_id = isolated["id"].as_str().unwrap();
    let child_nested = rig
        .root
        .path()
        .join("worktrees")
        .join(isolated_id)
        .join("tree/module/nested.txt");
    assert_eq!(std::fs::read_to_string(child_nested).unwrap(), "AAAA");
    assert_eq!(
        std::fs::read_to_string(nested.join("nested.txt")).unwrap(),
        "BBBB"
    );
    assert!(!nested.join("a10-nested-marker").exists());
    assert_eq!(git(&["rev-parse", "HEAD"]), isolated_head);
    assert_eq!(std::fs::read(&source_db).unwrap(), source_db_bytes);
}

#[test]
fn a10_merge_stale_source_is_refused_and_clean_divergence_is_reviewed() {
    let _fixture_guard = a10_fixture_guard();
    use devforge_control::{
        InformationScope,
        mutations::WorkspaceLifecycleInput,
        tools::{WorkspacePrepareAction as A, WorkspacePrepareInput},
    };
    let mut rig = Rig::runtime_setup_with(None, false, false, seed_a10_owned_worktree);
    let source = rig.projects[0].clone();
    let child = rig.projects[2].clone();
    let wid = child.worktree.as_ref().unwrap().clone();
    let target = rig.root.path().join("worktrees").join(&wid).join("tree");
    let git = |path: &Path, args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let grant = rig
        .native
        .issue_external_grant_with_information(
            &id(&source.id),
            &[
                Scope::Observer,
                Scope::ProjectManage,
                Scope::ExecutionTest,
                Scope::SensitiveOperation,
            ],
            &[InformationScope::WorkspaceContentScan],
            Duration::from_secs(600),
        )
        .unwrap();
    let mut client = rig.diagnostics(&grant);
    let prepare = Request {
        api_version: Default::default(),
        request_id: id("a10-diverge-preview"),
        project_id: id(&source.id),
        expected_revision: Some(source.revision),
        idempotency_key: None,
        input: WorkspacePrepareInput {
            action: A::Merge,
            workspace_id: Some(id(&wid)),
            name: None,
            branch: None,
            base: None,
            new_branch: None,
            database: None,
            dependencies: None,
            copy_environment: None,
        },
    };
    let target_head = git(&target, &["rev-parse", "HEAD"]);
    let preview = client.workspace_prepare(&prepare).unwrap();
    assert!(preview.error().is_none(), "{:?}", preview.error());
    let request = env_request(
        &source.id,
        source.revision,
        "a10-stale-merge",
        MutationInput::WorkspaceMerge(WorkspaceLifecycleInput {
            workspace_id: id(&wid),
            plan: preview.data().unwrap().plan.clone().unwrap(),
            limits: None,
        }),
    );
    let plan = client
        .workspace_plan(&request)
        .unwrap()
        .data()
        .unwrap()
        .clone();
    rig.native.approve_workspace_plan(&plan.digest).unwrap();
    std::fs::write(source.path.join("more.txt"), "source later commit").unwrap();
    git(&source.path, &["add", "more.txt"]);
    git(&source.path, &["commit", "-qm", "later source"]);
    let new_source_head = git(&source.path, &["rev-parse", "HEAD"]);
    let stale = client.workspace_apply(&request, &plan.digest).unwrap();
    if let Some(receipt) = stale.data() {
        assert_ne!(rig.complete(&grant, receipt), OperationState::Succeeded);
    } else {
        assert!(stale.error().is_some());
    }
    assert_eq!(git(&target, &["rev-parse", "HEAD"]), target_head);
    assert_eq!(git(&source.path, &["rev-parse", "HEAD"]), new_source_head);
    std::fs::write(target.join("target-only.txt"), "target divergence").unwrap();
    git(&target, &["add", "target-only.txt"]);
    git(&target, &["commit", "-qm", "diverge"]);
    let divergent_head = git(&target, &["rev-parse", "HEAD"]);
    let blocked = client.workspace_prepare(&prepare).unwrap();
    assert!(blocked.error().is_none(), "{:?}", blocked.error());
    let preview = blocked.data().unwrap();
    assert!(preview.blockers.is_empty(), "{:?}", preview.blockers);
    let merge = preview.merge.as_ref().expect("merge preview");
    assert_eq!(
        merge.effect,
        devforge_control::tools::WorkspaceMergeEffect::MergeCommit
    );
    let reviewed_tree = merge
        .result_tree
        .as_ref()
        .expect("reviewed merge tree")
        .as_str()
        .to_owned();
    assert!(preview.plan.is_some());
    let source_status = git(&source.path, &["status", "--porcelain"]);
    assert_eq!(
        a10_lifecycle_action(&mut rig, &grant, &wid, A::Merge, "a10-divergent-merge"),
        OperationState::Succeeded
    );
    let merged_head = git(&target, &["rev-parse", "HEAD"]);
    assert_ne!(merged_head, divergent_head);
    assert_eq!(git(&target, &["rev-parse", "HEAD^{tree}"]), reviewed_tree);
    let parents = git(&target, &["rev-list", "--parents", "-n", "1", "HEAD"]);
    assert_eq!(
        parents.split_whitespace().collect::<Vec<_>>(),
        vec![
            merged_head.as_str(),
            divergent_head.as_str(),
            new_source_head.as_str()
        ]
    );
    assert_eq!(git(&source.path, &["rev-parse", "HEAD"]), new_source_head);
    assert_eq!(git(&source.path, &["status", "--porcelain"]), source_status);
    assert_eq!(
        std::fs::read_to_string(source.path.join("dirty-canary")).unwrap(),
        "source remains untouched"
    );
}

#[test]
fn a10_external_create_and_archive_previews_never_execute_git_filters_or_fsmonitor() {
    let _fixture_guard = a10_fixture_guard();
    use devforge_control::{
        InformationScope,
        limits::Text,
        tools::{WorkspaceDatabase, WorkspacePrepareAction as A, WorkspacePrepareInput},
    };
    let mut rig = Rig::runtime_setup_with(None, false, false, seed_a10_owned_worktree);
    let source = rig.projects[0].clone();
    let child = rig.projects[2].clone();
    let wid = child.worktree.as_ref().unwrap().clone();
    let target = rig.root.path().join("worktrees").join(&wid).join("tree");
    let git = |path: &Path, args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    let head = git(&source.path, &["rev-parse", "HEAD"]);
    // Static, harmless commands create fixture markers if a filter/fsmonitor is
    // ever executed. All paths remain local to the disposable fixture.
    git(
        &source.path,
        &[
            "config",
            "filter.a10clean.clean",
            "sh -c 'printf clean > a10-clean-marker; cat'",
        ],
    );
    git(
        &source.path,
        &[
            "config",
            "filter.a10process.process",
            "sh -c 'printf process > a10-process-marker; exit 1'",
        ],
    );
    git(
        &source.path,
        &[
            "config",
            "core.fsmonitor",
            "sh -c 'printf monitor > a10-fsmonitor-marker'",
        ],
    );
    for path in [&source.path, &target] {
        std::fs::write(
            path.join(".gitattributes"),
            "public/index.php filter=a10clean\ncomposer.json filter=a10process\n",
        )
        .unwrap();
        let mut php = std::fs::read(path.join("public/index.php")).unwrap();
        let last = php.len() - 1;
        php[last] = if php[last] == b'X' { b'Y' } else { b'X' };
        std::fs::write(path.join("public/index.php"), php).unwrap();
        let mut json = std::fs::read(path.join("composer.json")).unwrap();
        json.push(b' ');
        std::fs::write(path.join("composer.json"), json).unwrap();
    }
    let scan = [InformationScope::WorkspaceContentScan];
    let grant = rig
        .native
        .issue_external_grant_with_information(
            &id(&source.id),
            &[Scope::Observer],
            &scan,
            Duration::from_secs(600),
        )
        .unwrap();
    let mut client = rig.diagnostics(&grant);
    let create = Request {
        api_version: Default::default(),
        request_id: id("a10-filter-create"),
        project_id: id(&source.id),
        expected_revision: Some(source.revision),
        idempotency_key: None,
        input: WorkspacePrepareInput {
            action: A::Create,
            workspace_id: None,
            name: Some(Text::new("filter child".into()).unwrap()),
            branch: Some(Text::new("a10-filter-child".into()).unwrap()),
            base: Some(Text::new("HEAD".into()).unwrap()),
            new_branch: Some(true),
            database: Some(WorkspaceDatabase::Empty),
            dependencies: Some(false),
            copy_environment: Some(false),
        },
    };
    let archive = Request {
        api_version: Default::default(),
        request_id: id("a10-filter-archive"),
        project_id: id(&source.id),
        expected_revision: Some(source.revision),
        idempotency_key: None,
        input: WorkspacePrepareInput {
            action: A::Archive,
            workspace_id: Some(id(&wid)),
            name: None,
            branch: None,
            base: None,
            new_branch: None,
            database: None,
            dependencies: None,
            copy_environment: None,
        },
    };
    let check_markers = |phase: &str| {
        for (label, path) in [("source", &source.path), ("target", &target)] {
            for name in [
                "a10-clean-marker",
                "a10-process-marker",
                "a10-fsmonitor-marker",
            ] {
                assert!(
                    !path.join(name).exists(),
                    "untrusted filter ran in {label} during {phase}: {name}"
                );
            }
        }
    };
    check_markers("fixture setup");
    let created = client.workspace_prepare(&create).unwrap();
    assert!(
        created.error().is_none(),
        "safe create preview: {:?}",
        created.error()
    );
    check_markers("create preview");
    let archived = client.workspace_prepare(&archive).unwrap();
    assert!(
        archived.error().is_none(),
        "safe archive preview: {:?}",
        archived.error()
    );
    assert!(
        archived
            .data()
            .unwrap()
            .blockers
            .iter()
            .any(|b| b.as_str() == "worktree_dirty")
    );
    check_markers("archive preview");
    check_markers("after archive");
    // A staged gitlink must be rejected before any status walk, even when the
    // caller holds the read-only content-scan right.
    git(
        &source.path,
        &[
            "-c",
            "core.fsmonitor=false",
            "-c",
            "filter.a10clean.clean=",
            "-c",
            "filter.a10process.process=",
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{head},a10-submodule"),
        ],
    );
    check_markers("staging gitlink");
    let staged = client.workspace_prepare(&create).unwrap();
    if let Some(preview) = staged.data() {
        assert_ne!(
            preview.create.as_ref().unwrap().source_dirty,
            Some(false),
            "nested source must not be reported clean when it is known dirty"
        );
    } else {
        assert!(staged.error().is_some());
    }
    check_markers("staged preview");
    // Commit only the staged index tree using commit-tree/update-ref, which do
    // not inspect dirty worktree bytes or invoke the configured clean filters.
    let tree = git(
        &source.path,
        &[
            "-c",
            "core.fsmonitor=false",
            "-c",
            "filter.a10clean.clean=",
            "-c",
            "filter.a10process.process=",
            "write-tree",
        ],
    );
    let commit = git(
        &source.path,
        &[
            "-c",
            "core.fsmonitor=false",
            "-c",
            "filter.a10clean.clean=",
            "-c",
            "filter.a10process.process=",
            "commit-tree",
            &tree,
            "-p",
            &head,
            "-m",
            "fixture gitlink",
        ],
    );
    git(
        &source.path,
        &[
            "-c",
            "core.fsmonitor=false",
            "-c",
            "filter.a10clean.clean=",
            "-c",
            "filter.a10process.process=",
            "update-ref",
            "HEAD",
            &commit,
            &head,
        ],
    );
    check_markers("creating gitlink commit");
    let committed = client.workspace_prepare(&create).unwrap();
    assert!(
        committed.data().is_none(),
        "committed gitlink must not yield a create ticket"
    );
    check_markers("committed preview");
}

#[test]
fn a10_isolated_create_preview_reports_nested_source_dirty_unknown_without_running_filters() {
    let _fixture_guard = a10_fixture_guard();
    use devforge_control::{
        InformationScope,
        limits::Text,
        tools::{WorkspaceDatabase, WorkspacePrepareAction as A, WorkspacePrepareInput},
    };
    let mut rig = Rig::runtime_setup_with(None, false, false, seed_a10_owned_worktree);
    let source = rig.projects[0].clone();
    let module = rig.root.path().join("module-source");
    std::fs::create_dir_all(&module).unwrap();
    let git = |path: &Path, args: &[&str]| {
        let out = std::process::Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    };
    git(&module, &["init", "-q"]);
    git(&module, &["config", "user.name", "Fixture"]);
    git(&module, &["config", "user.email", "fixture@example.test"]);
    std::fs::write(module.join("tracked.txt"), "AAAA").unwrap();
    git(&module, &["add", "."]);
    git(&module, &["commit", "-qm", "module"]);
    std::fs::remove_file(source.path.join("dirty-canary")).unwrap();
    git(
        &source.path,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            module.to_str().unwrap(),
            "module",
        ],
    );
    git(&source.path, &["add", ".gitmodules", "module"]);
    git(&source.path, &["commit", "-qm", "add module"]);
    let head = git(&source.path, &["rev-parse", "HEAD"]);
    let nested = source.path.join("module");
    git(
        &nested,
        &[
            "config",
            "filter.a10nested.clean",
            "sh -c 'printf clean > a10-nested-marker; cat'",
        ],
    );
    git(
        &nested,
        &[
            "config",
            "core.fsmonitor",
            "sh -c 'printf monitor > a10-nested-monitor'",
        ],
    );
    std::fs::write(
        nested.join(".gitattributes"),
        "tracked.txt filter=a10nested\n",
    )
    .unwrap();
    std::fs::write(nested.join("tracked.txt"), "BBBB").unwrap();
    let grant = rig
        .native
        .issue_external_grant_with_information(
            &id(&source.id),
            &[Scope::Observer],
            &[InformationScope::WorkspaceContentScan],
            Duration::from_secs(600),
        )
        .unwrap();
    let mut client = rig.diagnostics(&grant);
    let prepare = Request {
        api_version: Default::default(),
        request_id: id("a10-nested-preview"),
        project_id: id(&source.id),
        expected_revision: Some(source.revision),
        idempotency_key: None,
        input: WorkspacePrepareInput {
            action: A::Create,
            workspace_id: None,
            name: Some(Text::new("isolated child".into()).unwrap()),
            branch: Some(Text::new("a10-isolated-child".into()).unwrap()),
            base: Some(Text::new("HEAD".into()).unwrap()),
            new_branch: Some(true),
            database: Some(WorkspaceDatabase::Empty),
            dependencies: Some(false),
            copy_environment: Some(false),
        },
    };
    let response = client.workspace_prepare(&prepare).unwrap();
    assert!(response.error().is_none(), "{:?}", response.error());
    let preview = response.data().unwrap();
    assert!(preview.blockers.is_empty(), "{:?}", preview.blockers);
    let create = preview.create.as_ref().expect("create preview");
    assert!(create.isolated);
    assert_eq!(create.source_commit.as_str(), head);
    assert!(
        create.source_dirty.is_none(),
        "nested source dirtiness must be unverified"
    );
    assert!(preview.plan.is_some());
    assert!(!nested.join("a10-nested-marker").exists());
    assert!(!nested.join("a10-nested-monitor").exists());
}

#[test]
fn a10_workspace_approval_disconnect_and_quit_do_not_approve_late() {
    let _fixture_guard = a10_fixture_guard();
    use devforge_control::{
        InformationScope,
        mutations::WorkspaceLifecycleInput,
        tools::{WorkspacePrepareAction as A, WorkspacePrepareInput},
    };
    use devforge_manager::service_control::ReviewState;
    use std::io::Write;
    let policy = Policy {
        store_interval: Duration::from_secs(1),
        ..Policy::default()
    };
    let mut rig =
        Rig::runtime_setup_with_policy(None, false, false, policy, seed_a10_owned_worktree);
    let source = rig.projects[0].clone();
    let child = rig.projects[2].clone();
    let wid = child.worktree.as_ref().unwrap().clone();
    let grant = rig
        .native
        .issue_external_grant_with_information(
            &id(&source.id),
            &[
                Scope::Observer,
                Scope::ProjectManage,
                Scope::ExecutionTest,
                Scope::SensitiveOperation,
            ],
            &[InformationScope::WorkspaceContentScan],
            Duration::from_secs(600),
        )
        .unwrap();
    let mut client = rig.diagnostics(&grant);
    let prepare = Request {
        api_version: Default::default(),
        request_id: id("a10-disconnect-preview"),
        project_id: id(&source.id),
        expected_revision: Some(source.revision),
        idempotency_key: None,
        input: WorkspacePrepareInput {
            action: A::Merge,
            workspace_id: Some(id(&wid)),
            name: None,
            branch: None,
            base: None,
            new_branch: None,
            database: None,
            dependencies: None,
            copy_environment: None,
        },
    };
    let preview = client.workspace_prepare(&prepare).unwrap();
    assert!(preview.error().is_none(), "{:?}", preview.error());
    let preview = preview.data().unwrap();
    assert!(preview.blockers.is_empty(), "{:?}", preview.blockers);
    let req = env_request(
        &source.id,
        source.revision,
        "a10-disconnect-approval",
        MutationInput::WorkspaceMerge(WorkspaceLifecycleInput {
            workspace_id: id(&wid),
            plan: preview.plan.clone().unwrap(),
            limits: None,
        }),
    );
    let plan = client.workspace_plan(&req).unwrap().data().unwrap().clone();
    assert!(
        rig.native.approve_service_plan(&plan.digest).is_err(),
        "generic approval must reject workspace plans"
    );
    assert_eq!(
        rig.native
            .pending_reviews()
            .unwrap()
            .iter()
            .find(|v| v.digest == plan.digest)
            .unwrap()
            .state,
        ReviewState::Pending
    );
    // A dedicated owner session writes a correctly framed approval and then
    // disconnects while validation waits on the busy actor. No credentials are
    // printed or persisted by the fixture.
    thread::sleep(Duration::from_millis(1100));
    let auth = Client::new(rig.endpoint.clone())
        .request(
            &json!({"method":"authenticate","credential":rig.native_credential.expose_secret()}),
        )
        .unwrap();
    assert_eq!(
        auth["ok"], true,
        "fixture owner auth error: {}",
        auth["error"]
    );
    let session = auth["data"]["session"]
        .as_str()
        .expect("owner session")
        .to_owned();
    let release = rig.block();
    #[cfg(unix)]
    let mut connection = std::os::unix::net::UnixStream::connect(rig.endpoint.address()).unwrap();
    #[cfg(windows)]
    let mut connection = {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(rig.endpoint.address())
            {
                Ok(file) => break file,
                Err(error)
                    if error.raw_os_error()
                        == Some(windows_sys::Win32::Foundation::ERROR_PIPE_BUSY as i32)
                        && Instant::now() < until =>
                {
                    thread::sleep(Duration::from_millis(10))
                }
                Err(error) => panic!("isolated pipe connection failed: {error}"),
            }
        }
    };
    let payload = serde_json::to_vec(
        &json!({"method":"approve_workspace_plan","session":session.clone(),"digest":plan.digest}),
    )
    .unwrap();
    connection
        .write_all(&(payload.len() as u32).to_be_bytes())
        .unwrap();
    connection.write_all(&payload).unwrap();
    connection.flush().unwrap();
    thread::sleep(Duration::from_millis(200));
    drop(connection);
    thread::sleep(Duration::from_millis(200));
    drop(release);
    // This preview is a barrier after the delayed approval's actor read.
    let barrier = client.workspace_prepare(&prepare).unwrap();
    assert!(barrier.data().is_some(), "{:?}", barrier.error());
    assert_eq!(
        rig.native
            .pending_reviews()
            .unwrap()
            .iter()
            .find(|v| v.digest == plan.digest)
            .unwrap()
            .state,
        ReviewState::Pending,
        "disconnect must not approve later"
    );
    assert!(
        client
            .workspace_apply(&req, &plan.digest)
            .unwrap()
            .data()
            .is_none()
    );

    // A second owner begins a real workspace approval while actor work is held;
    // the first owner must still be able to quit without waiting for that work.
    let endpoint = rig.endpoint.clone();
    let digest = plan.digest.clone();
    let release = rig.block();
    let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(1);
    let waiting = thread::spawn(move || {
        ready_tx.send(()).unwrap();
        Client::new(endpoint)
            .request(&json!({"method":"approve_workspace_plan","session":session,"digest":digest}))
    });
    ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    thread::sleep(Duration::from_millis(100));
    let started = Instant::now();
    let quit = rig.native.quit();
    let elapsed = started.elapsed();
    drop(release);
    let approval = waiting.join().unwrap();
    assert!(
        quit.is_ok(),
        "owner Quit must remain available during workspace validation: {quit:?}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "Quit blocked behind workspace approval"
    );
    assert!(
        !matches!(approval, Ok(ref reply) if reply["ok"] == true),
        "Quit must prevent late workspace approval"
    );
}
