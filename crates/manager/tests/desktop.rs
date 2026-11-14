mod common;
use devforge_control::{Id, OperationState};
use devforge_core::{ErrorCode, SecretStore, SettingsRepository};
use devforge_manager::{
    Owner, PeerIdentity,
    actor::{ActorError, ActorHandle, OperationIdentity, Receipt, WorkspaceActor},
    auth::{DesktopAuth, DesktopAuthority, Policy, SessionCredential, TrustedDesktopAction},
    desktop::{AccessRequest, ReadRequest, SecretValue},
    payload,
};
use devforge_runtime::package::Target;
use devforge_workspace::{Command, LogFilter};
use rusqlite::Connection;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Default)]
struct Secrets {
    values: Mutex<BTreeMap<String, String>>,
    sets: AtomicUsize,
}
impl SecretStore for Secrets {
    fn get(&self, key: &str) -> Result<Option<String>, ErrorCode> {
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
struct Auth {
    auth: DesktopAuth,
    peer: PeerIdentity,
    session: SessionCredential,
}
impl Auth {
    fn new(owner: &Owner) -> Self {
        let peer = PeerIdentity {
            user: "desktop-facade-fixture".into(),
            process_id: Some(1),
        };
        let store = Secrets::default();
        let mut auth = DesktopAuth::new(
            owner.endpoint().clone(),
            peer.user.clone(),
            Policy::default(),
        )
        .unwrap();
        let key = auth
            .rotate_desktop_key(
                &TrustedDesktopAction::from_explicit_desktop_action(),
                &store,
                Instant::now(),
            )
            .unwrap();
        let session = auth
            .authenticate(&peer, &key, &store, Instant::now())
            .unwrap();
        Self {
            auth,
            peer,
            session,
        }
    }
    fn proof(&mut self) -> DesktopAuthority<'_> {
        self.auth
            .authorize(&self.peer, &self.session, Instant::now())
            .unwrap()
    }
}
fn id(value: &str) -> Id {
    Id::new(value).unwrap()
}
fn identity(project: &str, key: &str) -> OperationIdentity {
    OperationIdentity {
        project_id: id(project),
        idempotency_key: id(key),
    }
}
fn terminal(handle: &ActorHandle, proof: &DesktopAuthority<'_>, receipt: &Receipt) -> Receipt {
    let until = Instant::now() + Duration::from_secs(15);
    loop {
        let current = handle
            .status(proof, &receipt.project_id, &receipt.operation_id)
            .unwrap();
        if current.state.terminal() {
            return current;
        }
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(2));
    }
}
fn fixture(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    fs::create_dir_all(path.join("public")).unwrap();
    fs::write(path.join("public/index.php"), "<?php").unwrap();
    fs::write(
        path.join("artisan"),
        "<?php // metadata only, never executed",
    )
    .unwrap();
    fs::write(
        path.join("composer.json"),
        r#"{"require":{"php":"^8.3","laravel/framework":"^13"}}"#,
    )
    .unwrap();
    path
}
fn import(path: PathBuf) -> Command {
    let catalog: Value =
        serde_json::from_str(include_str!("../../../runtime/catalog.json")).unwrap();
    let target = serde_json::to_value(Target::current().unwrap()).unwrap();
    let packages = catalog["packages"].as_object().unwrap();
    let php = packages
        .iter()
        .find(|(_, p)| {
            p["kind"] == "php" && p["target"] == target && p["executables"].get("php").is_some()
        })
        .unwrap()
        .0
        .clone();
    let node = packages
        .iter()
        .find(|(_, p)| p["kind"] == "node" && p["target"] == target)
        .unwrap()
        .0
        .clone();
    Command::Import {
        path,
        name: "Facade".into(),
        php,
        node,
        domain: "facade.devforge.test".into(),
        https: false,
        vite: false,
        queue: false,
    }
}
fn register(handle: &ActorHandle, proof: &DesktopAuthority<'_>, path: PathBuf) -> Id {
    let receipt = handle
        .submit_desktop(proof, identity("desktop-root", "import"), import(path))
        .unwrap();
    assert_eq!(
        terminal(handle, proof, &receipt).state,
        OperationState::Succeeded
    );
    id(handle
        .result(proof, &receipt.project_id, &receipt.operation_id)
        .unwrap()["project_id"]
        .as_str()
        .unwrap())
}
fn read(
    handle: &ActorHandle,
    proof: &DesktopAuthority<'_>,
    key: &str,
    request: ReadRequest,
) -> (Receipt, Value) {
    let receipt = handle.desktop_read(proof, id(key), request).unwrap();
    let receipt = terminal(handle, proof, &receipt);
    if receipt.state != OperationState::Succeeded {
        let detail = handle
            .failure_detail(proof, &receipt.project_id, &receipt.operation_id)
            .unwrap();
        panic!("facade {key} failed: {:?}/{}", detail.scope, detail.code);
    }
    let image = handle
        .read_image(proof, &receipt.project_id, &receipt.operation_id)
        .unwrap();
    let value: Value = serde_json::from_slice(image.bytes()).unwrap();
    assert_eq!(value["ok"], true);
    (receipt, value["data"].clone())
}

