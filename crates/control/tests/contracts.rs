use devforge_control::*;
use serde_json::{Value, json};
fn id(v: &str) -> Id {
    Id::new(v).unwrap()
}
fn digest(v: char) -> Digest {
    Digest::new(v.to_string().repeat(64)).unwrap()
}
fn binding(project: &str, revision: u64) -> ProjectBinding {
    ProjectBinding::from_registered(id(project), &std::env::current_dir().unwrap(), revision)
        .unwrap()
}
fn request() -> Value {
    json!({"api_version":1,"request_id":"r1","project_id":"p1","expected_revision":4,"idempotency_key":"k1","input":{}})
}
#[test]
fn strict_wire_version_and_unknown_fields_fail_closed() {
    let mut raw = request();
    assert!(serde_json::from_value::<Request<Value>>(raw.clone()).is_ok());
    raw["api_version"] = json!(2);
    assert!(serde_json::from_value::<Request<Value>>(raw).is_err());
    let mut raw = request();
    raw["confirm"] = json!(true);
    assert!(serde_json::from_value::<Request<Value>>(raw).is_err());
    assert!(serde_json::from_str::<Scope>("\"administrator\"").is_err());
    assert!(serde_json::from_str::<Scope>("\"observer,execution_test\"").is_err());
    assert!(serde_json::from_str::<Id>("\"../other\"").is_err());
    assert!(Id::new("a\nb").is_err());
    assert!(Id::new("a".repeat(129)).is_err());
}
#[test]
fn mutations_require_bound_project_revision_and_idempotency() {
    let r: Request<Value> = serde_json::from_value(request()).unwrap();
    assert_eq!(r.validate(&binding("p1", 4), true), Ok(()));
    assert_eq!(
        r.validate(&binding("p2", 4), true),
        Err(ErrorCode::ProjectMismatch)
    );
    assert_eq!(
        r.validate(&binding("p1", 5), true),
        Err(ErrorCode::RevisionConflict)
    );
    let mut raw = request();
    raw.as_object_mut().unwrap().remove("expected_revision");
    let r: Request<Value> = serde_json::from_value(raw).unwrap();
    assert_eq!(
        r.validate(&binding("p1", 4), true),
        Err(ErrorCode::InvalidInput)
    );
    assert_eq!(r.validate(&binding("p1", 4), false), Ok(()));
    let mut raw = request();
    raw["idempotency_key"] = Value::Null;
    let r: Request<Value> = serde_json::from_value(raw).unwrap();
    assert_eq!(
        r.validate(&binding("p1", 4), true),
        Err(ErrorCode::InvalidInput)
    );
}
#[test]
fn grants_are_client_project_expiry_and_capability_scoped() {
    let mut grant = Grant::issue(
        id("client"),
        id("p1"),
        vec![Scope::Observer, Scope::ProjectManage],
        100,
    )
    .unwrap();
    assert_eq!(
        grant.authorize(&id("client"), &binding("p1", 4), Scope::Observer, 99),
        Ok(())
    );
    assert_eq!(
        grant.authorize(&id("client"), &binding("p1", 4), Scope::ExecutionTest, 99),
        Err(ErrorCode::Unauthorized)
    );
    assert_eq!(
        grant.authorize(&id("other"), &binding("p1", 4), Scope::Observer, 99),
        Err(ErrorCode::Unauthorized)
    );
    assert_eq!(
        grant.authorize(&id("client"), &binding("p2", 4), Scope::Observer, 99),
        Err(ErrorCode::ProjectMismatch)
    );
    assert_eq!(
        grant.authorize(&id("client"), &binding("p1", 4), Scope::Observer, 100),
        Err(ErrorCode::GrantExpired)
    );
    grant.revoke();
    assert_eq!(
        grant.authorize(&id("client"), &binding("p1", 4), Scope::Observer, 99),
        Err(ErrorCode::GrantRevoked)
    );
    assert!(
        Grant::issue(
            id("client"),
            id("p1"),
            vec![Scope::Observer, Scope::Observer],
            100
        )
        .is_err()
    );
}
#[test]
fn reviewed_plan_refuses_stale_digest_project_and_expiration() {
    let plan = ReviewedPlan::record(&binding("p1", 4), id("db.restore"), digest('a'), 100).unwrap();
    assert_eq!(
        plan.verify(&binding("p1", 4), &id("db.restore"), &digest('a'), 99),
        Ok(())
    );
    assert_eq!(
        plan.verify(&binding("p1", 5), &id("db.restore"), &digest('a'), 99),
        Err(ErrorCode::RevisionConflict)
    );
    assert_eq!(
        plan.verify(&binding("p2", 4), &id("db.restore"), &digest('a'), 99),
        Err(ErrorCode::ProjectMismatch)
    );
    assert_eq!(
        plan.verify(&binding("p1", 4), &id("db.restore"), &digest('b'), 99),
        Err(ErrorCode::ReviewMismatch)
    );
    assert_eq!(
        plan.verify(&binding("p1", 4), &id("db.restore"), &digest('a'), 100),
        Err(ErrorCode::ReviewExpired)
    );
    assert_eq!(
        plan.verify(&binding("p1", 4), &id("db.drop"), &digest('a'), 99),
        Err(ErrorCode::ReviewMismatch)
    );
    assert!(Digest::new("A".repeat(64)).is_err());
}
#[test]
fn duplicate_requests_reuse_only_the_exact_bound_request() {
    let stored = IdempotencyRecord::new(id("c1"), id("p1"), id("key"), digest('a'), id("op1"));
    assert_eq!(
        stored.replay(&id("c1"), &id("p1"), &id("key"), &digest('a')),
        Ok(&id("op1"))
    );
    for (client, project, key, d) in [
        ("c2", "p1", "key", 'a'),
        ("c1", "p2", "key", 'a'),
        ("c1", "p1", "other", 'a'),
        ("c1", "p1", "key", 'b'),
    ] {
        assert_eq!(
            stored.replay(&id(client), &id(project), &id(key), &digest(d)),
            Err(ErrorCode::IdempotencyConflict)
        );
    }
}
#[test]
fn cancellation_is_requested_not_reported_as_completed() {
    use OperationState::*;
    let running = Queued.transition(Running).unwrap();
    let cancelling = running.transition(CancelRequested).unwrap();
    assert!(!cancelling.terminal());
    // Completion can win a cancellation race; cancellation is not guaranteed undo.
    assert_eq!(cancelling.transition(Succeeded), Ok(Succeeded));
    for state in [Succeeded, Failed, Cancelled, Interrupted, Unknown] {
        assert!(state.terminal());
        assert_eq!(state.transition(Running), Err(ErrorCode::InvalidTransition));
    }
    assert_eq!(
        Queued.transition(Succeeded),
        Err(ErrorCode::InvalidTransition)
    );
}
#[test]
fn responses_have_one_outcome_and_keep_v1_wire_shape() {
    let response = Response::success(id("r1"), Some(id("op1")), json!({"state":"queued"}));
    let value = serde_json::to_value(response).unwrap();
    assert_eq!(
        value,
        json!({"api_version":1,"request_id":"r1","operation_id":"op1","ok":true,"data":{"state":"queued"},"warnings":[]})
    );
    let error: Response<Value> = Response::failure(
        id("r1"),
        None,
        Fault {
            code: ErrorCode::Busy,
            retryable: true,
        },
    );
    let value = serde_json::to_value(error).unwrap();
    assert_eq!(
        value,
        json!({"api_version":1,"request_id":"r1","ok":false,"error":{"code":"busy","retryable":true},"warnings":[]})
    );
}
#[test]
fn inventory_covers_all_families_without_advertising_a_live_api() {
    let entries = inventory::entries().unwrap();
    assert_eq!(entries.len(), 16);
    let debugger = entries
        .iter()
        .find(|e| e.family == inventory::Family::Debugger)
        .unwrap();
    assert!(
        debugger
            .unsupported_operations
            .iter()
            .any(|op| op == "arbitrary_expression_evaluation")
    );
    assert!(entries.iter().all(|e| !e.available));
    let mut duplicates = entries.clone();
    duplicates[1] = duplicates[0].clone();
    assert_eq!(
        inventory::validate(&duplicates),
        Err(ErrorCode::InvalidInput)
    );
    let mut falsely_live = entries;
    falsely_live[0].available = true;
    assert_eq!(
        inventory::validate(&falsely_live),
        Err(ErrorCode::InvalidInput)
    );
}
#[test]
fn registry_binding_rejects_relative_or_missing_roots() {
    assert!(ProjectBinding::from_registered(id("p1"), std::path::Path::new("."), 1).is_err());
    assert!(
        ProjectBinding::from_registered(
            id("p1"),
            &std::env::temp_dir().join("devforge-no-project-root-unknown"),
            1
        )
        .is_err()
    );
}

