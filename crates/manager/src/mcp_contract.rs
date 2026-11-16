//! Narrow dual-era MCP contract: modern requests are independently versioned;
//! only legacy initialize/initialized state is remembered. No I/O or authority
//! is minted here. The caller dispatches only the returned read-only input.
use crate::external_control::{ExternalInput, ExternalOutput, ExternalReply};
use devforge_control::{
    Digest, ErrorCode, Id, Request, Response,
    mutations::{MutationInput, MutationLimits, PackageInput, ServiceActionInput},
};
use serde::Deserialize;
use serde_json::{Value, json};
pub const MODERN_VERSION: &str = "2026-07-28";
pub const LEGACY_VERSION: &str = "2025-11-25";
pub const MAX_REQUEST_BYTES: usize = 65_536;
pub const MAX_RESPONSE_BYTES: usize = 262_144;
const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";
const TOOLS: [&str; 5] = [
    "health.health",
    "services.status",
    "projects.list",
    "projects.get",
    "operations.status",
];
#[derive(Clone, Copy)]
enum Version {
    Modern,
    Legacy,
}
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Legacy {
    #[default]
    New,
    Initialized,
    Ready,
}
pub struct Protocol {
    project: Id,
    legacy: Legacy,
    counter: u64,
    service_actions: bool,
    runtime_actions: bool,
    environment_actions: bool,
    database_actions: bool,
    domain_actions: bool,
    execution_actions: bool,
    support_export_actions: bool,
    site_probe_actions: bool,
    debugger_actions: bool,
}
pub enum Action {
    Reply(Value),
    Ignore,
    Cancel(Value),
    Call(ToolCall),
    Service(ServiceCall),
    Runtime(RuntimeCall),
    Environment(EnvCall),
    Database(DatabaseCall),
    Domain(DomainCall),
    Diagnostics(DiagnosticsCall),
    Activity(ActivityCall),
    Execution(ExecutionCall),
}
pub enum ExecutionInput {
    Plan(Request<MutationInput>),
    Apply {
        request: Request<MutationInput>,
        review: Digest,
    },
    Lookup {
        idempotency_key: Id,
    },
    Output {
        operation_id: Id,
    },
    Cancel {
        operation_id: Id,
    },
    DebuggerPlan(Request<MutationInput>),
    DebuggerApply {
        request: Request<MutationInput>,
        review: Digest,
    },
    DebuggerLookup {
        idempotency_key: Id,
    },
    SupportPlan(Request<MutationInput>),
    SupportApply {
        request: Request<MutationInput>,
        review: Digest,
    },
    SupportLookup {
        idempotency_key: Id,
    },
}
pub struct ExecutionCall {
    pub input: ExecutionInput,
    pub request_id: Id,
    pub project_id: Id,
    id: Value,
    version: Version,
}
impl ExecutionCall {
    pub fn jsonrpc_id(&self) -> &Value {
        &self.id
    }
}
pub struct DiagnosticsCall {
    pub request: Request<devforge_control::tools::ToolInput>,
    id: Value,
    version: Version,
}
impl DiagnosticsCall {
    pub fn jsonrpc_id(&self) -> &Value {
        &self.id
    }
}
pub enum ActivityInput {
    Mark(Request<devforge_control::tools::ActivityMarkInput>),
    Wait(Request<devforge_control::tools::ActivityWaitInput>),
    SiteProbe(Request<devforge_control::tools::SiteProbeInput>),
    DebuggerInspect(Request<devforge_control::tools::DebuggerInspectInput>),
    DebuggerProfile(Request<devforge_control::tools::DebuggerProfileInput>),
    SupportPreview(Request<devforge_control::tools::SupportPreviewInput>),
    SupportRead(Request<devforge_control::tools::SupportReadInput>),
}
pub struct ActivityCall {
    pub input: ActivityInput,
    pub request_id: Id,
    id: Value,
    version: Version,
}
impl ActivityCall {
    pub fn jsonrpc_id(&self) -> &Value {
        &self.id
    }
}
#[derive(Clone)]
pub enum DomainInput {
    Facts(crate::service_control::DomainReadKind),
    Prepare(crate::service_control::DomainPlanKind),
    Plan(Request<MutationInput>),
    Apply {
        request: Request<MutationInput>,
        review: Digest,
    },
    Lookup {
        idempotency_key: Id,
    },
}
pub struct DomainCall {
    pub input: DomainInput,
    pub request_id: Id,
    pub project_id: Id,
    id: Value,
    version: Version,
}
impl DomainCall {
    pub fn jsonrpc_id(&self) -> &Value {
        &self.id
    }
}
#[derive(Clone)]
pub enum DatabaseInput {
    Choices,
    Plan(Request<MutationInput>),
    Apply {
        request: Request<MutationInput>,
        review: Digest,
    },
    Lookup {
        idempotency_key: Id,
    },
}
pub struct DatabaseCall {
    pub input: DatabaseInput,
    pub request_id: Id,
    pub project_id: Id,
    id: Value,
    version: Version,
}
impl DatabaseCall {
    pub fn jsonrpc_id(&self) -> &Value {
        &self.id
    }
}
#[derive(Clone)]
pub enum EnvInput {
    Plan(Request<MutationInput>),
    Apply {
        request: Request<MutationInput>,
        review: Digest,
    },
    Lookup {
        idempotency_key: Id,
    },
}
pub struct EnvCall {
    pub input: EnvInput,
    pub request_id: Id,
    pub project_id: Id,
    id: Value,
    version: Version,
}
impl EnvCall {
    pub fn jsonrpc_id(&self) -> &Value {
        &self.id
    }
}
#[derive(Clone)]
pub enum RuntimeInput {
    Install {
        request: Request<MutationInput>,
        review: Option<Digest>,
    },
    Select {
        expected_revision: u64,
        idempotency_key: Id,
        php: Id,
        node: Id,
        limits: Option<MutationLimits>,
        review: Option<Digest>,
    },
    Lookup {
        idempotency_key: Id,
    },
}
pub struct RuntimeCall {
    pub input: RuntimeInput,
    pub request_id: Id,
    pub project_id: Id,
    id: Value,
    version: Version,
}
impl RuntimeCall {
    pub fn jsonrpc_id(&self) -> &Value {
        &self.id
    }
}
#[derive(Clone)]
pub enum ServiceInput {
    Plan(Request<MutationInput>),
    Apply {
        request: Request<MutationInput>,
        review: Digest,
    },
    Lookup {
        project_id: Id,
        idempotency_key: Id,
    },
}
pub struct ServiceCall {
    pub input: ServiceInput,
    pub request_id: Id,
    id: Value,
    version: Version,
}
impl ServiceCall {
    pub fn jsonrpc_id(&self) -> &Value {
        &self.id
    }
}
enum Kind {
    Tool,
    ListResources,
    ReadResource(String),
}
pub struct ToolCall {
    pub input: ExternalInput,
    pub request_id: Id,
    id: Value,
    version: Version,
    kind: Kind,
    project: Id,
}
impl ToolCall {
    pub fn jsonrpc_id(&self) -> &Value {
        &self.id
    }
}
impl Protocol {
    pub fn new(project_id: Id) -> Self {
        Self {
            project: project_id,
            legacy: Legacy::New,
            counter: 0,
            service_actions: false,
            runtime_actions: false,
            environment_actions: false,
            database_actions: false,
            domain_actions: false,
            execution_actions: false,
            support_export_actions: false,
            site_probe_actions: false,
            debugger_actions: false,
        }
    }
    /// Surface selection only; grants and native exact-plan approval remain
    /// mandatory at the manager. The default constructor stays read-only.
    pub fn with_service_actions(project_id: Id) -> Self {
        let mut protocol = Self::new(project_id);
        protocol.service_actions = true;
        protocol
    }
    pub fn with_action_surfaces(
        project_id: Id,
        service_actions: bool,
        runtime_actions: bool,
    ) -> Self {
        let mut protocol = Self::new(project_id);
        protocol.service_actions = service_actions;
        protocol.runtime_actions = runtime_actions;
        protocol
    }
    pub fn with_environment_surfaces(
        project_id: Id,
        service: bool,
        runtime: bool,
        environment: bool,
    ) -> Self {
        let mut protocol = Self::with_action_surfaces(project_id, service, runtime);
        protocol.environment_actions = environment;
        protocol
    }
    pub fn with_database_surfaces(
        project: Id,
        service: bool,
        runtime: bool,
        environment: bool,
        database: bool,
    ) -> Self {
        let mut protocol = Self::with_environment_surfaces(project, service, runtime, environment);
        protocol.database_actions = database;
        protocol
    }
    pub fn with_domain_surfaces(
        project: Id,
        service: bool,
        runtime: bool,
        environment: bool,
        database: bool,
        domain: bool,
    ) -> Self {
        let mut protocol =
            Self::with_database_surfaces(project, service, runtime, environment, database);
        protocol.domain_actions = domain;
        protocol
    }
    /// Surface selection is not permission or project trust.
    pub fn with_execution_actions(mut self, enabled: bool) -> Self {
        self.execution_actions = enabled;
        self
    }
    pub fn with_debugger_actions(mut self, enabled: bool) -> Self {
        self.debugger_actions = enabled;
        self
    }
    pub fn with_site_probe_actions(mut self, enabled: bool) -> Self {
        self.site_probe_actions = enabled;
        self
    }
    pub fn with_support_export_actions(mut self, enabled: bool) -> Self {
        self.support_export_actions = enabled;
        self
    }
    pub fn receive(&mut self, value: Value) -> Action {
        let Some(object) = value.as_object() else {
            return Action::Reply(rpc_error(None, -32600, "Invalid request", None));
        };
        let id = value.get("id").filter(|id| valid_id(id)).cloned();
        if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
            || object
                .keys()
                .any(|key| !["jsonrpc", "id", "method", "params"].contains(&key.as_str()))
            || value.get("id").is_some() && id.is_none()
        {
            return Action::Reply(rpc_error(id, -32600, "Invalid request", None));
        }
        let Some(method) = value.get("method").and_then(Value::as_str) else {
            return Action::Reply(rpc_error(id, -32600, "Invalid request", None));
        };
        let mut params = match value.get("params") {
            None => json!({}),
            Some(p) if p.is_object() => p.clone(),
            _ => {
                return if id.is_none() {
                    Action::Ignore
                } else {
                    Action::Reply(rpc_error(id, -32602, "Invalid params", None))
                };
            }
        };
        if id.is_none() {
            if method == "notifications/initialized"
                && self.legacy == Legacy::Initialized
                && keys(&params, &[])
            {
                self.legacy = Legacy::Ready;
            }
            if method == "notifications/cancelled"
                && let Some(cancelled) = params.get("requestId").filter(|id| valid_id(id))
            {
                return Action::Cancel(cancelled.clone());
            }
            return Action::Ignore;
        }
        let id = id.expect("checked id");
        if method == "initialize" {
            if self.legacy != Legacy::New
                || params
                    .get("_meta")
                    .and_then(|m| m.get(META_VERSION))
                    .is_some()
            {
                return Action::Reply(rpc_error(Some(id), -32600, "Invalid initialization", None));
            }
            if !keys(
                &params,
                &["protocolVersion", "capabilities", "clientInfo", "_meta"],
            ) || !params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .is_some_and(|v| v.len() <= 64)
                || !params.get("capabilities").is_some_and(Value::is_object)
                || !params.get("clientInfo").is_some_and(implementation)
            {
                return Action::Reply(rpc_error(Some(id), -32602, "Invalid params", None));
            }
            self.legacy = Legacy::Initialized;
            return Action::Reply(rpc_success(
                id,
                Version::Legacy,
                json!({"protocolVersion":LEGACY_VERSION,"capabilities":capabilities(),"serverInfo":server_info()}),
            ));
        }
        let version = if let Some(meta) = params.get("_meta") {
            if !meta.is_object() {
                return Action::Reply(rpc_error(
                    Some(id),
                    -32602,
                    "Invalid protocol metadata",
                    None,
                ));
            }
            if let Some(version) = meta.get(META_VERSION) {
                let Some(requested) = version.as_str() else {
                    return Action::Reply(rpc_error(
                        Some(id),
                        -32602,
                        "Invalid protocol metadata",
                        None,
                    ));
                };
                if requested != MODERN_VERSION {
                    return Action::Reply(rpc_error(
                        Some(id),
                        -32022,
                        "Unsupported protocol version",
                        Some(
                            json!({"supported":[MODERN_VERSION,LEGACY_VERSION],"requested":if requested.len()<=64{requested}else{"invalid"}}),
                        ),
                    ));
                }
                if !meta.get(META_CAPABILITIES).is_some_and(Value::is_object)
                    || meta
                        .get("io.modelcontextprotocol/clientInfo")
                        .is_some_and(|v| !implementation(v))
                {
                    return Action::Reply(rpc_error(
                        Some(id),
                        -32602,
                        "Invalid protocol metadata",
                        None,
                    ));
                }
                Version::Modern
            } else if self.legacy == Legacy::Ready {
                Version::Legacy
            } else {
                return Action::Reply(rpc_error(
                    Some(id),
                    -32602,
                    "Missing protocol metadata",
                    None,
                ));
            }
        } else if self.legacy == Legacy::Ready {
            Version::Legacy
        } else {
            return Action::Reply(rpc_error(
                Some(id),
                -32602,
                "Missing protocol metadata",
                None,
            ));
        };
        params.as_object_mut().expect("object").remove("_meta");
        if method == "tools/call"
            && self.service_actions
            && let Some(service) = self.service_tool(&params, id.clone(), version)
        {
            return match service {
                Ok(call) => Action::Service(call),
                Err(()) => Action::Reply(rpc_error(Some(id), -32602, "Invalid params", None)),
            };
        }
        if method == "tools/call"
            && self.debugger_actions
            && let Some(debugger) = self.debugger_tool(&params, id.clone(), version)
        {
            return match debugger {
                Ok(call) => Action::Execution(call),
                Err(()) => Action::Reply(rpc_error(Some(id), -32602, "Invalid params", None)),
            };
        }
        if method == "tools/call"
            && self.support_export_actions
            && let Some(support) = self.support_export_tool(&params, id.clone(), version)
        {
            return match support {
                Ok(call) => Action::Execution(call),
                Err(()) => Action::Reply(rpc_error(Some(id), -32602, "Invalid params", None)),
            };
        }
        if method == "tools/call"
            && self.execution_actions
            && let Some(execution) = self.execution_tool(&params, id.clone(), version)
        {
            return match execution {
                Ok(call) => Action::Execution(call),
                Err(()) => Action::Reply(rpc_error(Some(id), -32602, "Invalid params", None)),
            };
        }
        if method == "tools/call"
            && self.runtime_actions
            && let Some(runtime) = self.runtime_tool(&params, id.clone(), version)
        {
            return match runtime {
                Ok(call) => Action::Runtime(call),
                Err(()) => Action::Reply(rpc_error(Some(id), -32602, "Invalid params", None)),
            };
        }
        if method == "tools/call"
            && self.environment_actions
            && let Some(environment) = self.env_tool(&params, id.clone(), version)
        {
            return match environment {
                Ok(call) => Action::Environment(call),
                Err(()) => Action::Reply(rpc_error(Some(id), -32602, "Invalid params", None)),
            };
        }
        if method == "tools/call"
            && self.database_actions
            && let Some(database) = self.database_tool(&params, id.clone(), version)
        {
            return match database {
                Ok(call) => Action::Database(call),
                Err(()) => Action::Reply(rpc_error(Some(id), -32602, "Invalid params", None)),
            };
        }
        if method == "tools/call"
            && self.domain_actions
            && let Some(domain) = self.domain_tool(&params, id.clone(), version)
        {
            return match domain {
                Ok(call) => Action::Domain(call),
                Err(()) => Action::Reply(rpc_error(Some(id), -32602, "Invalid params", None)),
            };
        }
        if method == "tools/call"
            && let Some(activity) = self.activity_tool(&params, id.clone(), version)
        {
            return match activity {
                Ok(call) => Action::Activity(call),
                Err(()) => Action::Reply(rpc_error(Some(id), -32602, "Invalid params", None)),
            };
        }
        if method == "tools/call"
            && let Some(diagnostics) = self.diagnostics_tool(&params, id.clone(), version)
        {
            return match diagnostics {
                Ok(call) => Action::Diagnostics(call),
                Err(()) => Action::Reply(rpc_error(Some(id), -32602, "Invalid params", None)),
            };
        }
        let result = match method {
            "server/discover" if matches!(version, Version::Modern) => {
                if keys(&params, &[]) {
                    return Action::Reply(rpc_success(
                        id,
                        version,
                        json!({"supportedVersions":[MODERN_VERSION,LEGACY_VERSION],"capabilities":capabilities()}),
                    ));
                } else {
                    Err(())
                }
            }
            "ping" => {
                if keys(&params, &[]) {
                    return Action::Reply(rpc_success(id, version, json!({})));
                } else {
                    Err(())
                }
            }
            "tools/list" => {
                if keys(&params, &[]) {
                    return Action::Reply(rpc_success(
                        id,
                        version,
                        json!({"tools":tool_definitions(self.service_actions,self.runtime_actions,self.environment_actions,self.database_actions,self.domain_actions,self.execution_actions,(self.support_export_actions,self.site_probe_actions,self.debugger_actions))}),
                    ));
                } else {
                    Err(())
                }
            }
            "resources/list" => {
                if keys(&params, &[]) {
                    Ok((
                        ExternalInput::ProjectGet {
                            project_id: self.project.clone(),
                        },
                        Kind::ListResources,
                    ))
                } else {
                    Err(())
                }
            }
            "resources/read" => {
                if keys(&params, &["uri"])
                    && params.get("uri").and_then(Value::as_str)
                        == Some(summary_uri(&self.project).as_str())
                {
                    Ok((
                        ExternalInput::ProjectGet {
                            project_id: self.project.clone(),
                        },
                        Kind::ReadResource(summary_uri(&self.project)),
                    ))
                } else {
                    Err(())
                }
            }
            "tools/call" => self.tool(&params).map(|input| (input, Kind::Tool)),
            _ => return Action::Reply(rpc_error(Some(id), -32601, "Method not found", None)),
        };
        match result {
            Ok((input, kind)) => {
                self.counter = self.counter.wrapping_add(1);
                Action::Call(ToolCall {
                    input,
                    request_id: Id::new(format!("mcp-{}", self.counter)).expect("fixed id"),
                    id,
                    version,
                    kind,
                    project: self.project.clone(),
                })
            }
            Err(()) => Action::Reply(rpc_error(Some(id), -32602, "Invalid params", None)),
        }
    }
    fn activity_tool(
        &mut self,
        params: &Value,
        id: Value,
        version: Version,
    ) -> Option<Result<ActivityCall, ()>> {
        let name = params.get("name").and_then(Value::as_str)?;
        if !matches!(
            name,
            "activity.mark"
                | "activity.wait"
                | "support.preview"
                | "support.read"
                | "site.probe"
                | "debugger.inspect"
                | "debugger.profile"
        ) {
            return None;
        }
        if name.starts_with("debugger.") && !self.debugger_actions {
            return None;
        }
        if name == "site.probe" && !self.site_probe_actions {
            return None;
        }
        if !keys(params, &["name", "arguments"]) {
            return Some(Err(()));
        }
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let allowed: &[&str] = if name == "debugger.inspect" {
            &["detail", "locals"]
        } else if name == "debugger.profile" {
            &["profile_id"]
        } else if name == "site.probe" {
            &["path"]
        } else if name == "support.preview" {
            &[]
        } else if name == "support.read" {
            &["artifact_id"]
        } else if name == "activity.mark" {
            &[
                "stream",
                "detail",
                "context_id",
                "subject",
                "recipient",
                "sender",
                "since",
            ]
        } else {
            &["bookmark", "timeout_ms", "context_id"]
        };
        if !keys(&arguments, allowed)
            || (name == "site.probe"
                && arguments
                    .get("path")
                    .and_then(Value::as_str)
                    .is_none_or(|path| {
                        path.is_empty()
                            || path.len() > 1024
                            || !path.starts_with('/')
                            || path.starts_with("//")
                            || path
                                .bytes()
                                .any(|b| b <= 0x20 || b >= 0x7f || matches!(b, b'?' | b'#' | b'\\'))
                    }))
            || ["subject", "recipient", "sender", "since"]
                .iter()
                .any(|k| arguments.get(k).is_some_and(Value::is_null))
            || (name == "activity.mark"
                && arguments.get("stream").and_then(Value::as_str) == Some("mail")
                && arguments.get("context_id").is_some())
            || (name == "activity.mark"
                && arguments.get("stream").and_then(Value::as_str) != Some("mail")
                && ["subject", "recipient", "sender", "since"]
                    .iter()
                    .any(|k| arguments.get(k).is_some()))
            || arguments.get("context_id").is_some_and(|value| {
                value.as_str().is_none_or(|id| {
                    id.len() != 32 || !id.bytes().all(|byte| byte.is_ascii_hexdigit())
                })
            })
        {
            return Some(Err(()));
        }
        self.counter = self.counter.wrapping_add(1);
        let request_id = Id::new(format!("mcp-{}", self.counter)).expect("fixed request ID");
        let envelope = |input| Request {
            api_version: Default::default(),
            request_id: request_id.clone(),
            project_id: self.project.clone(),
            expected_revision: None,
            idempotency_key: None,
            input,
        };
        let input = if name == "debugger.inspect" {
            match serde_json::from_value::<devforge_control::tools::DebuggerInspectInput>(arguments)
            {
                Ok(input) => ActivityInput::DebuggerInspect(Request {
                    api_version: Default::default(),
                    request_id: request_id.clone(),
                    project_id: self.project.clone(),
                    expected_revision: None,
                    idempotency_key: None,
                    input,
                }),
                Err(_) => return Some(Err(())),
            }
        } else if name == "debugger.profile" {
            match serde_json::from_value::<devforge_control::tools::DebuggerProfileInput>(arguments)
            {
                Ok(input) => ActivityInput::DebuggerProfile(Request {
                    api_version: Default::default(),
                    request_id: request_id.clone(),
                    project_id: self.project.clone(),
                    expected_revision: None,
                    idempotency_key: None,
                    input,
                }),
                Err(_) => return Some(Err(())),
            }
        } else if name == "site.probe" {
            match serde_json::from_value::<devforge_control::tools::SiteProbeInput>(arguments) {
                Ok(input) => ActivityInput::SiteProbe(Request {
                    api_version: Default::default(),
                    request_id: request_id.clone(),
                    project_id: self.project.clone(),
                    expected_revision: None,
                    idempotency_key: None,
                    input,
                }),
                Err(_) => return Some(Err(())),
            }
        } else if name == "support.preview" {
            match serde_json::from_value::<devforge_control::tools::SupportPreviewInput>(arguments)
            {
                Ok(input) => ActivityInput::SupportPreview(Request {
                    api_version: Default::default(),
                    request_id: request_id.clone(),
                    project_id: self.project.clone(),
                    expected_revision: None,
                    idempotency_key: None,
                    input,
                }),
                Err(_) => return Some(Err(())),
            }
        } else if name == "support.read" {
            match serde_json::from_value::<devforge_control::tools::SupportReadInput>(arguments) {
                Ok(input) => ActivityInput::SupportRead(Request {
                    api_version: Default::default(),
                    request_id: request_id.clone(),
                    project_id: self.project.clone(),
                    expected_revision: None,
                    idempotency_key: None,
                    input,
                }),
                Err(_) => return Some(Err(())),
            }
        } else if name == "activity.mark" {
            match serde_json::from_value::<devforge_control::tools::ActivityMarkInput>(arguments) {
                Ok(input) => ActivityInput::Mark(envelope(input)),
                Err(_) => return Some(Err(())),
            }
        } else {
            match serde_json::from_value::<devforge_control::tools::ActivityWaitInput>(arguments) {
                Ok(input) => ActivityInput::Wait(Request {
                    api_version: Default::default(),
                    request_id: request_id.clone(),
                    project_id: self.project.clone(),
                    expected_revision: None,
                    idempotency_key: None,
                    input,
                }),
                Err(_) => return Some(Err(())),
            }
        };
        Some(Ok(ActivityCall {
            input,
            request_id,
            id,
            version,
        }))
    }
    pub fn complete_activity(call: ActivityCall, result: Result<Value, ErrorCode>) -> Value {
        let content = result
            .and_then(|value| {
                if value.get("api_version") != Some(&json!(1))
                    || value.get("request_id") != Some(&json!(call.request_id))
                    || value.get("ok").and_then(Value::as_bool).is_none_or(|ok| {
                        ok != value.get("data").is_some() || ok == value.get("error").is_some()
                    })
                {
                    Err(ErrorCode::InvalidInput)
                } else {
                    Ok(value)
                }
            })
            .unwrap_or_else(|code| {
                serde_json::to_value(Response::<Value>::failure(
                    call.request_id,
                    None,
                    code.fault(),
                ))
                .expect("fixed error")
            });
        let is_error = content.get("ok") != Some(&Value::Bool(true));
        rpc_success(
            call.id,
            call.version,
            json!({"content":[{"type":"text","text":content.to_string()}],"structuredContent":content,"isError":is_error}),
        )
    }
    fn diagnostics_tool(
        &mut self,
        params: &Value,
        id: Value,
        version: Version,
    ) -> Option<Result<DiagnosticsCall, ()>> {
        let name = params.get("name").and_then(Value::as_str)?;
        if !matches!(
            name,
            "health.doctor" | "logs.search" | "inspector.request" | "mail.list" | "mail.show"
        ) {
            return None;
        }
        if !keys(params, &["name", "arguments"]) {
            return Some(Err(()));
        }
        let mut arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let allowed: &[&str] = match name {
            "logs.search" => &["page", "level", "query", "content", "since"],
            "inspector.request" => &["page", "context_id", "detail"],
            "mail.list" => &[
                "page",
                "subject",
                "recipient",
                "sender",
                "since",
                "detail",
                "mail_id",
            ],
            "mail.show" => &["mail_id", "page"],
            _ => &["page"],
        };
        // Explicit null is not a content request; reject ambiguous wire input.
        if !keys(&arguments, allowed)
            || (name == "mail.show" && arguments.get("page").is_some_and(Value::is_null))
            || [
                "query",
                "content",
                "since",
                "detail",
                "subject",
                "recipient",
                "sender",
                "mail_id",
            ]
            .iter()
            .any(|key| arguments.get(key).is_some_and(Value::is_null))
        {
            return Some(Err(()));
        }
        if name == "mail.show" {
            if arguments.get("mail_id").and_then(Value::as_str).is_none() {
                return Some(Err(()));
            }
            arguments["detail"] = json!(true);
        }
        let tool = match name {
            "health.doctor" => "health.health",
            "mail.show" => "mail.list",
            _ => name,
        };
        let input = match serde_json::from_value::<devforge_control::tools::ToolInput>(
            json!({"tool":tool,"arguments":arguments}),
        ) {
            Ok(input) => input,
            Err(_) => return Some(Err(())),
        };
        self.counter = self.counter.wrapping_add(1);
        let request_id = Id::new(format!("mcp-{}", self.counter)).expect("fixed request ID");
        Some(Ok(DiagnosticsCall {
            request: Request {
                api_version: Default::default(),
                request_id,
                project_id: self.project.clone(),
                expected_revision: None,
                idempotency_key: None,
                input,
            },
            id,
            version,
        }))
    }
    pub fn complete_diagnostics(call: DiagnosticsCall, result: Result<Value, ErrorCode>) -> Value {
        let content = result
            .and_then(|value| {
                if value.get("api_version") != Some(&json!(1))
                    || value.get("request_id") != Some(&json!(call.request.request_id))
                    || value.get("ok").and_then(Value::as_bool).is_none_or(|ok| {
                        ok != value.get("data").is_some() || ok == value.get("error").is_some()
                    })
                {
                    Err(ErrorCode::InvalidInput)
                } else {
                    Ok(value)
                }
            })
            .unwrap_or_else(|code| {
                serde_json::to_value(Response::<Value>::failure(
                    call.request.request_id,
                    None,
                    code.fault(),
                ))
                .expect("fixed error envelope")
            });
        let is_error = content.get("ok") != Some(&Value::Bool(true));
        rpc_success(
            call.id,
            call.version,
            json!({"content":[{"type":"text","text":content.to_string()}],"structuredContent":content,"isError":is_error}),
        )
    }
    fn domain_tool(
        &mut self,
        params: &Value,
        id: Value,
        version: Version,
    ) -> Option<Result<DomainCall, ()>> {
        use crate::service_control::{DomainPlanKind, DomainReadKind};
        let name = params.get("name").and_then(Value::as_str)?;
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let lookup = name == "operations.status" && args.get("idempotency_key").is_some();
        if !lookup
            && !matches!(
                name,
                "domains.url"
                    | "domains.ports"
                    | "domains.certificate_status"
                    | "domains.hosts_plan"
                    | "domains.trust_plan"
                    | "domains.apply"
                    | "domains.cleanup"
            )
        {
            return None;
        }
        if !keys(params, &["name", "arguments"]) {
            return Some(Err(()));
        }
        self.counter = self.counter.wrapping_add(1);
        let request_id = Id::new(format!("mcp-{}", self.counter)).expect("fixed request ID");
        let result = (|| {
            if lookup {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Lookup {
                    idempotency_key: Id,
                }
                let input: Lookup = serde_json::from_value(args).map_err(|_| ())?;
                return Ok(DomainInput::Lookup {
                    idempotency_key: input.idempotency_key,
                });
            }
            if !matches!(name, "domains.apply" | "domains.cleanup") {
                if !keys(&args, &[]) {
                    return Err(());
                }
                return Ok(match name {
                    "domains.url" => DomainInput::Facts(DomainReadKind::Url),
                    "domains.ports" => DomainInput::Facts(DomainReadKind::Ports),
                    "domains.certificate_status" => {
                        DomainInput::Facts(DomainReadKind::CertificateStatus)
                    }
                    "domains.hosts_plan" => DomainInput::Prepare(DomainPlanKind::Hosts),
                    "domains.trust_plan" => DomainInput::Prepare(DomainPlanKind::Trust),
                    _ => return Err(()),
                });
            }
            let mut args = args.as_object().cloned().ok_or(())?;
            let action = match args.remove("action") {
                None => {
                    if name == "domains.cleanup" {
                        "prepare".to_owned()
                    } else {
                        "plan".to_owned()
                    }
                }
                Some(Value::String(action)) => action,
                _ => return Err(()),
            };
            if name == "domains.cleanup" && action == "prepare" {
                return if args.is_empty() {
                    Ok(DomainInput::Prepare(DomainPlanKind::Cleanup))
                } else {
                    Err(())
                };
            }
            if !matches!(action.as_str(), "plan" | "apply") {
                return Err(());
            }
            let expected_revision =
                serde_json::from_value::<u64>(args.remove("expected_revision").ok_or(())?)
                    .map_err(|_| ())?;
            if expected_revision > devforge_control::limits::SafeInt::MAX {
                return Err(());
            }
            let idempotency_key =
                serde_json::from_value::<Id>(args.remove("idempotency_key").ok_or(())?)
                    .map_err(|_| ())?;
            let review_value = args.remove("review");
            let applying = action == "apply";
            if applying != review_value.is_some() {
                return Err(());
            }
            let review = review_value
                .map(serde_json::from_value::<Digest>)
                .transpose()
                .map_err(|_| ())?;
            let input: MutationInput =
                serde_json::from_value(json!({"tool":name,"arguments":args})).map_err(|_| ())?;
            let request = Request {
                api_version: Default::default(),
                request_id: request_id.clone(),
                project_id: self.project.clone(),
                expected_revision: Some(expected_revision),
                idempotency_key: Some(idempotency_key),
                input,
            };
            Ok(match review {
                None => DomainInput::Plan(request),
                Some(review) => DomainInput::Apply { request, review },
            })
        })();
        Some(result.map(|input| DomainCall {
            input,
            request_id,
            project_id: self.project.clone(),
            id,
            version,
        }))
    }
    pub fn complete_domain(call: DomainCall, result: Result<Value, ErrorCode>) -> Value {
        let invalid = if matches!(call.input, DomainInput::Apply { .. }) {
            ErrorCode::AdmissionUnconfirmed
        } else {
            ErrorCode::InvalidInput
        };
        let content = result
            .and_then(|value| {
                if value.get("api_version") != Some(&json!(1))
                    || value.get("request_id") != Some(&json!(call.request_id))
                    || value.get("ok").and_then(Value::as_bool).is_none_or(|ok| {
                        ok != value.get("data").is_some() || ok == value.get("error").is_some()
                    })
                {
                    Err(invalid)
                } else {
                    Ok(value)
                }
            })
            .unwrap_or_else(|code| {
                serde_json::to_value(Response::<Value>::failure(
                    call.request_id,
                    None,
                    code.fault(),
                ))
                .expect("fixed error envelope")
            });
        let is_error = content.get("ok") != Some(&Value::Bool(true));
        rpc_success(
            call.id,
            call.version,
            json!({"content":[{"type":"text","text":content.to_string()}],"structuredContent":content,"isError":is_error}),
        )
    }
    fn database_tool(
        &mut self,
        params: &Value,
        id: Value,
        version: Version,
    ) -> Option<Result<DatabaseCall, ()>> {
        let name = params.get("name").and_then(Value::as_str)?;
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let lookup = name == "operations.status" && args.get("idempotency_key").is_some();
        if !lookup && !matches!(name, "database.plan" | "database.bind" | "database.restore") {
            return None;
        }
        if !keys(params, &["name", "arguments"]) {
            return Some(Err(()));
        }
        self.counter = self.counter.wrapping_add(1);
        let request_id = Id::new(format!("mcp-{}", self.counter)).expect("fixed request ID");
        let result = (|| {
            if lookup {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Lookup {
                    idempotency_key: Id,
                }
                let input: Lookup = serde_json::from_value(args).map_err(|_| ())?;
                return Ok(DatabaseInput::Lookup {
                    idempotency_key: input.idempotency_key,
                });
            }
            let mut args = args.as_object().cloned().ok_or(())?;
            let planning = name == "database.plan";
            let action = if planning {
                match args.remove("action") {
                    None => "choices".to_owned(),
                    Some(Value::String(action)) => action,
                    _ => return Err(()),
                }
            } else {
                name.strip_prefix("database.").ok_or(())?.to_owned()
            };
            if action == "choices" && planning {
                return if args.is_empty() {
                    Ok(DatabaseInput::Choices)
                } else {
                    Err(())
                };
            }
            let tool = match action.as_str() {
                "bind" => "database.bind",
                "restore" => "database.restore",
                _ => return Err(()),
            };
            let expected_revision =
                serde_json::from_value::<u64>(args.remove("expected_revision").ok_or(())?)
                    .map_err(|_| ())?;
            if expected_revision > devforge_control::limits::SafeInt::MAX {
                return Err(());
            }
            let idempotency_key =
                serde_json::from_value::<Id>(args.remove("idempotency_key").ok_or(())?)
                    .map_err(|_| ())?;
            let review_value = args.remove("review");
            if planning && review_value.is_some() || !planning && review_value.is_none() {
                return Err(());
            }
            let review = review_value
                .map(serde_json::from_value::<Digest>)
                .transpose()
                .map_err(|_| ())?;
            let input: MutationInput =
                serde_json::from_value(json!({"tool":tool,"arguments":args})).map_err(|_| ())?;
            let request = Request {
                api_version: Default::default(),
                request_id: request_id.clone(),
                project_id: self.project.clone(),
                expected_revision: Some(expected_revision),
                idempotency_key: Some(idempotency_key),
                input,
            };
            Ok(match review {
                None => DatabaseInput::Plan(request),
                Some(review) => DatabaseInput::Apply { request, review },
            })
        })();
        Some(result.map(|input| DatabaseCall {
            input,
            request_id,
            project_id: self.project.clone(),
            id,
            version,
        }))
    }
    pub fn complete_database(call: DatabaseCall, result: Result<Value, ErrorCode>) -> Value {
        let invalid = if matches!(call.input, DatabaseInput::Apply { .. }) {
            ErrorCode::AdmissionUnconfirmed
        } else {
            ErrorCode::InvalidInput
        };
        let content = result
            .and_then(|value| {
                if value.get("api_version") != Some(&json!(1))
                    || value.get("request_id") != Some(&json!(call.request_id))
                    || value.get("ok").and_then(Value::as_bool).is_none_or(|ok| {
                        ok != value.get("data").is_some() || ok == value.get("error").is_some()
                    })
                {
                    Err(invalid)
                } else {
                    Ok(value)
                }
            })
            .unwrap_or_else(|code| {
                serde_json::to_value(Response::<Value>::failure(
                    call.request_id,
                    None,
                    code.fault(),
                ))
                .expect("fixed error envelope")
            });
        let is_error = content.get("ok") != Some(&Value::Bool(true));
        rpc_success(
            call.id,
            call.version,
            json!({"content":[{"type":"text","text":content.to_string()}],"structuredContent":content,"isError":is_error}),
        )
    }
    fn env_tool(
        &mut self,
        params: &Value,
        id: Value,
        version: Version,
    ) -> Option<Result<EnvCall, ()>> {
        let name = params.get("name").and_then(Value::as_str)?;
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let lookup = name == "operations.status" && args.get("idempotency_key").is_some();
        if !lookup
            && !matches!(
                name,
                "environment.plan" | "environment.apply" | "environment.yaml_save"
            )
        {
            return None;
        }
        if !keys(params, &["name", "arguments"]) {
            return Some(Err(()));
        }
        self.counter = self.counter.wrapping_add(1);
        let request_id = Id::new(format!("mcp-{}", self.counter)).expect("fixed request ID");
        let result = (|| {
            if lookup {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Lookup {
                    idempotency_key: Id,
                }
                let parsed: Lookup = serde_json::from_value(args).map_err(|_| ())?;
                return Ok(EnvInput::Lookup {
                    idempotency_key: parsed.idempotency_key,
                });
            }
            let mut args = args.as_object().cloned().ok_or(())?;
            let planning = name == "environment.plan";
            let action = if planning {
                match args.remove("action") {
                    None => "preview".to_owned(),
                    Some(Value::String(action)) => action,
                    _ => return Err(()),
                }
            } else {
                name.strip_prefix("environment.").ok_or(())?.to_owned()
            };
            let tool = match action.as_str() {
                "preview" if planning => "environment.plan",
                "apply" => "environment.apply",
                "yaml_save" => "environment.yaml_save",
                _ => return Err(()),
            };
            let expected_revision = match args.remove("expected_revision") {
                None => None,
                Some(value) => serde_json::from_value::<Option<u64>>(value).map_err(|_| ())?,
            };
            let idempotency_key = match args.remove("idempotency_key") {
                None => None,
                Some(value) => serde_json::from_value::<Option<Id>>(value).map_err(|_| ())?,
            };
            let review_value = args.remove("review");
            if planning && review_value.is_some() || !planning && review_value.is_none() {
                return Err(());
            }
            let review = review_value
                .map(serde_json::from_value::<Digest>)
                .transpose()
                .map_err(|_| ())?;
            if expected_revision
                .is_some_and(|revision| revision > devforge_control::limits::SafeInt::MAX)
            {
                return Err(());
            }
            if tool != "environment.plan"
                && (expected_revision.is_none() || idempotency_key.is_none())
            {
                return Err(());
            }
            if tool != "environment.yaml_save" && !args.contains_key("write_env") {
                args.insert("write_env".into(), json!(false));
            }
            let input: MutationInput =
                serde_json::from_value(json!({"tool":tool,"arguments":args})).map_err(|_| ())?;
            if matches!(&input,MutationInput::YamlSave(candidate) if candidate.text.as_str().len()>crate::service_control::MAX_EXTERNAL_YAML_BYTES)
            {
                return Err(());
            }
            let request = Request {
                api_version: Default::default(),
                request_id: request_id.clone(),
                project_id: self.project.clone(),
                expected_revision,
                idempotency_key,
                input,
            };
            Ok(match review {
                None => EnvInput::Plan(request),
                Some(review) => EnvInput::Apply { request, review },
            })
        })();
        Some(result.map(|input| EnvCall {
            input,
            request_id,
            project_id: self.project.clone(),
            id,
            version,
        }))
    }
    pub fn complete_environment(call: EnvCall, result: Result<Value, ErrorCode>) -> Value {
        let invalid = if matches!(call.input, EnvInput::Apply { .. }) {
            ErrorCode::AdmissionUnconfirmed
        } else {
            ErrorCode::InvalidInput
        };
        let content = result
            .and_then(|value| {
                if value.get("api_version") != Some(&json!(1))
                    || value.get("request_id") != Some(&json!(call.request_id))
                    || value.get("ok").and_then(Value::as_bool).is_none_or(|ok| {
                        ok != value.get("data").is_some() || ok == value.get("error").is_some()
                    })
                {
                    Err(invalid)
                } else {
                    Ok(value)
                }
            })
            .unwrap_or_else(|code| {
                serde_json::to_value(Response::<Value>::failure(
                    call.request_id,
                    None,
                    code.fault(),
                ))
                .expect("fixed error envelope")
            });
        let is_error = content.get("ok") != Some(&Value::Bool(true));
        rpc_success(
            call.id,
            call.version,
            json!({"content":[{"type":"text","text":content.to_string()}],"structuredContent":content,"isError":is_error}),
        )
    }
    fn debugger_tool(
        &mut self,
        params: &Value,
        id: Value,
        version: Version,
    ) -> Option<Result<ExecutionCall, ()>> {
        use devforge_control::mutations::{
            DebuggerAssignInput, DebuggerBreakpointsInput, DebuggerControlInput,
            DebuggerExportInput, DebuggerInstallInput, DebuggerModeInput, DebuggerStartInput,
        };
        let name = params.get("name").and_then(Value::as_str)?;
        if !matches!(
            name,
            "debugger.breakpoints"
                | "debugger.control"
                | "debugger.lookup"
                | "debugger.install"
                | "debugger.mode"
                | "debugger.start"
                | "debugger.primitive_assignment"
                | "debugger.export"
        ) {
            return None;
        }
        if !keys(params, &["name", "arguments"]) {
            return Some(Err(()));
        }
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        self.counter = self.counter.wrapping_add(1);
        let request_id = Id::new(format!("mcp-{}", self.counter)).expect("fixed request ID");
        let result = (|| {
            if name == "debugger.lookup" {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Lookup {
                    idempotency_key: Id,
                }
                let parsed: Lookup = serde_json::from_value(args).map_err(|_| ())?;
                return Ok(ExecutionInput::DebuggerLookup {
                    idempotency_key: parsed.idempotency_key,
                });
            }
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Args {
                action: String,
                expected_revision: u64,
                idempotency_key: Id,
                input: Value,
                review: Option<Digest>,
            }
            let parsed: Args = serde_json::from_value(args).map_err(|_| ())?;
            if parsed.expected_revision > devforge_control::limits::SafeInt::MAX
                || !matches!(parsed.action.as_str(), "plan" | "apply")
                || (parsed.action == "apply") != parsed.review.is_some()
            {
                return Err(());
            }
            let input = match name {
                "debugger.breakpoints" => MutationInput::DebuggerBreakpoints(
                    serde_json::from_value::<DebuggerBreakpointsInput>(parsed.input)
                        .map_err(|_| ())?,
                ),
                "debugger.control" => MutationInput::DebuggerControl(
                    serde_json::from_value::<DebuggerControlInput>(parsed.input).map_err(|_| ())?,
                ),
                "debugger.install" => MutationInput::DebuggerInstall(
                    serde_json::from_value::<DebuggerInstallInput>(parsed.input).map_err(|_| ())?,
                ),
                "debugger.mode" => MutationInput::DebuggerMode(
                    serde_json::from_value::<DebuggerModeInput>(parsed.input).map_err(|_| ())?,
                ),
                "debugger.start" => MutationInput::DebuggerStart(
                    serde_json::from_value::<DebuggerStartInput>(parsed.input).map_err(|_| ())?,
                ),
                "debugger.primitive_assignment" => MutationInput::DebuggerAssign(
                    serde_json::from_value::<DebuggerAssignInput>(parsed.input).map_err(|_| ())?,
                ),
                "debugger.export" => MutationInput::DebuggerExport(
                    serde_json::from_value::<DebuggerExportInput>(parsed.input).map_err(|_| ())?,
                ),
                _ => return Err(()),
            };
            let request = Request {
                api_version: Default::default(),
                request_id: request_id.clone(),
                project_id: self.project.clone(),
                expected_revision: Some(parsed.expected_revision),
                idempotency_key: Some(parsed.idempotency_key),
                input,
            };
            Ok(match parsed.review {
                Some(review) => ExecutionInput::DebuggerApply { request, review },
                None => ExecutionInput::DebuggerPlan(request),
            })
        })();
        Some(result.map(|input| ExecutionCall {
            input,
            request_id,
            project_id: self.project.clone(),
            id,
            version,
        }))
    }
    fn support_export_tool(
        &mut self,
        params: &Value,
        id: Value,
        version: Version,
    ) -> Option<Result<ExecutionCall, ()>> {
        let name = params.get("name").and_then(Value::as_str)?;
        if !matches!(name, "support.export" | "support.export_lookup") {
            return None;
        }
        if !keys(params, &["name", "arguments"]) {
            return Some(Err(()));
        }
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        self.counter = self.counter.wrapping_add(1);
        let request_id = Id::new(format!("mcp-{}", self.counter)).expect("fixed request ID");
        let result = (|| {
            if name == "support.export_lookup" {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Lookup {
                    idempotency_key: Id,
                }
                let parsed: Lookup = serde_json::from_value(args).map_err(|_| ())?;
                return Ok(ExecutionInput::SupportLookup {
                    idempotency_key: parsed.idempotency_key,
                });
            }
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Args {
                action: String,
                expected_revision: u64,
                idempotency_key: Id,
                artifact_id: Id,
                limits: Option<MutationLimits>,
                review: Option<Digest>,
            }
            let parsed: Args = serde_json::from_value(args).map_err(|_| ())?;
            if parsed.expected_revision > devforge_control::limits::SafeInt::MAX
                || !matches!(parsed.action.as_str(), "plan" | "apply")
                || (parsed.action == "apply") != parsed.review.is_some()
            {
                return Err(());
            }
            let input =
                MutationInput::SupportExport(devforge_control::mutations::SupportExportInput {
                    artifact_id: parsed.artifact_id,
                    limits: parsed.limits,
                });
            let request = Request {
                api_version: Default::default(),
                request_id: request_id.clone(),
                project_id: self.project.clone(),
                expected_revision: Some(parsed.expected_revision),
                idempotency_key: Some(parsed.idempotency_key),
                input,
            };
            Ok(match parsed.review {
                Some(review) => ExecutionInput::SupportApply { request, review },
                None => ExecutionInput::SupportPlan(request),
            })
        })();
        Some(result.map(|input| ExecutionCall {
            input,
            request_id,
            project_id: self.project.clone(),
            id,
            version,
        }))
    }
    fn execution_tool(
        &mut self,
        params: &Value,
        id: Value,
        version: Version,
    ) -> Option<Result<ExecutionCall, ()>> {
        use devforge_control::mutations::{ExecutionArguments, ExecutionTool};
        let name = params.get("name").and_then(Value::as_str)?;
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let lookup = name == "operations.status" && args.get("idempotency_key").is_some();
        if !lookup
            && !matches!(
                name,
                "execution.php"
                    | "execution.artisan"
                    | "execution.composer"
                    | "execution.node"
                    | "execution.npm"
                    | "execution.git"
                    | "execution.managed_test_job"
                    | "execution.output"
                    | "execution.cancel"
            )
        {
            return None;
        }
        if !keys(params, &["name", "arguments"]) {
            return Some(Err(()));
        }
        self.counter = self.counter.wrapping_add(1);
        let request_id = Id::new(format!("mcp-{}", self.counter)).expect("fixed request ID");
        let result = (|| {
            if lookup {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Lookup {
                    idempotency_key: Id,
                }
                let parsed: Lookup = serde_json::from_value(args).map_err(|_| ())?;
                return Ok(ExecutionInput::Lookup {
                    idempotency_key: parsed.idempotency_key,
                });
            }
            if matches!(name, "execution.output" | "execution.cancel") {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Output {
                    operation_id: Id,
                }
                let parsed: Output = serde_json::from_value(args).map_err(|_| ())?;
                return Ok(if name == "execution.cancel" {
                    ExecutionInput::Cancel {
                        operation_id: parsed.operation_id,
                    }
                } else {
                    ExecutionInput::Output {
                        operation_id: parsed.operation_id,
                    }
                });
            }
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Args {
                action: String,
                expected_revision: u64,
                idempotency_key: Id,
                args: ExecutionArguments,
                tool: Option<ExecutionTool>,
                limits: Option<MutationLimits>,
                review: Option<Digest>,
            }
            let parsed: Args = serde_json::from_value(args).map_err(|_| ())?;
            if parsed.expected_revision > devforge_control::limits::SafeInt::MAX
                || parsed
                    .limits
                    .as_ref()
                    .is_some_and(|limits| limits.timeout_ms.get() > 300_000)
                || !matches!(parsed.action.as_str(), "plan" | "apply")
                || (parsed.action == "apply") != parsed.review.is_some()
                || (name == "execution.managed_test_job") != parsed.tool.is_some()
            {
                return Err(());
            }
            let mut arguments = json!({"args":parsed.args,"limits":parsed.limits});
            if let Some(tool) = parsed.tool {
                arguments["tool"] = json!(tool);
            }
            let input = serde_json::from_value(json!({"tool":name,"arguments":arguments}))
                .map_err(|_| ())?;
            let request = Request {
                api_version: Default::default(),
                request_id: request_id.clone(),
                project_id: self.project.clone(),
                expected_revision: Some(parsed.expected_revision),
                idempotency_key: Some(parsed.idempotency_key),
                input,
            };
            Ok(match parsed.review {
                Some(review) => ExecutionInput::Apply { request, review },
                None => ExecutionInput::Plan(request),
            })
        })();
        Some(result.map(|input| ExecutionCall {
            input,
            request_id,
            project_id: self.project.clone(),
            id,
            version,
        }))
    }
    pub fn complete_execution(call: ExecutionCall, result: Result<Value, ErrorCode>) -> Value {
        let invalid = if matches!(
            call.input,
            ExecutionInput::Apply { .. }
                | ExecutionInput::SupportApply { .. }
                | ExecutionInput::DebuggerApply { .. }
        ) {
            ErrorCode::AdmissionUnconfirmed
        } else {
            ErrorCode::InvalidInput
        };
        let content = result
            .and_then(|value| {
                if value.get("api_version") != Some(&json!(1))
                    || value.get("request_id") != Some(&json!(call.request_id))
                    || value.get("ok").and_then(Value::as_bool).is_none_or(|ok| {
                        ok != value.get("data").is_some() || ok == value.get("error").is_some()
                    })
                {
                    Err(invalid)
                } else {
                    Ok(value)
                }
            })
            .unwrap_or_else(|code| {
                serde_json::to_value(Response::<Value>::failure(
                    call.request_id,
                    None,
                    code.fault(),
                ))
                .expect("fixed error envelope")
            });
        let is_error = content.get("ok") != Some(&Value::Bool(true));
        rpc_success(
            call.id,
            call.version,
            json!({"content":[{"type":"text","text":content.to_string()}],"structuredContent":content,"isError":is_error}),
        )
    }
    fn runtime_tool(
        &mut self,
        params: &Value,
        id: Value,
        version: Version,
    ) -> Option<Result<RuntimeCall, ()>> {
        let name = params.get("name").and_then(Value::as_str)?;
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let lookup = name == "operations.status" && args.get("idempotency_key").is_some();
        if !lookup
            && !matches!(
                name,
                "runtimes.plan" | "runtimes.install" | "runtimes.select"
            )
        {
            return None;
        }
        if !keys(params, &["name", "arguments"]) {
            return Some(Err(()));
        }
        self.counter = self.counter.wrapping_add(1);
        let request_id = Id::new(format!("mcp-{}", self.counter)).expect("fixed request ID");
        let result = (|| {
            if lookup {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Lookup {
                    idempotency_key: Id,
                }
                let parsed: Lookup = serde_json::from_value(args).map_err(|_| ())?;
                return Ok(RuntimeInput::Lookup {
                    idempotency_key: parsed.idempotency_key,
                });
            }
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Args {
                action: Option<String>,
                expected_revision: u64,
                idempotency_key: Id,
                package_id: Option<Id>,
                php: Option<Id>,
                node: Option<Id>,
                limits: Option<MutationLimits>,
                review: Option<Digest>,
            }
            let parsed: Args = serde_json::from_value(args).map_err(|_| ())?;
            let plan = name == "runtimes.plan";
            if parsed.expected_revision > devforge_control::limits::SafeInt::MAX
                || plan && parsed.review.is_some()
                || !plan && parsed.review.is_none()
                || !plan && parsed.action.is_some()
            {
                return Err(());
            }
            let action = if plan {
                parsed.action.as_deref().ok_or(())?
            } else {
                name.strip_prefix("runtimes.").ok_or(())?
            };
            match action {
                "install" if parsed.php.is_none() && parsed.node.is_none() => {
                    let input = MutationInput::RuntimeInstall(PackageInput {
                        package_id: parsed.package_id.ok_or(())?,
                        limits: parsed.limits,
                    });
                    Ok(RuntimeInput::Install {
                        request: Request {
                            api_version: Default::default(),
                            request_id: request_id.clone(),
                            project_id: self.project.clone(),
                            expected_revision: Some(parsed.expected_revision),
                            idempotency_key: Some(parsed.idempotency_key),
                            input,
                        },
                        review: parsed.review,
                    })
                }
                "select" if parsed.package_id.is_none() => Ok(RuntimeInput::Select {
                    expected_revision: parsed.expected_revision,
                    idempotency_key: parsed.idempotency_key,
                    php: parsed.php.ok_or(())?,
                    node: parsed.node.ok_or(())?,
                    limits: parsed.limits,
                    review: parsed.review,
                }),
                _ => Err(()),
            }
        })();
        Some(result.map(|input| RuntimeCall {
            input,
            request_id,
            project_id: self.project.clone(),
            id,
            version,
        }))
    }
    pub fn complete_runtime(call: RuntimeCall, result: Result<Value, ErrorCode>) -> Value {
        let admission = matches!(
            call.input,
            RuntimeInput::Install {
                review: Some(_),
                ..
            } | RuntimeInput::Select {
                review: Some(_),
                ..
            }
        );
        let invalid = if admission {
            ErrorCode::AdmissionUnconfirmed
        } else {
            ErrorCode::InvalidInput
        };
        let content = result
            .and_then(|value| {
                if value.get("api_version") != Some(&json!(1))
                    || value.get("request_id") != Some(&json!(call.request_id))
                    || value.get("ok").and_then(Value::as_bool).is_none_or(|ok| {
                        ok != value.get("data").is_some() || ok == value.get("error").is_some()
                    })
                {
                    Err(invalid)
                } else {
                    Ok(value)
                }
            })
            .unwrap_or_else(|code| {
                serde_json::to_value(Response::<Value>::failure(
                    call.request_id,
                    None,
                    code.fault(),
                ))
                .expect("fixed error envelope")
            });
        let is_error = content.get("ok") != Some(&Value::Bool(true));
        rpc_success(
            call.id,
            call.version,
            json!({"content":[{"type":"text","text":content.to_string()}],"structuredContent":content,"isError":is_error}),
        )
    }
    fn service_tool(
        &mut self,
        params: &Value,
        id: Value,
        version: Version,
    ) -> Option<Result<ServiceCall, ()>> {
        let name = params.get("name").and_then(Value::as_str)?;
        let args = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let lookup = name == "operations.status" && args.get("idempotency_key").is_some();
        if !lookup
            && !matches!(
                name,
                "services.plan" | "services.start" | "services.stop" | "services.restart"
            )
        {
            return None;
        }
        if !keys(params, &["name", "arguments"]) {
            return Some(Err(()));
        }
        self.counter = self.counter.wrapping_add(1);
        let request_id = Id::new(format!("mcp-{}", self.counter)).expect("fixed request ID");
        let input = (|| {
            if lookup {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Lookup {
                    idempotency_key: Id,
                }
                let parsed: Lookup = serde_json::from_value(args).map_err(|_| ())?;
                return Ok(ServiceInput::Lookup {
                    project_id: self.project.clone(),
                    idempotency_key: parsed.idempotency_key,
                });
            }
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Plan {
                action: String,
                expected_revision: u64,
                idempotency_key: Id,
                limits: Option<MutationLimits>,
            }
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Apply {
                expected_revision: u64,
                idempotency_key: Id,
                review: Digest,
                limits: Option<MutationLimits>,
            }
            let (action, revision, key, review, limits) = if name == "services.plan" {
                let parsed: Plan = serde_json::from_value(args).map_err(|_| ())?;
                (
                    parsed.action,
                    parsed.expected_revision,
                    parsed.idempotency_key,
                    None,
                    parsed.limits,
                )
            } else {
                let parsed: Apply = serde_json::from_value(args).map_err(|_| ())?;
                (
                    name.strip_prefix("services.").ok_or(())?.to_owned(),
                    parsed.expected_revision,
                    parsed.idempotency_key,
                    Some(parsed.review),
                    parsed.limits,
                )
            };
            if revision > devforge_control::limits::SafeInt::MAX {
                return Err(());
            }
            let input = match action.as_str() {
                "start" => MutationInput::Start(ServiceActionInput { limits }),
                "stop" => MutationInput::Stop(ServiceActionInput { limits }),
                "restart" => MutationInput::Restart(ServiceActionInput { limits }),
                _ => return Err(()),
            };
            let request = Request {
                api_version: Default::default(),
                request_id: request_id.clone(),
                project_id: self.project.clone(),
                expected_revision: Some(revision),
                idempotency_key: Some(key),
                input,
            };
            Ok(match review {
                None => ServiceInput::Plan(request),
                Some(review) => ServiceInput::Apply { request, review },
            })
        })();
        Some(input.map(|input| ServiceCall {
            input,
            request_id,
            id,
            version,
        }))
    }
    pub fn complete_service(call: ServiceCall, result: Result<Value, ErrorCode>) -> Value {
        let invalid = if matches!(call.input, ServiceInput::Apply { .. }) {
            ErrorCode::AdmissionUnconfirmed
        } else {
            ErrorCode::InvalidInput
        };
        let content = result
            .and_then(|value| {
                if value.get("api_version") != Some(&json!(1))
                    || value.get("request_id") != Some(&json!(call.request_id))
                    || value.get("ok").and_then(Value::as_bool).is_none_or(|ok| {
                        ok != value.get("data").is_some() || ok == value.get("error").is_some()
                    })
                {
                    Err(invalid)
                } else {
                    Ok(value)
                }
            })
            .unwrap_or_else(|code| {
                serde_json::to_value(Response::<Value>::failure(
                    call.request_id,
                    None,
                    code.fault(),
                ))
                .expect("fixed error envelope")
            });
        let is_error = content.get("ok") != Some(&Value::Bool(true));
        rpc_success(
            call.id,
            call.version,
            json!({"content":[{"type":"text","text":content.to_string()}],"structuredContent":content,"isError":is_error}),
        )
    }
    fn tool(&self, params: &Value) -> Result<ExternalInput, ()> {
        if !keys(params, &["name", "arguments"]) {
            return Err(());
        }
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let name = params.get("name").and_then(Value::as_str).ok_or(())?;
        if name == TOOLS[4] {
            if !keys(&arguments, &["operation_id"]) {
                return Err(());
            }
            let operation_id = arguments
                .get("operation_id")
                .and_then(Value::as_str)
                .and_then(|v| Id::new(v).ok())
                .ok_or(())?;
            return Ok(ExternalInput::OperationStatus {
                project_id: self.project.clone(),
                operation_id,
            });
        }
        if !keys(&arguments, &[]) {
            return Err(());
        }
        match name {
            "health.health" => Ok(ExternalInput::Health {}),
            "services.status" => Ok(ExternalInput::Status {}),
            "projects.list" => Ok(ExternalInput::ProjectList {}),
            "projects.get" => Ok(ExternalInput::ProjectGet {
                project_id: self.project.clone(),
            }),
            _ => Err(()),
        }
    }
    pub fn complete(call: ToolCall, result: Result<ExternalReply, ErrorCode>) -> Value {
        let result = result.and_then(|reply| {
            if reply.request_id != call.request_id
                || reply
                    .data
                    .as_ref()
                    .is_some_and(|data| data.tool() != call.input.tool())
            {
                Err(ErrorCode::InvalidInput)
            } else {
                Ok(reply)
            }
        });
        match call.kind {
            Kind::Tool => {
                let content = match result {
                    Ok(reply) => serde_json::to_value(reply).unwrap_or_else(
                        |_| json!({"ok":false,"error":{"code":"storage_unavailable"}}),
                    ),
                    Err(code) => json!({"ok":false,"error":{"code":code}}),
                };
                let is_error = content.get("ok") != Some(&Value::Bool(true));
                rpc_success(
                    call.id,
                    call.version,
                    json!({"content":[{"type":"text","text":content.to_string()}],"structuredContent":content,"isError":is_error}),
                )
            }
            kind => {
                let reply = match result {
                    Ok(reply) if reply.ok => reply,
                    Ok(reply) => {
                        return resource_failure(
                            call.id,
                            reply
                                .error
                                .map(|e| e.code)
                                .unwrap_or(ErrorCode::StorageUnavailable),
                        );
                    }
                    Err(code) => return resource_failure(call.id, code),
                };
                let Some(ExternalOutput::ProjectGet(output)) = &reply.data else {
                    return resource_failure(call.id, ErrorCode::InvalidInput);
                };
                if output.items.len() != 1 || output.items[0].project_id != call.project {
                    return resource_failure(call.id, ErrorCode::ProjectMismatch);
                }
                let uri = summary_uri(&output.items[0].project_id);
                let result = match kind {
                    Kind::ListResources => {
                        json!({"resources":[{"uri":uri,"name":"project-summary","mimeType":"application/json","description":"Granted project metadata and observation freshness"}]})
                    }
                    Kind::ReadResource(requested) if requested == uri => {
                        let text = match serde_json::to_string(output) {
                            Ok(text) => text,
                            Err(_) => {
                                return resource_failure(call.id, ErrorCode::StorageUnavailable);
                            }
                        };
                        json!({"contents":[{"uri":uri,"mimeType":"application/json","text":text}]})
                    }
                    _ => return resource_failure(call.id, ErrorCode::ProjectMismatch),
                };
                rpc_success(call.id, call.version, result)
            }
        }
    }
}
pub fn parse_error() -> Value {
    rpc_error(None, -32700, "Parse error", None)
}
pub fn output_limit(id: Option<Value>) -> Value {
    rpc_error(
        id,
        -33000,
        "Output limit exceeded",
        Some(json!({"code":"output_limit_exceeded"})),
    )
}
fn resource_failure(id: Value, code: ErrorCode) -> Value {
    rpc_error(
        Some(id),
        -33001,
        "Resource unavailable",
        Some(json!({"code":code})),
    )
}
fn rpc_error(id: Option<Value>, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut value = json!({"jsonrpc":"2.0","error":{"code":code,"message":message}});
    if let Some(id) = id {
        value["id"] = id;
    }
    if let Some(data) = data {
        value["error"]["data"] = data;
    }
    value
}
fn rpc_success(id: Value, version: Version, mut result: Value) -> Value {
    if matches!(version, Version::Modern) {
        result["resultType"] = json!("complete");
        result["_meta"] = json!({"io.modelcontextprotocol/serverInfo":server_info()});
        if ["supportedVersions", "tools", "resources", "contents"]
            .iter()
            .any(|field| result.get(*field).is_some())
        {
            result["ttlMs"] = json!(0);
            result["cacheScope"] = json!(if result.get("resources").is_some()
                || result.get("contents").is_some()
            {
                "private"
            } else {
                "public"
            });
        }
    }
    json!({"jsonrpc":"2.0","id":id,"result":result})
}
fn capabilities() -> Value {
    json!({"tools":{},"resources":{}})
}
fn server_info() -> Value {
    json!({"name":"devforge","version":env!("CARGO_PKG_VERSION")})
}
fn summary_uri(project: &Id) -> String {
    format!("devforge://project/{}/summary", project.as_str())
}
fn valid_id(value: &Value) -> bool {
    match value {
        Value::String(v) => v.len() <= 256,
        Value::Number(n) => n.is_i64() || n.is_u64(),
        _ => false,
    }
}
fn keys(value: &Value, allowed: &[&str]) -> bool {
    value
        .as_object()
        .is_some_and(|map| map.keys().all(|key| allowed.contains(&key.as_str())))
}
fn implementation(value: &Value) -> bool {
    value.is_object()
        && ["name", "version"].into_iter().all(|key| {
            value
                .get(key)
                .and_then(Value::as_str)
                .is_some_and(|v| !v.is_empty() && v.len() <= 256)
        })
}
fn tool_definitions(
    service_actions: bool,
    runtime_actions: bool,
    environment_actions: bool,
    database_actions: bool,
    domain_actions: bool,
    execution: bool,
    (support_export, site_probe, debugger): (bool, bool, bool),
) -> Vec<Value> {
    let mut definitions = read_tool_definitions();
    if debugger {
        use devforge_control::export::Wire;
        definitions.push(json!({"name":"debugger.inspect","description":"Inspect bounded project debugger metadata. Stack and profile metadata require SourceArtifacts; primitive locals require the explicit DebuggerValues information grant. Values are best-effort redacted. No expression evaluation.","inputSchema":devforge_control::tools::DebuggerInspectInput::schema(),"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}));
        definitions.push(json!({"name":"debugger.profile","description":"Read a bounded profiler summary by project-owned profile ID. Requires SourceArtifacts. It does not export files or evaluate expressions.","inputSchema":devforge_control::tools::DebuggerProfileInput::schema(),"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}));
        for (name, schema) in [
            (
                "debugger.breakpoints",
                devforge_control::mutations::DebuggerBreakpointsInput::schema(),
            ),
            (
                "debugger.control",
                devforge_control::mutations::DebuggerControlInput::schema(),
            ),
            (
                "debugger.install",
                devforge_control::mutations::DebuggerInstallInput::schema(),
            ),
            (
                "debugger.mode",
                devforge_control::mutations::DebuggerModeInput::schema(),
            ),
            (
                "debugger.start",
                devforge_control::mutations::DebuggerStartInput::schema(),
            ),
            (
                "debugger.primitive_assignment",
                devforge_control::mutations::DebuggerAssignInput::schema(),
            ),
            (
                "debugger.export",
                devforge_control::mutations::DebuggerExportInput::schema(),
            ),
        ] {
            definitions.push(json!({"name":name,"description":"Plan or submit a finite project-scoped debugger action using an exact plan digest. Install, primitive assignment and export require exact native approval and additional grants. A successful control or assignment operation means queue acceptance only, not that the debugger applied it; observe a later session snapshot and errors. No arbitrary evaluation.","inputSchema":{"type":"object","additionalProperties":false,"properties":{"action":{"enum":["plan","apply"]},"expected_revision":devforge_control::limits::SafeInt::schema(),"idempotency_key":Id::schema(),"review":Digest::schema(),"input":schema},"required":["action","expected_revision","idempotency_key","input"]},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":false,"openWorldHint":false}}));
        }
        definitions.push(json!({"name":"debugger.lookup","description":"Look up an owned debugger operation by idempotency key.","inputSchema":{"type":"object","additionalProperties":false,"properties":{"idempotency_key":Id::schema()},"required":["idempotency_key"]},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}));
    }

    if site_probe {
        use devforge_control::export::Wire;
        definitions.push(json!({"name":"site.probe","description":"Send one bounded GET to the bound managed project site; application state may change. Requires ExecutionTest and a separately enabled surface. The caller supplies only a path, never host, query, credentials or headers. Redirects are not followed, TLS is validated, and no body or response headers are returned. The server-generated trace context is an association hint that can be spoofed; it does not prove causation. An admitted GET can take effect even if the client times out, access is revoked, or the site restarts; do not retry automatically.","inputSchema":devforge_control::tools::SiteProbeInput::schema(),"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":false,"openWorldHint":false}}));
    }
    if support_export {
        use devforge_control::export::Wire;
        definitions.push(json!({"name":"support.export","description":"Plan or apply a local export of the exact retained support preview. Requires Observer and SensitiveOperation grants and native owner approval of the exact plan. Destination is manager-owned; no caller path or automatic sharing.","inputSchema":{"type":"object","additionalProperties":false,"properties":{"action":{"enum":["plan","apply"]},"artifact_id":Id::schema(),"expected_revision":devforge_control::limits::SafeInt::schema(),"idempotency_key":Id::schema(),"review":Digest::schema(),"limits":MutationLimits::schema()},"required":["action","artifact_id","expected_revision","idempotency_key"]},"annotations":{"readOnlyHint":false,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}));
        definitions.push(json!({"name":"support.export_lookup","description":"Look up a prior support export by its original idempotency key; absence is only a point-in-time observation.","inputSchema":{"type":"object","additionalProperties":false,"properties":{"idempotency_key":Id::schema()},"required":["idempotency_key"]},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}}));
    }
    if execution {
        definitions.extend(execution_tool_definitions());
    }
    definitions.extend(diagnostics_tool_definitions());
    definitions.extend(activity_tool_definitions());
    if service_actions {
        definitions.extend(service_tool_definitions());
    }
    if runtime_actions {
        definitions.extend(runtime_tool_definitions());
    }
    if environment_actions {
        definitions.extend(environment_tool_definitions());
    }
    if database_actions {
        definitions.extend(database_tool_definitions());
    }
    if domain_actions {
        definitions.extend(domain_tool_definitions());
    }
    if (service_actions
        || runtime_actions
        || environment_actions
        || database_actions
        || domain_actions
        || execution)
        && let Some(status) = definitions
            .iter_mut()
            .find(|tool| tool["name"] == "operations.status")
    {
        let operation = status["inputSchema"].clone();
        let mut lookup = operation.clone();
        lookup["properties"] = json!({"idempotency_key":{"type":"string","minLength":1,"maxLength":128,"pattern":"^[A-Za-z0-9_.:-]+$"}});
        lookup["required"] = json!(["idempotency_key"]);
        status["inputSchema"] = json!({"type":"object","oneOf":[operation,lookup]});
    }
    definitions
}
fn read_tool_definitions() -> Vec<Value> {
    TOOLS.into_iter().map(|name|json!({"name":name,"description":match name{"health.health"=>"Granted project metadata health","services.status"=>"Granted project cached managed service status","projects.list"=>"List only the granted project metadata","projects.get"=>"Inspect the granted project metadata",_=>"Observe only the external client's own operation"},"inputSchema":if name==TOOLS[4]{json!({"type":"object","properties":{"operation_id":{"type":"string","minLength":1,"maxLength":128,"pattern":"^[A-Za-z0-9_.:-]+$"}},"required":["operation_id"],"additionalProperties":false})}else{json!({"type":"object","properties":{},"additionalProperties":false})},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}})).collect()
}

fn execution_tool_definitions() -> Vec<Value> {
    use devforge_control::export::Wire;
    use devforge_control::mutations::{ExecutionCommandInput, ManagedTestInput};
    ["execution.php","execution.artisan","execution.composer","execution.node","execution.npm","execution.git","execution.managed_test_job","execution.output","execution.cancel"].into_iter().map(|name|{
        let output=name=="execution.output";let cancel=name=="execution.cancel";
        let mut schema=if output||cancel{json!({"type":"object","properties":{"operation_id":Id::schema()},"required":["operation_id"],"additionalProperties":false})}else if name=="execution.managed_test_job"{ManagedTestInput::schema()}else{ExecutionCommandInput::schema()};
        if !output&&!cancel {
            schema["properties"]["limits"]["anyOf"][0]["properties"]["timeout_ms"]["maximum"]=json!(300000);
            schema["allOf"]=json!([{"if":{"properties":{"action":{"const":"apply"}}},"then":{"required":["review"]},"else":{"not":{"required":["review"]}}}]);
            schema["properties"]["action"]=json!({"enum":["plan","apply"]});schema["properties"]["expected_revision"]=json!({"type":"integer","minimum":0,"maximum":devforge_control::limits::SafeInt::MAX});schema["properties"]["idempotency_key"]=Id::schema();schema["properties"]["review"]=Digest::schema();
            let required=schema["required"].as_array_mut().expect("fixed input schema");required.extend([json!("action"),json!("expected_revision"),json!("idempotency_key")]);
        }
        json!({"name":name,"description":if cancel{"Request cancellation of only this original external client’s operation through existing operations.cancel. Requires current Observer+ProjectManage grants. No rollback guarantee; inspect terminal status/output."}else if output{"Read only the original client's and original grant's retained redacted execution output and exit code; unavailable output is an explicit error."}else if name=="execution.managed_test_job"{"Plan/apply an exact native-reviewed managed test command using an actual isolated TestDataContext. Missing context refuses execution; never uses the active development database. Arguments may contain secrets and stay private. Current ExecutionTest grant, exact revision and native approval are mandatory. Limits bound cooperative timeout/output; cancellation does not undo effects. Keep the original key after uncertainty or volatile idempotency_unverifiable and reconcile operations.status; never silently replay."}else{"Plan/apply one exact native-reviewed managed tool invocation (no unrestricted shell). Arguments are sensitive private input, not authority. Current ExecutionTest grant, exact revision and native approval required. Limits bound cooperative timeout/output; cancellation does not undo effects. Preserve original key after uncertainty or volatile idempotency_unverifiable and reconcile operations.status; never silently replay."},"inputSchema":schema,"annotations":{"readOnlyHint":output,"destructiveHint":!output,"idempotentHint":true,"openWorldHint":!output}})
    }).collect()
}
fn service_tool_definitions() -> Vec<Value> {
    use devforge_control::export::Wire;
    ["services.plan","services.start","services.stop","services.restart"].into_iter().map(|name|{
        let mut schema=ServiceActionInput::schema();
        schema["properties"]["expected_revision"]=json!({"type":"integer","minimum":0,"maximum":devforge_control::limits::SafeInt::MAX});
        schema["properties"]["idempotency_key"]=Id::schema();
        let mut required=vec!["expected_revision","idempotency_key"];
        let plan=name=="services.plan";
        if plan{schema["properties"]["action"]=json!({"enum":["start","stop","restart"]});required.push("action");}else{schema["properties"]["review"]=Digest::schema();required.push("review");}
        schema["required"]=json!(required);
        json!({"name":name,"description":if plan{"Preview exact service intent. A native owner must approve it; a digest is not approval. Cached dependencies only."}else{"Apply a native-approved exact service intent. Start/restart execute trusted project code. Configured database/cache/SMTP dependencies may remain running after startup failure or cancellation; restart may already have stopped the selected session before a later denial. No rollback or hard deadline guarantee. Preserve the original idempotency key on admission_unconfirmed; reconcile using operations.status with that key, never silently replay with a new key."},"inputSchema":schema,"annotations":{"readOnlyHint":plan,"destructiveHint":!plan,"idempotentHint":true,"openWorldHint":!plan}})
    }).collect()
}

fn runtime_tool_definitions() -> Vec<Value> {
    use devforge_control::export::Wire;
    let build = |action: &str, plan: bool| {
        let mut schema = ServiceActionInput::schema();
        let properties = schema["properties"].as_object_mut().expect("fixed schema");
        properties.insert(
            "expected_revision".into(),
            json!({"type":"integer","minimum":0,"maximum":devforge_control::limits::SafeInt::MAX}),
        );
        properties.insert("idempotency_key".into(), Id::schema());
        let mut required = vec!["expected_revision", "idempotency_key"];
        if plan {
            properties.insert("action".into(), json!({"const":action}));
            required.push("action");
        } else {
            properties.insert("review".into(), Digest::schema());
            required.push("review");
        }
        if action == "install" {
            properties.insert("package_id".into(), Id::schema());
            required.push("package_id");
        } else {
            properties.insert("php".into(), Id::schema());
            properties.insert("node".into(), Id::schema());
            required.extend(["php", "node"]);
        }
        schema["required"] = json!(required);
        schema
    };
    ["runtimes.plan","runtimes.install","runtimes.select"].into_iter().map(|name|{
        let plan=name=="runtimes.plan";let schema=if plan{json!({"type":"object","oneOf":[build("install",true),build("select",true)]})}else{build(name.strip_prefix("runtimes.").expect("fixed prefix"),false)};
        json!({"name":name,"description":if plan{"Preview a finite runtime install or PHP/Node selection intent. A native owner must approve the exact plan; this preview is not approval."}else if name=="runtimes.install"{"Install only a catalog-pinned artifact into the shared managed cache. Project pins do not switch. Current grants and native exact-plan approval remain mandatory. Preserve the original key on admission_unconfirmed and reconcile using operations.status; no hidden retry/new key."}else{"Select PHP/Node only. Name, Composer, domain, HTTPS, Vite and queue are preserved by the server. A running selection may stop/restart the selected session and execute trusted project code. Failures/cancellation need not roll back committed pins or started shared dependencies. Preserve original key and lookup receipt after uncertainty."},"inputSchema":schema,"annotations":{"readOnlyHint":plan,"destructiveHint":name=="runtimes.select","idempotentHint":true,"openWorldHint":!plan}})
    }).collect()
}

fn environment_tool_definitions() -> Vec<Value> {
    use devforge_control::export::Wire;
    let build = |action: &str, planning: bool| {
        let mut props = serde_json::Map::new();
        let mut required = vec![];
        if planning {
            props.insert("action".into(), json!({"const":action}));
            if action != "preview" {
                required.push("action");
            }
        }
        props.insert(
            "expected_revision".into(),
            json!({"type":"integer","minimum":0,"maximum":devforge_control::limits::SafeInt::MAX}),
        );
        props.insert("idempotency_key".into(), Id::schema());
        props.insert("limits".into(),json!({"anyOf":[devforge_control::mutations::MutationLimits::schema(),{"type":"null"}]}));
        if action != "preview" {
            required.extend(["expected_revision", "idempotency_key"]);
        }
        if !planning {
            props.insert("review".into(), Digest::schema());
            required.push("review");
        }
        if action == "yaml_save" {
            props.insert("expected".into(),json!({"anyOf":[Digest::schema(),{"type":"null"}],"description":"Opaque version from masked preview; null/missing is create-only, never a raw document SHA."}));
            props.insert("text".into(),json!({"type":"string","maxLength":crate::service_control::MAX_EXTERNAL_YAML_BYTES,"x-maxUtf8Bytes":crate::service_control::MAX_EXTERNAL_YAML_BYTES}));
            required.push("text");
        } else {
            props.insert(
                "write_env".into(),
                json!({"type":"boolean","default":false}),
            );
        }
        json!({"type":"object","properties":props,"required":required,"additionalProperties":false})
    };
    ["environment.plan","environment.apply","environment.yaml_save"].into_iter().map(|name|{
        let planning=name=="environment.plan";let schema=if planning{json!({"type":"object","oneOf":[build("preview",true),build("apply",true),build("yaml_save",true)]})}else{build(name.strip_prefix("environment.").expect("fixed prefix"),false)};
        json!({"name":name,"description":if planning{"Masked configuration preview or native-review plan. Default action preview returns opaque document version even when existing YAML is invalid. Use action apply or yaml_save to prepare the exact write intent. No raw YAML/.env values, references, core hashes or root paths are exported. YAML text is limited to16KiB UTF8."}else if name=="environment.apply"{"Apply only the native-approved configuration plan with current PM+SensitiveOperation+ExecutionTest grants. Existing backend may write project/configuration/credential links, install dependencies or change owned database bindings; no atomic rollback/hard deadline promise. Preserve original key after uncertainty and use operations.status key lookup."}else{"Save a native-approved YAML candidate up to16KiB UTF8 with an opaque expected version from masked preview (null=create-only). No secret binding/export tool. Input and equivalence fingerprint stay in RAM; after restart/eviction, idempotency_unverifiable requires original-key receipt lookup, never hidden replay or a new key."},"inputSchema":schema,"annotations":{"readOnlyHint":planning,"destructiveHint":!planning,"idempotentHint":true,"openWorldHint":name=="environment.apply"}})
    }).collect()
}

fn database_tool_definitions() -> Vec<Value> {
    use devforge_control::export::Wire;
    let build = |action: &str, planning: bool| {
        let mut properties = serde_json::Map::new();
        let mut required = vec![];
        if planning {
            properties.insert("action".into(), json!({"const":action}));
            if action != "choices" {
                required.push("action");
            }
        }
        if action != "choices" {
            properties.insert("expected_revision".into(),json!({"type":"integer","minimum":0,"maximum":devforge_control::limits::SafeInt::MAX}));
            properties.insert("idempotency_key".into(), Id::schema());
            properties.insert("limits".into(),json!({"anyOf":[devforge_control::mutations::MutationLimits::schema(),{"type":"null"}]}));
            required.extend(["expected_revision", "idempotency_key"]);
            let target = if action == "bind" {
                "instance_id"
            } else {
                "backup_id"
            };
            properties.insert(target.into(), Id::schema());
            required.push(target);
            if !planning {
                properties.insert("review".into(), Digest::schema());
                required.push("review");
            }
        }
        json!({"type":"object","additionalProperties":false,"properties":properties,"required":required})
    };
    ["database.plan","database.bind","database.restore"].into_iter().map(|name|{
        let planning=name=="database.plan";let schema=if planning{json!({"type":"object","oneOf":[build("choices",true),build("bind",true),build("restore",true)]})}else{build(name.strip_prefix("database.").expect("fixed prefix"),false)};
        json!({"name":name,"description":if planning{"Scoped owned DB choices or native-review plan. Default action choices lists only this project's current/retained instance IDs and own backup IDs. Bind/restore plans require PM+Sensitive and stopped project. Private paths, credentials, database names, checksums and identities remain at the manager/native-plan boundary."}else if name=="database.bind"{"Apply only the exact native-approved owned instance binding with PM+Sensitive and stopped project. It may start a managed DB daemon or provision a project-owned staged binding; failure/cancellation need not undo completed prerequisites. No admin credentials or SQL tool is exposed. Keep original key after uncertainty and lookup its receipt."}else{"Restore only an owned backup by ID through the existing staged restore/activation transaction. PM+Sensitive, stopped project and exact native approval are mandatory. Private checksum/identity/engine checks are server-owned. Staging or completed prerequisites may remain after failure; late cancellation does not undo successful activation. Preserve original key and reconcile; no path, SQL import, clone or export tool."},"inputSchema":schema,"annotations":{"readOnlyHint":planning,"destructiveHint":!planning,"idempotentHint":true,"openWorldHint":!planning}})
    }).collect()
}

fn domain_tool_definitions() -> Vec<Value> {
    use devforge_control::export::Wire;
    let write_schema = |action: &str| {
        let mut props = serde_json::Map::from_iter([
            ("action".into(), json!({"const":action})),
            ("plan".into(), Digest::schema()),
            (
                "expected_revision".into(),
                json!({"type":"integer","minimum":0,"maximum":devforge_control::limits::SafeInt::MAX}),
            ),
            ("idempotency_key".into(), Id::schema()),
            (
                "limits".into(),
                json!({"anyOf":[devforge_control::mutations::MutationLimits::schema(),{"type":"null"}]}),
            ),
        ]);
        let mut required = vec!["plan", "expected_revision", "idempotency_key"];
        if action == "apply" {
            props.insert("review".into(), Digest::schema());
            required.extend(["action", "review"]);
        }
        json!({"type":"object","properties":props,"required":required,"additionalProperties":false})
    };
    ["domains.url","domains.ports","domains.certificate_status","domains.hosts_plan","domains.trust_plan","domains.apply","domains.cleanup"].into_iter().map(|name|{
        let write=matches!(name,"domains.apply"|"domains.cleanup");let schema=if name=="domains.cleanup"{let mut plan=write_schema("plan");plan["required"].as_array_mut().expect("fixed schema").push(json!("action"));json!({"type":"object","oneOf":[{"type":"object","properties":{"action":{"const":"prepare"}},"additionalProperties":false},plan,write_schema("apply")]})}else if write{json!({"type":"object","oneOf":[write_schema("plan"),write_schema("apply")]})}else{json!({"type":"object","properties":{},"additionalProperties":false})};
        json!({"name":name,"description":if !write{"Read only this project's URL/ports/certificate status or prepare opaque hosts/trust plan. Missing CA requires native owner preparation; this route never creates CA, reads its private key or exports global domains/hosts/paths/DER."}else if name=="domains.cleanup"{"Prepare/plan/apply cleanup of only the selected project's owned hosts entries. Other projects and shared CA are preserved. PM+Sensitive, native exact-plan approval AND separate explicit native owner confirmation of the exact machine change are mandatory before helper execution. No agent confirmation/elevation/consent flag is accepted. Preserve original key after uncertainty; no rollback/hard deadline promise."}else{"Plan/apply a prepared hosts or trust ticket. PM+Sensitive, native exact-plan approval AND separate explicit native owner confirmation of the exact machine change are mandatory. Generic approval, grant or tool action never authorizes elevation. No helper path, hosts contents, CA DER/key or owner-wide domains accepted/exported. Preserve original key and reconcile after uncertain admission."},"inputSchema":schema,"annotations":{"readOnlyHint":!write,"destructiveHint":write,"idempotentHint":true,"openWorldHint":false}})
    }).collect()
}

fn activity_tool_definitions() -> Vec<Value> {
    use devforge_control::{
        export::Wire,
        tools::{ActivityMarkInput, ActivityWaitInput, SupportPreviewInput, SupportReadInput},
    };
    [("activity.mark", ActivityMarkInput::schema()), ("activity.wait", ActivityWaitInput::schema()), ("support.preview", SupportPreviewInput::schema()), ("support.read", SupportReadInput::schema())].into_iter().map(|(name, mut schema)| {
        // Omitted context is supported; explicit null is rejected at MCP boundary.
        if name.starts_with("activity.") { schema["properties"]["context_id"] = json!({"type":"string","minLength":32,"maxLength":32,"pattern":"^[A-Fa-f0-9]{32}$"}); }
        if name == "activity.mark" {
            for key in ["subject", "recipient", "sender"] { schema["properties"][key] = devforge_control::limits::Text::<256>::schema(); }
            schema["properties"]["since"] = devforge_control::limits::SafeInt::schema();
        }
        json!({"name":name,"description":if name=="support.preview"{"Create a bounded support preview from cached doctor metadata and fixed runbook codes. It excludes environment values, keys, database dumps, log/mail content and arbitrary files. Returns a project/client/grant-bound opaque artifact ID, never a path. No automatic sharing or export."}else if name=="support.read"{"Read the same unexpired support preview through its opaque artifact ID. The handle is bound to the original project, client, grant and revision; it is not a path and does not grant filesystem access."}else if name=="activity.mark"{"Create an opaque project- and grant-bound checkpoint before an action. Select inspector, logs or mail; detail requires its explicit information grant. Mail subject/sender/recipient filters require MailContent even when detail=false; since alone is metadata. Context applies only to inspector/logs. Checkpoints expire after 120 seconds, may be evicted, and do not survive manager restart."}else{"Poll for the next observed event for up to the requested 3000ms maximum. No new poll starts after the requested duration; it allows up to 1s grace for an in-flight read and final checks, plus up to 250ms registry contention, subject to scheduling and transport, not hard real-time; that read may return a match. An optional context narrows it; use the returned bookmark for the next call, including after timeout. Checkpoints retain the original 120-second expiry. Stdio calls on one session are serialized; a wait occupies that session until it finishes. Every result reports continuity=unverified: timeout means no matching event was observed by polling, not proof that no transient event occurred. Expired, evicted or replaced sources require a new checkpoint; detected retention gaps are errors, not timeouts."},"inputSchema":schema,"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":name=="support.read","openWorldHint":false}})
    }).collect()
}
fn diagnostics_tool_definitions() -> Vec<Value> {
    use devforge_control::{
        export::Wire,
        tools::{HealthInput, InspectorInput, LogInput, MailInput},
    };
    ["health.doctor","logs.search","inspector.request","mail.list","mail.show"].into_iter().map(|name|{
        let mut schema=match name{"health.doctor"=>HealthInput::schema(),"logs.search"=>LogInput::schema(),"inspector.request"=>InspectorInput::schema(),"mail.show"=>json!({"type":"object","additionalProperties":false,"properties":{"mail_id":Id::schema(),"page":devforge_control::limits::PageRequest::schema()},"required":["mail_id"]}),_=>MailInput::schema()};
        let properties=schema["properties"].as_object_mut().expect("fixed schema");
        if name == "logs.search" { properties.insert("query".into(),devforge_control::limits::Text::<256>::schema()); properties.insert("content".into(),bool::schema()); properties.insert("since".into(),devforge_control::limits::SafeInt::schema()); }
        if name == "inspector.request" { properties.insert("detail".into(),bool::schema()); }
        if name == "mail.list" { for key in ["subject","recipient","sender"] { properties.insert(key.into(),devforge_control::limits::Text::<256>::schema()); } properties.insert("detail".into(),bool::schema()); properties.insert("since".into(),devforge_control::limits::SafeInt::schema()); properties.insert("mail_id".into(),Id::schema()); }
        json!({"name":name,"description":if name=="logs.search"{"Project-scoped bounded log metadata. Set content=true or provide query to request redacted log content; both require the explicitly granted log_content right. Query searches only the redacted projection. Forward opaque page cursor unchanged; expired cursors require a fresh request."}else if name=="inspector.request"{"Project-scoped Inspector metadata by default; detail=true requires the explicitly granted inspector_queries right and returns bounded request/job/query/cache/dump details. Dump values are bounded and masked on a best-effort basis; unrecognized secrets may remain. Repeated query counts describe the retained context window and are candidates, not proof of N+1. For detail rows use detail.duration_ms; absent means unmeasured, and the required legacy elapsed_ms can be a zero placeholder for cache/dump. detail.truncated describes projection text; capture_truncated is optional and absent means the collector status is unknown. Forward opaque page cursor unchanged."}else if name=="mail.list" || name=="mail.show" {"Project-scoped mail. Observer gets metadata; detail, mail_id, subject, recipient or sender require MailContent. since alone is metadata. Authorized details may contain sensitive verification/reset URL candidates with query tokens intact; candidates are not validated or fetched. Attachment metadata only; no export or delete. Forward opaque cursors unchanged. Frozen cached cursor pages may include a locally deleted message until 120s TTL, including a row not previously delivered; cached origin/time remain explicit. Local deletion does not revoke a remote token."}else{"Project-scoped Observer health metadata. Forward opaque page cursor unchanged."},"inputSchema":schema,"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}})
    }).collect()
}