#[test]
fn real_owner_reads_keep_large_source_and_all_existing_read_families() {
    let root = common::root("df-desktop-");
    let owner = common::acquire(root.path()).unwrap();
    let secrets = Arc::new(Secrets::default());
    let actor = WorkspaceActor::start(owner.claim_workspace().unwrap(), secrets).unwrap();
    let handle = actor.handle();
    let mut auth = Auth::new(&owner);
    let proof = auth.proof();
    let path = fixture(root.path(), "project");
    let project = register(&handle, &proof, path.clone());
    fs::create_dir(path.join("app")).unwrap();
    let code = "\0".repeat(200 * 1024);
    fs::write(path.join("app/Large.php"), &code).unwrap();
    let (_, source) = read(
        &handle,
        &proof,
        "source",
        ReadRequest::DebugSource {
            project_id: project.clone(),
            file: "app/Large.php".into(),
        },
    );
    assert!(source["code"].as_str().unwrap() == code);
    for (name, request) in [
        (
            "inspector",
            ReadRequest::Inspector {
                project_id: project.clone(),
                context: None,
            },
        ),
        (
            "inbox",
            ReadRequest::Inbox {
                project_id: project.clone(),
                query: String::new(),
                page: 0,
            },
        ),
        (
            "logs",
            ReadRequest::Logs {
                project_id: project.clone(),
                filter: LogFilter::default(),
            },
        ),
        (
            "debug",
            ReadRequest::DebugView {
                project_id: project.clone(),
            },
        ),
        (
            "document",
            ReadRequest::ConfigurationDocument {
                project_id: project.clone(),
            },
        ),
    ] {
        let _ = read(&handle, &proof, name, request);
    }
    let (_, document) = read(
        &handle,
        &proof,
        "document-to-save",
        ReadRequest::ConfigurationDocument {
            project_id: project.clone(),
        },
    );
    let saved = handle
        .submit_desktop(
            &proof,
            identity(project.as_str(), "save-before-plan"),
            Command::ConfigurationSave {
                id: project.as_str().into(),
                expected: None,
                text: document["text"].as_str().unwrap().into(),
                consent: true,
            },
        )
        .unwrap();
    assert_eq!(
        terminal(&handle, &proof, &saved).state,
        OperationState::Succeeded
    );
    let _ = read(
        &handle,
        &proof,
        "plan",
        ReadRequest::ConfigurationPlan {
            project_id: project.clone(),
            write_env: false,
        },
    );
    for (name, request) in [
        (
            "message",
            ReadRequest::MailMessage {
                project_id: project.clone(),
                message: id(&"0".repeat(32)),
            },
        ),
        (
            "part",
            ReadRequest::MailPart {
                project_id: project.clone(),
                message: id(&"0".repeat(32)),
                index: 0,
            },
        ),
        (
            "profile",
            ReadRequest::DebugProfile {
                project_id: project.clone(),
                profile: id("missing"),
            },
        ),
    ] {
        let receipt = handle.desktop_read(&proof, id(name), request).unwrap();
        assert_eq!(
            terminal(&handle, &proof, &receipt).state,
            OperationState::Failed
        );
        let detail = handle
            .failure_detail(&proof, &project, &receipt.operation_id)
            .unwrap();
        assert!(!detail.code.is_empty());
    }
    actor.shutdown().unwrap();
}