#[test]
fn existing_resources_are_resolved_within_registered_root() {
    let project = binding("p1", 1);
    assert!(
        project
            .resolve_existing(std::path::Path::new("Cargo.toml"))
            .is_ok()
    );
    assert!(
        project
            .resolve_existing(std::path::Path::new("../Cargo.toml"))
            .is_err()
    );
    assert!(project.resolve_existing(&std::env::temp_dir()).is_err());
    assert!(
        project
            .resolve_existing(std::path::Path::new("missing-resource"))
            .is_err()
    );
}

#[test]
fn project_code_paths_require_execution_grant_even_when_implicit() {
    use inventory::Family::*;
    let entries = inventory::entries().unwrap();
    for (family, names) in [
        (Projects, vec!["create", "prepare"]),
        (Services, vec!["start", "restart"]),
        (Runtimes, vec!["select"]),
        (Environment, vec!["setup", "apply"]),
        (Inspector, vec!["capture_toggle", "companion_install"]),
        (Workspaces, vec!["create", "merge"]),
    ] {
        let entry = entries.iter().find(|entry| entry.family == family).unwrap();
        for name in names {
            let op = entry.operations.iter().find(|op| op.name == name).unwrap();
            assert!(
                op.required_scopes.contains(&Scope::ExecutionTest),
                "{family:?}.{name}"
            );
            assert!(
                op.effects
                    .iter()
                    .any(|effect| effect == "trusted_project_code_execution"),
                "{family:?}.{name}"
            );
        }
    }
    let environment = entries
        .iter()
        .find(|entry| entry.family == Environment)
        .unwrap();
    for name in ["setup", "apply"] {
        let op = environment
            .operations
            .iter()
            .find(|op| op.name == name)
            .unwrap();
        assert!(op.review_required);
        assert!(op.required_scopes.contains(&Scope::SensitiveOperation));
        assert!(
            op.effects
                .iter()
                .any(|effect| effect == "owned_database_write")
        );
    }
    // Xdebug provider installation builds/downloads verified SDKs; it is not
    // InspectorInstall, which runs Composer and Artisan project discovery.
    let debugger = entries
        .iter()
        .find(|entry| entry.family == Debugger)
        .unwrap();
    let install = debugger
        .operations
        .iter()
        .find(|op| op.name == "install")
        .unwrap();
    assert!(!install.required_scopes.contains(&Scope::ExecutionTest));
    assert!(install.review_required);
    assert!(
        install
            .effects
            .iter()
            .any(|effect| effect == "verified_provider_download_or_sdk_build")
    );
}

