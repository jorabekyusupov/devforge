//! Credential policy fixtures only. Never opens native Keychain/Credential Manager.
mod common;
use devforge_core::{ErrorCode, SecretStore};
use devforge_manager::{
    Endpoint, Owner, PeerIdentity,
    auth::{
        AuthError, DesktopAuth, DesktopCredential, NativeGuiPolicy, Policy, SessionCredential,
        TrustedDesktopAction,
    },
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[derive(Default)]
struct MemoryStore {
    values: RefCell<BTreeMap<String, String>>,
    gets: Cell<usize>,
    sets: Cell<usize>,
    deletes: Cell<usize>,
    deny: Cell<bool>,
    get_delay: Cell<Duration>,
}
impl SecretStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<String>, ErrorCode> {
        self.gets.set(self.gets.get() + 1);
        std::thread::sleep(self.get_delay.get());
        if self.deny.get() {
            return Err(ErrorCode::SecretUnavailable);
        }
        Ok(self.values.borrow().get(key).cloned())
    }
    fn set(&self, key: &str, value: &str) -> Result<(), ErrorCode> {
        self.sets.set(self.sets.get() + 1);
        if self.deny.get() {
            return Err(ErrorCode::SecretUnavailable);
        }
        self.values
            .borrow_mut()
            .insert(key.to_owned(), value.to_owned());
        Ok(())
    }
    fn delete(&self, key: &str) -> Result<(), ErrorCode> {
        self.deletes.set(self.deletes.get() + 1);
        if self.deny.get() {
            return Err(ErrorCode::SecretUnavailable);
        }
        self.values.borrow_mut().remove(key);
        Ok(())
    }
}
fn peer() -> PeerIdentity {
    PeerIdentity {
        user: "fixture-owner".into(),
        process_id: Some(41),
    }
}
fn action() -> TrustedDesktopAction {
    TrustedDesktopAction::from_explicit_desktop_action()
}
fn make_auth(owner: &Owner, policy: Policy) -> DesktopAuth {
    DesktopAuth::new(owner.endpoint().clone(), peer().user, policy).unwrap()
}
fn make_root() -> tempfile::TempDir {
    common::root("df-auth-")
}

#[test]
fn construction_does_not_probe_credentials_and_missing_key_is_never_created() {
    let root = make_root();
    let owner = Owner::acquire(root.path()).unwrap();
    let store = MemoryStore::default();
    let mut auth = make_auth(&owner, Policy::default());
    assert_eq!((store.gets.get(), store.sets.get()), (0, 0));
    let token = DesktopCredential::from_wire(&"a".repeat(64)).unwrap();
    assert_eq!(
        auth.authenticate(&peer(), &token, &store, Instant::now())
            .unwrap_err(),
        AuthError::NotProvisioned
    );
    assert_eq!(store.sets.get(), 0);
    assert!(store.values.borrow().is_empty());
}

#[test]
fn malformed_wire_credentials_do_not_reach_the_store() {
    let store = MemoryStore::default();
    for invalid in [
        "",
        "desktop",
        &"a".repeat(63),
        &"A".repeat(64),
        &"g".repeat(64),
        &"a".repeat(65),
        &"é".repeat(32),
    ] {
        assert_eq!(
            DesktopCredential::from_wire(invalid).unwrap_err(),
            AuthError::Unauthorized
        );
        assert_eq!(
            SessionCredential::from_wire(invalid).unwrap_err(),
            AuthError::Unauthorized
        );
    }
    assert_eq!((store.gets.get(), store.sets.get()), (0, 0));
}

#[test]
fn delayed_failed_store_work_rearms_cooldown_from_completion() {
    for denied in [false, true] {
        let root = make_root();
        let owner = Owner::acquire(root.path()).unwrap();
        let store = MemoryStore::default();
        let mut auth = make_auth(&owner, Policy::default());
        let now = Instant::now();
        let key = auth.rotate_desktop_key(&action(), &store, now).unwrap();
        store.get_delay.set(Duration::from_millis(20));
        store.deny.set(denied);
        let invalid = DesktopCredential::from_wire(&"b".repeat(64)).unwrap();
        assert_eq!(
            auth.authenticate(&peer(), &invalid, &store, now)
                .unwrap_err(),
            if denied {
                AuthError::SecretUnavailable
            } else {
                AuthError::Unauthorized
            }
        );
        // Virtual trusted time avoids sleeping30 seconds. The blocking20ms
        // must not consume the post-return cooldown, including `get` errors.
        assert_eq!(
            auth.authenticate(&peer(), &key, &store, now + Duration::from_secs(30))
                .unwrap_err(),
            AuthError::RateLimited
        );
        assert_eq!(store.gets.get(), 1);
    }
}