#[test]
fn polling_read_receipts_are_bounded_ephemeral_and_do_not_grow_sql_journal() {
    let root = common::root("df-desktop-");
    let owner = common::acquire(root.path()).unwrap();
    let actor = WorkspaceActor::start(
        owner.claim_workspace().unwrap(),
        Arc::new(Secrets::default()),
    )
    .unwrap();
    let handle = actor.handle();
    let mut auth = Auth::new(&owner);
    let proof = auth.proof();
    let project = register(&handle, &proof, fixture(root.path(), "project"));
    let db = Connection::open(root.path().join("control/operations.sqlite3")).unwrap();
    let before: i64 = db
        .query_row("SELECT count(*) FROM operations", [], |row| row.get(0))
        .unwrap();
    let mut first = None;
    let mut last = None;
    for n in 0..180 {
        let receipt = handle
            .desktop_read(
                &proof,
                id(&format!("poll-{n}")),
                ReadRequest::ConfigurationDocument {
                    project_id: project.clone(),
                },
            )
            .unwrap();
        assert_eq!(
            terminal(&handle, &proof, &receipt).state,
            OperationState::Succeeded
        );
        if first.is_none() {
            first = Some(receipt.clone());
        }
        last = Some(receipt);
    }
    assert_eq!(
        db.query_row::<i64, _, _>("SELECT count(*) FROM operations", [], |row| row.get(0))
            .unwrap(),
        before
    );
    let first = first.unwrap();
    let last = last.unwrap();
    assert_eq!(
        handle
            .status(&proof, &project, &first.operation_id)
            .unwrap_err(),
        ActorError::ReadExpired
    );
    assert!(
        handle
            .read_image(&proof, &project, &last.operation_id)
            .is_ok()
    );
    let listed = handle
        .operations(&proof, Some(project.clone()), None, 100)
        .unwrap();
    assert!(listed.items.is_empty());
    assert!(listed.active_reads.is_empty());
    actor.shutdown().unwrap();
    drop(owner);
    let owner = common::acquire(root.path()).unwrap();
    let mut auth = Auth::new(&owner);
    let proof = auth.proof();
    let actor = WorkspaceActor::start(
        owner.claim_workspace().unwrap(),
        Arc::new(Secrets::default()),
    )
    .unwrap();
    assert_eq!(
        actor
            .handle()
            .status(&proof, &project, &last.operation_id)
            .unwrap_err(),
        ActorError::ReadExpired
    );
    actor.shutdown().unwrap();
}

#[test]
fn background_reads_do_not_set_mutation_busy_and_active_ids_remain_recoverable() {
    let root = common::root("df-desktop-");
    let owner = common::acquire(root.path()).unwrap();
    let actor = WorkspaceActor::start(
        owner.claim_workspace().unwrap(),
        Arc::new(Secrets::default()),
    )
    .unwrap();
    let handle = actor.handle();
    let mut auth = Auth::new(&owner);
    let proof = auth.proof();
    let project = register(&handle, &proof, fixture(root.path(), "project"));
    let lock = Connection::open(root.path().join("projects/projects.sqlite3")).unwrap();
    lock.execute_batch("BEGIN EXCLUSIVE;").unwrap();
    let read = handle
        .desktop_read(
            &proof,
            id("background"),
            ReadRequest::ConfigurationDocument {
                project_id: project.clone(),
            },
        )
        .unwrap();
    let until = Instant::now() + Duration::from_secs(6);
    while handle
        .status(&proof, &project, &read.operation_id)
        .unwrap()
        .state
        != OperationState::Running
    {
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(2));
    }
    assert!(
        !handle.cached_snapshot(&proof).unwrap().busy,
        "background reads cannot disable mutation controls"
    );
    let page = handle
        .operations(&proof, Some(project.clone()), None, 10)
        .unwrap();
    assert!(
        page.active_reads
            .iter()
            .any(|entry| entry.operation_id == read.operation_id)
    );
    let command = handle
        .submit_desktop(
            &proof,
            identity(project.as_str(), "user-command"),
            Command::Favorite {
                id: project.as_str().into(),
                favorite: true,
            },
        )
        .unwrap();
    assert!(
        handle.cached_snapshot(&proof).unwrap().busy,
        "queued user effect must remain visible"
    );
    handle.cancel(&proof, &project, &read.operation_id).unwrap();
    lock.execute_batch("COMMIT;").unwrap();
    assert_eq!(
        terminal(&handle, &proof, &read).state,
        OperationState::Cancelled
    );
    assert_eq!(
        terminal(&handle, &proof, &command).state,
        OperationState::Succeeded
    );
    actor.shutdown().unwrap();
}