#[test]
fn plan_generation_does_not_claim_apply_effects_and_mail_delete_is_not_export() {
    use inventory::Family::*;
    let entries = inventory::entries().unwrap();
    for (family, name) in [
        (Environment, "plan"),
        (Domains, "hosts_plan"),
        (Domains, "trust_plan"),
        (Database, "restore_plan"),
        (Runtimes, "remove_plan"),
        (Workspaces, "plan"),
    ] {
        let op = entries
            .iter()
            .find(|entry| entry.family == family)
            .unwrap()
            .operations
            .iter()
            .find(|op| op.name == name)
            .unwrap();
        assert_eq!(op.required_scopes, vec![Scope::Observer]);
        assert_eq!(op.effects, vec!["read", "review_plan_output"]);
        assert!(!op.review_required);
    }
    let delete = entries
        .iter()
        .find(|entry| entry.family == Mail)
        .unwrap()
        .operations
        .iter()
        .find(|op| op.name == "delete")
        .unwrap();
    assert_eq!(delete.effects, vec!["reviewed_mail_delete"]);
    assert!(delete.review_required);
}

#[test]
fn family_summary_cannot_understate_operation_grants_or_effects() {
    let entries = inventory::entries().unwrap();
    let mut invalid = entries.clone();
    invalid
        .iter_mut()
        .find(|entry| entry.family == inventory::Family::Services)
        .unwrap()
        .scopes = vec![Scope::Observer];
    assert_eq!(inventory::validate(&invalid), Err(ErrorCode::InvalidInput));
    let mut invalid = entries.clone();
    invalid
        .iter_mut()
        .find(|entry| entry.family == inventory::Family::Environment)
        .unwrap()
        .effects = vec!["read".into()];
    assert_eq!(inventory::validate(&invalid), Err(ErrorCode::InvalidInput));
    let mut invalid = entries;
    invalid[0].operations[0].core_status = inventory::CoreStatus::Unsupported;
    assert_eq!(inventory::validate(&invalid), Err(ErrorCode::InvalidInput));
}
