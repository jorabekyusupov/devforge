//! Typed initial projection surface. No handler is registered or executed here.
use crate::export::Wire;
use crate::{
    inventory::{self, CoreStatus, Policy},
    limits::*,
    *,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

macro_rules! contract {
    ($name:ident {$($(#[$field_meta:meta])* $field:ident : $ty:ty),* $(,)?}) => {
        #[derive(Debug,Clone,Serialize,Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct $name { $($(#[$field_meta])* pub $field:$ty),* }
        impl Wire for $name {
            fn schema()->Value {
                let mut properties=serde_json::Map::new();
                let mut required=vec![];
                $(properties.insert(stringify!($field).into(),<$ty as Wire>::schema());
                  if !<$ty as Wire>::optional(){required.push(stringify!($field));})*
                json!({"type":"object","additionalProperties":false,"properties":properties,"required":required})
            }
            fn ts()->String {stringify!($name).into()}
            fn declaration()->Option<String> {
                let mut s=format!("export interface {} {{\n",stringify!($name));
                $(s.push_str(&format!("  {}{}: {};\n",stringify!($field),if <$ty as Wire>::optional(){"?"}else{""},<$ty as Wire>::ts()));)*
                s.push_str("}\n");Some(s)
            }
        }
    }
}
// Sensitive mail filters and content must never be rendered by Debug.
macro_rules! private_contract {
    ($name:ident {$($(#[$field_meta:meta])* $field:ident : $ty:ty),* $(,)?}) => {
        #[derive(Clone,Serialize,Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct $name { $($(#[$field_meta])* pub $field:$ty),* }
        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(concat!(stringify!($name), "([REDACTED])"))
            }
        }
        impl Wire for $name {
            fn schema()->Value {
                let mut properties=serde_json::Map::new(); let mut required=vec![];
                $(properties.insert(stringify!($field).into(),<$ty as Wire>::schema());
                  if !<$ty as Wire>::optional(){required.push(stringify!($field));})*
                json!({"type":"object","additionalProperties":false,"properties":properties,"required":required})
            }
            fn ts()->String {stringify!($name).into()}
            fn declaration()->Option<String> {
                let mut s=format!("export interface {} {{\n",stringify!($name));
                $(s.push_str(&format!("  {}{}: {};\n",stringify!($field),if <$ty as Wire>::optional(){"?"}else{""},<$ty as Wire>::ts()));)*
                s.push_str("}\n"); Some(s)
            }
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceState {
    Starting,
    Running,
    Ready,
    Backoff,
    Stopping,
    Stopped,
    Succeeded,
    Failed,
    Unknown,
}
impl Wire for ServiceState {
    fn schema() -> Value {
        json!({"type":"string","enum":["starting","running","ready","backoff","stopping","stopped","succeeded","failed","unknown"]})
    }
    fn ts() -> String {
        "\"starting\" | \"running\" | \"ready\" | \"backoff\" | \"stopping\" | \"stopped\" | \"succeeded\" | \"failed\" | \"unknown\"".into()
    }
}
pub(crate) use contract;
contract!(ProjectionWindow {
    epoch: Digest,
    high_water: SafeInt,
    low_water: SafeInt,
    row_truncated: bool,
    byte_truncated: bool,
    bounded: bool,
    source_missing: bool
});
contract!(ProjectionFreshness {
    origin: Origin,
    observed_at: SafeInt,
    revision: SafeInt,
    #[serde(default,skip_serializing_if="Option::is_none")]
    window:Option<ProjectionWindow>
});
contract!(HealthInput{page:Option<PageRequest>});
contract!(ProjectInput{page:Option<PageRequest>});
contract!(RuntimeInput{page:Option<PageRequest>,kind:Option<Text<64>>});
contract!(ServiceInput{page:Option<PageRequest>,service_id:Option<Id>});
contract!(EnvironmentInput{page:Option<PageRequest>,name:Option<Id>});
contract!(DomainInput{page:Option<PageRequest>});
contract!(DatabaseInput{page:Option<PageRequest>});
contract!(LogInput{page:Option<PageRequest>,query:Option<Text<256>>,level:Option<Text<32>>,#[serde(default,skip_serializing_if="Option::is_none")] since:Option<SafeInt>,#[serde(default,skip_serializing_if="Option::is_none")] content:Option<bool>});
contract!(InspectorInput{page:Option<PageRequest>,context_id:Option<Id>,#[serde(default,skip_serializing_if="Option::is_none")] detail:Option<bool>});

/// A server-held observation checkpoint, distinct from a frozen pagination cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityStream {
    Inspector,
    Logs,
    Mail,
}
impl Wire for ActivityStream {
    fn schema() -> Value {
        json!({"type":"string","enum":["inspector","logs","mail"]})
    }
    fn ts() -> String {
        "\"inspector\" | \"logs\" | \"mail\"".into()
    }
}
private_contract!(ActivityMarkInput{stream:ActivityStream,detail:bool,context_id:Option<Id>,#[serde(default,skip_serializing_if="Option::is_none")] subject:Option<Text<256>>,#[serde(default,skip_serializing_if="Option::is_none")] recipient:Option<Text<256>>,#[serde(default,skip_serializing_if="Option::is_none")] sender:Option<Text<256>>,#[serde(default,skip_serializing_if="Option::is_none")] since:Option<SafeInt>});
contract!(ActivityMarkOutput { bookmark: Id });
contract!(ActivityWaitInput{bookmark:Id,timeout_ms:Bounded<3000>,context_id:Option<Id>});

contract!(DebuggerInput{page:Option<PageRequest>});
private_contract!(MailInput{page:Option<PageRequest>,subject:Option<Text<256>>,recipient:Option<Text<256>>,#[serde(default,skip_serializing_if="Option::is_none")] sender:Option<Text<256>>,#[serde(default,skip_serializing_if="Option::is_none")] since:Option<SafeInt>,#[serde(default,skip_serializing_if="Option::is_none")] detail:Option<bool>,#[serde(default,skip_serializing_if="Option::is_none")] mail_id:Option<Id>});
contract!(ExecutionInput{page:Option<PageRequest>,operation_id:Id});
contract!(HttpInput{page:Option<PageRequest>,operation_id:Id});
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspacePrepareAction {
    Create,
    Archive,
    Restore,
    Purge,
    Merge,
}
impl Wire for WorkspacePrepareAction {
    fn schema() -> Value {
        json!({"type":"string","enum":["create","archive","restore","purge","merge"]})
    }
    fn ts() -> String {
        "\"create\" | \"archive\" | \"restore\" | \"purge\" | \"merge\"".into()
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceDatabase {
    Empty,
    Clone,
}
impl Wire for WorkspaceDatabase {
    fn schema() -> Value {
        json!({"type":"string","enum":["empty","clone"]})
    }
    fn ts() -> String {
        "\"empty\" | \"clone\"".into()
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceListInput {}
impl Wire for WorkspaceListInput {
    fn schema() -> Value {
        json!({"type":"object","additionalProperties":false,"properties":{},"required":[]})
    }
    fn ts() -> String {
        "WorkspaceListInput".into()
    }
    fn declaration() -> Option<String> {
        Some("export interface WorkspaceListInput {}\n".into())
    }
}
contract!(WorkspaceListItem { workspace_id: Id, name: Text<200>, stage: Id, project_id: Option<Id> });
contract!(WorkspaceListOutput { project_id: Id, revision: SafeInt, origin: Origin, observed_at: SafeInt, items: Vec<WorkspaceListItem>, truncated: bool });
contract!(WorkspacePrepareInput { action: WorkspacePrepareAction, workspace_id: Option<Id>, name: Option<Text<200>>, branch: Option<Text<256>>, base: Option<Text<256>>, new_branch: Option<bool>, database: Option<WorkspaceDatabase>, dependencies: Option<bool>, copy_environment: Option<bool> });
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceMergeEffect {
    FastForward,
    AlreadyIncluded,
    MergeCommit,
}
impl Wire for WorkspaceMergeEffect {
    fn schema() -> Value {
        json!({"type":"string","enum":["fast_forward","already_included","merge_commit"]})
    }
    fn ts() -> String {
        "\"fast_forward\" | \"already_included\" | \"merge_commit\"".into()
    }
}
contract!(WorkspaceMergePreview { source_head: Text<64>, target_head: Text<64>, target_branch: Text<256>, target_project_revision: SafeInt, source_dirty: bool, effect: WorkspaceMergeEffect, result_tree: Option<Text<64>>, isolated: bool });
contract!(WorkspaceCreatePreview { source_commit: Text<64>, source_dirty: Option<bool>, isolated: bool });
contract!(WorkspacePrepareOutput { project_id: Id, revision: SafeInt, kind: WorkspacePrepareAction, workspace_id: Option<Id>, blockers: Vec<Id>, plan: Option<Digest>, create: Option<WorkspaceCreatePreview>, merge: Option<WorkspaceMergePreview> });
contract!(WorkspaceInput{page:Option<PageRequest>});
contract!(OperationInput{page:Option<PageRequest>,operation_id:Id});
contract!(ArtifactInput{page:Option<PageRequest>,artifact_id:Id});

contract!(HealthItem{code:Id,retryable:bool,next_action:Option<Id>,#[serde(default,skip_serializing_if="Option::is_none")] evidence:Option<Id>,#[serde(default,skip_serializing_if="Option::is_none")] impact:Option<Id>});
contract!(ProjectItem{project_id:Id,name:Text<256>,php:Id,node:Id,composer:Id,revision:SafeInt});
contract!(RuntimeItem{runtime_id:Id,kind:Text<64>,version:Text<64>,installed:bool,problem:Option<Id>});
contract!(ServiceItem {
    service_id: Id,
    state: ServiceState,
    ready: Option<bool>
});
contract!(EnvironmentItem {
    name: Id,
    present: bool
});
contract!(DomainItem{url:Text<2048>,certificate_active:bool,system_trusted:bool});
contract!(DatabaseItem{engine:Text<32>,bound:bool,ready:bool});
contract!(LogItem{event_id:Id,level:Text<32>,code:Id,observed_at:SafeInt,#[serde(default,skip_serializing_if="Option::is_none")] message:Option<Text<2048>>,#[serde(default,skip_serializing_if="Option::is_none")] exception:Option<Text<2048>>});
contract!(InspectorDetail{kind:Id,observed_at:SafeInt,text:Text<2048>,#[serde(default,skip_serializing_if="Option::is_none")] duration_ms:Option<SafeInt>,truncated:bool,#[serde(default,skip_serializing_if="Option::is_none")] repeated_count:Option<SafeInt>,#[serde(default,skip_serializing_if="Option::is_none")] sql_ms:Option<SafeInt>,#[serde(default,skip_serializing_if="Option::is_none")] peak_memory_bytes:Option<SafeInt>,#[serde(default,skip_serializing_if="Option::is_none")] capture_truncated:Option<bool>});
contract!(InspectorItem{context_id:Id,method:Text<16>,status:Option<u16>,queries:SafeInt,elapsed_ms:SafeInt,#[serde(default,skip_serializing_if="Option::is_none")] detail:Option<InspectorDetail>});
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "stream", rename_all = "snake_case", deny_unknown_fields)]
pub enum ActivityEvent {
    Inspector { data: InspectorItem },
    Logs { context_id: Id, data: LogItem },
    Mail { data: MailItem },
}
impl Wire for ActivityEvent {
    fn schema() -> Value {
        json!({"oneOf":[
            {"type":"object","additionalProperties":false,"required":["stream","data"],"properties":{"stream":{"const":"inspector"},"data":InspectorItem::schema()}},
            {"type":"object","additionalProperties":false,"required":["stream","context_id","data"],"properties":{"stream":{"const":"logs"},"context_id":Id::schema(),"data":LogItem::schema()}},
            {"type":"object","additionalProperties":false,"required":["stream","data"],"properties":{"stream":{"const":"mail"},"data":MailItem::schema()}}
        ]})
    }
    fn ts() -> String {
        "ActivityEvent".into()
    }
    fn declaration() -> Option<String> {
        Some("export type ActivityEvent = { stream: \"inspector\"; data: InspectorItem } | { stream: \"logs\"; context_id: string; data: LogItem } | { stream: \"mail\"; data: MailItem };\n".into())
    }
}
/// Polling cannot prove that no event was inserted and removed between reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityContinuity {
    Unverified,
}
impl Wire for ActivityContinuity {
    fn schema() -> Value {
        json!({"type":"string","enum":["unverified"]})
    }
    fn ts() -> String {
        "\"unverified\"".into()
    }
}
contract!(ActivityWaitOutput{bookmark:Id,event:Option<ActivityEvent>,timed_out:bool,continuity:ActivityContinuity});

// Debugger data is split by information scope in the manager. Live values may
// contain private application data and must never appear in Debug output.
contract!(DebuggerInspectInput {
    detail: bool,
    locals: bool
});
contract!(DebuggerProfileInput { profile_id: Id });
private_contract!(DebuggerFrame { depth: SafeInt, function: Text<512>, file: Option<Text<512>>, line: SafeInt });
private_contract!(DebuggerLocal { name: Text<512>, kind: Text<512>, value: Text<512> });
contract!(DebuggerSession { session_id: Id, revision: SafeInt, state: Text<64>, depth: SafeInt, frame_count: SafeInt, variable_count: SafeInt, frames: Vec<DebuggerFrame>, locals: Vec<DebuggerLocal> });
contract!(DebuggerProfileMetadata {
    id: Id,
    created_ms: SafeInt,
    bytes: SafeInt,
    oversized: bool
});
contract!(DebuggerInspectOutput { project_revision: SafeInt, mode: Text<32>, session: Option<DebuggerSession>, profiles: Vec<DebuggerProfileMetadata>, profile_count: SafeInt });
private_contract!(DebuggerProfileFunction { file: Text<512>, name: Text<512>, self_ms: f64, inclusive_ms: f64, memory_bytes: i64, calls: SafeInt });
contract!(DebuggerProfileOutput { total_ms: f64, memory_bytes: i64, functions: Vec<DebuggerProfileFunction>, truncated: bool });

// This preview contains only fixed diagnostic codes; artifact handles are
// opaque, grant-bound and never represent filesystem paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SupportPreviewInput {}
impl Wire for SupportPreviewInput {
    fn schema() -> Value {
        json!({"type":"object","additionalProperties":false,"properties":{},"required":[]})
    }
    fn ts() -> String {
        "SupportPreviewInput".into()
    }
    fn declaration() -> Option<String> {
        Some("export interface SupportPreviewInput {}\n".into())
    }
}
// Only a project-relative path is accepted; host, credentials, query and
// transport are derived by the manager rather than supplied by the client.
contract!(SiteProbeInput { path: Text<1024> });
contract!(SiteProbeOutput {
    status: u16,
    elapsed_ms: SafeInt,
    context_id: Id,
    tls: bool,
    redirect: bool
});
contract!(SupportReadInput { artifact_id: Id });
contract!(SupportDoctor {
    code: Id,
    origin: Origin,
    observed_at: SafeInt,
    revision: SafeInt
});
contract!(SupportRunbook{evidence:Id,impact:Id,next_action:Option<Id>});
contract!(SupportBundlePreview{schema_version:u16,doctor:SupportDoctor,runbook:SupportRunbook,omitted:Vec<Id>});
contract!(SupportPreviewOutput {
    artifact_id: Id,
    preview: SupportBundlePreview,
    expires_at: SafeInt
});

contract!(DebuggerItem{installed:bool,mode:Text<32>,primitive_assignment_supported:bool});
private_contract!(MailAttachmentMetadata{name:Text<256>,mime:Text<128>,size:SafeInt});
private_contract!(MailContentDetail{subject:Text<512>,from:Text<512>,to:Text<1024>,text:Text<8192>,links:Vec<Text<2048>>,attachment_metadata:Vec<MailAttachmentMetadata>,truncated:bool});
contract!(MailItem {
    mail_id: Id,
    received_at: SafeInt,
    attachments: SafeInt,
    #[serde(default,skip_serializing_if="Option::is_none")]
    detail:Option<MailContentDetail>
});
contract!(ExecutionItem{operation_id:Id,state:OperationState,exit_code:Option<i32>});
contract!(HttpItem{operation_id:Id,status:Option<u16>,tls_verified:Option<bool>});
contract!(WorkspaceItem{workspace_id:Id,project_id:Id,state:Text<32>});
contract!(OperationItem {
    operation_id: Id,
    state: OperationState,
    cancel_supported: bool
});
contract!(ArtifactItem{artifact_id:Id,kind:Text<64>,bytes:SafeInt});
macro_rules! output {
    ($name:ident,$item:ty)=>{contract!($name{freshness:ProjectionFreshness,items:Vec<$item>,next_cursor:Option<Cursor>});};
}
output!(HealthOutput, HealthItem);
output!(ProjectOutput, ProjectItem);
output!(RuntimeOutput, RuntimeItem);
output!(ServiceOutput, ServiceItem);
output!(EnvironmentOutput, EnvironmentItem);
output!(DomainOutput, DomainItem);
output!(DatabaseOutput, DatabaseItem);
output!(LogOutput, LogItem);
output!(InspectorOutput, InspectorItem);
output!(DebuggerOutput, DebuggerItem);
output!(MailOutput, MailItem);
output!(ExecutionOutput, ExecutionItem);
output!(HttpOutput, HttpItem);
output!(WorkspaceOutput, WorkspaceItem);
output!(OperationOutput, OperationItem);
output!(ArtifactOutput, ArtifactItem);

pub struct TypedSpec {
    pub name: &'static str,
    pub input_schema: fn() -> Value,
    pub output_schema: fn() -> Value,
    pub input_ts: &'static str,
    pub output_ts: &'static str,
}
macro_rules! tool_set {
    ($($variant:ident($name:literal,$input:ident,$output:ident)),* $(,)?)=>{
        #[derive(Debug,Clone,Serialize,Deserialize)]
        #[serde(tag="tool",content="arguments",deny_unknown_fields)]
        pub enum ToolInput {$(#[serde(rename=$name)] $variant($input)),*}
        #[derive(Debug,Clone,Serialize,Deserialize)]
        #[serde(tag="tool",content="data",deny_unknown_fields)]
        pub enum ToolOutput {$(#[serde(rename=$name)] $variant($output)),*}
        impl ToolInput {
            pub fn tool(&self)->&'static str {match self {$(Self::$variant(_)=>$name),*}}
            pub fn page(&self)->PageRequest {match self {$(Self::$variant(p)=>p.page.clone().unwrap_or_default()),*}}
        }
        impl ToolOutput {
            pub fn tool(&self)->&'static str {match self {$(Self::$variant(_)=>$name),*}}
            pub fn rows(&self)->usize {match self {$(Self::$variant(p)=>p.items.len()),*}}
            pub fn next_cursor(&self)->Option<&Cursor>{match self {$(Self::$variant(p)=>p.next_cursor.as_ref()),*}}
        }
        pub fn typed_specs()->Vec<TypedSpec>{vec![$(TypedSpec{name:$name,input_schema:$input::schema,output_schema:$output::schema,input_ts:stringify!($input),output_ts:stringify!($output)}),*]}
    }
}
tool_set! {
    Health("health.health",HealthInput,HealthOutput),
    Project("projects.get",ProjectInput,ProjectOutput),
    Runtime("runtimes.catalog",RuntimeInput,RuntimeOutput),
    Service("services.status",ServiceInput,ServiceOutput),
    Environment("environment.masked_get",EnvironmentInput,EnvironmentOutput),
    Domain("domains.url",DomainInput,DomainOutput),
    Database("database.metadata",DatabaseInput,DatabaseOutput),
    Log("logs.search",LogInput,LogOutput),
    Inspector("inspector.request",InspectorInput,InspectorOutput),
    Debugger("debugger.capability",DebuggerInput,DebuggerOutput),
    Mail("mail.list",MailInput,MailOutput),
    Execution("execution.exit_code",ExecutionInput,ExecutionOutput),
    Http("http.readiness_tls_response_projection",HttpInput,HttpOutput),
    Workspace("workspaces.list",WorkspaceInput,WorkspaceOutput),
    Operation("operations.status",OperationInput,OperationOutput),
    Artifact("artifacts.scoped_artifact_metadata",ArtifactInput,ArtifactOutput),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemaStatus {
    TypedProjection,
    TypedMutation,
    TypedPlan,
    Planned,
    Unsupported,
}
#[derive(Debug, Clone, Serialize)]
pub struct ToolContract {
    pub name: String,
    pub schema_status: SchemaStatus,
    pub live_available: bool,
    pub policy: Policy,
    pub required_scopes: Vec<Scope>,
    pub effects: Vec<String>,
    pub review_required: bool,
    pub unavailable_error: ErrorCode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
}
pub fn contracts() -> Result<Vec<ToolContract>> {
    let typed = typed_specs();
    let mutations = crate::mutations::typed_specs();
    let mut result = vec![];
    for family in inventory::entries()? {
        let family_name =
            serde_json::to_value(family.family).map_err(|_| ErrorCode::InvalidInput)?;
        let family_name = family_name.as_str().ok_or(ErrorCode::InvalidInput)?;
        for op in family.operations {
            let name = format!("{family_name}.{}", op.name);
            let read_spec = typed.iter().find(|spec| spec.name == name);
            let mutation_spec = mutations.iter().find(|spec| spec.name == name);
            let spec = read_spec.or(mutation_spec);
            let schema_status = if op.core_status == CoreStatus::Unsupported {
                SchemaStatus::Unsupported
            } else if read_spec.is_some() {
                SchemaStatus::TypedProjection
            } else if mutation_spec.is_some() {
                if name == "environment.plan" {
                    SchemaStatus::TypedPlan
                } else {
                    SchemaStatus::TypedMutation
                }
            } else {
                SchemaStatus::Planned
            };
            result.push(ToolContract {
                name,
                schema_status,
                live_available: false,
                policy: op.policy,
                required_scopes: op.required_scopes,
                effects: op.effects,
                review_required: op.review_required,
                unavailable_error: ErrorCode::UnsupportedCapability,
                input_schema: spec.map(|s| (s.input_schema)()),
                output_schema: spec.map(|s| (s.output_schema)()),
            });
        }
    }
    Ok(result)
}

/// Looked up by trusted manager/store code, not deserialized from request input.
pub struct OperationAccess<'a> {
    pub operation_id: &'a Id,
    pub owner_client: &'a Id,
    pub project_id: &'a Id,
    pub original_scopes: &'a [Scope],
}
/// Prepared metadata only; dispatch still fails until an actual handler is installed.
#[derive(Debug)]
pub struct PreparedRead {
    request: Request<ToolInput>,
    budget: ReadBudget,
}
impl PreparedRead {
    pub fn request(&self) -> &Request<ToolInput> {
        &self.request
    }
    pub fn check_deadline(&self) -> Result<()> {
        self.budget.check_deadline()
    }
    pub fn require_live_handler(&self) -> Result<()> {
        Err(ErrorCode::UnsupportedCapability)
    }
    pub fn check_output(
        &self,
        output: &ToolOutput,
        binding: &ProjectBinding,
        now: u64,
    ) -> Result<()> {
        self.request.validate(binding, false)?;
        if output.tool() != self.request.input.tool() {
            return Err(ErrorCode::InvalidInput);
        }
        if let Some(cursor) = output.next_cursor() {
            cursor.validate(binding, output.tool(), now)?;
        }
        self.budget.check_payload(output.rows(), output)?;
        let matches_resource = match (&self.request.input, output) {
            (ToolInput::Project(_), ToolOutput::Project(result)) => result
                .items
                .iter()
                .all(|item| &item.project_id == binding.project_id()),
            (ToolInput::Workspace(_), ToolOutput::Workspace(result)) => result
                .items
                .iter()
                .all(|item| &item.project_id == binding.project_id()),
            (ToolInput::Execution(input), ToolOutput::Execution(result)) => result
                .items
                .iter()
                .all(|item| item.operation_id == input.operation_id),
            (ToolInput::Http(input), ToolOutput::Http(result)) => result
                .items
                .iter()
                .all(|item| item.operation_id == input.operation_id),
            (ToolInput::Operation(input), ToolOutput::Operation(result)) => result
                .items
                .iter()
                .all(|item| item.operation_id == input.operation_id),
            (ToolInput::Artifact(input), ToolOutput::Artifact(result)) => result
                .items
                .iter()
                .all(|item| item.artifact_id == input.artifact_id),
            (ToolInput::Mail(input), ToolOutput::Mail(result)) => result.items.iter().all(|item| {
                input.mail_id.as_ref().is_none_or(|id| id == &item.mail_id)
                    && (input.detail == Some(true) || item.detail.is_none())
            }),
            _ => true,
        };
        if !matches_resource {
            return Err(ErrorCode::ProjectMismatch);
        }
        Ok(())
    }
}
/// The authenticated client, registry binding, grant and operation access are
/// trusted server inputs. Parsing user JSON does not authenticate any of them.
/// `now` and all wire timestamps/expiries are Unix milliseconds.
pub fn prepare_read(
    bytes: &[u8],
    binding: &ProjectBinding,
    grant: &Grant,
    client: &Id,
    now: u64,
    operation: Option<OperationAccess<'_>>,
) -> Result<PreparedRead> {
    if bytes.len() > 65536 {
        return Err(ErrorCode::InvalidInput);
    }
    // Distinguish unsupported wire version from malformed payload without leaking raw errors.
    let raw: Value = serde_json::from_slice(bytes).map_err(|_| ErrorCode::InvalidInput)?;
    if raw
        .get("api_version")
        .and_then(Value::as_u64)
        .is_some_and(|v| v != API_VERSION as u64)
    {
        return Err(ErrorCode::UnsupportedVersion);
    }
    let envelope: Request<Value> =
        serde_json::from_value(raw.clone()).map_err(|_| ErrorCode::InvalidInput)?;
    envelope.validate(binding, false)?;
    if envelope.expected_revision.is_some_and(|v| v > SafeInt::MAX) {
        return Err(ErrorCode::InvalidInput);
    }
    let name = envelope
        .input
        .get("tool")
        .and_then(Value::as_str)
        .ok_or(ErrorCode::InvalidInput)?;
    let contract = contracts()?
        .into_iter()
        .find(|c| c.name == name)
        .ok_or(ErrorCode::UnsupportedCapability)?;
    if contract.schema_status != SchemaStatus::TypedProjection {
        return Err(ErrorCode::UnsupportedCapability);
    }
    let request: Request<ToolInput> =
        serde_json::from_value(raw).map_err(|_| ErrorCode::InvalidInput)?;
    for scope in &contract.required_scopes {
        grant.authorize(client, binding, *scope, now)?;
    }
    if contract.policy == Policy::OperationOwnerAndOriginalGrant {
        let access = operation.ok_or(ErrorCode::Unauthorized)?;
        let target = match &request.input {
            ToolInput::Operation(input) => &input.operation_id,
            ToolInput::Execution(input) => &input.operation_id,
            ToolInput::Http(input) => &input.operation_id,
            _ => return Err(ErrorCode::Unauthorized),
        };
        if target != access.operation_id
            || client != access.owner_client
            || binding.project_id() != access.project_id
            || access.original_scopes.is_empty()
        {
            return Err(ErrorCode::Unauthorized);
        }
        for scope in access.original_scopes {
            grant.authorize(client, binding, *scope, now)?;
        }
    }
    let page = request.input.page();
    if let Some(cursor) = &page.cursor {
        cursor.validate(binding, request.input.tool(), now)?;
    }
    Ok(PreparedRead {
        request,
        budget: ReadBudget::start(page),
    })
}

pub fn declarations() -> Vec<String> {
    macro_rules! collect {($($t:ty),*)=>{vec![$(<$t as Wire>::declaration().unwrap()),*]};}
    collect!(
        ProjectionFreshness,
        ProjectionWindow,
        HealthInput,
        ProjectInput,
        RuntimeInput,
        ServiceInput,
        EnvironmentInput,
        DomainInput,
        DatabaseInput,
        LogInput,
        InspectorInput,
        ActivityMarkInput,
        ActivityMarkOutput,
        ActivityWaitInput,
        ActivityEvent,
        ActivityWaitOutput,
        DebuggerInspectInput,
        DebuggerProfileInput,
        DebuggerFrame,
        DebuggerLocal,
        DebuggerSession,
        DebuggerProfileMetadata,
        DebuggerInspectOutput,
        DebuggerProfileFunction,
        DebuggerProfileOutput,
        SiteProbeInput,
        SiteProbeOutput,
        SupportPreviewInput,
        SupportReadInput,
        SupportDoctor,
        SupportRunbook,
        SupportBundlePreview,
        SupportPreviewOutput,
        DebuggerInput,
        MailInput,
        ExecutionInput,
        HttpInput,
        WorkspaceInput,
        WorkspaceListInput,
        WorkspaceListItem,
        WorkspaceListOutput,
        WorkspacePrepareInput,
        WorkspaceMergePreview,
        WorkspaceCreatePreview,
        WorkspacePrepareOutput,
        OperationInput,
        ArtifactInput,
        HealthItem,
        ProjectItem,
        RuntimeItem,
        ServiceItem,
        EnvironmentItem,
        DomainItem,
        DatabaseItem,
        LogItem,
        InspectorDetail,
        InspectorItem,
        DebuggerItem,
        MailAttachmentMetadata,
        MailContentDetail,
        MailItem,
        ExecutionItem,
        HttpItem,
        WorkspaceItem,
        OperationItem,
        ArtifactItem,
        HealthOutput,
        ProjectOutput,
        RuntimeOutput,
        ServiceOutput,
        EnvironmentOutput,
        DomainOutput,
        DatabaseOutput,
        LogOutput,
        InspectorOutput,
        DebuggerOutput,
        MailOutput,
        ExecutionOutput,
        HttpOutput,
        WorkspaceOutput,
        OperationOutput,
        ArtifactOutput
    )
}