#[test]
fn owned_operation_pages_pin_dataset_and_do_not_return_foreign_namespace() {
    let root = common::root("df-desktop-");
    let owner = common::acquire(root.path()).unwrap();
    let actor = WorkspaceActor::start(
        owner.claim_workspace().unwrap(),
        Arc::new(Secrets::default()),
    )
    .unwrap();
    let handle = actor.handle();
    let mut auth = Auth::new(&owner);
    let proof = auth.proof();
    let project = register(&handle, &proof, fixture(root.path(), "project"));
    for n in 0..8 {
        let job = handle
            .submit_desktop(
                &proof,
                identity(project.as_str(), &format!("visit-{n}")),
                Command::Visit {
                    id: project.as_str().into(),
                },
            )
            .unwrap();
        terminal(&handle, &proof, &job);
    }
    let first = handle
        .operations(&proof, Some(project.clone()), None, 3)
        .unwrap();
    assert_eq!(first.items.len(), 3);
    let mut items = first.items;
    let mut cursor = first.next_cursor;
    let later = handle
        .submit_desktop(
            &proof,
            identity(project.as_str(), "late"),
            Command::Visit {
                id: project.as_str().into(),
            },
        )
        .unwrap();
    terminal(&handle, &proof, &later);
    while let Some(next) = cursor {
        let page = handle
            .operations(&proof, Some(project.clone()), Some(next), 3)
            .unwrap();
        items.extend(page.items);
        cursor = page.next_cursor;
    }
    assert_eq!(items.len(), 8);
    assert!(
        items
            .iter()
            .all(|item| item.project_id == project && item.operation_id != later.operation_id)
    );
    let cursor = handle
        .operations(&proof, Some(project.clone()), None, 1)
        .unwrap()
        .next_cursor
        .unwrap();
    assert!(matches!(
        handle.operations(&proof, Some(id("foreign")), Some(cursor), 1),
        Err(ActorError::InvalidInput)
    ));
    actor.shutdown().unwrap();
}

