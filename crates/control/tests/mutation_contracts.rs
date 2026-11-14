use devforge_control::{
    limits::SafeInt,
    mutations::*,
    tools::{self, SchemaStatus},
    *,
};
use serde_json::{Value, json};
use std::hash::{Hash, Hasher};
fn id(s: &str) -> Id {
    Id::new(s).unwrap()
}
fn binding(project: &str, revision: u64) -> ProjectBinding {
    ProjectBinding::from_registered(id(project), &std::env::current_dir().unwrap(), revision)
        .unwrap()
}
fn all_grants() -> Grant {
    Grant::issue(
        id("client"),
        id("project"),
        vec![
            Scope::Observer,
            Scope::ProjectManage,
            Scope::ExecutionTest,
            Scope::SensitiveOperation,
        ],
        1000,
    )
    .unwrap()
}
// This fixture tests canonical-material plumbing, not SHA-256 correctness. Production
// callers supply their existing SHA-256 implementation, never this test-only hash.
fn fixture_digest(bytes: &[u8]) -> Result<Digest> {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    Digest::new(format!("{:064x}", h.finish()))
}
fn state_digest() -> Digest {
    Digest::new("a".repeat(64)).unwrap()
}
fn args(name: &str) -> Value {
    match name {
        "runtimes.install" | "runtimes.remove" => json!({"package_id":"php-8.5-arm64"}),
        "runtimes.select" => {
            json!({"name":"Demo","php":"php-8.5-arm64","node":"node24-arm64","domain":"demo.devforge.test","https":true,"vite":true,"queue":false})
        }
        "environment.plan" | "environment.apply" => json!({"write_env":true}),
        "environment.yaml_save" => {
            json!({"expected":null,"text":"version: 1\nproject:\n  name: Demo\n"})
        }
        "workspaces.archive" | "workspaces.restore" | "workspaces.purge" => {
            json!({"workspace_id":"workspace1"})
        }
        _ => json!({}),
    }
}
fn call(name: &str, input: Value) -> Value {
    json!({"api_version":1,"request_id":"req1","project_id":"project","expected_revision":1,"idempotency_key":"key1","input":{"tool":name,"arguments":input}})
}
fn request_digest(raw: &Value, binding: &ProjectBinding) -> Digest {
    let typed: Request<MutationInput> = serde_json::from_value(raw.clone()).unwrap();
    fixture_digest(&fingerprint_material(&typed, binding).unwrap()).unwrap()
}
fn prepare(
    raw: &Value,
    binding: &ProjectBinding,
    grant: &Grant,
    trust: Option<&ProjectTrust>,
    review: Option<ReviewApproval<'_>>,
    now: u64,
) -> Result<PreparedMutation> {
    prepare_mutation(
        &serde_json::to_vec(raw).unwrap(),
        MutationContext {
            binding,
            grant,
            client: &id("client"),
            now,
            trust,
            review,
        },
        fixture_digest,
    )
}
#[test]
fn every_initial_command_has_typed_mapping_and_can_be_prepared_without_execution() {
    let binding = binding("project", 1);
    let trust = ProjectTrust::record(&binding);
    let grant = all_grants();
    let backend = state_digest();
    let contracts = tools::contracts().unwrap();
    assert_eq!(contracts.len(), 155);
    assert_eq!(
        contracts
            .iter()
            .filter(|c| c.schema_status == SchemaStatus::TypedMutation)
            .count(),
        11
    );
    assert_eq!(
        contracts
            .iter()
            .filter(|c| c.schema_status == SchemaStatus::TypedPlan)
            .count(),
        1
    );
    for spec in typed_specs() {
        let raw = call(spec.name, args(spec.name));
        let fingerprint = request_digest(&raw, &binding);
        let review = ReviewedPlan::record(&binding, id(spec.name), backend.clone(), 1000).unwrap();
        let approval = ReviewApproval {
            plan: &review,
            current_backend_digest: &backend,
            request_fingerprint: &fingerprint,
        };
        let prepared = prepare(&raw, &binding, &grant, Some(&trust), Some(approval), 100).unwrap();
        assert_eq!(prepared.request().input.tool(), spec.name);
        assert_eq!(prepared.binding().project_id(), binding.project_id());
        assert!(!prepared.descriptor().workspace_command.is_empty());
        assert_eq!(
            prepared.descriptor().mutates,
            spec.name != "environment.plan"
        );
        assert_eq!(
            prepared.require_live_handler(),
            Err(ErrorCode::UnsupportedCapability)
        );
    }
}
#[test]
fn client_confirmation_cannot_replace_server_trust_or_grants() {
    let binding = binding("project", 1);
    let grant = all_grants();
    let trust = ProjectTrust::record(&binding);
    let raw = call("services.start", json!({}));
    assert_eq!(
        prepare(&raw, &binding, &grant, None, None, 100).unwrap_err(),
        ErrorCode::Unauthorized
    );
    let managed = Grant::issue(
        id("client"),
        id("project"),
        vec![Scope::ProjectManage],
        1000,
    )
    .unwrap();
    assert_eq!(
        prepare(&raw, &binding, &managed, Some(&trust), None, 100).unwrap_err(),
        ErrorCode::Unauthorized
    );
    let other_trust = ProjectTrust::record(&self::binding("other", 1));
    assert_eq!(
        prepare(&raw, &binding, &grant, Some(&other_trust), None, 100).unwrap_err(),
        ErrorCode::Unauthorized
    );
    for field in ["confirm", "trusted", "consent"] {
        let mut raw = raw.clone();
        raw["input"]["arguments"][field] = json!(true);
        assert_eq!(
            prepare(&raw, &binding, &grant, Some(&trust), None, 100).unwrap_err(),
            ErrorCode::InvalidInput
        );
    }
    let mut revoked = all_grants();
    revoked.revoke();
    assert_eq!(
        prepare(&raw, &binding, &revoked, Some(&trust), None, 100).unwrap_err(),
        ErrorCode::GrantRevoked
    );
}
#[test]
fn mutations_reject_missing_key_revision_wrong_project_version_and_bounds() {
    let binding = binding("project", 1);
    let grant = all_grants();
    let trust = ProjectTrust::record(&binding);
    for field in ["idempotency_key", "expected_revision"] {
        let mut raw = call("services.start", json!({}));
        raw.as_object_mut().unwrap().remove(field);
        assert_eq!(
            prepare(&raw, &binding, &grant, Some(&trust), None, 100).unwrap_err(),
            ErrorCode::InvalidInput
        );
    }
    for (field, value, code) in [
        ("expected_revision", json!(2), ErrorCode::RevisionConflict),
        (
            "expected_revision",
            json!(SafeInt::MAX + 1),
            ErrorCode::InvalidInput,
        ),
        ("project_id", json!("other"), ErrorCode::ProjectMismatch),
        ("api_version", json!(2), ErrorCode::UnsupportedVersion),
    ] {
        let mut raw = call("services.start", json!({}));
        raw[field] = value;
        assert_eq!(
            prepare(&raw, &binding, &grant, Some(&trust), None, 100).unwrap_err(),
            code
        );
    }
    let raw = call(
        "services.start",
        json!({"limits":{"timeout_ms":1800001,"max_output_bytes":65536}}),
    );
    assert_eq!(
        prepare(&raw, &binding, &grant, Some(&trust), None, 100).unwrap_err(),
        ErrorCode::InvalidInput
    );
    let raw = call(
        "services.start",
        json!({"limits":{"timeout_ms":1000,"max_output_bytes":0}}),
    );
    assert_eq!(
        prepare(&raw, &binding, &grant, Some(&trust), None, 100).unwrap_err(),
        ErrorCode::InvalidInput
    );
}
#[test]
fn sensitive_review_binds_exact_action_backend_state_arguments_revision_and_expiry() {
    let binding = binding("project", 1);
    let grant = all_grants();
    let trust = ProjectTrust::record(&binding);
    let raw = call("environment.apply", args("environment.apply"));
    assert_eq!(
        prepare(&raw, &binding, &grant, Some(&trust), None, 100).unwrap_err(),
        ErrorCode::Unauthorized
    );
    let fingerprint = request_digest(&raw, &binding);
    let backend = state_digest();
    let review =
        ReviewedPlan::record(&binding, id("environment.apply"), backend.clone(), 200).unwrap();
    let approval = || ReviewApproval {
        plan: &review,
        current_backend_digest: &backend,
        request_fingerprint: &fingerprint,
    };
    let prepared = prepare(&raw, &binding, &grant, Some(&trust), Some(approval()), 100).unwrap();
    assert_eq!(prepared.approved_review(), Some(&backend));
    let mut changed = raw.clone();
    changed["input"]["arguments"]["write_env"] = json!(false);
    assert_eq!(
        prepare(
            &changed,
            &binding,
            &grant,
            Some(&trust),
            Some(approval()),
            100
        )
        .unwrap_err(),
        ErrorCode::ReviewMismatch
    );
    assert_eq!(
        prepare(&raw, &binding, &grant, Some(&trust), Some(approval()), 200).unwrap_err(),
        ErrorCode::ReviewExpired
    );
    let changed_backend = Digest::new("b".repeat(64)).unwrap();
    assert_eq!(
        prepare(
            &raw,
            &binding,
            &grant,
            Some(&trust),
            Some(ReviewApproval {
                plan: &review,
                current_backend_digest: &changed_backend,
                request_fingerprint: &fingerprint
            }),
            100
        )
        .unwrap_err(),
        ErrorCode::ReviewMismatch
    );
    let wrong_action =
        ReviewedPlan::record(&binding, id("environment.yaml_save"), backend.clone(), 200).unwrap();
    assert_eq!(
        prepare(
            &raw,
            &binding,
            &grant,
            Some(&trust),
            Some(ReviewApproval {
                plan: &wrong_action,
                current_backend_digest: &backend,
                request_fingerprint: &fingerprint
            }),
            100
        )
        .unwrap_err(),
        ErrorCode::ReviewMismatch
    );
    let stale = ReviewedPlan::record(
        &self::binding("project", 0),
        id("environment.apply"),
        backend.clone(),
        200,
    )
    .unwrap();
    assert_eq!(
        prepare(
            &raw,
            &binding,
            &grant,
            Some(&trust),
            Some(ReviewApproval {
                plan: &stale,
                current_backend_digest: &backend,
                request_fingerprint: &fingerprint
            }),
            100
        )
        .unwrap_err(),
        ErrorCode::RevisionConflict
    );
}
#[test]
fn fingerprint_is_retry_stable_but_binds_target_and_typed_arguments() {
    let binding = binding("project", 1);
    let raw = call("runtimes.remove", args("runtimes.remove"));
    let original = request_digest(&raw, &binding);
    let mut retry = raw.clone();
    retry["request_id"] = json!("req2");
    retry["idempotency_key"] = json!("another-key");
    assert_eq!(request_digest(&retry, &binding), original);
    let mut changed = raw.clone();
    changed["input"]["arguments"]["package_id"] = json!("other-runtime");
    assert_ne!(request_digest(&changed, &binding), original);
    let mut other = raw.clone();
    other["project_id"] = json!("other-project");
    assert_ne!(
        request_digest(&other, &self::binding("other-project", 1)),
        original
    );
    let record = IdempotencyRecord::new(
        id("client"),
        id("project"),
        id("key1"),
        original.clone(),
        id("op1"),
    );
    assert_eq!(
        record.replay(
            &id("client"),
            &id("project"),
            &id("key1"),
            &request_digest(&retry, &binding)
        ),
        Ok(&id("op1"))
    );
    assert_eq!(
        record.replay(
            &id("client"),
            &id("project"),
            &id("key1"),
            &request_digest(&changed, &binding)
        ),
        Err(ErrorCode::IdempotencyConflict)
    );
}
#[test]
fn plan_is_nonmutating_and_unimplemented_actions_remain_unavailable() {
    let binding = binding("project", 1);
    let observer = Grant::issue(id("client"), id("project"), vec![Scope::Observer], 1000).unwrap();
    let mut raw = call("environment.plan", args("environment.plan"));
    raw.as_object_mut().unwrap().remove("idempotency_key");
    let prepared = prepare(&raw, &binding, &observer, None, None, 100).unwrap();
    assert!(!prepared.descriptor().mutates);
    assert!(prepared.approved_review().is_none());
    assert_eq!(
        prepared.descriptor().effects,
        vec!["read", "review_plan_output"]
    );
    assert_eq!(
        prepare(
            &call("workspaces.create", json!({})),
            &binding,
            &all_grants(),
            None,
            None,
            100
        )
        .unwrap_err(),
        ErrorCode::UnsupportedCapability
    );
    assert_eq!(
        prepare(
            &call("debugger.arbitrary_expression_evaluation", json!({})),
            &binding,
            &all_grants(),
            None,
            None,
            100
        )
        .unwrap_err(),
        ErrorCode::UnsupportedCapability
    );
}
#[test]
fn domain_document_and_payload_types_match_the_actual_command_boundary() {
    for invalid in [
        "localhost",
        "evil.com",
        "A.devforge.test",
        "-a.devforge.test",
        "a-.devforge.test",
        "a..devforge.test",
    ] {
        assert!(DomainName::new(invalid.into()).is_err(), "{invalid}");
    }
    assert!(DomainName::new("demo.devforge.test".into()).is_ok());
    assert!(YamlText::new("version: 1\nproject:\n\tname: Demo\r\n".into()).is_ok());
    assert!(YamlText::new("key:\0secret".into()).is_err());
    assert!(serde_json::from_value::<ConfigureInput>(json!({"name":"","php":"php1","node":"node1","domain":"demo.devforge.test","https":true,"vite":false,"queue":false})).is_err());
    let mut raw = args("environment.apply");
    raw["trusted"] = json!(true);
    assert!(serde_json::from_value::<EnvironmentApplyInput>(raw).is_err());
}