#[test]
fn delayed_success_anchors_interval_and_fixed_session_ttl_to_issuance() {
    let root = make_root();
    let owner = Owner::acquire(root.path()).unwrap();
    let store = MemoryStore::default();
    let mut auth = make_auth(
        &owner,
        Policy {
            session_lifetime: Duration::from_secs(10),
            ..Policy::default()
        },
    );
    let now = Instant::now();
    let key = auth.rotate_desktop_key(&action(), &store, now).unwrap();
    store.get_delay.set(Duration::from_millis(20));
    let started = Instant::now();
    let session = auth.authenticate(&peer(), &key, &store, now).unwrap();
    let completed = now + started.elapsed();
    assert_eq!(
        auth.authenticate(&peer(), &key, &store, now + Duration::from_secs(5))
            .unwrap_err(),
        AuthError::RateLimited
    );
    assert_eq!(store.gets.get(), 1);
    // A prompt must not use up the credential's lifetime before issuance.
    assert!(
        auth.authorize(&peer(), &session, now + Duration::from_secs(10))
            .is_ok()
    );
    assert_eq!(
        auth.authorize(&peer(), &session, completed + Duration::from_secs(10))
            .unwrap_err(),
        AuthError::Unauthorized
    );
}

#[test]
fn held_authority_proof_must_still_be_current_when_consumed() {
    let root = make_root();
    let owner = Owner::acquire(root.path()).unwrap();
    let store = MemoryStore::default();
    let mut auth = make_auth(
        &owner,
        Policy {
            session_lifetime: Duration::from_secs(10),
            ..Policy::default()
        },
    );
    let now = Instant::now();
    let key = auth.rotate_desktop_key(&action(), &store, now).unwrap();
    let session = auth.authenticate(&peer(), &key, &store, now).unwrap();
    let proof = auth.authorize(&peer(), &session, now).unwrap();
    assert!(proof.check_current(now + Duration::from_secs(1)).is_ok());
    // Keep the same borrowed proof: retaining it must not retain authority.
    assert_eq!(
        proof.check_current(now + Duration::from_secs(11)),
        Err(AuthError::Unauthorized)
    );
    assert_eq!(store.gets.get(), 1);
}

#[test]
fn ordinary_store_auth_is_not_implicitly_renewable() {
    let root = make_root();
    let owner = Owner::acquire(root.path()).unwrap();
    let store = MemoryStore::default();
    let mut auth = make_auth(&owner, Policy::default());
    let now = Instant::now();
    let key = auth.rotate_desktop_key(&action(), &store, now).unwrap();
    let session = auth.authenticate(&peer(), &key, &store, now).unwrap();
    assert_eq!(
        auth.renew_native_gui(&peer(), &session, now).unwrap_err(),
        AuthError::Unauthorized
    );
    assert!(auth.authorize(&peer(), &session, now).is_ok());
    assert_eq!(store.gets.get(), 1);
}

#[test]
fn explicit_native_gui_renewal_rotates_token_without_store_and_has_absolute_deadline() {
    let root = make_root();
    let owner = Owner::acquire(root.path()).unwrap();
    let store = MemoryStore::default();
    let mut auth = make_auth(
        &owner,
        Policy {
            session_lifetime: Duration::from_secs(10),
            ..Policy::default()
        },
    );
    let now = Instant::now();
    let key = auth.rotate_desktop_key(&action(), &store, now).unwrap();
    let context = NativeGuiPolicy {
        context_lifetime: Duration::from_secs(25),
        ..NativeGuiPolicy::default()
    };
    let session = auth
        .authenticate_native_gui(&peer(), &key, &store, context, now)
        .unwrap();
    store.deny.set(true);
    let mut wrong = peer();
    wrong.process_id = Some(42);
    assert_eq!(
        auth.renew_native_gui(&wrong, &session, now).unwrap_err(),
        AuthError::Unauthorized
    );
    let next = auth
        .renew_native_gui(&peer(), &session, now + Duration::from_secs(9))
        .unwrap();
    assert_ne!(session.expose_secret(), next.expose_secret());
    assert_eq!(
        auth.authorize(&peer(), &session, now + Duration::from_secs(9))
            .unwrap_err(),
        AuthError::Unauthorized
    );
    let last = auth
        .renew_native_gui(&peer(), &next, now + Duration::from_secs(18))
        .unwrap();
    assert!(
        auth.authorize(&peer(), &last, now + Duration::from_secs(24))
            .is_ok()
    );
    // Renewal near the deadline cannot move it. Use a margin around completion
    // anchored issuance; no long real sleeps or native store calls are needed.
    assert_eq!(
        auth.renew_native_gui(&peer(), &last, now + Duration::from_secs(26))
            .unwrap_err(),
        AuthError::Unauthorized
    );
    assert_eq!(store.gets.get(), 1);
}