#[test]
fn volatile_secret_idempotency_never_journals_secret_or_offline_verifier() {
    let root = common::root("df-desktop-");
    let owner = common::acquire(root.path()).unwrap();
    let secrets = Arc::new(Secrets::default());
    let actor = WorkspaceActor::start(owner.claim_workspace().unwrap(), secrets.clone()).unwrap();
    let handle = actor.handle();
    let mut auth = Auth::new(&owner);
    let proof = auth.proof();
    let project = register(&handle, &proof, fixture(root.path(), "project"));
    let canary = "pin-0420-only-fixture";
    let request = AccessRequest::ConfigurationSecret {
        project_id: project.clone(),
        reference: "fixture-secret".into(),
        value: SecretValue::new(canary).unwrap(),
    };
    let receipt = handle
        .desktop_access(&proof, id("secret-original"), request.clone())
        .unwrap();
    assert_eq!(
        terminal(&handle, &proof, &receipt).state,
        OperationState::Succeeded
    );
    let sets = secrets.sets.load(Ordering::SeqCst);
    assert_eq!(
        handle
            .desktop_access(&proof, id("secret-original"), request.clone())
            .unwrap()
            .operation_id,
        receipt.operation_id
    );
    assert_eq!(secrets.sets.load(Ordering::SeqCst), sets);
    let changed = AccessRequest::ConfigurationSecret {
        project_id: project.clone(),
        reference: "fixture-secret".into(),
        value: SecretValue::new("different-pin").unwrap(),
    };
    assert!(matches!(
        handle.desktop_access(&proof, id("secret-original"), changed),
        Err(ActorError::IdempotencyConflict)
    ));
    let db = Connection::open(root.path().join("control/operations.sqlite3")).unwrap();
    let stored: String = db
        .query_row(
            "SELECT fingerprint FROM operations WHERE operation_id=?1",
            [receipt.operation_id.as_str()],
            |row| row.get(0),
        )
        .unwrap();
    assert!(stored.starts_with("volatile-secret-v1:"));
    let raw = serde_json::to_vec(&(project.clone(), "desktop_access", &request)).unwrap();
    assert!(
        stored
            != serde_json::to_value(payload::sha256(&raw))
                .unwrap()
                .as_str()
                .unwrap()
    );
    for n in 0..128 {
        let job = handle
            .desktop_access(&proof, id(&format!("secret-{n}")), request.clone())
            .unwrap();
        assert_eq!(
            terminal(&handle, &proof, &job).state,
            OperationState::Succeeded
        );
    }
    assert!(matches!(
        handle.desktop_access(&proof, id("secret-original"), request.clone()),
        Err(ActorError::IdempotencyUnverifiable)
    ));
    assert_eq!(
        handle
            .operation_by_key(&proof, project.clone(), id("secret-original"))
            .unwrap()
            .unwrap()
            .operation_id,
        receipt.operation_id
    );
    assert!(
        handle
            .operation_by_key(&proof, id("foreign-project"), id("secret-original"))
            .unwrap()
            .is_none()
    );
    actor.shutdown().unwrap();
    drop(owner);
    let bytes = fs::read(root.path().join("control/operations.sqlite3")).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains(canary));
    assert!(
        !text.contains(
            serde_json::to_value(payload::sha256(&raw))
                .unwrap()
                .as_str()
                .unwrap()
        )
    );
    let owner = common::acquire(root.path()).unwrap();
    let actor = WorkspaceActor::start(owner.claim_workspace().unwrap(), secrets).unwrap();
    let mut auth = Auth::new(&owner);
    let proof = auth.proof();
    assert!(matches!(
        actor
            .handle()
            .desktop_access(&proof, id("secret-original"), request),
        Err(ActorError::IdempotencyUnverifiable)
    ));
    assert_eq!(
        actor
            .handle()
            .status(&proof, &project, &receipt.operation_id)
            .unwrap()
            .state,
        OperationState::Succeeded
    );
    assert_eq!(
        actor
            .handle()
            .operation_by_key(&proof, project.clone(), id("secret-original"))
            .unwrap()
            .unwrap()
            .operation_id,
        receipt.operation_id
    );
    actor.shutdown().unwrap();
}

