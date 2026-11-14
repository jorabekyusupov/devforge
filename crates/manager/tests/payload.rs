mod common;
use devforge_control::{Id, OperationState};
use devforge_core::{ErrorCode, SecretStore};
use devforge_manager::{
    Owner, PeerIdentity,
    actor::{ActorError, ActorHandle, OperationIdentity, Receipt, SnapshotView, WorkspaceActor},
    auth::{
        DesktopAuth, DesktopAuthority, Policy as AuthPolicy, SessionCredential,
        TrustedDesktopAction,
    },
    payload::{
        self, CHUNK_BYTES, Error, MAX_COMMAND_BYTES, Policy, Store, UploadDescriptor, UploadKey,
    },
};
use devforge_runtime::package::Target;
use devforge_workspace::{Command, Log, Snapshot};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

#[derive(Default)]
struct Secrets(Mutex<BTreeMap<String, String>>);
impl SecretStore for Secrets {
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
struct NoSecrets;
impl SecretStore for NoSecrets {
    fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
        panic!("metadata fixture cannot probe native credentials")
    }
    fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
        panic!("metadata fixture cannot change native credentials")
    }
    fn delete(&self, _: &str) -> Result<(), ErrorCode> {
        panic!("metadata fixture cannot delete native credentials")
    }
}
struct Auth {
    auth: DesktopAuth,
    peer: PeerIdentity,
    session: SessionCredential,
}
impl Auth {
    fn new(owner: &Owner, pid: u32) -> Self {
        let peer = PeerIdentity {
            user: "payload-policy-fixture".into(),
            process_id: Some(pid),
        };
        let store = Secrets::default();
        let mut auth = DesktopAuth::new(
            owner.endpoint().clone(),
            peer.user.clone(),
            AuthPolicy::default(),
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
fn http(project: &str, body: String) -> Command {
    serde_json::from_value(json!({"action":"debug_http","id":project,"request":{"method":"POST","path":"/large","headers":{"authorization":"fixture-upload-secret"},"body":body}})).unwrap()
}
fn key(descriptor: &UploadDescriptor) -> UploadKey {
    UploadKey {
        upload_id: descriptor.upload_id.clone(),
        epoch: descriptor.epoch.clone(),
    }
}
fn upload(
    store: &mut Store,
    proof: &DesktopAuthority<'_>,
    bytes: &[u8],
    name: &str,
) -> UploadDescriptor {
    let descriptor = store
        .begin(
            proof,
            identity("project-a", name),
            bytes.len() as u64,
            payload::sha256(bytes),
            Instant::now(),
        )
        .unwrap();
    for (index, chunk) in bytes.chunks(CHUNK_BYTES).enumerate() {
        let encoded = payload::hex(chunk);
        assert!(serde_json::to_vec(&json!({"method":"upload_chunk","session":"0".repeat(64),"upload_id":descriptor.upload_id,"epoch":descriptor.epoch,"index":index,"data_hex":encoded})).unwrap().len()<=devforge_manager::MAX_REQUEST_BYTES);
        store
            .chunk(
                proof,
                &key(&descriptor),
                index as u32,
                &payload::hex(chunk),
                Instant::now(),
            )
            .unwrap();
    }
    descriptor
}

#[test]
fn one_mib_nul_and_unicode_http_commands_roundtrip_without_executing_http() {
    let root = common::root("df-payload-");
    let owner = common::acquire(root.path()).unwrap();
    let mut auth = Auth::new(&owner, 1);
    let proof = auth.proof();
    let mut store = Store::new(owner.endpoint().clone(), Policy::default()).unwrap();
    let mut unicode = "\0Ў🧪".repeat((1024 * 1024) / 7);
    unicode.push_str(&"\0".repeat(1024 * 1024 - unicode.len()));
    for (name, body) in [("nul", "\0".repeat(1024 * 1024)), ("unicode", unicode)] {
        assert_eq!(body.len(), 1024 * 1024);
        let command = http("project-a", body.clone());
        if let Command::DebugHttp { request, .. } = &command {
            assert!(request.validate().is_ok());
        }
        let bytes = payload::encode_command(&command).unwrap();
        assert!(bytes.len() <= MAX_COMMAND_BYTES);
        if name == "nul" {
            assert!(bytes.len() > 6 * 1024 * 1024);
        }
        let descriptor = upload(&mut store, &proof, &bytes, name);
        let (operation, decoded) = store
            .finalize(&proof, &key(&descriptor), Instant::now())
            .unwrap()
            .into_parts(&proof)
            .unwrap();
        assert_eq!(operation.project_id, id("project-a"));
        match decoded {
            Command::DebugHttp { request, .. } => {
                assert!(request.body == body);
                assert!(
                    request
                        .headers
                        .get("authorization")
                        .is_some_and(|value| value == "fixture-upload-secret")
                );
            }
            _ => panic!("wrong command variant"),
        }
        assert_eq!(
            store
                .finalize(&proof, &key(&descriptor), Instant::now())
                .unwrap_err(),
            Error::UnknownUpload
        );
    }
    // This is a codec/admission-preparation test, not a server/HTTP success pass.
    assert_eq!(
        fs::read_dir(root.path().join("control")).unwrap().count(),
        1
    );
}

#[test]
fn gaps_tamper_incomplete_expiry_overflow_and_replay_return_no_command() {
    let root = common::root("df-payload-");
    let owner = common::acquire(root.path()).unwrap();
    let mut auth = Auth::new(&owner, 1);
    let proof = auth.proof();
    let mut store = Store::new(owner.endpoint().clone(), Policy::default()).unwrap();
    let bytes = payload::encode_command(&Command::Visit {
        id: "project-a".into(),
    })
    .unwrap();
    let now = Instant::now();
    let begin = |store: &mut Store, name: &str| {
        store
            .begin(
                &proof,
                identity("project-a", name),
                bytes.len() as u64,
                payload::sha256(&bytes),
                now,
            )
            .unwrap()
    };
    let gap = begin(&mut store, "gap");
    assert_eq!(
        store
            .chunk(&proof, &key(&gap), 1, &payload::hex(&bytes), now)
            .unwrap_err(),
        Error::OrderMismatch
    );
    assert_eq!(
        store.finalize(&proof, &key(&gap), now).unwrap_err(),
        Error::UnknownUpload
    );
    let incomplete = begin(&mut store, "incomplete");
    assert_eq!(
        store.finalize(&proof, &key(&incomplete), now).unwrap_err(),
        Error::SizeMismatch
    );
    let tamper = begin(&mut store, "tamper");
    let mut changed = bytes.to_vec();
    changed[0] ^= 1;
    store
        .chunk(&proof, &key(&tamper), 0, &payload::hex(&changed), now)
        .unwrap();
    assert_eq!(
        store.finalize(&proof, &key(&tamper), now).unwrap_err(),
        Error::DigestMismatch
    );
    let over = begin(&mut store, "over");
    assert_eq!(
        store
            .chunk(
                &proof,
                &key(&over),
                0,
                &"0".repeat((CHUNK_BYTES + 1) * 2),
                now
            )
            .unwrap_err(),
        Error::InvalidInput
    );
    let expired = begin(&mut store, "expiry");
    assert_eq!(
        store
            .finalize(&proof, &key(&expired), now + Duration::from_secs(31))
            .unwrap_err(),
        Error::Expired
    );
    assert!(matches!(
        store.begin(
            &proof,
            identity("project-a", "huge"),
            (MAX_COMMAND_BYTES + 1) as u64,
            payload::sha256(&bytes),
            now
        ),
        Err(Error::InvalidInput)
    ));
    let invalid = br#"{"action":"visit","id":"project-a","unknown":true}"#;
    let descriptor = upload(&mut store, &proof, invalid, "invalid-json-schema");
    assert_eq!(
        store.finalize(&proof, &key(&descriptor), now).unwrap_err(),
        Error::InvalidInput
    );
}

#[test]
fn owner_peer_root_epoch_and_pending_reservations_are_enforced() {
    let root = common::root("df-payload-");
    let other = common::root("df-payload-");
    let owner = common::acquire(root.path()).unwrap();
    let foreign_owner = common::acquire(other.path()).unwrap();
    let mut a = Auth::new(&owner, 1);
    let mut b = Auth::new(&owner, 2);
    let mut foreign = Auth::new(&foreign_owner, 1);
    let a = a.proof();
    let b = b.proof();
    let foreign = foreign.proof();
    let mut store = Store::new(owner.endpoint().clone(), Policy::default()).unwrap();
    let bytes = payload::encode_command(&Command::Visit {
        id: "project-a".into(),
    })
    .unwrap();
    let now = Instant::now();
    let descriptor = store
        .begin(
            &a,
            identity("project-a", "owner"),
            bytes.len() as u64,
            payload::sha256(&bytes),
            now,
        )
        .unwrap();
    assert_eq!(
        store
            .chunk(&b, &key(&descriptor), 0, &payload::hex(&bytes), now)
            .unwrap_err(),
        Error::Unauthorized
    );
    assert_eq!(
        store
            .chunk(&foreign, &key(&descriptor), 0, &payload::hex(&bytes), now)
            .unwrap_err(),
        Error::Unauthorized
    );
    let mut wrong = key(&descriptor);
    wrong.epoch = id("other-epoch");
    assert_eq!(
        store.finalize(&a, &wrong, now).unwrap_err(),
        Error::Unauthorized
    );
    store
        .chunk(&a, &key(&descriptor), 0, &payload::hex(&bytes), now)
        .unwrap();
    store.finalize(&a, &key(&descriptor), now).unwrap();
    let one = store
        .begin(
            &a,
            identity("project-a", "one"),
            MAX_COMMAND_BYTES as u64,
            payload::sha256(&bytes),
            now,
        )
        .unwrap();
    let two = store
        .begin(
            &a,
            identity("project-a", "two"),
            MAX_COMMAND_BYTES as u64,
            payload::sha256(&bytes),
            now,
        )
        .unwrap();
    assert!(matches!(
        store.begin(
            &a,
            identity("project-a", "three"),
            1,
            payload::sha256(&bytes),
            now
        ),
        Err(Error::Capacity)
    ));
    store.abort(&a, &key(&one), now).unwrap();
    let three = store
        .begin(
            &a,
            identity("project-a", "three"),
            1,
            payload::sha256(&bytes),
            now,
        )
        .unwrap();
    store.abort(&a, &key(&two), now).unwrap();
    store.abort(&a, &key(&three), now).unwrap();
}

#[test]
fn snapshot_pages_pin_epoch_and_preserve_all_typed_log_bytes_with_resync_errors() {
    let root = common::root("df-payload-");
    let owner = common::acquire(root.path()).unwrap();
    let mut auth = Auth::new(&owner, 1);
    let mut foreign = Auth::new(&owner, 2);
    let proof = auth.proof();
    let foreign = foreign.proof();
    let snapshot = Snapshot {
        logs: (0..15)
            .map(|_| Log {
                project_id: Some("project-a".into()),
                process: "codec-fixture".into(),
                stream: "stdout".into(),
                text: "\0".repeat(16 * 1024),
            })
            .collect(),
        ..Snapshot::default()
    };
    let view = SnapshotView {
        snapshot: Arc::new(snapshot),
        observed_at: 123,
        busy: false,
        stopped: false,
        error: None,
        progress: None,
    };
    let mut store = Store::new(owner.endpoint().clone(), Policy::default()).unwrap();
    let now = Instant::now();
    let descriptor = store.snapshot(&proof, &view, now).unwrap();
    assert!(descriptor.size > devforge_manager::MAX_RESPONSE_BYTES as u64);
    assert!(matches!(
        store.page(&foreign, &descriptor.cursor, now),
        Err(Error::Unauthorized)
    ));
    let mut wrong = descriptor.cursor.clone();
    wrong.epoch = id("stale-epoch");
    assert!(matches!(
        store.page(&proof, &wrong, now),
        Err(Error::ResyncRequired)
    ));
    let mut wrong = descriptor.cursor.clone();
    wrong.offset = 1;
    assert!(matches!(
        store.page(&proof, &wrong, now),
        Err(Error::ResyncRequired)
    ));
    let newer = SnapshotView {
        snapshot: Arc::new(Snapshot::default()),
        observed_at: 124,
        busy: true,
        stopped: false,
        error: None,
        progress: None,
    };
    let newer = store.snapshot(&proof, &newer, now).unwrap();
    let mut cursor = Some(descriptor.cursor.clone());
    let mut bytes = Vec::new();
    while let Some(next) = cursor {
        let page = store.page(&proof, &next, now).unwrap();
        assert_eq!(page.offset, bytes.len() as u64);
        bytes.extend_from_slice(&payload::decode_chunk(&page.data_hex).unwrap());
        cursor = page.next_cursor;
    }
    assert_eq!(bytes.len() as u64, descriptor.size);
    assert!(payload::sha256(&bytes) == descriptor.digest);
    let data: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(data["observed_at"], 123);
    assert_eq!(data["snapshot"]["logs"].as_array().unwrap().len(), 15);
    for log in data["snapshot"]["logs"].as_array().unwrap() {
        assert!(log["text"].as_str().unwrap() == "\0".repeat(16 * 1024));
    }
    store
        .release(&proof, &descriptor.snapshot_id, &descriptor.epoch, now)
        .unwrap();
    assert!(matches!(
        store.page(&proof, &descriptor.cursor, now),
        Err(Error::ResyncRequired)
    ));
    assert!(matches!(
        store.page(&proof, &newer.cursor, now + Duration::from_secs(31)),
        Err(Error::ResyncRequired)
    ));
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
        thread::sleep(Duration::from_millis(5));
    }
}
fn project(root: &Path) -> PathBuf {
    let path = root.join("fixture");
    fs::create_dir_all(path.join("public")).unwrap();
    fs::write(path.join("public/index.php"), "<?php").unwrap();
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
        name: "Payload fixture".into(),
        php,
        node,
        domain: "payload.devforge.test".into(),
        https: false,
        vite: false,
        queue: false,
    }
}

#[test]
fn actual_actor_waiting_byte_cap_rejects_before_ledger_and_cancel_releases_body_memory() {
    let root = common::root("df-payload-");
    let owner = common::acquire(root.path()).unwrap();
    let mut auth = Auth::new(&owner, 1);
    let proof = auth.proof();
    let actor =
        WorkspaceActor::start(owner.claim_workspace().unwrap(), Arc::new(NoSecrets)).unwrap();
    let handle = actor.handle();
    let imported = handle
        .submit_desktop(
            &proof,
            identity("desktop-root", "import"),
            import(project(root.path())),
        )
        .unwrap();
    assert_eq!(
        terminal(&handle, &proof, &imported).state,
        OperationState::Succeeded
    );
    let project = handle
        .result(&proof, &imported.project_id, &imported.operation_id)
        .unwrap()["project_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let lock = Connection::open(root.path().join("projects/projects.sqlite3")).unwrap();
    lock.execute_batch("BEGIN EXCLUSIVE;").unwrap();
    let active = handle
        .submit_desktop(
            &proof,
            identity(&project, "active"),
            Command::Favorite {
                id: project.clone(),
                favorite: true,
            },
        )
        .unwrap();
    let until = Instant::now() + Duration::from_secs(6);
    while handle
        .status(&proof, &id(&project), &active.operation_id)
        .unwrap()
        .state
        != OperationState::Running
    {
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(2));
    }
    let command = http(&project, "\0".repeat(1024 * 1024));
    let size = payload::encode_command(&command).unwrap().len();
    let a = handle
        .submit_desktop(&proof, identity(&project, "large-a"), command.clone())
        .unwrap();
    let b = handle
        .submit_desktop(&proof, identity(&project, "large-b"), command.clone())
        .unwrap();
    assert_eq!(handle.queue_usage(&proof).unwrap().waiting_bytes, size * 2);
    let db = Connection::open(root.path().join("control/operations.sqlite3")).unwrap();
    let count: i64 = db
        .query_row("SELECT count(*) FROM operations", [], |row| row.get(0))
        .unwrap();
    assert!(matches!(
        handle.submit_desktop(&proof, identity(&project, "large-c"), command.clone()),
        Err(ActorError::Busy)
    ));
    assert_eq!(
        db.query_row::<i64, _, _>("SELECT count(*) FROM operations", [], |row| row.get(0))
            .unwrap(),
        count
    );
    handle
        .cancel(&proof, &id(&project), &a.operation_id)
        .unwrap();
    assert_eq!(handle.queue_usage(&proof).unwrap().waiting_bytes, size);
    let c = handle
        .submit_desktop(&proof, identity(&project, "large-c"), command)
        .unwrap();
    assert_eq!(handle.queue_usage(&proof).unwrap().waiting_bytes, size * 2);
    for job in [&b, &c] {
        handle
            .cancel(&proof, &id(&project), &job.operation_id)
            .unwrap();
    }
    assert_eq!(handle.queue_usage(&proof).unwrap().waiting_bytes, 0);
    lock.execute_batch("COMMIT;").unwrap();
    assert_eq!(
        terminal(&handle, &proof, &active).state,
        OperationState::Succeeded
    );
    for job in [&a, &b, &c] {
        assert_eq!(
            terminal(&handle, &proof, job).state,
            OperationState::Cancelled
        );
    }
    actor.shutdown().unwrap();
    assert!(
        !String::from_utf8_lossy(
            &fs::read(root.path().join("control/operations.sqlite3")).unwrap()
        )
        .contains("fixture-upload-secret")
    );
    // No HTTP operation executed: all large jobs were canceled before dequeue.
}
