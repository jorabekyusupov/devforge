//! Narrow dual-era MCP contract: modern requests are independently versioned;
//! only legacy initialize/initialized state is remembered. No I/O or authority
//! is minted here. The caller dispatches only the returned read-only input.
use crate::external_control::{ExternalInput, ExternalOutput, ExternalReply};
use devforge_control::{
    Digest, ErrorCode, Id, Request, Response,
    mutations::{MutationInput, MutationLimits, ServiceActionInput},
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
}
pub enum Action {
    Reply(Value),
    Ignore,
    Cancel(Value),
    Call(ToolCall),
    Service(ServiceCall),
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
        }
    }
    /// Surface selection only; grants and native exact-plan approval remain
    /// mandatory at the manager. The default constructor stays read-only.
    pub fn with_service_actions(project_id: Id) -> Self {
        let mut protocol = Self::new(project_id);
        protocol.service_actions = true;
        protocol
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
                        json!({"tools":tool_definitions(self.service_actions)}),
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
fn tool_definitions(service_actions: bool) -> Vec<Value> {
    let mut definitions = read_tool_definitions();
    if service_actions {
        definitions.extend(service_tool_definitions());
        if let Some(status) = definitions
            .iter_mut()
            .find(|tool| tool["name"] == "operations.status")
        {
            let operation = status["inputSchema"].clone();
            let mut lookup = operation.clone();
            lookup["properties"] = json!({"idempotency_key":{"type":"string","minLength":1,"maxLength":128,"pattern":"^[A-Za-z0-9_.:-]+$"}});
            lookup["required"] = json!(["idempotency_key"]);
            status["inputSchema"] = json!({"type":"object","oneOf":[operation,lookup]});
        }
    }
    definitions
}
fn read_tool_definitions() -> Vec<Value> {
    TOOLS.into_iter().map(|name|json!({"name":name,"description":match name{"health.health"=>"Granted project metadata health","services.status"=>"Granted project cached managed service status","projects.list"=>"List only the granted project metadata","projects.get"=>"Inspect the granted project metadata",_=>"Observe only the external client's own operation"},"inputSchema":if name==TOOLS[4]{json!({"type":"object","properties":{"operation_id":{"type":"string","minLength":1,"maxLength":128,"pattern":"^[A-Za-z0-9_.:-]+$"}},"required":["operation_id"],"additionalProperties":false})}else{json!({"type":"object","properties":{},"additionalProperties":false})},"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}})).collect()
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
