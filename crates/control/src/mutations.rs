//! Initial command payloads and trusted preparation; no backend is executed.
use crate::{
    export::Wire,
    limits::*,
    tools::{SchemaStatus, TypedSpec, contract},
    *,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct DomainName(String);
impl DomainName {
    pub fn new(value: String) -> Result<Self> {
        if value.len() > 230
            || !value.ends_with(".devforge.test")
            || value.split('.').any(|label| {
                label.is_empty()
                    || label.len() > 63
                    || label.starts_with('-')
                    || label.ends_with('-')
                    || !label
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            })
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for DomainName {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::new(String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
impl Wire for DomainName {
    fn schema() -> Value {
        json!({"type":"string","maxLength":230,"pattern":"^(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\\.)+devforge\\.test$"})
    }
    fn ts() -> String {
        "string".into()
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct YamlText(String);
impl YamlText {
    pub fn new(value: String) -> Result<Self> {
        if value.chars().count() > 65536
            || value
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for YamlText {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::new(String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
impl Wire for YamlText {
    fn schema() -> Value {
        json!({"type":"string","maxLength":65536,"pattern":"^[^\\u0000-\\u0008\\u000b\\u000c\\u000e-\\u001f\\u007f-\\u009f]*$"})
    }
    fn ts() -> String {
        "string".into()
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct ProjectName(Text<200>);
impl ProjectName {
    pub fn new(value: String) -> Result<Self> {
        if value.trim().is_empty() || value.len() > 200 {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self(Text::new(value)?))
    }
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}
impl<'de> Deserialize<'de> for ProjectName {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::new(String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
impl Wire for ProjectName {
    fn schema() -> Value {
        let mut schema = Text::<200>::schema();
        schema["minLength"] = json!(1);
        schema["x-maxUtf8Bytes"] = json!(200);
        schema["description"] = json!(
            "Server additionally rejects trim-empty names and names above 200 UTF-8 bytes; maxLength alone counts Unicode scalar values."
        );
        schema
    }
    fn ts() -> String {
        "string".into()
    }
}
contract!(MutationLimits{timeout_ms:Bounded<1800000>,max_output_bytes:Bounded<262144>});
impl Default for MutationLimits {
    fn default() -> Self {
        Self {
            timeout_ms: Bounded::new(300000).expect("valid default"),
            max_output_bytes: Bounded::new(65536).expect("valid default"),
        }
    }
}
contract!(ServiceActionInput{limits:Option<MutationLimits>});
contract!(PackageInput{package_id:Id,limits:Option<MutationLimits>});
contract!(ConfigureInput{name:ProjectName,php:Id,node:Id,domain:DomainName,https:bool,vite:bool,queue:bool,limits:Option<MutationLimits>});
contract!(EnvironmentPlanInput{write_env:bool,limits:Option<MutationLimits>});
contract!(EnvironmentApplyInput{write_env:bool,limits:Option<MutationLimits>});
contract!(YamlSaveInput{expected:Option<Digest>,text:YamlText,limits:Option<MutationLimits>});
contract!(WorkspaceLifecycleInput{workspace_id:Id,limits:Option<MutationLimits>});
contract!(DatabaseBindInput{instance_id:Id,limits:Option<MutationLimits>});
contract!(DatabaseRestoreInput{backup_id:Id,limits:Option<MutationLimits>});
contract!(MutationReceipt {
    project_id: Id,
    operation_id: Id,
    state: OperationState,
    revision: SafeInt
});
contract!(PlanReceipt {
    project_id: Id,
    revision: SafeInt,
    digest: Digest,
    request_fingerprint: Digest,
    expires_at: SafeInt,
    blocked: bool
});

macro_rules! mutation_set {
    ($($variant:ident($tool:literal,$input:ident,$output:ident,$command:literal)),* $(,)?)=>{
        #[derive(Debug,Clone,Serialize,Deserialize)]
        #[serde(tag="tool",content="arguments",deny_unknown_fields)]
        pub enum MutationInput{$(#[serde(rename=$tool)]$variant($input)),*}
        #[derive(Debug,Clone,Serialize,Deserialize)]
        #[serde(tag="tool",content="data",deny_unknown_fields)]
        pub enum MutationOutput{$(#[serde(rename=$tool)]$variant($output)),*}
        impl MutationInput {
            pub fn tool(&self)->&'static str{match self{$(Self::$variant(_)=>$tool),*}}
            pub fn workspace_command(&self)->&'static str{match self{$(Self::$variant(_)=>$command),*}}
            pub fn limits(&self)->MutationLimits{match self{$(Self::$variant(p)=>p.limits.clone().unwrap_or_default()),*}}
            pub fn mutates(&self)->bool{!matches!(self,Self::EnvironmentPlan(_))}
        }
        impl MutationOutput {
            pub fn tool(&self)->&'static str{match self{$(Self::$variant(_)=>$tool),*}}
            pub fn project_id(&self)->&Id{match self{$(Self::$variant(value)=>&value.project_id),*}}
        }
        pub fn typed_specs()->Vec<TypedSpec>{vec![$(TypedSpec{name:$tool,input_schema:$input::schema,output_schema:$output::schema,input_ts:stringify!($input),output_ts:stringify!($output)}),*]}
    }
}
mutation_set! {
    Start("services.start",ServiceActionInput,MutationReceipt,"start"),
    Stop("services.stop",ServiceActionInput,MutationReceipt,"stop"),
    Restart("services.restart",ServiceActionInput,MutationReceipt,"restart"),
    RuntimeInstall("runtimes.install",PackageInput,MutationReceipt,"install"),
    RuntimeRemove("runtimes.remove",PackageInput,MutationReceipt,"remove_runtime"),
    Configure("runtimes.select",ConfigureInput,MutationReceipt,"configure"),
    EnvironmentPlan("environment.plan",EnvironmentPlanInput,PlanReceipt,"configuration_plan"),
    EnvironmentApply("environment.apply",EnvironmentApplyInput,MutationReceipt,"configuration_apply"),
    YamlSave("environment.yaml_save",YamlSaveInput,MutationReceipt,"configuration_save"),
    DatabaseBind("database.bind",DatabaseBindInput,MutationReceipt,"switch_database"),
    DatabaseRestore("database.restore",DatabaseRestoreInput,MutationReceipt,"restore_backup"),
    WorkspaceArchive("workspaces.archive",WorkspaceLifecycleInput,MutationReceipt,"worktree_cleanup"),
    WorkspaceRestore("workspaces.restore",WorkspaceLifecycleInput,MutationReceipt,"worktree_archive.restore"),
    WorkspacePurge("workspaces.purge",WorkspaceLifecycleInput,MutationReceipt,"worktree_archive.purge"),
}

/// Trusted project consent record. Registry/consent code issues this, never a client boolean.
#[derive(Debug, Clone)]
pub struct ProjectTrust {
    project_id: Id,
    canonical_root: PathBuf,
}
impl ProjectTrust {
    pub fn record(binding: &ProjectBinding) -> Self {
        Self {
            project_id: binding.project_id().clone(),
            canonical_root: binding.root().into(),
        }
    }
    fn verify(&self, binding: &ProjectBinding) -> Result<()> {
        if &self.project_id != binding.project_id() || self.canonical_root != binding.root() {
            return Err(ErrorCode::Unauthorized);
        }
        Ok(())
    }
}
/// Server-loaded consent and freshly computed backend state digest. The request
/// fingerprint was persisted when the user reviewed the exact arguments.
pub struct ReviewApproval<'a> {
    pub plan: &'a ReviewedPlan,
    pub current_backend_digest: &'a Digest,
    pub request_fingerprint: &'a Digest,
}
pub struct MutationContext<'a> {
    pub binding: &'a ProjectBinding,
    pub grant: &'a Grant,
    pub client: &'a Id,
    pub now: u64,
    pub trust: Option<&'a ProjectTrust>,
    pub review: Option<ReviewApproval<'a>>,
}
#[derive(Debug, Clone)]
pub struct MutationDescriptor {
    pub action: Id,
    pub workspace_command: &'static str,
    pub mutates: bool,
    pub required_scopes: Vec<Scope>,
    pub effects: Vec<String>,
    pub review_required: bool,
}
#[derive(Debug)]
pub struct PreparedMutation {
    request: Request<MutationInput>,
    binding: ProjectBinding,
    descriptor: MutationDescriptor,
    request_digest: Digest,
    approved_review: Option<Digest>,
    limits: MutationLimits,
}
impl PreparedMutation {
    pub fn request(&self) -> &Request<MutationInput> {
        &self.request
    }
    pub fn binding(&self) -> &ProjectBinding {
        &self.binding
    }
    pub fn descriptor(&self) -> &MutationDescriptor {
        &self.descriptor
    }
    pub fn request_digest(&self) -> &Digest {
        &self.request_digest
    }
    pub fn approved_review(&self) -> Option<&Digest> {
        self.approved_review.as_ref()
    }
    pub fn limits(&self) -> &MutationLimits {
        &self.limits
    }
    pub fn require_live_handler(&self) -> Result<()> {
        Err(ErrorCode::UnsupportedCapability)
    }
    pub fn check_output(&self, output: &MutationOutput) -> Result<()> {
        if output.tool() != self.request.input.tool() {
            return Err(ErrorCode::InvalidInput);
        }
        if output.project_id() != self.binding.project_id() {
            return Err(ErrorCode::ProjectMismatch);
        }
        let receipt_budget = ReadBudget::start(PageRequest {
            limit: Bounded::new(1)?,
            max_bytes: Bounded::new(self.limits.max_output_bytes.get())?,
            timeout_ms: Bounded::new(30000)?,
            cursor: None,
        });
        receipt_budget.check_payload(1, output)
    }
}
/// Canonical fingerprint material excludes request_id/idempotency_key so a retry
/// may receive a new request ID without becoming a different operation. It binds
/// canonical project root, revision, tool and all typed arguments, including limits.
/// The trusted caller supplies a real SHA-256 provider; client-supplied hashes are not accepted.
pub fn fingerprint_material(
    request: &Request<MutationInput>,
    binding: &ProjectBinding,
) -> Result<Vec<u8>> {
    request.validate(binding, request.input.mutates())?;
    let canonical_root = binding.root().to_str().ok_or(ErrorCode::InvalidInput)?;
    serde_json::to_vec(&json!({"api_version":API_VERSION,"project_id":binding.project_id(),"canonical_root":canonical_root,"revision":binding.revision(),"input":request.input}))
        .map_err(|_|ErrorCode::InvalidInput)
}
/// Unix-ms clock; preparation validates policy only. Persistence, atomic duplicate
/// lookup, final resource/revision recheck and deadline enforcement belong to manager/adapter.
pub fn prepare_mutation(
    bytes: &[u8],
    context: MutationContext<'_>,
    sha256: impl FnOnce(&[u8]) -> Result<Digest>,
) -> Result<PreparedMutation> {
    if bytes.len() > 65536 {
        return Err(ErrorCode::InvalidInput);
    }
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
    if envelope.expected_revision.is_some_and(|v| v > SafeInt::MAX) {
        return Err(ErrorCode::InvalidInput);
    }
    envelope.validate(context.binding, false)?;
    let name = envelope
        .input
        .get("tool")
        .and_then(Value::as_str)
        .ok_or(ErrorCode::InvalidInput)?;
    let contract = crate::tools::contracts()?
        .into_iter()
        .find(|c| c.name == name)
        .ok_or(ErrorCode::UnsupportedCapability)?;
    if !matches!(
        contract.schema_status,
        SchemaStatus::TypedMutation | SchemaStatus::TypedPlan
    ) {
        return Err(ErrorCode::UnsupportedCapability);
    }
    let request: Request<MutationInput> =
        serde_json::from_value(raw).map_err(|_| ErrorCode::InvalidInput)?;
    request.validate(context.binding, request.input.mutates())?;
    for scope in &contract.required_scopes {
        context
            .grant
            .authorize(context.client, context.binding, *scope, context.now)?;
    }
    if contract.required_scopes.contains(&Scope::ExecutionTest) {
        context
            .trust
            .ok_or(ErrorCode::Unauthorized)?
            .verify(context.binding)?;
    }
    let request_digest = sha256(&fingerprint_material(&request, context.binding)?)?;
    let action = Id::new(request.input.tool())?;
    let approved_review = if contract.review_required {
        let approval = context.review.ok_or(ErrorCode::Unauthorized)?;
        if &request_digest != approval.request_fingerprint {
            return Err(ErrorCode::ReviewMismatch);
        }
        approval.plan.verify(
            context.binding,
            &action,
            approval.current_backend_digest,
            context.now,
        )?;
        Some(approval.current_backend_digest.clone())
    } else {
        None
    };
    let descriptor = MutationDescriptor {
        action,
        workspace_command: request.input.workspace_command(),
        mutates: request.input.mutates(),
        required_scopes: contract.required_scopes,
        effects: contract.effects,
        review_required: contract.review_required,
    };
    let limits = request.input.limits();
    Ok(PreparedMutation {
        request,
        binding: context.binding.clone(),
        descriptor,
        request_digest,
        approved_review,
        limits,
    })
}
pub fn declarations() -> Vec<String> {
    macro_rules! collect {($($t:ty),*)=>{vec![$(<$t as Wire>::declaration().unwrap()),*]};}
    collect!(
        MutationLimits,
        ServiceActionInput,
        PackageInput,
        ConfigureInput,
        EnvironmentPlanInput,
        EnvironmentApplyInput,
        YamlSaveInput,
        WorkspaceLifecycleInput,
        DatabaseBindInput,
        DatabaseRestoreInput,
        MutationReceipt,
        PlanReceipt
    )
}

#[cfg(all(test, unix))]
mod path_tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;
    #[test]
    fn non_utf8_registered_root_returns_error_instead_of_panicking() {
        // APFS rejects creating malformed UTF-8 directory names. Exercise the
        // persisted binding representation directly; other Unix filesystems
        // can supply this metadata through an actual canonical registry path.
        let binding = ProjectBinding {
            project_id: Id::new("project").unwrap(),
            canonical_root: PathBuf::from(std::ffi::OsString::from_vec(vec![b'/', b'p', 0xff])),
            revision: 1,
        };
        let request:Request<MutationInput>=serde_json::from_value(json!({"api_version":1,"request_id":"r1","project_id":"project","expected_revision":1,"idempotency_key":"k1","input":{"tool":"services.stop","arguments":{}}})).unwrap();
        assert_eq!(
            fingerprint_material(&request, &binding),
            Err(ErrorCode::InvalidInput)
        );
    }
}