#[test]
fn native_context_cannot_renew_after_expiry_or_revoke_and_policy_is_server_bounded() {
    let root = make_root();
    let owner = Owner::acquire(root.path()).unwrap();
    let store = MemoryStore::default();
    let mut auth = make_auth(
        &owner,
        Policy {
            session_lifetime: Duration::from_secs(10),
            ..Policy::default()
        },
    );
    let now = Instant::now();
    let key = auth.rotate_desktop_key(&action(), &store, now).unwrap();
    let invalid = NativeGuiPolicy {
        context_lifetime: Duration::from_secs(24 * 3600 + 1),
        ..NativeGuiPolicy::default()
    };
    assert_eq!(
        auth.authenticate_native_gui(&peer(), &key, &store, invalid, now)
            .unwrap_err(),
        AuthError::InvalidPolicy
    );
    assert_eq!(store.gets.get(), 0);
    let session = auth
        .authenticate_native_gui(&peer(), &key, &store, NativeGuiPolicy::default(), now)
        .unwrap();
    assert_eq!(
        auth.renew_native_gui(&peer(), &session, now + Duration::from_secs(11))
            .unwrap_err(),
        AuthError::Unauthorized
    );
    let session = auth
        .authenticate_native_gui(
            &peer(),
            &key,
            &store,
            NativeGuiPolicy::default(),
            now + Duration::from_secs(12),
        )
        .unwrap();
    auth.revoke_all();
    assert_eq!(
        auth.renew_native_gui(&peer(), &session, now + Duration::from_secs(13))
            .unwrap_err(),
        AuthError::Unauthorized
    );
}

#[test]
fn explicit_key_issues_server_session_bound_to_root_and_kernel_peer() {
    let root = make_root();
    let owner = Owner::acquire(root.path()).unwrap();
    let store = MemoryStore::default();
    let mut auth = make_auth(&owner, Policy::default());
    let now = Instant::now();
    let credential = auth.rotate_desktop_key(&action(), &store, now).unwrap();
    assert_eq!(credential.expose_secret().len(), 64);
    assert_eq!((store.gets.get(), store.sets.get()), (0, 1));
    let session = auth
        .authenticate(&peer(), &credential, &store, now)
        .unwrap();
    assert_ne!(session.expose_secret(), credential.expose_secret());
    let authority = auth.authorize(&peer(), &session, now).unwrap();
    assert_eq!(authority.root(), owner.endpoint().root());
    assert_eq!(authority.peer(), &peer());
    // Cache checks never access the credential service, even while it is denied.
    store.deny.set(true);
    assert!(auth.authorize(&peer(), &session, now).is_ok());
    assert_eq!(store.gets.get(), 1);
    let mut foreign = peer();
    foreign.user = "other-user".into();
    assert_eq!(
        auth.authorize(&foreign, &session, now).unwrap_err(),
        AuthError::Unauthorized
    );
    foreign = peer();
    foreign.process_id = Some(99);
    assert_eq!(
        auth.authorize(&foreign, &session, now).unwrap_err(),
        AuthError::Unauthorized
    );
    let other_root = make_root();
    let other_owner = Owner::acquire(other_root.path()).unwrap();
    let mut other = make_auth(&other_owner, Policy::default());
    assert_eq!(
        other.authorize(&peer(), &session, now).unwrap_err(),
        AuthError::Unauthorized
    );
    let forged = SessionCredential::from_wire(credential.expose_secret()).unwrap();
    assert_eq!(
        auth.authorize(&peer(), &forged, now).unwrap_err(),
        AuthError::Unauthorized
    );
}