#[test]
fn raw_http_query_and_yaml_commands_use_volatile_proof_without_database_verifiers() {
    let root = common::root("df-desktop-");
    let owner = common::acquire(root.path()).unwrap();
    let secrets = Arc::new(Secrets::default());
    let actor = WorkspaceActor::start(owner.claim_workspace().unwrap(), secrets.clone()).unwrap();
    let handle = actor.handle();
    let mut auth = Auth::new(&owner);
    let proof = auth.proof();
    let path = fixture(root.path(), "project");
    let project = register(&handle, &proof, path.clone());
    let (_, document) = read(
        &handle,
        &proof,
        "raw-policy-document",
        ReadRequest::ConfigurationDocument {
            project_id: project.clone(),
        },
    );
    let canary = "fp-policy-canary-pin-0007";
    let yaml = format!(
        "{}\n# password: {canary}\n",
        document["text"].as_str().unwrap()
    );
    let http:Command=serde_json::from_value(serde_json::json!({"action":"debug_http","id":project,"request":{"method":"POST","path":format!("/login?token={canary}"),"headers":{"authorization":canary},"body":format!("password={canary}")}})).unwrap();
    let commands = vec![
        (
            "raw-yaml",
            Command::ConfigurationSave {
                id: project.as_str().into(),
                expected: None,
                text: yaml,
                consent: true,
            },
        ),
        ("raw-http", http),
        (
            "raw-query",
            Command::DebugRequest {
                id: project.as_str().into(),
                path: format!("/login?token={canary}"),
            },
        ),
    ];
    let mut saved = Vec::new();
    let mut digests = Vec::new();
    for (key, command) in commands {
        let receipt = handle
            .submit_desktop(&proof, identity(project.as_str(), key), command.clone())
            .unwrap();
        let complete = terminal(&handle, &proof, &receipt);
        if key == "raw-yaml" {
            assert_eq!(complete.state, OperationState::Succeeded);
            assert!(
                fs::read_to_string(path.join("devforge.yaml"))
                    .unwrap()
                    .contains(canary)
            );
        } else {
            assert_eq!(
                complete.state,
                OperationState::Failed,
                "no running debugger/runtime fixture, no HTTP request is executed"
            );
        }
        let raw = serde_json::to_vec(&(project.clone(), &command)).unwrap();
        let digest = serde_json::to_value(payload::sha256(&raw))
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned();
        let db = Connection::open(root.path().join("control/operations.sqlite3")).unwrap();
        let stored: String = db
            .query_row(
                "SELECT fingerprint FROM operations WHERE operation_id=?1",
                [receipt.operation_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert!(stored.starts_with("volatile-secret-v1:"));
        assert!(stored != digest);
        digests.push(digest);
        let duplicate = handle
            .submit_desktop(&proof, identity(project.as_str(), key), command.clone())
            .unwrap();
        assert_eq!(duplicate.operation_id, receipt.operation_id);
        assert_eq!(duplicate.state, complete.state);
        let mut changed = command.clone();
        match &mut changed {
            Command::ConfigurationSave { text, .. } => text.push_str("# changed\n"),
            Command::DebugHttp { request, .. } => request.body.push('x'),
            Command::DebugRequest { path, .. } => path.push('x'),
            _ => panic!("unexpected command"),
        }
        assert!(matches!(
            handle.submit_desktop(&proof, identity(project.as_str(), key), changed),
            Err(ActorError::IdempotencyConflict)
        ));
        saved.push((key, command, receipt));
    }
    actor.shutdown().unwrap();
    drop(owner);
    let bytes = fs::read(root.path().join("control/operations.sqlite3")).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains(canary));
    for digest in &digests {
        assert!(!text.contains(digest));
    }
    let owner = common::acquire(root.path()).unwrap();
    let actor = WorkspaceActor::start(owner.claim_workspace().unwrap(), secrets).unwrap();
    let handle = actor.handle();
    let mut auth = Auth::new(&owner);
    let proof = auth.proof();
    for (key, command, receipt) in saved {
        assert!(matches!(
            handle.submit_desktop(&proof, identity(project.as_str(), key), command),
            Err(ActorError::IdempotencyUnverifiable)
        ));
        assert_eq!(
            handle
                .operation_by_key(&proof, project.clone(), id(key))
                .unwrap()
                .unwrap()
                .operation_id,
            receipt.operation_id
        );
    }
    actor.shutdown().unwrap();
}

#[test]
fn discovery_owner_loads_only_saved_root_and_uses_atomic_preferences_patch() {
    let root = common::root("df-desktop-");
    let watched = common::root("df-watch-");
    let owner = common::acquire(root.path()).unwrap();
    let mut settings = devforge_platform::SqliteSettings::open(root.path()).unwrap();
    settings
        .save(&devforge_core::Settings {
            language: "ru".into(),
            theme: devforge_core::Theme::Dark,
            project_folder: None,
        })
        .unwrap();
    let actor = WorkspaceActor::start(
        owner.claim_workspace().unwrap(),
        Arc::new(Secrets::default()),
    )
    .unwrap();
    let handle = actor.handle();
    let mut auth = Auth::new(&owner);
    let proof = auth.proof();
    assert!(
        handle
            .cached_snapshot(&proof)
            .unwrap()
            .snapshot
            .discovery
            .root
            .is_none()
    );
    let project = fixture(watched.path(), "detected");
    let configured = handle
        .desktop_access(
            &proof,
            id("folder"),
            AccessRequest::DiscoveryRoot {
                path: watched.path().to_string_lossy().into_owned(),
            },
        )
        .unwrap();
    assert_eq!(
        terminal(&handle, &proof, &configured).state,
        OperationState::Succeeded
    );
    handle.discovery_request(&proof).unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let snapshot = handle.cached_snapshot(&proof).unwrap();
        if snapshot.snapshot.discovery.entries.iter().any(|entry| {
            entry.path == fs::canonicalize(&project).unwrap() && entry.state == "needs_runtime"
        }) {
            break;
        }
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(20));
    }
    let persisted = settings.load().unwrap();
    assert_eq!(persisted.language, "ru");
    assert_eq!(persisted.theme, devforge_core::Theme::Dark);
    assert_eq!(
        persisted.project_folder.as_deref(),
        Some(
            fs::canonicalize(watched.path())
                .unwrap()
                .to_string_lossy()
                .as_ref()
        )
    );
    actor.shutdown().unwrap();
}
