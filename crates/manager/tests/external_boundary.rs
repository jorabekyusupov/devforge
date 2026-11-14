mod common;
use devforge_control::{ErrorCode, Id, Scope};
use devforge_core::{ErrorCode as CoreError, SecretStore};
use devforge_manager::{
    Client,
    auth::{DesktopAuth, DesktopCredential, Policy, TrustedDesktopAction},
    controller::{Controller, StartIntent, Stores, local_owner_user},
    external_auth::{ExternalCredential, IssuedGrant},
    external_client::ExternalClient,
    external_control::{ExternalInput, ExternalReply},
    native_client::NativeClient,
};
use devforge_projects::project::{Catalog, ProjectStore};
use devforge_runtime::package::Target;
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Default)]
struct MemoryStore {
    values: Mutex<BTreeMap<String, String>>,
    gets: AtomicUsize,
}
impl SecretStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<String>, CoreError> {
        self.gets.fetch_add(1, Ordering::Relaxed);
        Ok(self.values.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &str, value: &str) -> Result<(), CoreError> {
        self.values.lock().unwrap().insert(key.into(), value.into());
        Ok(())
    }
    fn delete(&self, key: &str) -> Result<(), CoreError> {
        self.values.lock().unwrap().remove(key);
        Ok(())
    }
}
fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}
fn project(root: &Path, name: &str) -> devforge_projects::project::Project {
    let source = root.join(name);
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
            .find(|(_, p)| p.kind == kind && p.target == target)
            .unwrap()
            .0
            .clone()
    };
    ProjectStore::open(root.join("projects"))
        .unwrap()
        .import(&source, name, &select("php"), &select("node"), &catalog)
        .unwrap()
}
struct Running {
    client: NativeClient,
    transport: Client,
    worker: Option<thread::JoinHandle<devforge_manager::controller::Result<()>>>,
    endpoint: devforge_manager::Endpoint,
}
impl Running {
    fn new(root: &Path, store: Arc<MemoryStore>) -> Self {
        let owner = common::acquire(root).unwrap();
        let mut auth = DesktopAuth::new(
            owner.endpoint().clone(),
            local_owner_user().unwrap(),
            Policy::default(),
        )
        .unwrap();
        let key: DesktopCredential = auth
            .rotate_desktop_key(
                &TrustedDesktopAction::from_explicit_desktop_action(),
                store.as_ref(),
                Instant::now(),
            )
            .unwrap();
        drop(owner);
        let controller = Controller::start(
            root,
            StartIntent::AutoStart,
            Stores {
                workspace: Arc::new(MemoryStore::default()),
                desktop: store.clone(),
            },
        )
        .unwrap();
        let endpoint = controller.endpoint().clone();
        let transport = Client::new(endpoint.clone());
        let worker = thread::spawn(move || controller.serve(None));
        let until = Instant::now() + Duration::from_secs(5);
        while ExternalClient::observe_owner(&endpoint).is_err() {
            assert!(Instant::now() < until);
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(store.gets.load(Ordering::Relaxed), 0);
        let client = NativeClient::authenticate_explicit(endpoint.clone(), &key).unwrap();
        Self {
            client,
            transport,
            worker: Some(worker),
            endpoint,
        }
    }
    fn issue(&mut self, project: &str, manage: bool) -> IssuedGrant {
        self.client
            .issue_external_grant(
                &id(project),
                if manage {
                    &[Scope::Observer, Scope::ProjectManage]
                } else {
                    &[Scope::Observer]
                },
                Duration::from_secs(60),
            )
            .unwrap()
    }
    fn external(&self, credential: &ExternalCredential) -> ExternalClient {
        ExternalClient::connect(
            self.endpoint.clone(),
            ExternalCredential::from_wire(credential.expose_secret()).unwrap(),
        )
        .unwrap()
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.client.quit();
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap().unwrap();
        }
    }
}
fn call(client: &mut ExternalClient, input: ExternalInput) -> ExternalReply {
    client.request(id("external-test"), input, None).unwrap()
}
fn failure(reply: ExternalReply, code: ErrorCode) {
    assert!(!reply.ok);
    assert_eq!(reply.error.unwrap().code, code);
    assert!(reply.data.is_none());
}