#[test]
fn different_roots_have_distinct_persistent_keys_and_credentials() {
    let a = make_root();
    let b = make_root();
    let owner_a = Owner::acquire(a.path()).unwrap();
    let owner_b = Owner::acquire(b.path()).unwrap();
    let store = MemoryStore::default();
    let mut auth_a = make_auth(&owner_a, Policy::default());
    let mut auth_b = make_auth(&owner_b, Policy::default());
    let now = Instant::now();
    let key_a = auth_a.rotate_desktop_key(&action(), &store, now).unwrap();
    let key_b = auth_b.rotate_desktop_key(&action(), &store, now).unwrap();
    assert_eq!(store.values.borrow().len(), 2);
    assert_ne!(key_a.expose_secret(), key_b.expose_secret());
    assert_eq!(
        auth_b
            .authenticate(&peer(), &key_a, &store, now)
            .unwrap_err(),
        AuthError::Unauthorized
    );
}

#[test]
fn failed_and_unavailable_attempts_are_throttled_without_repeated_store_prompts() {
    let root = make_root();
    let owner = Owner::acquire(root.path()).unwrap();
    let store = MemoryStore::default();
    let mut auth = make_auth(&owner, Policy::default());
    let now = Instant::now();
    let credential = auth.rotate_desktop_key(&action(), &store, now).unwrap();
    let invalid = DesktopCredential::from_wire(&"b".repeat(64)).unwrap();
    assert_eq!(
        auth.authenticate(&peer(), &invalid, &store, now)
            .unwrap_err(),
        AuthError::Unauthorized
    );
    for _ in 0..100 {
        assert_eq!(
            auth.authenticate(&peer(), &credential, &store, now + Duration::from_secs(29))
                .unwrap_err(),
            AuthError::RateLimited
        );
    }
    assert_eq!(store.gets.get(), 1);
    store.deny.set(true);
    assert_eq!(
        auth.authenticate(&peer(), &credential, &store, now + Duration::from_secs(31))
            .unwrap_err(),
        AuthError::SecretUnavailable
    );
    assert_eq!(
        auth.authenticate(&peer(), &credential, &store, now + Duration::from_secs(60))
            .unwrap_err(),
        AuthError::RateLimited
    );
    assert_eq!(store.gets.get(), 2);
    store.deny.set(false);
    assert!(
        auth.authenticate(&peer(), &credential, &store, now + Duration::from_secs(62))
            .is_ok()
    );
    // Successful attempts also have a minimum interval.
    assert_eq!(
        auth.authenticate(&peer(), &credential, &store, now + Duration::from_secs(63))
            .unwrap_err(),
        AuthError::RateLimited
    );
    assert_eq!(store.gets.get(), 3);
}

#[test]
fn foreign_peer_cannot_trigger_store_access_or_consume_owner_cooldown() {
    let root = make_root();
    let owner = Owner::acquire(root.path()).unwrap();
    let store = MemoryStore::default();
    let mut auth = make_auth(&owner, Policy::default());
    let now = Instant::now();
    let key = auth.rotate_desktop_key(&action(), &store, now).unwrap();
    let foreign = PeerIdentity {
        user: "foreign".into(),
        process_id: Some(41),
    };
    assert_eq!(
        auth.authenticate(&foreign, &key, &store, now).unwrap_err(),
        AuthError::Unauthorized
    );
    assert_eq!(store.gets.get(), 0);
    assert!(auth.authenticate(&peer(), &key, &store, now).is_ok());
}

#[test]
fn fixed_expiry_capacity_and_individual_revocation_are_enforced() {
    let root = make_root();
    let owner = Owner::acquire(root.path()).unwrap();
    let store = MemoryStore::default();
    let policy = Policy {
        max_sessions: 1,
        session_lifetime: Duration::from_secs(10),
        ..Policy::default()
    };
    let mut auth = make_auth(&owner, policy);
    let now = Instant::now();
    let key = auth.rotate_desktop_key(&action(), &store, now).unwrap();
    let first = auth.authenticate(&peer(), &key, &store, now).unwrap();
    assert_eq!(
        auth.authenticate(&peer(), &key, &store, now + Duration::from_secs(6))
            .unwrap_err(),
        AuthError::SessionCapacity
    );
    assert_eq!(
        auth.authorize(&peer(), &first, now + Duration::from_secs(11))
            .unwrap_err(),
        AuthError::Unauthorized
    );
    let second = auth
        .authenticate(&peer(), &key, &store, now + Duration::from_secs(37))
        .unwrap();
    let mut wrong = peer();
    wrong.process_id = Some(42);
    assert_eq!(
        auth.revoke_session(&wrong, &second).unwrap_err(),
        AuthError::Unauthorized
    );
    assert!(
        auth.authorize(&peer(), &second, now + Duration::from_secs(38))
            .is_ok()
    );
    auth.revoke_session(&peer(), &second).unwrap();
    assert_eq!(
        auth.authorize(&peer(), &second, now + Duration::from_secs(38))
            .unwrap_err(),
        AuthError::Unauthorized
    );
}

