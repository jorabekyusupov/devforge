use devforge_control::{export, inventory::Family, limits::*, tools::*, *};
use serde::{Serialize, Serializer, ser::SerializeSeq};
use serde_json::{Value, json};
use std::{cell::Cell, time::Duration};
fn id(v: &str) -> Id {
    Id::new(v).unwrap()
}
fn binding(project: &str, revision: u64) -> ProjectBinding {
    ProjectBinding::from_registered(id(project), &std::env::current_dir().unwrap(), revision)
        .unwrap()
}
fn grant(scopes: Vec<Scope>) -> Grant {
    Grant::issue(id("client"), id("project"), scopes, 1000).unwrap()
}
fn arguments(name: &str) -> Value {
    match name {
        "execution.exit_code" | "operations.status" | "http.readiness_tls_response_projection" => {
            json!({"operation_id":"op1"})
        }
        "artifacts.scoped_artifact_metadata" => json!({"artifact_id":"artifact1"}),
        _ => json!({}),
    }
}
fn call(tool: &str, args: Value) -> Value {
    json!({"api_version":1,"request_id":"req1","project_id":"project","expected_revision":1,"input":{"tool":tool,"arguments":args}})
}
fn prepare(raw: Value, grant: &Grant) -> Result<PreparedRead> {
    prepare_read(
        &serde_json::to_vec(&raw).unwrap(),
        &binding("project", 1),
        grant,
        &id("client"),
        100,
        None,
    )
}
#[test]
fn all_inventory_operations_have_explicit_schema_availability_and_policy() {
    let contracts = contracts().unwrap();
    assert_eq!(contracts.len(), 155);
    assert_eq!(
        contracts
            .iter()
            .filter(|c| c.schema_status == SchemaStatus::TypedProjection)
            .count(),
        16
    );
    let full = grant(vec![
        Scope::Observer,
        Scope::ProjectManage,
        Scope::ExecutionTest,
        Scope::SensitiveOperation,
    ]);
    for contract in &contracts {
        assert!(!contract.live_available);
        assert_eq!(contract.unavailable_error, ErrorCode::UnsupportedCapability);
        match contract.schema_status {
            SchemaStatus::TypedProjection => {
                assert!(contract.input_schema.is_some() && contract.output_schema.is_some());
                let raw = call(&contract.name, arguments(&contract.name));
                let operation = OperationAccess {
                    operation_id: &id("op1"),
                    owner_client: &id("client"),
                    project_id: &id("project"),
                    original_scopes: &[Scope::ExecutionTest],
                };
                let prepared = prepare_read(
                    &serde_json::to_vec(&raw).unwrap(),
                    &binding("project", 1),
                    &full,
                    &id("client"),
                    100,
                    Some(operation),
                )
                .unwrap();
                assert_eq!(
                    prepared.require_live_handler(),
                    Err(ErrorCode::UnsupportedCapability)
                );
            }
            SchemaStatus::TypedMutation | SchemaStatus::TypedPlan => {
                assert!(contract.input_schema.is_some() && contract.output_schema.is_some());
                assert_eq!(
                    prepare(call(&contract.name, json!({})), &full).unwrap_err(),
                    ErrorCode::UnsupportedCapability
                );
            }
            SchemaStatus::Planned | SchemaStatus::Unsupported => {
                assert!(contract.input_schema.is_none() && contract.output_schema.is_none());
                assert_eq!(
                    prepare(call(&contract.name, json!({})), &full).unwrap_err(),
                    ErrorCode::UnsupportedCapability
                );
            }
        }
    }
}
#[test]
fn typed_payloads_reject_unknown_fields_wrong_types_and_unsafe_numbers() {
    let observer = grant(vec![Scope::Observer]);
    assert_eq!(
        prepare(
            call("environment.masked_get", json!({"raw_secrets":true})),
            &observer
        )
        .unwrap_err(),
        ErrorCode::InvalidInput
    );
    assert_eq!(
        prepare(call("mail.list", json!({"recipient":5})), &observer).unwrap_err(),
        ErrorCode::InvalidInput
    );
    assert_eq!(
        prepare(
            call("logs.search", json!({"query":"secret\nline"})),
            &observer
        )
        .unwrap_err(),
        ErrorCode::InvalidInput
    );
    assert_eq!(
        prepare(
            call("logs.search", json!({"query":"x".repeat(257)})),
            &observer
        )
        .unwrap_err(),
        ErrorCode::InvalidInput
    );
    // JSON Schema maxLength and Rust both count Unicode scalar values.
    assert!(
        prepare(
            call("logs.search", json!({"query":"🦀".repeat(65)})),
            &observer
        )
        .is_ok()
    );
    assert_eq!(
        prepare(
            call("logs.search", json!({"query":"🦀".repeat(257)})),
            &observer
        )
        .unwrap_err(),
        ErrorCode::InvalidInput
    );
    let mut raw = call("health.health", json!({}));
    raw["api_version"] = json!(2);
    assert_eq!(
        prepare(raw, &observer).unwrap_err(),
        ErrorCode::UnsupportedVersion
    );
    let mut raw = call("health.health", json!({}));
    raw["expected_revision"] = json!(SafeInt::MAX + 1);
    assert_eq!(
        prepare(raw, &observer).unwrap_err(),
        ErrorCode::RevisionConflict
    );
    assert!(serde_json::from_value::<SafeInt>(json!(SafeInt::MAX + 1)).is_err());
    assert!(SafeInt::new(SafeInt::MAX).is_ok());
}
#[test]
fn typed_dispatch_checks_project_grant_revocation_and_operation_owner() {
    let mut observer = grant(vec![Scope::Observer]);
    assert!(prepare(call("health.health", json!({})), &observer).is_ok());
    let mut other = call("health.health", json!({}));
    other["project_id"] = json!("other");
    assert_eq!(
        prepare(other, &observer).unwrap_err(),
        ErrorCode::ProjectMismatch
    );
    observer.revoke();
    assert_eq!(
        prepare(call("health.health", json!({})), &observer).unwrap_err(),
        ErrorCode::GrantRevoked
    );
    let observer = grant(vec![Scope::Observer]);
    let raw =
        serde_json::to_vec(&call("operations.status", json!({"operation_id":"op1"}))).unwrap();
    assert_eq!(
        prepare_read(
            &raw,
            &binding("project", 1),
            &observer,
            &id("client"),
            100,
            None
        )
        .unwrap_err(),
        ErrorCode::Unauthorized
    );
    let access = OperationAccess {
        operation_id: &id("op1"),
        owner_client: &id("client"),
        project_id: &id("project"),
        original_scopes: &[Scope::ExecutionTest],
    };
    assert_eq!(
        prepare_read(
            &raw,
            &binding("project", 1),
            &observer,
            &id("client"),
            100,
            Some(access)
        )
        .unwrap_err(),
        ErrorCode::Unauthorized
    );
    let full = grant(vec![Scope::Observer, Scope::ExecutionTest]);
    let access = OperationAccess {
        operation_id: &id("other-op"),
        owner_client: &id("client"),
        project_id: &id("project"),
        original_scopes: &[Scope::ExecutionTest],
    };
    assert_eq!(
        prepare_read(
            &raw,
            &binding("project", 1),
            &full,
            &id("client"),
            100,
            Some(access)
        )
        .unwrap_err(),
        ErrorCode::Unauthorized
    );
}
#[test]
fn cursor_is_project_tool_revision_and_expiry_bound() {
    let observer = grant(vec![Scope::Observer]);
    let cursor = json!({"project_id":"project","tool":"mail.list","revision":1,"offset":100,"expires_at":101});
    let args =
        |cursor| json!({"page":{"limit":100,"max_bytes":65536,"timeout_ms":5000,"cursor":cursor}});
    assert!(prepare(call("mail.list", args(cursor.clone())), &observer).is_ok());
    for (field, value, code) in [
        ("tool", json!("logs.search"), ErrorCode::CursorExpired),
        ("revision", json!(2), ErrorCode::CursorExpired),
        ("expires_at", json!(100), ErrorCode::CursorExpired),
        ("project_id", json!("other"), ErrorCode::ProjectMismatch),
    ] {
        let mut changed = cursor.clone();
        changed[field] = value;
        assert_eq!(
            prepare(call("mail.list", args(changed)), &observer).unwrap_err(),
            code
        );
    }
    for (field, value) in [
        ("limit", 501),
        ("limit", 0),
        ("max_bytes", 262145),
        ("timeout_ms", 30001),
    ] {
        let mut page = json!({"limit":100,"max_bytes":65536,"timeout_ms":5000});
        page[field] = json!(value);
        assert_eq!(
            prepare(call("mail.list", json!({"page":page})), &observer).unwrap_err(),
            ErrorCode::InvalidInput
        );
    }
}
struct Streaming<'a>(&'a Cell<usize>);
impl Serialize for Streaming<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(None)?;
        for _ in 0..1_000_000 {
            self.0.set(self.0.get() + 1);
            seq.serialize_element("0123456789")?;
        }
        seq.end()
    }
}
#[test]
fn serialization_stops_at_byte_budget_without_buffering_the_full_payload() {
    let page: PageRequest =
        serde_json::from_value(json!({"limit":1,"max_bytes":32,"timeout_ms":30000})).unwrap();
    let budget = ReadBudget::start(page);
    let visited = Cell::new(0);
    assert_eq!(
        budget.check_payload(1, &Streaming(&visited)),
        Err(ErrorCode::OutputLimitExceeded)
    );
    assert!(visited.get() < 10, "serializer kept consuming after bound");
    assert_eq!(
        budget.check_payload(2, &json!([])),
        Err(ErrorCode::OutputLimitExceeded)
    );
    assert_eq!(
        budget.check_elapsed(Duration::from_millis(30000)),
        Err(ErrorCode::TimedOut)
    );
    assert_eq!(budget.check_elapsed(Duration::from_millis(29999)), Ok(()));
}
#[test]
fn bounded_outputs_are_typed_and_do_not_conflate_unknown_tls_or_service_state() {
    let observer = grant(vec![Scope::Observer]);
    let read = prepare(
        call(
            "services.status",
            json!({"page":{"limit":1,"max_bytes":1000,"timeout_ms":5000}}),
        ),
        &observer,
    )
    .unwrap();
    let output:ToolOutput=serde_json::from_value(json!({"tool":"services.status","data":{"freshness":{"origin":"persisted","observed_at":99,"revision":1},"items":[{"service_id":"web","state":"stopped","ready":null}],"next_cursor":null}})).unwrap();
    assert!(
        read.check_output(&output, &binding("project", 1), 100)
            .is_ok()
    );
    let invalid: std::result::Result<ToolOutput, _> = serde_json::from_value(
        json!({"tool":"services.status","data":{"freshness":{"origin":"persisted","observed_at":99,"revision":1},"items":[{"service_id":"web","state":"queued","ready":true}]}}),
    );
    assert!(invalid.is_err());
    let http: HttpItem =
        serde_json::from_value(json!({"operation_id":"op1","status":null,"tls_verified":null}))
            .unwrap();
    assert!(http.tls_verified.is_none());
    let http = contracts()
        .unwrap()
        .into_iter()
        .find(|c| c.name == "http.readiness_tls_response_projection")
        .unwrap();
    assert_eq!(http.required_scopes, vec![Scope::Observer]);
    assert_eq!(http.effects, vec!["read"]);
    let inventory = devforge_control::inventory::entries().unwrap();
    let active = inventory
        .iter()
        .find(|e| e.family == Family::Http)
        .unwrap()
        .operations
        .iter()
        .find(|op| op.name == "managed_site_request")
        .unwrap();
    assert!(active.required_scopes.contains(&Scope::ExecutionTest));
}
#[test]
fn stable_fault_mapping_does_not_include_private_payload_text() {
    let fault = ErrorCode::CursorExpired.fault();
    assert!(!fault.retryable);
    assert_eq!(
        serde_json::to_value(fault).unwrap(),
        json!({"code":"cursor_expired","retryable":false})
    );
    assert!(ErrorCode::Busy.fault().retryable);
    assert!(ErrorCode::TimedOut.fault().retryable);
    assert!(!ErrorCode::Interrupted.fault().retryable);
}
#[test]
fn checked_json_and_typescript_match_the_rust_contract_export() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("contracts");
    assert_eq!(
        std::fs::read_to_string(root.join("control-contracts.json")).unwrap(),
        export::json_document().unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(root.join("control-contracts.ts")).unwrap(),
        export::typescript()
    );
    let doc: Value = serde_json::from_str(&export::json_document().unwrap()).unwrap();
    assert_eq!(doc["tools"].as_array().unwrap().len(), 155);
    assert_eq!(
        doc["request_schema"]["properties"]["expected_revision"]["anyOf"][0]["maximum"],
        json!(SafeInt::MAX)
    );
}