#[test]
fn external_metadata_is_one_project_redacted_and_never_native_authority() {
    let root = common::root("df-ext-boundary-");
    let a = project(root.path(), "Alpha");
    let b = project(root.path(), "Bravo");
    let store = Arc::new(MemoryStore::default());
    let mut owner = Running::new(root.path(), store.clone());
    let grant = owner.issue(&a.id, false);
    let mut external = owner.external(&grant.credential);
    let gets = store.gets.load(Ordering::Relaxed);
    let reply = call(&mut external, ExternalInput::ProjectList {});
    assert!(reply.ok);
    let text = serde_json::to_string(&reply).unwrap();
    assert!(text.contains(&a.id));
    assert!(!text.contains(&b.id));
    assert!(!text.contains(&a.path.to_string_lossy().to_string()));
    assert!(!text.contains("environment_references"));
    assert!(!text.contains("password"));
    assert!(text.contains("cached"));
    assert!(call(&mut external, ExternalInput::Status {}).ok);
    let unsupported=owner.transport.request(&json!({"method":"external","credential":grant.credential.expose_secret(),"request":{"api_version":1,"request_id":"unsupported","expected_revision":null,"input":{"tool":"execution.run","arguments":{}}}})).unwrap();
    let unsupported: ExternalReply = serde_json::from_value(unsupported).unwrap();
    failure(unsupported, ErrorCode::UnsupportedCapability);
    let version=owner.transport.request(&json!({"method":"external","credential":grant.credential.expose_secret(),"request":{"api_version":2,"request_id":"version","expected_revision":null,"input":{"tool":"projects.list","arguments":{}}}})).unwrap();
    failure(
        serde_json::from_value(version).unwrap(),
        ErrorCode::UnsupportedVersion,
    );
    let role=owner.transport.request(&json!({"method":"external","credential":grant.credential.expose_secret(),"role":"desktop","request":{"api_version":1,"request_id":"role","expected_revision":null,"input":{"tool":"projects.list","arguments":{}}}})).unwrap();
    failure(
        serde_json::from_value(role).unwrap(),
        ErrorCode::InvalidInput,
    );
    failure(
        call(
            &mut external,
            ExternalInput::ProjectGet {
                project_id: id(&b.id),
            },
        ),
        ErrorCode::ProjectMismatch,
    );
    failure(
        external
            .request(
                id("revision"),
                ExternalInput::ProjectList {},
                Some(a.revision + 1),
            )
            .unwrap(),
        ErrorCode::RevisionConflict,
    );
    let invalid = ExternalCredential::from_wire(&"0".repeat(64)).unwrap();
    let mut bad = owner.external(&invalid);
    failure(
        call(&mut bad, ExternalInput::ProjectList {}),
        ErrorCode::Unauthorized,
    );
    let response=owner.transport.request(&json!({"method":"external_grant_issue","session":grant.credential.expose_secret(),"project_id":a.id,"scopes":["observer"],"ttl_ms":60000})).unwrap();
    assert_eq!(response["ok"], false);
    let response=owner.transport.request(&json!({"method":"submit","session":grant.credential.expose_secret(),"project_id":a.id,"idempotency_key":"no-native-bypass","command":{"action":"visit","id":a.id}})).unwrap();
    assert_eq!(response["ok"], false);
    assert!(
        owner
            .client
            .issue_external_grant(
                &id(&a.id),
                &[Scope::Observer, Scope::ExecutionTest],
                Duration::from_secs(60)
            )
            .is_err()
    );
    let listed = serde_json::to_value(owner.client.list_external_grants().unwrap()).unwrap();
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert!(
        !listed
            .to_string()
            .contains(grant.credential.expose_secret())
    );
    assert_eq!(
        store.gets.load(Ordering::Relaxed),
        gets,
        "external routes do not read native stores"
    );
    owner
        .client
        .revoke_external_grant(&grant.record.grant_id)
        .unwrap();
    failure(
        call(&mut external, ExternalInput::ProjectList {}),
        ErrorCode::GrantRevoked,
    );
}