#[test]
fn rotation_and_revocation_invalidate_cache_and_failed_rotation_fails_closed() {
    let root = make_root();
    let owner = Owner::acquire(root.path()).unwrap();
    let store = MemoryStore::default();
    let mut auth = make_auth(&owner, Policy::default());
    let now = Instant::now();
    let old = auth.rotate_desktop_key(&action(), &store, now).unwrap();
    let session = auth.authenticate(&peer(), &old, &store, now).unwrap();
    let new = auth.rotate_desktop_key(&action(), &store, now).unwrap();
    assert_ne!(old.expose_secret(), new.expose_secret());
    assert_eq!(
        auth.authorize(&peer(), &session, now).unwrap_err(),
        AuthError::Unauthorized
    );
    let session = auth.authenticate(&peer(), &new, &store, now).unwrap();
    store.deny.set(true);
    assert_eq!(
        auth.rotate_desktop_key(&action(), &store, now).unwrap_err(),
        AuthError::SecretUnavailable
    );
    assert_eq!(
        auth.authorize(&peer(), &session, now).unwrap_err(),
        AuthError::Unauthorized
    );
    assert_eq!(
        auth.authenticate(&peer(), &new, &store, now + Duration::from_secs(31))
            .unwrap_err(),
        AuthError::Unauthorized
    );
    store.deny.set(false);
    let new = auth.rotate_desktop_key(&action(), &store, now).unwrap();
    let session = auth.authenticate(&peer(), &new, &store, now).unwrap();
    auth.revoke_desktop_key(&action(), &store, now).unwrap();
    assert!(store.values.borrow().is_empty());
    assert_eq!(
        auth.authorize(&peer(), &session, now).unwrap_err(),
        AuthError::Unauthorized
    );
}

#[test]
fn external_trusted_rotation_is_observed_on_next_auth_and_does_not_promise_instant_revoke() {
    let root = make_root();
    let owner = Owner::acquire(root.path()).unwrap();
    let store = MemoryStore::default();
    let mut manager = make_auth(&owner, Policy::default());
    let now = Instant::now();
    let old = manager.rotate_desktop_key(&action(), &store, now).unwrap();
    let session = manager.authenticate(&peer(), &old, &store, now).unwrap();
    // A different trusted desktop composition has its own cache, same root/store.
    let mut desktop = make_auth(&owner, Policy::default());
    let new = desktop.rotate_desktop_key(&action(), &store, now).unwrap();
    assert!(
        manager
            .authorize(&peer(), &session, now + Duration::from_secs(1))
            .is_ok()
    );
    let new_session = manager
        .authenticate(&peer(), &new, &store, now + Duration::from_secs(6))
        .unwrap();
    assert_eq!(
        manager
            .authorize(&peer(), &session, now + Duration::from_secs(6))
            .unwrap_err(),
        AuthError::Unauthorized
    );
    assert!(
        manager
            .authorize(&peer(), &new_session, now + Duration::from_secs(6))
            .is_ok()
    );
    desktop.revoke_desktop_key(&action(), &store, now).unwrap();
    assert_eq!(
        manager
            .authenticate(&peer(), &new, &store, now + Duration::from_secs(12))
            .unwrap_err(),
        AuthError::NotProvisioned
    );
    assert_eq!(
        manager
            .authorize(&peer(), &new_session, now + Duration::from_secs(12))
            .unwrap_err(),
        AuthError::Unauthorized
    );
}

