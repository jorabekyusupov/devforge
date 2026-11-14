//! Real isolated Workspace metadata and actor scheduling, without downloads,
//! native credentials, system integration, fake runtimes or a fake Workspace.
mod common;
use devforge_control::{Id, OperationState};
use devforge_control_adapter::DomainScope;
use devforge_core::{ErrorCode, SecretStore};
use devforge_manager::{
    Owner, PeerIdentity,
    actor::{
        ActorError, ActorHandle, CancelOutcome, OperationIdentity, QUEUE_CAPACITY, Receipt,
        WorkspaceActor,
    },
    auth::{DesktopAuth, DesktopAuthority, Policy, SessionCredential, TrustedDesktopAction},
};
use devforge_runtime::package::Target;
use devforge_workspace::Command;
use rusqlite::{Connection, params};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

struct NoWorkspaceSecrets;
impl SecretStore for NoWorkspaceSecrets {
    fn get(&self, _: &str) -> Result<Option<String>, ErrorCode> {
        panic!("metadata must not read native credentials")
    }
    fn set(&self, _: &str, _: &str) -> Result<(), ErrorCode> {
        panic!("metadata must not write native credentials")
    }
    fn delete(&self, _: &str) -> Result<(), ErrorCode> {
        panic!("metadata must not delete native credentials")
    }
}
#[derive(Default)]
struct MemoryAuthSecrets(Mutex<BTreeMap<String, String>>);
impl SecretStore for MemoryAuthSecrets {
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
struct Authorization {
    auth: DesktopAuth,
    peer: PeerIdentity,
    session: SessionCredential,
}
impl Authorization {
    fn new(owner: &Owner, pid: u32, policy: Policy) -> Self {
        // In-memory credential policy fixture, not OS-peer/native-keychain proof.
        let peer = PeerIdentity {
            user: "isolated-actor-user".into(),
            process_id: Some(pid),
        };
        let mut auth =
            DesktopAuth::new(owner.endpoint().clone(), peer.user.clone(), policy).unwrap();
        let secrets = MemoryAuthSecrets::default();
        let action = TrustedDesktopAction::from_explicit_desktop_action();
        let key = auth
            .rotate_desktop_key(&action, &secrets, Instant::now())
            .unwrap();
        let session = auth
            .authenticate(&peer, &key, &secrets, Instant::now())
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
fn fixture(root: &Path, name: &str) -> PathBuf {
    let path = root.join("fixtures").join(name);
    fs::create_dir_all(path.join("public")).unwrap();
    fs::write(path.join("public/index.php"), "<?php").unwrap();
    fs::write(path.join("composer.json"),r#"{"require":{"php":"^8.3","laravel/framework":"^13"},"scripts":{"post-install-cmd":"touch must-not-run"}}"#).unwrap();
    fs::write(path.join(".env"), "UNRELATED=very-private-fixture-value\n").unwrap();
    path
}
fn import(path: PathBuf, domain: &str) -> Command {
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
        name: "Private metadata label never stored in operation ledger".into(),
        php,
        node,
        domain: domain.into(),
        https: false,
        vite: false,
        queue: false,
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
        assert!(Instant::now() < until, "real metadata job did not finish");
        thread::sleep(Duration::from_millis(5));
    }
}
fn register(handle: &ActorHandle, proof: &DesktopAuthority<'_>, root: &Path, name: &str) -> String {
    let command = import(fixture(root, name), &format!("{name}.devforge.test"));
    let receipt = handle
        .submit_desktop(proof, identity("desktop-root", name), command)
        .unwrap();
    assert_eq!(
        terminal(handle, proof, &receipt).state,
        OperationState::Succeeded
    );
    handle
        .result(proof, &receipt.project_id, &receipt.operation_id)
        .unwrap()["project_id"]
        .as_str()
        .unwrap()
        .into()
}

#[test]
fn single_workspace_metadata_idempotency_persistence_and_lease_lifetime() {
    let root = common::root("df-actor-");
    let owner = common::acquire(root.path()).unwrap();
    let mut auth = Authorization::new(&owner, 11, Policy::default());
    let proof = auth.proof();
    let permit = owner.claim_workspace().unwrap();
    assert!(owner.claim_workspace().is_err());
    let actor = WorkspaceActor::start(permit, Arc::new(NoWorkspaceSecrets)).unwrap();
    let handle = actor.handle();
    drop(owner);
    assert!(Owner::acquire(root.path()).is_err());
    let path = fixture(root.path(), "alpha");
    let command = import(path.clone(), "alpha.devforge.test");
    let request = identity("desktop-root", "import-alpha");
    let receipt = handle
        .submit_desktop(&proof, request.clone(), command.clone())
        .unwrap();
    let duplicate = handle
        .submit_desktop(&proof, request.clone(), command.clone())
        .unwrap();
    assert_eq!(receipt.operation_id, duplicate.operation_id);
    assert_eq!(
        terminal(&handle, &proof, &receipt).state,
        OperationState::Succeeded
    );
    let project = handle
        .result(&proof, &receipt.project_id, &receipt.operation_id)
        .unwrap()["project_id"]
        .as_str()
        .unwrap()
        .to_string();
    let mut changed = command.clone();
    if let Command::Import { ref mut name, .. } = changed {
        *name = "different".into();
    }
    assert!(matches!(
        handle.submit_desktop(&proof, request.clone(), changed),
        Err(ActorError::IdempotencyConflict)
    ));
    for (key, command) in [
        (
            "favorite",
            Command::Favorite {
                id: project.clone(),
                favorite: true,
            },
        ),
        (
            "visit",
            Command::Visit {
                id: project.clone(),
            },
        ),
    ] {
        let job = handle
            .submit_desktop(&proof, identity(&project, key), command)
            .unwrap();
        assert_eq!(
            terminal(&handle, &proof, &job).state,
            OperationState::Succeeded
        );
    }
    let snapshot = handle.cached_snapshot(&proof).unwrap();
    assert_eq!(snapshot.snapshot.projects.len(), 1);
    assert!(snapshot.snapshot.projects[0].navigation.favorite);
    assert!(snapshot.snapshot.projects[0].navigation.last_opened_ms > 0);
    assert_eq!(
        fs::read_to_string(path.join(".env")).unwrap(),
        "UNRELATED=very-private-fixture-value\n"
    );
    assert!(!path.join("must-not-run").exists());
    actor.shutdown().unwrap();
    let ledger = fs::read(root.path().join("control/operations.sqlite3")).unwrap();
    let ledger = String::from_utf8_lossy(&ledger);
    assert!(!ledger.contains("Private metadata label"));
    assert!(!ledger.contains("very-private-fixture-value"));
    assert!(!ledger.contains("public/index.php"));

    let owner = common::acquire(root.path()).unwrap();
    let mut new_auth = Authorization::new(&owner, 99, Policy::default());
    let new_proof = new_auth.proof();
    let reopened = WorkspaceActor::start(
        owner.claim_workspace().unwrap(),
        Arc::new(NoWorkspaceSecrets),
    )
    .unwrap();
    let next = reopened.handle();
    let replay = next.submit_desktop(&new_proof, request, command).unwrap();
    assert_eq!(replay.operation_id, receipt.operation_id);
    assert_eq!(
        replay.client_id, receipt.client_id,
        "logical desktop ID must survive process/session changes"
    );
    assert_eq!(replay.state, OperationState::Succeeded);
    assert_eq!(
        next.result(&new_proof, &replay.project_id, &replay.operation_id)
            .unwrap_err(),
        ActorError::ResultUnavailable
    );
    assert_eq!(
        next.cached_snapshot(&new_proof)
            .unwrap()
            .snapshot
            .projects
            .len(),
        1
    );
    reopened.shutdown().unwrap();
}

#[test]
fn real_sqlite_contention_keeps_cache_and_cancel_fast_with_bounded_admission() {
    let root = common::root("df-actor-");
    let owner = common::acquire(root.path()).unwrap();
    let mut auth = Authorization::new(&owner, 12, Policy::default());
    let proof = auth.proof();
    let actor = WorkspaceActor::start(
        owner.claim_workspace().unwrap(),
        Arc::new(NoWorkspaceSecrets),
    )
    .unwrap();
    let handle = actor.handle();
    let a = register(&handle, &proof, root.path(), "contention-a");
    let b = register(&handle, &proof, root.path(), "contention-b");
    let block = Connection::open(root.path().join("projects/projects.sqlite3")).unwrap();
    block.execute_batch("BEGIN EXCLUSIVE;").unwrap();
    let active = handle
        .submit_desktop(
            &proof,
            identity(&a, "active"),
            Command::Favorite {
                id: a.clone(),
                favorite: true,
            },
        )
        .unwrap();
    // An idle periodic snapshot can begin its real SQLite read just before this
    // job is dequeued. ProjectStore deliberately waits up to three seconds on
    // this fixture's exclusive lock. Cover that existing bound plus CI scheduling
    // before asserting Running; this does not relax cache/cancel latency checks.
    let until = Instant::now() + Duration::from_secs(6);
    while handle
        .status(&proof, &id(&a), &active.operation_id)
        .unwrap()
        .state
        != OperationState::Running
    {
        assert!(Instant::now() < until);
        thread::sleep(Duration::from_millis(2));
    }
    let before = handle.cached_snapshot(&proof).unwrap();
    let started = Instant::now();
    let cached = handle.cached_snapshot(&proof).unwrap();
    assert!(started.elapsed() < Duration::from_millis(500));
    assert!(cached.busy);
    assert_eq!(cached.observed_at, before.observed_at);
    let mut queued = Vec::new();
    for n in 0..QUEUE_CAPACITY {
        let project = if n == 0 { &a } else { &b };
        queued.push(
            handle
                .submit_desktop(
                    &proof,
                    identity(project, &format!("queued-{n}")),
                    Command::Favorite {
                        id: project.clone(),
                        favorite: n != 0,
                    },
                )
                .unwrap(),
        );
    }
    let database = Connection::open(root.path().join("control/operations.sqlite3")).unwrap();
    let count: i64 = database
        .query_row("SELECT count(*) FROM operations", [], |row| row.get(0))
        .unwrap();
    assert!(matches!(
        handle.submit_desktop(
            &proof,
            identity(&b, "capacity-rejected"),
            Command::Visit { id: b.clone() }
        ),
        Err(ActorError::Busy)
    ));
    assert_eq!(
        database
            .query_row::<i64, _, _>("SELECT count(*) FROM operations", [], |row| row.get(0))
            .unwrap(),
        count
    );
    let started = Instant::now();
    assert_eq!(
        handle
            .cancel(&proof, &id(&a), &queued[0].operation_id)
            .unwrap(),
        CancelOutcome::Requested
    );
    assert!(started.elapsed() < Duration::from_millis(500));
    assert_eq!(
        handle
            .cancel(&proof, &id(&a), &active.operation_id)
            .unwrap(),
        CancelOutcome::Requested
    );
    block.execute_batch("COMMIT;").unwrap();
    // The real ProjectStore lock waits at most three seconds. Durable admission
    // of 32 receipts can exceed that on a busy host; that is a truthful domain
    // failure, not successful cancellation or rollback. Do not extend production
    // timeouts to make this fixture deterministic. A fast release still succeeds.
    let active_terminal = terminal(&handle, &proof, &active);
    let active_succeeded = match active_terminal.state {
        OperationState::Succeeded => true,
        OperationState::Failed => {
            let detail = handle
                .failure_detail(&proof, &id(&a), &active.operation_id)
                .unwrap();
            assert_eq!(detail.scope, DomainScope::Project);
            assert_eq!(detail.code, "storage_unavailable");
            false
        }
        state => panic!("metadata lock must succeed or report its storage failure: {state:?}"),
    };
    assert_eq!(
        terminal(&handle, &proof, &queued[0]).state,
        OperationState::Cancelled
    );
    for receipt in &queued[1..] {
        assert_eq!(
            terminal(&handle, &proof, receipt).state,
            OperationState::Succeeded
        );
    }
    let snapshot = handle.cached_snapshot(&proof).unwrap();
    assert_eq!(
        snapshot
            .snapshot
            .projects
            .iter()
            .find(|row| row.project.id == a)
            .unwrap()
            .navigation
            .favorite,
        active_succeeded,
        "cancelled queued A must preserve the actual active A outcome"
    );
    assert!(
        snapshot
            .snapshot
            .projects
            .iter()
            .find(|row| row.project.id == b)
            .unwrap()
            .navigation
            .favorite
    );
    assert_eq!(
        handle
            .cancel(&proof, &id(&b), &queued[1].operation_id)
            .unwrap(),
        CancelOutcome::AlreadyTerminal
    );
    actor.shutdown().unwrap();
}

#[test]
fn restart_marks_queued_interrupted_and_running_unknown_without_replay() {
    let root = common::root("df-actor-");
    let owner = common::acquire(root.path()).unwrap();
    let mut auth = Authorization::new(&owner, 1, Policy::default());
    let proof = auth.proof();
    let actor = WorkspaceActor::start(
        owner.claim_workspace().unwrap(),
        Arc::new(NoWorkspaceSecrets),
    )
    .unwrap();
    let handle = actor.handle();
    let project = register(&handle, &proof, root.path(), "recovery");
    let original = handle
        .submit_desktop(
            &proof,
            identity(&project, "client-identity"),
            Command::Visit {
                id: project.clone(),
            },
        )
        .unwrap();
    terminal(&handle, &proof, &original);
    actor.shutdown().unwrap();
    drop(owner);
    let db = Connection::open(root.path().join("control/operations.sqlite3")).unwrap();
    for (operation, state) in [
        ("was-queued", "queued"),
        ("was-running", "running"),
        ("was-cancelling", "cancel_requested"),
    ] {
        db.execute("INSERT INTO operations(operation_id,client_id,project_id,idempotency_key,fingerprint,state,created_at,updated_at) VALUES(?1,?2,?3,?1,?4,?5,1,1)",params![operation,original.client_id.as_str(),project,"a".repeat(64),state]).unwrap();
    }
    drop(db);
    let owner = common::acquire(root.path()).unwrap();
    let mut auth = Authorization::new(&owner, 2, Policy::default());
    let proof = auth.proof();
    let reopened = WorkspaceActor::start(
        owner.claim_workspace().unwrap(),
        Arc::new(NoWorkspaceSecrets),
    )
    .unwrap();
    let handle = reopened.handle();
    for (operation, state) in [
        ("was-queued", OperationState::Interrupted),
        ("was-running", OperationState::Unknown),
        ("was-cancelling", OperationState::Unknown),
    ] {
        let receipt = handle
            .status(&proof, &id(&project), &id(operation))
            .unwrap();
        assert_eq!(receipt.state, state);
        assert_eq!(receipt.code.as_deref(), Some("owner_interrupted"));
        assert_eq!(
            handle
                .result(&proof, &id(&project), &id(operation))
                .unwrap_err(),
            ActorError::ResultUnavailable
        );
    }
    assert_eq!(
        handle
            .cached_snapshot(&proof)
            .unwrap()
            .snapshot
            .projects
            .len(),
        1
    );
    assert!(
        !handle.cached_snapshot(&proof).unwrap().snapshot.projects[0]
            .navigation
            .favorite
    );
    reopened.shutdown().unwrap();
}

#[test]
fn real_domain_failure_is_terminal_coarse_and_idempotent() {
    let root = common::root("df-actor-");
    let owner = common::acquire(root.path()).unwrap();
    let mut auth = Authorization::new(&owner, 7, Policy::default());
    let proof = auth.proof();
    let actor = WorkspaceActor::start(
        owner.claim_workspace().unwrap(),
        Arc::new(NoWorkspaceSecrets),
    )
    .unwrap();
    let handle = actor.handle();
    let request = identity("missing-project", "missing-visit");
    let command = Command::Visit {
        id: "missing-project".into(),
    };
    let receipt = handle
        .submit_desktop(&proof, request.clone(), command.clone())
        .unwrap();
    let failure = terminal(&handle, &proof, &receipt);
    assert_eq!(failure.state, OperationState::Failed);
    assert_eq!(failure.code.as_deref(), Some("project_failed"));
    let detail = handle
        .failure_detail(&proof, &failure.project_id, &failure.operation_id)
        .unwrap();
    assert_eq!(detail.scope, DomainScope::Project);
    assert_eq!(detail.code, "not_found");
    assert_eq!(detail.project_id, failure.project_id);
    assert_eq!(
        handle
            .result(&proof, &failure.project_id, &failure.operation_id)
            .unwrap_err(),
        ActorError::OperationFailed
    );
    let retry = handle.submit_desktop(&proof, request, command).unwrap();
    assert_eq!(retry.operation_id, receipt.operation_id);
    assert_eq!(retry.state, OperationState::Failed);
    actor.shutdown().unwrap();
}

#[test]
fn project_subject_cannot_relabel_another_project_or_ambiguous_resource() {
    let root = common::root("df-actor-");
    let owner = common::acquire(root.path()).unwrap();
    let mut auth = Authorization::new(&owner, 8, Policy::default());
    let proof = auth.proof();
    let actor = WorkspaceActor::start(
        owner.claim_workspace().unwrap(),
        Arc::new(NoWorkspaceSecrets),
    )
    .unwrap();
    let handle = actor.handle();
    let a = register(&handle, &proof, root.path(), "subject-a");
    let b = register(&handle, &proof, root.path(), "subject-b");
    let db = Connection::open(root.path().join("control/operations.sqlite3")).unwrap();
    let before: i64 = db
        .query_row("SELECT count(*) FROM operations", [], |row| row.get(0))
        .unwrap();
    assert!(matches!(
        handle.submit_desktop(
            &proof,
            identity("desktop-root", "reserved-not-project"),
            Command::Visit {
                id: "desktop-root".into()
            }
        ),
        Err(ActorError::SubjectMismatch)
    ));
    assert!(matches!(
        handle.submit_desktop(
            &proof,
            identity(&a, "mislabel-b"),
            Command::Favorite {
                id: b.clone(),
                favorite: true
            }
        ),
        Err(ActorError::SubjectMismatch)
    ));
    assert!(matches!(
        handle.submit_desktop(
            &proof,
            identity(&a, "global-mislabel"),
            Command::RefreshRuntimes
        ),
        Err(ActorError::SubjectMismatch)
    ));
    assert!(matches!(
        handle.submit_desktop(
            &proof,
            identity(&a, "ambiguous-worktree"),
            Command::WorktreeOpen {
                id: "worktree-record-not-project".into()
            }
        ),
        Err(ActorError::UnsupportedSubject)
    ));
    assert!(matches!(
        handle.submit_desktop(
            &proof,
            identity("desktop-root", "ambiguous-cache"),
            Command::StartCache {
                instance: "project-cache-instance".into()
            }
        ),
        Err(ActorError::UnsupportedSubject)
    ));
    assert_eq!(
        db.query_row::<i64, _, _>("SELECT count(*) FROM operations", [], |row| row.get(0))
            .unwrap(),
        before
    );
    assert!(
        !handle
            .cached_snapshot(&proof)
            .unwrap()
            .snapshot
            .projects
            .iter()
            .find(|row| row.project.id == b)
            .unwrap()
            .navigation
            .favorite
    );
    let actual = handle
        .submit_desktop(
            &proof,
            identity(&b, "correct-b"),
            Command::Favorite {
                id: b.clone(),
                favorite: true,
            },
        )
        .unwrap();
    assert_eq!(
        handle
            .status(&proof, &id(&a), &actual.operation_id)
            .unwrap_err(),
        ActorError::OperationNotFound
    );
    assert_eq!(
        handle
            .cancel(&proof, &id(&a), &actual.operation_id)
            .unwrap_err(),
        ActorError::OperationNotFound
    );
    assert_eq!(
        terminal(&handle, &proof, &actual).state,
        OperationState::Succeeded
    );
    assert!(
        handle
            .cached_snapshot(&proof)
            .unwrap()
            .snapshot
            .projects
            .iter()
            .find(|row| row.project.id == b)
            .unwrap()
            .navigation
            .favorite
    );
    actor.shutdown().unwrap();
}

#[test]
fn expiry_while_waiting_for_admission_rolls_back_but_committed_jobs_continue() {
    let root = common::root("df-actor-");
    let owner = common::acquire(root.path()).unwrap();
    let actor = WorkspaceActor::start(
        owner.claim_workspace().unwrap(),
        Arc::new(NoWorkspaceSecrets),
    )
    .unwrap();
    let handle = actor.handle();
    let mut initial = Authorization::new(&owner, 9, Policy::default());
    let initial_proof = initial.proof();
    let project = register(&handle, &initial_proof, root.path(), "expiry");
    let ledger = Connection::open(root.path().join("control/operations.sqlite3")).unwrap();
    ledger.execute_batch("BEGIN IMMEDIATE;").unwrap();
    let policy = Policy {
        session_lifetime: Duration::from_millis(100),
        ..Policy::default()
    };
    let mut short = Authorization::new(&owner, 10, policy);
    let proof = short.proof();
    let release = thread::spawn(move || {
        thread::sleep(Duration::from_millis(150));
        ledger.execute_batch("COMMIT;").unwrap();
    });
    let started = Instant::now();
    let rejected = handle.submit_desktop(
        &proof,
        identity(&project, "expired-before-commit"),
        Command::Visit {
            id: project.clone(),
        },
    );
    assert!(
        started.elapsed() >= Duration::from_millis(100),
        "fixture must exercise a real admission wait"
    );
    assert!(matches!(rejected, Err(ActorError::Unauthorized)));
    release.join().unwrap();
    let ledger = Connection::open(root.path().join("control/operations.sqlite3")).unwrap();
    assert_eq!(
        ledger
            .query_row::<i64, _, _>(
                "SELECT count(*) FROM operations WHERE idempotency_key='expired-before-commit'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        0
    );

    let block = Connection::open(root.path().join("projects/projects.sqlite3")).unwrap();
    block.execute_batch("BEGIN EXCLUSIVE;").unwrap();
    let mut short = Authorization::new(&owner, 11, policy);
    let proof = short.proof();
    let admitted = handle
        .submit_desktop(
            &proof,
            identity(&project, "accepted-before-expiry"),
            Command::Favorite {
                id: project.clone(),
                favorite: true,
            },
        )
        .unwrap();
    thread::sleep(Duration::from_millis(120));
    assert_eq!(
        handle
            .status(&proof, &id(&project), &admitted.operation_id)
            .unwrap_err(),
        ActorError::Unauthorized
    );
    block.execute_batch("COMMIT;").unwrap();
    let mut fresh = Authorization::new(&owner, 12, Policy::default());
    let fresh_proof = fresh.proof();
    assert_eq!(
        terminal(&handle, &fresh_proof, &admitted).state,
        OperationState::Succeeded
    );
    assert!(
        handle
            .cached_snapshot(&fresh_proof)
            .unwrap()
            .snapshot
            .projects[0]
            .navigation
            .favorite
    );
    actor.shutdown().unwrap();
}

#[cfg(unix)]
#[test]
fn ledger_symlink_is_rejected_without_write_and_failed_start_releases_permit_share() {
    use std::os::unix::fs::symlink;
    let root = common::root("df-actor-");
    let outside = common::root("df-actor-");
    let forbidden = outside.path().join("must-not-create.sqlite3");
    let owner = common::acquire(root.path()).unwrap();
    symlink(&forbidden, root.path().join("control/operations.sqlite3")).unwrap();
    assert!(matches!(
        WorkspaceActor::start(
            owner.claim_workspace().unwrap(),
            Arc::new(NoWorkspaceSecrets)
        ),
        Err(ActorError::StorageUnavailable)
    ));
    assert!(!forbidden.exists());
    assert!(owner.claim_workspace().is_err());
    drop(owner);
    let fresh = common::acquire(root.path()).unwrap();
    fs::remove_file(root.path().join("control/operations.sqlite3")).unwrap();
    WorkspaceActor::start(
        fresh.claim_workspace().unwrap(),
        Arc::new(NoWorkspaceSecrets),
    )
    .unwrap()
    .shutdown()
    .unwrap();
}

#[test]
fn foreign_root_authority_and_expired_held_proof_cannot_read_or_admit() {
    let root = common::root("df-actor-");
    let other = common::root("df-actor-");
    let owner = common::acquire(root.path()).unwrap();
    let foreign = common::acquire(other.path()).unwrap();
    let mut auth = Authorization::new(&foreign, 1, Policy::default());
    let proof = auth.proof();
    let actor = WorkspaceActor::start(
        owner.claim_workspace().unwrap(),
        Arc::new(NoWorkspaceSecrets),
    )
    .unwrap();
    let handle = actor.handle();
    assert!(matches!(
        handle.cached_snapshot(&proof),
        Err(ActorError::Unauthorized)
    ));
    assert!(matches!(
        handle.submit_desktop(
            &proof,
            identity("p", "k"),
            Command::Visit { id: "p".into() }
        ),
        Err(ActorError::Unauthorized)
    ));
    let policy = Policy {
        session_lifetime: Duration::from_millis(100),
        ..Policy::default()
    };
    let mut auth = Authorization::new(&owner, 2, policy);
    let proof = auth.proof();
    thread::sleep(Duration::from_millis(120));
    assert!(matches!(
        handle.cached_snapshot(&proof),
        Err(ActorError::Unauthorized)
    ));
    assert!(matches!(
        handle.submit_desktop(
            &proof,
            identity("p", "k"),
            Command::Visit { id: "p".into() }
        ),
        Err(ActorError::Unauthorized)
    ));
    assert_eq!(
        handle.status(&proof, &id("p"), &id("op")).unwrap_err(),
        ActorError::Unauthorized
    );
    assert_eq!(
        handle.result(&proof, &id("p"), &id("op")).unwrap_err(),
        ActorError::Unauthorized
    );
    assert_eq!(
        handle.cancel(&proof, &id("p"), &id("op")).unwrap_err(),
        ActorError::Unauthorized
    );
    assert_eq!(
        handle
            .failure_detail(&proof, &id("p"), &id("op"))
            .unwrap_err(),
        ActorError::Unauthorized
    );
    actor.shutdown().unwrap();
}

#[test]
fn exact_domain_failure_is_private_ram_only_and_unavailable_after_eviction_or_restart() {
    let root = common::root("df-actor-");
    let owner = common::acquire(root.path()).unwrap();
    let mut auth = Authorization::new(&owner, 21, Policy::default());
    let proof = auth.proof();
    let actor = WorkspaceActor::start(
        owner.claim_workspace().unwrap(),
        Arc::new(NoWorkspaceSecrets),
    )
    .unwrap();
    let handle = actor.handle();
    let mut invalid = import(
        fixture(root.path(), "invalid-name"),
        "invalid-name.devforge.test",
    );
    if let Command::Import { ref mut name, .. } = invalid {
        *name = String::new();
    }
    let first = handle
        .submit_desktop(
            &proof,
            identity("desktop-root", "invalid-project-name"),
            invalid,
        )
        .unwrap();
    assert_eq!(
        terminal(&handle, &proof, &first).state,
        OperationState::Failed
    );
    let detail = handle
        .failure_detail(&proof, &first.project_id, &first.operation_id)
        .unwrap();
    assert_eq!(detail.scope, DomainScope::Project);
    assert_eq!(detail.code, "invalid_project");
    assert_eq!(detail.project_id, id("desktop-root"));
    assert_eq!(
        handle
            .failure_detail(&proof, &id("another-project"), &first.operation_id)
            .unwrap_err(),
        ActorError::OperationNotFound
    );
    let mut latest = first.clone();
    // These are real failing metadata jobs, not inserted cache fixtures. Failed
    // and successful outcomes share the same retained row/byte budget.
    for n in 0..devforge_manager::actor::RESULT_ROWS {
        latest = handle
            .submit_desktop(
                &proof,
                identity("missing-project", &format!("failure-eviction-{n}")),
                Command::Visit {
                    id: "missing-project".into(),
                },
            )
            .unwrap();
        assert_eq!(
            terminal(&handle, &proof, &latest).state,
            OperationState::Failed
        );
    }
    assert_eq!(
        handle
            .failure_detail(&proof, &first.project_id, &first.operation_id)
            .unwrap_err(),
        ActorError::ResultUnavailable
    );
    assert_eq!(
        handle
            .failure_detail(&proof, &latest.project_id, &latest.operation_id)
            .unwrap()
            .code,
        "not_found"
    );
    actor.shutdown().unwrap();
    drop(owner);
    let bytes = fs::read(root.path().join("control/operations.sqlite3")).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains("invalid_project"));
    assert!(!text.contains("not_found"));
    assert!(text.contains("project_failed"));
    let owner = common::acquire(root.path()).unwrap();
    let mut auth = Authorization::new(&owner, 22, Policy::default());
    let proof = auth.proof();
    let reopened = WorkspaceActor::start(
        owner.claim_workspace().unwrap(),
        Arc::new(NoWorkspaceSecrets),
    )
    .unwrap();
    let handle = reopened.handle();
    assert_eq!(
        handle
            .status(&proof, &latest.project_id, &latest.operation_id)
            .unwrap()
            .code
            .as_deref(),
        Some("project_failed")
    );
    assert_eq!(
        handle
            .failure_detail(&proof, &latest.project_id, &latest.operation_id)
            .unwrap_err(),
        ActorError::ResultUnavailable
    );
    reopened.shutdown().unwrap();
}
