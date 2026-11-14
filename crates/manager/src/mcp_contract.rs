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
                        json!({"tools":tool_definitions(self.service_actions,self.runtime_actions,self.environment_actions,self.database_actions)}),
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
) -> Vec<Value> {
    let mut definitions = read_tool_definitions();
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
    if (service_actions || runtime_actions || environment_actions || database_actions)
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
