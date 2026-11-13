//! Deterministic JSON schema and TypeScript export from contract field definitions.
use crate::{limits::*, tools, *};
use serde_json::{Value, json};

pub trait Wire {
    fn schema() -> Value;
    fn ts() -> String;
    fn optional() -> bool {
        false
    }
    fn declaration() -> Option<String> {
        None
    }
}
impl Wire for Id {
    fn schema() -> Value {
        json!({"type":"string","minLength":1,"maxLength":128,"pattern":"^[A-Za-z0-9_.:-]+$"})
    }
    fn ts() -> String {
        "string".into()
    }
}
impl Wire for bool {
    fn schema() -> Value {
        json!({"type":"boolean"})
    }
    fn ts() -> String {
        "boolean".into()
    }
}
impl Wire for i32 {
    fn schema() -> Value {
        json!({"type":"integer","minimum":i32::MIN,"maximum":i32::MAX})
    }
    fn ts() -> String {
        "number".into()
    }
}
impl Wire for u16 {
    fn schema() -> Value {
        json!({"type":"integer","minimum":0,"maximum":u16::MAX})
    }
    fn ts() -> String {
        "number".into()
    }
}
impl Wire for SafeInt {
    fn schema() -> Value {
        json!({"type":"integer","minimum":0,"maximum":SafeInt::MAX})
    }
    fn ts() -> String {
        "number".into()
    }
}
impl<const MAX: u64> Wire for Bounded<MAX> {
    fn schema() -> Value {
        json!({"type":"integer","minimum":1,"maximum":MAX})
    }
    fn ts() -> String {
        "number".into()
    }
}
impl<const MAX: usize> Wire for Text<MAX> {
    fn schema() -> Value {
        json!({"type":"string","maxLength":MAX,"pattern":"^[^\\u0000-\\u001f\\u007f-\\u009f]*$"})
    }
    fn ts() -> String {
        "string".into()
    }
}
impl<T: Wire> Wire for Option<T> {
    fn schema() -> Value {
        json!({"anyOf":[T::schema(),{"type":"null"}]})
    }
    fn ts() -> String {
        format!("{} | null", T::ts())
    }
    fn optional() -> bool {
        true
    }
}
impl<T: Wire> Wire for Vec<T> {
    fn schema() -> Value {
        json!({"type":"array","maxItems":500,"items":T::schema()})
    }
    fn ts() -> String {
        format!("Array<{}>", T::ts())
    }
}
fn enum_schema<T: Serialize>(values: &[T]) -> Value {
    json!({"type":"string","enum":values.iter().map(|v|serde_json::to_value(v).expect("known enum")).collect::<Vec<_>>()})
}
fn enum_ts<T: Serialize>(values: &[T]) -> String {
    values
        .iter()
        .map(|v| serde_json::to_string(v).expect("known enum"))
        .collect::<Vec<_>>()
        .join(" | ")
}
impl Wire for Origin {
    fn schema() -> Value {
        enum_schema(&[Origin::Live, Origin::Persisted, Origin::Cached])
    }
    fn ts() -> String {
        enum_ts(&[Origin::Live, Origin::Persisted, Origin::Cached])
    }
}
impl Wire for OperationState {
    fn schema() -> Value {
        use OperationState::*;
        enum_schema(&[
            Queued,
            Running,
            CancelRequested,
            Succeeded,
            Failed,
            Cancelled,
            Interrupted,
            Unknown,
        ])
    }
    fn ts() -> String {
        use OperationState::*;
        enum_ts(&[
            Queued,
            Running,
            CancelRequested,
            Succeeded,
            Failed,
            Cancelled,
            Interrupted,
            Unknown,
        ])
    }
}
impl Wire for Cursor {
    fn schema() -> Value {
        json!({"type":"object","additionalProperties":false,"required":["project_id","tool","revision","offset","expires_at"],"properties":{
        "project_id":Id::schema(),"tool":Id::schema(),"revision":SafeInt::schema(),"offset":SafeInt::schema(),"expires_at":SafeInt::schema()}})
    }
    fn ts() -> String {
        "Cursor".into()
    }
    fn declaration() -> Option<String> {
        Some("export interface Cursor { project_id: string; tool: string; revision: number; offset: number; expires_at: number; }\n".into())
    }
}
impl Wire for PageRequest {
    fn schema() -> Value {
        json!({"type":"object","additionalProperties":false,"required":["limit","max_bytes","timeout_ms"],"properties":{
        "limit":Bounded::<500>::schema(),"max_bytes":Bounded::<262144>::schema(),"timeout_ms":Bounded::<30000>::schema(),"cursor":Option::<Cursor>::schema()}})
    }
    fn ts() -> String {
        "PageRequest".into()
    }
    fn declaration() -> Option<String> {
        Some("export interface PageRequest { limit: number; max_bytes: number; timeout_ms: number; cursor?: Cursor | null; }\n".into())
    }
}
pub const ERRORS: &[ErrorCode] = &[
    ErrorCode::InvalidInput,
    ErrorCode::CursorExpired,
    ErrorCode::OutputLimitExceeded,
    ErrorCode::TimedOut,
    ErrorCode::UnsupportedVersion,
    ErrorCode::Unauthorized,
    ErrorCode::GrantExpired,
    ErrorCode::GrantRevoked,
    ErrorCode::ProjectMismatch,
    ErrorCode::RevisionConflict,
    ErrorCode::ReviewExpired,
    ErrorCode::ReviewMismatch,
    ErrorCode::IdempotencyConflict,
    ErrorCode::InvalidTransition,
    ErrorCode::UnsupportedCapability,
    ErrorCode::Busy,
    ErrorCode::Interrupted,
    ErrorCode::StorageUnavailable,
];
/// Schema for the concrete envelope accepted by prepare_read, not unrestricted Request<T>.
/// Wire timestamps are Unix milliseconds; timeout_ms is a monotonic duration.
pub fn request_schema() -> Value {
    let variants=tools::typed_specs().iter().map(|s|json!({"type":"object","additionalProperties":false,"required":["tool","arguments"],"properties":{"tool":{"const":s.name},"arguments":(s.input_schema)()}})).collect::<Vec<_>>();
    json!({"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","additionalProperties":false,"required":["api_version","request_id","project_id","input"],"properties":{
        "api_version":{"const":API_VERSION},"request_id":Id::schema(),"project_id":Id::schema(),"expected_revision":Option::<SafeInt>::schema(),"idempotency_key":Option::<Id>::schema(),"input":{"oneOf":variants}}})
}
pub fn json_document() -> Result<String> {
    serde_json::to_string_pretty(&json!({"api_version":API_VERSION,"live_available":false,"timestamp_unit":"unix_milliseconds","timeout_unit":"monotonic_milliseconds","request_schema":request_schema(),"error_codes":ERRORS,"tools":tools::contracts()?}))
        .map(|s|s+"\n").map_err(|_|ErrorCode::InvalidInput)
}
pub fn typescript() -> String {
    let mut text = String::from(
        "// Generated by devforge-control export_contracts. Do not edit.\n// Projection schemas only; no live handler is installed.\n// observed_at/expires_at: Unix milliseconds; timeout_ms: monotonic duration.\n",
    );
    text.push_str(&format!("export type ErrorCode = {};\n", enum_ts(ERRORS)));
    text.push_str(&Cursor::declaration().unwrap());
    text.push_str(&PageRequest::declaration().unwrap());
    for declaration in tools::declarations() {
        text.push_str(&declaration);
    }
    for (name, output) in [("ToolInputs", false), ("ToolOutputs", true)] {
        text.push_str(&format!("export interface {name} {{\n"));
        for s in tools::typed_specs() {
            text.push_str(&format!(
                "  {:?}: {};\n",
                s.name,
                if output { s.output_ts } else { s.input_ts }
            ));
        }
        text.push_str("}\n");
    }
    text.push_str("export type ToolInput = { [K in keyof ToolInputs]: { tool: K; arguments: ToolInputs[K] } }[keyof ToolInputs];\n");
    text.push_str("export interface ReadRequest { api_version: 1; request_id: string; project_id: string; expected_revision?: number | null; idempotency_key?: string | null; input: ToolInput; }\n");
    text.push_str("export type ReadResponse<K extends keyof ToolOutputs> = { api_version: 1; request_id: string; operation_id?: string; warnings: ErrorCode[] } & ({ ok: true; data: { tool: K; data: ToolOutputs[K] }; error?: never } | { ok: false; data?: never; error: { code: ErrorCode; retryable: boolean } });\n");
    text
}