#[test]
fn projection_result_cannot_return_another_project_or_requested_resource() {
    let observer = grant(vec![Scope::Observer]);
    let call = prepare(call("projects.get", json!({})), &observer).unwrap();
    let output:ToolOutput=serde_json::from_value(json!({"tool":"projects.get","data":{"freshness":{"origin":"live","observed_at":99,"revision":1},"items":[{"project_id":"other","name":"Demo","php":"php1","node":"node1","composer":"composer1","revision":1}]}})).unwrap();
    assert_eq!(
        call.check_output(&output, &binding("project", 1), 100),
        Err(ErrorCode::ProjectMismatch)
    );
}

#[test]
fn every_operation_backed_projection_requires_owner_and_original_grants() {
    let registry = binding("project", 1);
    let client = id("client");
    let other = id("other-client");
    let project = id("project");
    let op = id("op1");
    let full = grant(vec![Scope::Observer, Scope::ExecutionTest]);
    let observer = grant(vec![Scope::Observer]);
    for tool in [
        "operations.status",
        "execution.exit_code",
        "http.readiness_tls_response_projection",
    ] {
        let bytes = serde_json::to_vec(&call(tool, json!({"operation_id":"op1"}))).unwrap();
        assert_eq!(
            prepare_read(&bytes, &registry, &full, &client, 100, None).unwrap_err(),
            ErrorCode::Unauthorized
        );
        let access = || OperationAccess {
            operation_id: &op,
            owner_client: &client,
            project_id: &project,
            original_scopes: &[Scope::ExecutionTest],
        };
        assert!(prepare_read(&bytes, &registry, &full, &client, 100, Some(access())).is_ok());
        assert_eq!(
            prepare_read(&bytes, &registry, &observer, &client, 100, Some(access())).unwrap_err(),
            ErrorCode::Unauthorized
        );
        let other_owner = OperationAccess {
            operation_id: &op,
            owner_client: &other,
            project_id: &project,
            original_scopes: &[Scope::ExecutionTest],
        };
        assert_eq!(
            prepare_read(&bytes, &registry, &full, &client, 100, Some(other_owner)).unwrap_err(),
            ErrorCode::Unauthorized
        );
    }
}

#[test]
fn prepared_projection_cannot_be_checked_against_another_binding() {
    let observer = grant(vec![Scope::Observer]);
    let prepared = prepare(call("health.health", json!({})), &observer).unwrap();
    let output:ToolOutput=serde_json::from_value(json!({"tool":"health.health","data":{"freshness":{"origin":"live","observed_at":99,"revision":1},"items":[]}})).unwrap();
    assert_eq!(
        prepared.check_output(&output, &binding("other-project", 1), 100),
        Err(ErrorCode::ProjectMismatch)
    );
}