#[test]
fn external_operation_namespace_and_cancel_scope_are_checked_against_the_original_client() {
    let root = common::root("df-ext-op-");
    let project = project(root.path(), "Operations");
    let mut owner = Running::new(root.path(), Arc::new(MemoryStore::default()));
    let managed = owner.issue(&project.id, true);
    let observer = owner.issue(&project.id, false);
    let other = owner.issue(&project.id, true);
    let receipt = owner
        .client
        .admit(
            &id(&project.id),
            &id("native-operation"),
            &devforge_workspace::Command::Favorite {
                id: project.id.clone(),
                favorite: true,
            },
        )
        .unwrap();
    let until = Instant::now() + Duration::from_secs(5);
    while !owner
        .client
        .status(&id(&project.id), &receipt.operation_id)
        .unwrap()
        .state
        .terminal()
    {
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(10));
    }
    let mut client = owner.external(&managed.credential);
    let status = || ExternalInput::OperationStatus {
        project_id: id(&project.id),
        operation_id: receipt.operation_id.clone(),
    };
    let cancel = || ExternalInput::OperationCancel {
        project_id: id(&project.id),
        operation_id: receipt.operation_id.clone(),
    };
    failure(call(&mut client, status()), ErrorCode::Unauthorized);
    failure(call(&mut client, cancel()), ErrorCode::Unauthorized);
    // There is deliberately no external admission route in M13.2. Relabel one
    // completed real receipt in this isolated journal to exercise the own-row
    // projection; this is fixture setup, not proof of external execution.
    let db = rusqlite::Connection::open(root.path().join("control/operations.sqlite3")).unwrap();
    db.execute(
        "UPDATE operations SET client_id=?1 WHERE operation_id=?2",
        rusqlite::params![
            managed.record.client_id.as_str(),
            receipt.operation_id.as_str()
        ],
    )
    .unwrap();
    drop(db);
    let own = call(&mut client, status());
    assert!(own.ok);
    assert!(serde_json::to_string(&own).unwrap().contains("succeeded"));
    let terminal = call(&mut client, cancel());
    assert!(terminal.ok);
    assert!(
        serde_json::to_string(&terminal)
            .unwrap()
            .contains("already_terminal")
    );
    let mut foreign = owner.external(&other.credential);
    failure(call(&mut foreign, status()), ErrorCode::Unauthorized);
    failure(call(&mut foreign, cancel()), ErrorCode::Unauthorized);
    let mut readonly = owner.external(&observer.credential);
    failure(call(&mut readonly, cancel()), ErrorCode::Unauthorized);
    assert!(
        owner
            .client
            .operation_by_key(&id(&project.id), &id("native-operation"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn replaced_registered_directory_cannot_receive_or_use_a_grant_and_expiry_is_enforced() {
    let root = common::root("df-ext-root-");
    let project = project(root.path(), "Original");
    let mut owner = Running::new(root.path(), Arc::new(MemoryStore::default()));
    let short = owner
        .client
        .issue_external_grant(
            &id(&project.id),
            &[Scope::Observer],
            Duration::from_millis(10),
        )
        .unwrap();
    let mut expired = owner.external(&short.credential);
    thread::sleep(Duration::from_millis(30));
    failure(
        call(&mut expired, ExternalInput::ProjectList {}),
        ErrorCode::GrantExpired,
    );
    let grant = owner.issue(&project.id, false);
    let mut client = owner.external(&grant.credential);
    std::fs::rename(&project.path, root.path().join("retained-original")).unwrap();
    std::fs::create_dir(&project.path).unwrap();
    failure(
        call(&mut client, ExternalInput::ProjectList {}),
        ErrorCode::ProjectMismatch,
    );
    assert!(
        owner
            .client
            .issue_external_grant(
                &id(&project.id),
                &[Scope::Observer],
                Duration::from_secs(60)
            )
            .is_err()
    );
}

#[test]
fn stdio_file_binding_is_verified_by_manager_and_cannot_enable_cancel_or_foreign_projects() {
    use devforge_manager::{credential_file::CredentialFile, external_client::BoundReadOnlyClient};
    let root = common::root("df-mcp-binding-");
    let a = project(root.path(), "Bound");
    let b = project(root.path(), "Foreign");
    let mut owner = Running::new(root.path(), Arc::new(MemoryStore::default()));
    let grant = owner.issue(&a.id, true);
    let write = |name: &str, project: &str| {
        let path = root.path().join(name);
        common::credential_file(&path,&serde_json::to_vec(&json!({"schema":1,"project_id":project,"credential":grant.credential.expose_secret()})).unwrap());
        CredentialFile::load(&path).unwrap()
    };
    assert!(matches!(
        BoundReadOnlyClient::connect(owner.endpoint.clone(), write("wrong-project", &b.id)),
        Err(ErrorCode::ProjectMismatch)
    ));
    let mut client =
        BoundReadOnlyClient::connect(owner.endpoint.clone(), write("correct-project", &a.id))
            .unwrap();
    assert!(
        client
            .request(id("metadata"), ExternalInput::ProjectList {})
            .unwrap()
            .ok
    );
    assert!(matches!(
        client.request(
            id("no-cancel"),
            ExternalInput::OperationCancel {
                project_id: id(&a.id),
                operation_id: id("not-admitted")
            }
        ),
        Err(ErrorCode::UnsupportedCapability)
    ));
    assert!(matches!(
        client.request(
            id("no-foreign"),
            ExternalInput::ProjectGet {
                project_id: id(&b.id)
            }
        ),
        Err(ErrorCode::ProjectMismatch)
    ));
    owner
        .client
        .revoke_external_grant(&grant.record.grant_id)
        .unwrap();
    failure(
        client
            .request(id("revoked"), ExternalInput::Health {})
            .unwrap(),
        ErrorCode::GrantRevoked,
    );
    drop(owner);
    assert!(
        client
            .request(id("no-autostart"), ExternalInput::Health {})
            .is_err()
    );
    assert!(devforge_platform::distribution::AppLease::acquire(root.path()).is_ok());
}