#[test]
fn secrets_are_absent_from_debug_and_store_errors() {
    let root = make_root();
    let owner = Owner::acquire(root.path()).unwrap();
    let store = MemoryStore::default();
    let mut auth = make_auth(&owner, Policy::default());
    let now = Instant::now();
    let key = auth.rotate_desktop_key(&action(), &store, now).unwrap();
    let session = auth.authenticate(&peer(), &key, &store, now).unwrap();
    let auth_debug = format!("{auth:?}");
    let text = format!(
        "{auth_debug} {key:?} {session:?} {:?}",
        auth.authorize(&peer(), &session, now).unwrap()
    );
    assert!(!text.contains(key.expose_secret()));
    assert!(!text.contains(session.expose_secret()));
    assert!(!text.contains(root.path().to_str().unwrap()));
    assert_eq!(
        AuthError::SecretUnavailable.to_string(),
        "manager_auth_secret_unavailable"
    );
}

#[test]
fn invalid_stored_key_revokes_sessions_without_adopting_or_replacing_it() {
    let root = make_root();
    let owner = Owner::acquire(root.path()).unwrap();
    let store = MemoryStore::default();
    let mut auth = make_auth(&owner, Policy::default());
    let now = Instant::now();
    let key = auth.rotate_desktop_key(&action(), &store, now).unwrap();
    let session = auth.authenticate(&peer(), &key, &store, now).unwrap();
    for value in store.values.borrow_mut().values_mut() {
        *value = "client-selected-token".into();
    }
    assert_eq!(
        auth.authenticate(&peer(), &key, &store, now + Duration::from_secs(6))
            .unwrap_err(),
        AuthError::SecretUnavailable
    );
    assert_eq!(
        auth.authorize(&peer(), &session, now + Duration::from_secs(6))
            .unwrap_err(),
        AuthError::Unauthorized
    );
    assert_eq!(store.sets.get(), 1);
    assert_eq!(
        store.values.borrow().values().next().unwrap(),
        "client-selected-token"
    );
}

#[test]
fn failed_persistent_revoke_is_reported_and_does_not_claim_restart_protection() {
    let root = make_root();
    let owner = Owner::acquire(root.path()).unwrap();
    let store = MemoryStore::default();
    let mut auth = make_auth(&owner, Policy::default());
    let now = Instant::now();
    let key = auth.rotate_desktop_key(&action(), &store, now).unwrap();
    let session = auth.authenticate(&peer(), &key, &store, now).unwrap();
    store.deny.set(true);
    assert_eq!(
        auth.revoke_desktop_key(&action(), &store, now).unwrap_err(),
        AuthError::SecretUnavailable
    );
    assert_eq!(
        auth.authorize(&peer(), &session, now).unwrap_err(),
        AuthError::Unauthorized
    );
    assert_eq!(
        auth.authenticate(&peer(), &key, &store, now + Duration::from_secs(31))
            .unwrap_err(),
        AuthError::Unauthorized
    );
    assert_eq!(store.values.borrow().len(), 1);
    // A failed delete left the persistent key intact. A fresh manager may still
    // verify it: do not misrepresent an in-memory revoke as durable completion.
    store.deny.set(false);
    let mut restarted = make_auth(&owner, Policy::default());
    assert_eq!(
        restarted.authorize(&peer(), &session, now).unwrap_err(),
        AuthError::Unauthorized
    );
    assert!(restarted.authenticate(&peer(), &key, &store, now).is_ok());
}

#[test]
fn invalid_policy_and_unsafe_endpoint_are_rejected_without_credentials() {
    let root = make_root();
    let owner = Owner::acquire(root.path()).unwrap();
    for policy in [
        Policy {
            max_sessions: 0,
            ..Policy::default()
        },
        Policy {
            max_sessions: 33,
            ..Policy::default()
        },
        Policy {
            session_lifetime: Duration::ZERO,
            ..Policy::default()
        },
        Policy {
            store_interval: Duration::ZERO,
            ..Policy::default()
        },
    ] {
        assert_eq!(
            DesktopAuth::new(owner.endpoint().clone(), peer().user, policy).unwrap_err(),
            AuthError::InvalidPolicy
        );
    }
    let endpoint = Endpoint::existing(root.path()).unwrap();
    drop(owner);
    std::fs::remove_dir(root.path().join("control")).unwrap_err(); // lock files retained
    std::fs::rename(root.path().join("control"), root.path().join("old-control")).unwrap();
    assert_eq!(
        DesktopAuth::new(endpoint, peer().user, Policy::default()).unwrap_err(),
        AuthError::UnsafeEndpoint
    );
}