#[test]
fn command_receipts_are_project_tool_and_byte_bound() {
    let binding = binding("project", 1);
    let grant = all_grants();
    let raw = call("services.stop", json!({}));
    let prepared = prepare(&raw, &binding, &grant, None, None, 100).unwrap();
    let output = |tool, project| {
        serde_json::from_value::<MutationOutput>(json!({"tool":tool,"data":{"project_id":project,"operation_id":"op1","state":"succeeded","revision":1}})).unwrap()
    };
    assert!(
        prepared
            .check_output(&output("services.stop", "project"))
            .is_ok()
    );
    assert_eq!(
        prepared.check_output(&output("services.stop", "other")),
        Err(ErrorCode::ProjectMismatch)
    );
    assert_eq!(
        prepared.check_output(&output("services.start", "project")),
        Err(ErrorCode::InvalidInput)
    );
    let raw = call(
        "services.stop",
        json!({"limits":{"timeout_ms":1000,"max_output_bytes":32}}),
    );
    let small = prepare(&raw, &binding, &grant, None, None, 100).unwrap();
    assert_eq!(
        small.check_output(&output("services.stop", "project")),
        Err(ErrorCode::OutputLimitExceeded)
    );
}

#[test]
fn project_name_matches_core_byte_and_trim_constraints() {
    assert!(ProjectName::new(" \t\n".into()).is_err());
    assert!(ProjectName::new(" ".into()).is_err());
    assert!(ProjectName::new("x".repeat(201)).is_err());
    assert!(ProjectName::new("x".repeat(200)).is_ok());
    assert!(ProjectName::new("é".repeat(100)).is_ok());
    assert!(ProjectName::new(format!("{}x", "é".repeat(100))).is_err());
    assert!(ProjectName::new("é".repeat(101)).is_err());
}
