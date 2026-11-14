//! Additive JSON CLI. Metadata is resolved before roots, IPC or credentials.
use devforge_control::{
    API_VERSION, Digest, ErrorCode, Id, Request, Response,
    mutations::{MutationInput, PackageInput, ServiceActionInput},
};
use serde_json::{Value, json};
use std::io::{self, IsTerminal, Read};
use zeroize::Zeroizing;

const COMMANDS: &[(&str, &str)] = &[
    ("health", "devforge api health"),
    ("status", "devforge api status --credential-stdin"),
    (
        "project list",
        "devforge api project list --credential-stdin",
    ),
    (
        "project inspect",
        "devforge api project inspect PROJECT_ID --credential-stdin",
    ),
    (
        "operation status",
        "devforge api operation status OPERATION_ID --project PROJECT_ID --credential-stdin",
    ),
    (
        "operation cancel",
        "devforge api operation cancel OPERATION_ID --project PROJECT_ID --credential-stdin",
    ),
    ("schema", "devforge api schema"),
    ("capabilities", "devforge api capabilities"),
    (
        "service plan",
        "devforge api service plan ACTION PROJECT_ID --revision N --idempotency-key KEY --credential-stdin",
    ),
    (
        "service start",
        "devforge api service start PROJECT_ID --revision N --idempotency-key KEY --review DIGEST --credential-stdin",
    ),
    (
        "service stop",
        "devforge api service stop PROJECT_ID --revision N --idempotency-key KEY --review DIGEST --credential-stdin",
    ),
    (
        "service restart",
        "devforge api service restart PROJECT_ID --revision N --idempotency-key KEY --review DIGEST --credential-stdin",
    ),
    (
        "operation lookup",
        "devforge api operation lookup KEY --project PROJECT_ID --credential-stdin",
    ),
    (
        "runtime plan",
        "devforge api runtime plan install PROJECT_ID --package PACKAGE_ID --revision N --idempotency-key KEY --credential-stdin",
    ),
    (
        "runtime install",
        "devforge api runtime install PROJECT_ID --package PACKAGE_ID --revision N --idempotency-key KEY --review DIGEST --credential-stdin",
    ),
    (
        "runtime select",
        "devforge api runtime select PROJECT_ID --php PHP_ID --node NODE_ID --revision N --idempotency-key KEY --review DIGEST --credential-stdin",
    ),
    (
        "env plan",
        "devforge api env plan PROJECT_ID --credential-file ABSOLUTE_PATH",
    ),
    (
        "env plan apply",
        "devforge api env plan apply PROJECT_ID --revision N --idempotency-key KEY --credential-file ABSOLUTE_PATH [--write-env]",
    ),
    (
        "env plan yaml-save",
        "devforge api env plan yaml-save PROJECT_ID --revision N --idempotency-key KEY --input-stdin --credential-file ABSOLUTE_PATH",
    ),
    (
        "env apply",
        "devforge api env apply PROJECT_ID --revision N --idempotency-key KEY --review TICKET --credential-file ABSOLUTE_PATH [--write-env]",
    ),
    (
        "env yaml-save",
        "devforge api env yaml-save PROJECT_ID --revision N --idempotency-key KEY --review TICKET --input-stdin --credential-file ABSOLUTE_PATH",
    ),
    (
        "env lookup",
        "devforge api env lookup KEY --project PROJECT_ID --credential-file ABSOLUTE_PATH",
    ),
];

#[derive(Debug)]
struct Invocation {
    request: Id,
    words: Vec<String>,
    project: Option<Id>,
    credential_stdin: bool,
    revision: Option<u64>,
    idempotency: Option<Id>,
    review: Option<Digest>,
    package: Option<Id>,
    php: Option<Id>,
    node: Option<Id>,
    credential_file: Option<std::path::PathBuf>,
    input_stdin: bool,
    write_env: bool,
}
impl Invocation {
    fn parse(args: &[String], request: Id) -> Result<Self, ErrorCode> {
        let mut invocation = Self {
            request,
            words: vec![],
            project: None,
            credential_stdin: false,
            revision: None,
            idempotency: None,
            review: None,
            package: None,
            php: None,
            node: None,
            credential_file: None,
            input_stdin: false,
            write_env: false,
        };
        let mut request_seen = false;
        let mut version_seen = false;
        let mut json_seen = false;
        let mut index = 0;
        while index < args.len() {
            let argument = &args[index];
            match argument.as_str() {
                "--api-version" | "--request-id" | "--project" | "--revision"
                | "--idempotency-key" | "--review" | "--package" | "--php" | "--node"
                | "--credential-file" => {
                    index += 1;
                    let value = args.get(index).ok_or(ErrorCode::InvalidInput)?;
                    match argument.as_str() {
                        "--api-version" if !version_seen => {
                            version_seen = true;
                            let version: u16 =
                                value.parse().map_err(|_| ErrorCode::InvalidInput)?;
                            if version != API_VERSION {
                                return Err(ErrorCode::UnsupportedVersion);
                            }
                        }
                        "--request-id" if !request_seen => {
                            request_seen = true;
                            invocation.request = Id::new(value)?;
                        }
                        "--project" if invocation.project.is_none() => {
                            invocation.project = Some(Id::new(value)?)
                        }
                        "--revision" if invocation.revision.is_none() => {
                            let revision = value.parse().map_err(|_| ErrorCode::InvalidInput)?;
                            if revision > devforge_control::limits::SafeInt::MAX {
                                return Err(ErrorCode::InvalidInput);
                            }
                            invocation.revision = Some(revision);
                        }
                        "--idempotency-key" if invocation.idempotency.is_none() => {
                            invocation.idempotency = Some(Id::new(value)?)
                        }
                        "--review" if invocation.review.is_none() => {
                            invocation.review = Some(Digest::new(value.clone())?)
                        }
                        "--package" if invocation.package.is_none() => {
                            invocation.package = Some(Id::new(value)?)
                        }
                        "--php" if invocation.php.is_none() => {
                            invocation.php = Some(Id::new(value)?)
                        }
                        "--node" if invocation.node.is_none() => {
                            invocation.node = Some(Id::new(value)?)
                        }
                        "--credential-file" if invocation.credential_file.is_none() => {
                            invocation.credential_file = Some(value.into())
                        }
                        _ => return Err(ErrorCode::InvalidInput),
                    }
                }
                "--credential-stdin" if !invocation.credential_stdin => {
                    invocation.credential_stdin = true
                }
                "--input-stdin" if !invocation.input_stdin => invocation.input_stdin = true,
                "--write-env" if !invocation.write_env => invocation.write_env = true,
                "--json" if !json_seen => json_seen = true,
                flag if flag.starts_with('-') && !matches!(flag, "--help" | "-h") => {
                    return Err(ErrorCode::InvalidInput);
                }
                _ => invocation.words.push(argument.clone()),
            }
            index += 1;
        }
        Ok(invocation)
    }
}

/// Exactly one bounded credential frame; never argv, environment, a store or a
/// terminal prompt. EOF is required, so callers should close their private pipe.
fn credential() -> Result<Zeroizing<String>, ErrorCode> {
    let stdin = io::stdin();
    if stdin.is_terminal() {
        return Err(ErrorCode::Unauthorized);
    }
    let mut bytes = Zeroizing::new(Vec::new());
    stdin
        .lock()
        .take(67)
        .read_to_end(&mut bytes)
        .map_err(|_| ErrorCode::Unauthorized)?;
    let length = bytes.len();
    if bytes.ends_with(b"\r\n") {
        bytes.truncate(length - 2);
    } else if bytes.ends_with(b"\n") {
        bytes.truncate(length - 1);
    }
    if bytes.len() != 64
        || !bytes
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
    {
        return Err(ErrorCode::Unauthorized);
    }
    String::from_utf8(bytes.to_vec())
        .map(Zeroizing::new)
        .map_err(|_| ErrorCode::Unauthorized)
}

fn metadata(words: &[String]) -> Result<Option<Value>, ErrorCode> {
    let help = words.is_empty()
        || words
            .iter()
            .any(|word| matches!(word.as_str(), "--help" | "-h" | "help"));
    if help {
        let path = words
            .iter()
            .filter(|word| !matches!(word.as_str(), "--help" | "-h" | "help"))
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        if !path.is_empty()
            && !COMMANDS
                .iter()
                .any(|(name, _)| *name == path || name.starts_with(&(path.clone() + " ")))
        {
            return Err(ErrorCode::InvalidInput);
        }
        return Ok(Some(
            json!({"usage":"devforge api [--api-version 1] [--request-id ID] COMMAND [--json]", "commands":COMMANDS.iter().filter(|(name,_)| path.is_empty() || *name==path || name.starts_with(&(path.clone()+" "))).map(|(name,example)|json!({"name":name,"example":example})).collect::<Vec<_>>(),"credential_input":"Environment uses protected --credential-file; YAML canonical JSON input uses --input-stdin. Other API credentials use explicit --credential-stdin. Never pass token or YAML values in argv/environment.","exit_codes":exit_codes()}),
        ));
    }
    match words
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["capabilities"] => Ok(Some(
            json!({"api_version":API_VERSION,"owner_autostart":false,"native_desktop_authority":false,"external_grant_required":true,"project_visibility":"credential-bound project only","commands":COMMANDS.iter().filter(|(name,_)| !matches!(*name,"schema"|"capabilities")).map(|(name,_)|json!({"name":name,"implemented":true,"requires_connection":true,"requires_grant":*name!="health","required_scopes":match *name {"health"=>vec![],"operation cancel"=>vec!["observer","project_manage"],"env apply"|"env plan apply"=>vec!["project_manage","sensitive_operation","execution_test"],"env yaml-save"|"env plan yaml-save"=>vec!["project_manage","sensitive_operation"],"service start"|"service restart"|"runtime select"=>vec!["project_manage","execution_test"],"service stop"|"runtime install"=>vec!["project_manage"],"service plan"|"runtime plan"=>vec!["project_manage"],_=>vec!["observer"]},"execution_requires_native_approval":name.starts_with("service ")||name.starts_with("runtime ")||matches!(*name,"env apply"|"env yaml-save"|"env plan apply"|"env plan yaml-save"),"scope_by_action":if *name=="service plan"{json!({"start":["project_manage","execution_test"],"stop":["project_manage"],"restart":["project_manage","execution_test"]})}else if *name=="runtime plan"{json!({"install":["project_manage"],"select":["project_manage","execution_test"]})}else{Value::Null},"partial_effects":if matches!(*name,"service plan"|"service start"|"service restart"){json!(["configured_database_cache_smtp_may_remain_running_after_failure_or_cancel","restart_may_have_stopped_selected_session_before_later_denial"])}else{json!([])},"effect":match *name{"operation cancel"=>"cancel_owned_operation","service start"|"service stop"|"service restart"=>"reviewed_owned_service_lifecycle","runtime install"=>"reviewed_shared_cache_write_without_pin_switch","runtime select"=>"reviewed_php_node_selection_with_possible_session_restart","env apply"=>"reviewed_existing_configuration_backend_writes","env yaml-save"=>"reviewed_yaml_and_private_core_history_write",_=>"read_only"}})).collect::<Vec<_>>(),"deferred":["general_execution_tools","content_reads","other_project_mutations"],"planning_inventory_is_live":false}),
        )),
        ["schema"] => Ok(Some(schema())),
        _ => Ok(None),
    }
}
fn exit_codes() -> Value {
    json!({"0":"success","2":"invalid_input_or_unsupported_version","3":"unauthorized_or_grant_denied","4":"owner_or_storage_unavailable","5":"conflict_or_busy","6":"unsupported_or_bounded_output","7":"timeout_interrupted_unconfirmed_or_unverifiable"})
}
fn exit_code(code: ErrorCode) -> i32 {
    match code {
        ErrorCode::InvalidInput | ErrorCode::UnsupportedVersion => 2,
        ErrorCode::Unauthorized
        | ErrorCode::GrantExpired
        | ErrorCode::GrantRevoked
        | ErrorCode::ProjectMismatch => 3,
        ErrorCode::StorageUnavailable => 4,
        ErrorCode::RevisionConflict
        | ErrorCode::ReviewExpired
        | ErrorCode::ReviewMismatch
        | ErrorCode::IdempotencyConflict
        | ErrorCode::InvalidTransition
        | ErrorCode::PreconditionFailed
        | ErrorCode::Busy => 5,
        ErrorCode::CursorExpired
        | ErrorCode::OutputLimitExceeded
        | ErrorCode::UnsupportedCapability => 6,
        ErrorCode::TimedOut
        | ErrorCode::Interrupted
        | ErrorCode::AdmissionUnconfirmed
        | ErrorCode::IdempotencyUnverifiable => 7,
    }
}
fn schema() -> Value {
    let id = json!({"type":"string","pattern":"^[A-Za-z0-9_.:-]+$","minLength":1,"maxLength":128});
    let inputs = COMMANDS.iter().map(|(name,_)| {
        let mut properties = serde_json::Map::from_iter([
            ("command".to_owned(),json!({"const":name})),
            ("api_version".to_owned(),json!({"const":API_VERSION})),
            ("request_id".to_owned(),id.clone()),
        ]);
        let mut required = vec!["command"];
        if *name=="project inspect" || name.starts_with("operation ") || name.starts_with("service ") || name.starts_with("runtime ") ||name.starts_with("env ") {
            properties.insert("project_id".into(),id.clone());
            required.push("project_id");
        }
        if name.starts_with("operation ") && *name!="operation lookup" {
            properties.insert("operation_id".into(),id.clone());
            required.push("operation_id");
        }
        if name.starts_with("service ") {
            properties.insert("expected_revision".into(),json!({"type":"integer","minimum":0,"maximum":devforge_control::limits::SafeInt::MAX}));
            properties.insert("idempotency_key".into(),id.clone());required.extend(["expected_revision","idempotency_key"]);
            if *name=="service plan"{properties.insert("action".into(),json!({"enum":["start","stop","restart"]}));required.push("action");}else{properties.insert("review".into(),json!({"type":"string","pattern":"^[0-9a-f]{64}$"}));required.push("review");}
        }
        if name.starts_with("runtime ") {
            properties.insert("expected_revision".into(),json!({"type":"integer","minimum":0,"maximum":devforge_control::limits::SafeInt::MAX}));properties.insert("idempotency_key".into(),id.clone());required.extend(["expected_revision","idempotency_key"]);
            if *name!="runtime plan"{properties.insert("review".into(),json!({"type":"string","pattern":"^[0-9a-f]{64}$"}));required.push("review");}
            if *name=="runtime install"{properties.insert("package_id".into(),id.clone());required.push("package_id");}else{
                properties.insert("php".into(),id.clone());properties.insert("node".into(),id.clone());
                if *name=="runtime select"{required.extend(["php","node"]);}else{properties.insert("package_id".into(),id.clone());properties.insert("action".into(),json!({"enum":["install","select"]}));required.push("action");}
            }
        }
        if name.starts_with("env ") {
            if *name=="env lookup"{properties.insert("idempotency_key".into(),id.clone());required.push("idempotency_key");}else{
                properties.insert("expected_revision".into(),json!({"type":"integer","minimum":0,"maximum":devforge_control::limits::SafeInt::MAX}));properties.insert("idempotency_key".into(),id.clone());
                if *name!="env plan"{required.extend(["expected_revision","idempotency_key"]);}
                if matches!(*name,"env apply"|"env yaml-save"){properties.insert("review".into(),json!({"type":"string","pattern":"^[0-9a-f]{64}$"}));required.push("review");}
                if name.ends_with("yaml-save"){
                    use devforge_control::export::Wire;
                    let mut payload=devforge_control::mutations::YamlSaveInput::schema();payload["properties"]["text"]["maxLength"]=json!(devforge_manager::service_control::MAX_EXTERNAL_YAML_BYTES);payload["properties"]["text"]["x-maxUtf8Bytes"]=json!(devforge_manager::service_control::MAX_EXTERNAL_YAML_BYTES);payload["properties"]["expected"]["description"]=json!("Opaque version from masked preview; null/missing is create-only. Never a raw document SHA.");properties.insert("stdin_input".into(),payload);required.push("stdin_input");
                }else{properties.insert("write_env".into(),json!({"type":"boolean","default":false}));}
            }
        }
        if *name=="operation lookup"{properties.insert("idempotency_key".into(),id.clone());required.push("idempotency_key");}
        let mut input=json!({"type":"object","additionalProperties":false,"properties":properties,"required":required});
        if *name=="runtime plan"{input["oneOf"]=json!([{"properties":{"action":{"const":"install"}},"required":["package_id"],"not":{"anyOf":[{"required":["php"]},{"required":["node"]}]}},{"properties":{"action":{"const":"select"}},"required":["php","node"],"not":{"required":["package_id"]}}]);}
        input
    }).collect::<Vec<_>>();
    let mut value = json!({"api_version":API_VERSION,"scope":"implemented JSON CLI and reviewed service and runtime install/PHP-Node and masked environment/YAML slices; other mutations remain unavailable","response":{"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","additionalProperties":false,"required":["api_version","request_id","ok","warnings"],"properties":{"api_version":{"const":API_VERSION},"request_id":{"type":"string","pattern":"^[A-Za-z0-9_.:-]+$","minLength":1,"maxLength":128},"operation_id":{"type":"string"},"ok":{"type":"boolean"},"data":{},"error":{"type":"object","additionalProperties":false,"required":["code","retryable"],"properties":{"code":{"enum":devforge_control::export::ERRORS},"retryable":{"type":"boolean"}}},"warnings":{"type":"array","items":{"enum":devforge_control::export::ERRORS}}},"oneOf":[{"properties":{"ok":{"const":true}},"required":["data"],"not":{"required":["error"]}},{"properties":{"ok":{"const":false}},"required":["error"],"not":{"required":["data"]}}]},"commands":COMMANDS.iter().map(|(name,example)|json!({"name":name,"example":example})).collect::<Vec<_>>(),"exit_codes":exit_codes()});
    value["input"] = json!({"$schema":"https://json-schema.org/draft/2020-12/schema","description":"Logical CLI arguments; bearer credentials are private stdin transport, never JSON fields.","oneOf":inputs});
    value
}

pub fn run(args: &[String]) -> i32 {
    let generated = Id::new(format!(
        "cli-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ))
    .expect("bounded native request ID");
    let invocation = match Invocation::parse(args, generated.clone()) {
        Ok(invocation) => invocation,
        Err(code) => {
            emit(Response::<Value>::failure(generated, None, code.fault()));
            return exit_code(code);
        }
    };
    let request = invocation.request.clone();
    let result = metadata(&invocation.words).and_then(|meta| match meta {
        Some(value)
            if invocation.project.is_none()
                && !invocation.credential_stdin
                && invocation.revision.is_none()
                && invocation.idempotency.is_none()
                && invocation.review.is_none()
                && invocation.package.is_none()
                && invocation.php.is_none()
                && invocation.node.is_none()
                && invocation.credential_file.is_none()
                && !invocation.input_stdin
                && !invocation.write_env =>
        {
            Ok((
                serde_json::to_value(Response::success(request.clone(), None, value))
                    .expect("control envelope"),
                0,
            ))
        }
        Some(_) => Err(ErrorCode::InvalidInput),
        None => live(&invocation),
    });
    match result {
        Ok((value, code)) => {
            println!("{value}");
            code
        }
        Err(code) => {
            emit(Response::<Value>::failure(request, None, code.fault()));
            exit_code(code)
        }
    }
}

fn emit(response: Response<Value>) {
    println!(
        "{}",
        serde_json::to_string(&response).expect("JSON control envelope")
    );
}
fn env_yaml_input() -> Result<devforge_control::mutations::YamlSaveInput, ErrorCode> {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Err(ErrorCode::InvalidInput);
    }
    let mut bytes = Zeroizing::new(Vec::new());
    stdin
        .lock()
        .take(devforge_manager::MAX_REQUEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ErrorCode::InvalidInput)?;
    if bytes.len() > devforge_manager::MAX_REQUEST_BYTES {
        return Err(ErrorCode::OutputLimitExceeded);
    }
    let input: devforge_control::mutations::YamlSaveInput =
        serde_json::from_slice(&bytes).map_err(|_| ErrorCode::InvalidInput)?;
    if input.text.as_str().len() > devforge_manager::service_control::MAX_EXTERNAL_YAML_BYTES {
        return Err(ErrorCode::OutputLimitExceeded);
    }
    Ok(input)
}
fn env_live(invocation: &Invocation) -> Result<(Value, i32), ErrorCode> {
    use devforge_control::mutations::{EnvironmentApplyInput, EnvironmentPlanInput};
    use devforge_manager::{
        Endpoint, credential_file::CredentialFile, external_client::BoundEnvClient,
    };
    if invocation.credential_stdin
        || invocation.package.is_some()
        || invocation.php.is_some()
        || invocation.node.is_some()
    {
        return Err(ErrorCode::InvalidInput);
    }
    enum Call {
        Preview,
        PlanApply,
        PlanYaml,
        Apply,
        Yaml,
        Lookup(Id),
    }
    let words = invocation
        .words
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let (project, call) = match words.as_slice() {
        ["env", "plan", project] if invocation.project.is_none() => {
            (Id::new(*project)?, Call::Preview)
        }
        ["env", "plan", "apply", project] if invocation.project.is_none() => {
            (Id::new(*project)?, Call::PlanApply)
        }
        ["env", "plan", "yaml-save", project] if invocation.project.is_none() => {
            (Id::new(*project)?, Call::PlanYaml)
        }
        ["env", "apply", project] if invocation.project.is_none() => {
            (Id::new(*project)?, Call::Apply)
        }
        ["env", "yaml-save", project] if invocation.project.is_none() => {
            (Id::new(*project)?, Call::Yaml)
        }
        ["env", "lookup", key] => (
            invocation.project.clone().ok_or(ErrorCode::InvalidInput)?,
            Call::Lookup(Id::new(*key)?),
        ),
        _ => return Err(ErrorCode::InvalidInput),
    };
    let yaml = matches!(call, Call::PlanYaml | Call::Yaml);
    let applying = matches!(call, Call::Apply | Call::Yaml);
    let writing = !matches!(call, Call::Preview | Call::Lookup(_));
    if invocation.input_stdin != yaml
        || yaml && invocation.write_env
        || applying != invocation.review.is_some()
    {
        return Err(ErrorCode::InvalidInput);
    }
    if writing && (invocation.revision.is_none() || invocation.idempotency.is_none()) {
        return Err(ErrorCode::InvalidInput);
    }
    if matches!(call, Call::Lookup(_))
        && (invocation.revision.is_some()
            || invocation.idempotency.is_some()
            || invocation.write_env)
    {
        return Err(ErrorCode::InvalidInput);
    }
    let file = invocation
        .credential_file
        .as_ref()
        .ok_or(ErrorCode::Unauthorized)?;
    let loaded = CredentialFile::load(file)?;
    if loaded.project_id() != &project {
        return Err(ErrorCode::ProjectMismatch);
    }
    let root = devforge_platform::app_data_dir().map_err(|_| ErrorCode::StorageUnavailable)?;
    let endpoint = Endpoint::existing(&root).map_err(|_| ErrorCode::StorageUnavailable)?;
    let mut client = BoundEnvClient::connect(endpoint, loaded)?;
    if let Call::Lookup(key) = call {
        return service_response(client.lookup(invocation.request.clone(), &key)?);
    }
    let input = if yaml {
        MutationInput::YamlSave(env_yaml_input()?)
    } else if matches!(call, Call::Preview) {
        MutationInput::EnvironmentPlan(EnvironmentPlanInput {
            write_env: invocation.write_env,
            limits: None,
        })
    } else {
        MutationInput::EnvironmentApply(EnvironmentApplyInput {
            write_env: invocation.write_env,
            limits: None,
        })
    };
    let request = Request {
        api_version: Default::default(),
        request_id: invocation.request.clone(),
        project_id: project,
        expected_revision: invocation.revision,
        idempotency_key: invocation.idempotency.clone(),
        input,
    };
    if applying {
        service_response(client.apply(
            &request,
            invocation.review.as_ref().expect("validated review"),
        )?)
    } else {
        service_response(client.plan(&request)?)
    }
}

fn runtime_live(invocation: &Invocation) -> Result<(Value, i32), ErrorCode> {
    use devforge_manager::{
        Endpoint, external_auth::ExternalCredential, external_client::RuntimeClient,
    };
    if invocation.project.is_some() {
        return Err(ErrorCode::InvalidInput);
    }
    let words = invocation
        .words
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let (action, project, plan) = match words.as_slice() {
        ["runtime", "plan", action, project] => (*action, *project, true),
        ["runtime", action, project] => (*action, *project, false),
        _ => return Err(ErrorCode::InvalidInput),
    };
    let project = Id::new(project)?;
    let revision = invocation.revision.ok_or(ErrorCode::InvalidInput)?;
    let key = invocation
        .idempotency
        .clone()
        .ok_or(ErrorCode::InvalidInput)?;
    enum Pins {
        Install(Id),
        Select(Id, Id),
    }
    let pins = match action {
        "install" if invocation.php.is_none() && invocation.node.is_none() => {
            Pins::Install(invocation.package.clone().ok_or(ErrorCode::InvalidInput)?)
        }
        "select" if invocation.package.is_none() => Pins::Select(
            invocation.php.clone().ok_or(ErrorCode::InvalidInput)?,
            invocation.node.clone().ok_or(ErrorCode::InvalidInput)?,
        ),
        _ => return Err(ErrorCode::InvalidInput),
    };
    if plan && invocation.review.is_some() || !plan && invocation.review.is_none() {
        return Err(ErrorCode::InvalidInput);
    }
    if !invocation.credential_stdin {
        return Err(ErrorCode::Unauthorized);
    }
    let credential = credential()?;
    let credential =
        ExternalCredential::from_wire(&credential).map_err(|_| ErrorCode::Unauthorized)?;
    let root = devforge_platform::app_data_dir().map_err(|_| ErrorCode::StorageUnavailable)?;
    let endpoint = Endpoint::existing(&root).map_err(|_| ErrorCode::StorageUnavailable)?;
    let mut client = RuntimeClient::connect(endpoint, credential)?;
    let request = match pins {
        Pins::Install(package_id) => Request {
            api_version: Default::default(),
            request_id: invocation.request.clone(),
            project_id: project,
            expected_revision: Some(revision),
            idempotency_key: Some(key),
            input: MutationInput::RuntimeInstall(PackageInput {
                package_id,
                limits: None,
            }),
        },
        Pins::Select(php, node) => client.select_request(
            invocation.request.clone(),
            project,
            revision,
            key,
            (php, node),
            None,
        )?,
    };
    if plan {
        service_response(client.plan(&request)?)
    } else {
        service_response(client.apply(
            &request,
            invocation.review.as_ref().expect("validated review"),
        )?)
    }
}

fn service_live(invocation: &Invocation) -> Result<(Value, i32), ErrorCode> {
    use devforge_manager::{
        Endpoint, external_auth::ExternalCredential, external_client::ServiceClient,
    };
    let words = invocation
        .words
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    enum Call {
        Plan(Request<MutationInput>),
        Apply(Request<MutationInput>, Digest),
        Lookup(Id, Id),
    }
    let call = match words.as_slice() {
        ["operation", "lookup", key]
            if invocation.revision.is_none()
                && invocation.idempotency.is_none()
                && invocation.review.is_none() =>
        {
            Call::Lookup(
                invocation.project.clone().ok_or(ErrorCode::InvalidInput)?,
                Id::new(*key)?,
            )
        }
        _ => {
            if invocation.project.is_some() {
                return Err(ErrorCode::InvalidInput);
            }
            let (action, project, plan) = match words.as_slice() {
                ["service", "plan", action, project] => (*action, *project, true),
                ["service", action, project] => (*action, *project, false),
                _ => return Err(ErrorCode::InvalidInput),
            };
            let input = match action {
                "start" => MutationInput::Start(ServiceActionInput { limits: None }),
                "stop" => MutationInput::Stop(ServiceActionInput { limits: None }),
                "restart" => MutationInput::Restart(ServiceActionInput { limits: None }),
                _ => return Err(ErrorCode::InvalidInput),
            };
            let request = Request {
                api_version: Default::default(),
                request_id: invocation.request.clone(),
                project_id: Id::new(project)?,
                expected_revision: Some(invocation.revision.ok_or(ErrorCode::InvalidInput)?),
                idempotency_key: Some(
                    invocation
                        .idempotency
                        .clone()
                        .ok_or(ErrorCode::InvalidInput)?,
                ),
                input,
            };
            if plan {
                if invocation.review.is_some() {
                    return Err(ErrorCode::InvalidInput);
                }
                Call::Plan(request)
            } else {
                Call::Apply(
                    request,
                    invocation.review.clone().ok_or(ErrorCode::InvalidInput)?,
                )
            }
        }
    };
    if !invocation.credential_stdin {
        return Err(ErrorCode::Unauthorized);
    }
    let credential = credential()?;
    let credential =
        ExternalCredential::from_wire(&credential).map_err(|_| ErrorCode::Unauthorized)?;
    let root = devforge_platform::app_data_dir().map_err(|_| ErrorCode::StorageUnavailable)?;
    let endpoint = Endpoint::existing(&root).map_err(|_| ErrorCode::StorageUnavailable)?;
    let mut client = ServiceClient::connect(endpoint, credential)?;
    match call {
        Call::Plan(request) => service_response(client.plan(&request)?),
        Call::Apply(request, review) => service_response(client.apply(&request, &review)?),
        Call::Lookup(project, key) => {
            service_response(client.lookup(invocation.request.clone(), &project, &key)?)
        }
    }
}
fn service_response<T: serde::Serialize>(reply: Response<T>) -> Result<(Value, i32), ErrorCode> {
    let code = reply
        .error()
        .map(|fault| exit_code(fault.code))
        .unwrap_or(0);
    Ok((
        serde_json::to_value(reply).map_err(|_| ErrorCode::InvalidInput)?,
        code,
    ))
}

// Restricted transport only: no NativeClient, owner launch or legacy Workspace.
fn live(invocation: &Invocation) -> Result<(Value, i32), ErrorCode> {
    use devforge_manager::{
        Endpoint, external_auth::ExternalCredential, external_client::ExternalClient,
        external_control::ExternalInput,
    };
    let words = invocation
        .words
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    if words.first() == Some(&"env") {
        return env_live(invocation);
    }
    if words.first() != Some(&"env")
        && (invocation.credential_file.is_some() || invocation.input_stdin || invocation.write_env)
    {
        return Err(ErrorCode::InvalidInput);
    }
    if words.first() == Some(&"runtime") {
        return runtime_live(invocation);
    }
    if invocation.package.is_some() || invocation.php.is_some() || invocation.node.is_some() {
        return Err(ErrorCode::InvalidInput);
    }
    if words.first() == Some(&"service") || matches!(words.as_slice(), ["operation", "lookup", _]) {
        return service_live(invocation);
    }
    if invocation.revision.is_some()
        || invocation.idempotency.is_some()
        || invocation.review.is_some()
    {
        return Err(ErrorCode::InvalidInput);
    }
    if words == ["health"] {
        if invocation.project.is_some() || invocation.credential_stdin {
            return Err(ErrorCode::InvalidInput);
        }
        let root = devforge_platform::app_data_dir().map_err(|_| ErrorCode::StorageUnavailable)?;
        let endpoint = Endpoint::existing(&root).map_err(|_| ErrorCode::StorageUnavailable)?;
        let health =
            ExternalClient::observe_owner(&endpoint).map_err(|error| error.control_code())?;
        return Ok((
            serde_json::to_value(Response::success(
                invocation.request.clone(),
                None,
                json!({"owner":health}),
            ))
            .expect("health envelope"),
            0,
        ));
    }
    let input = match words.as_slice() {
        ["status"] if invocation.project.is_none() => ExternalInput::Status {},
        ["project", "list"] if invocation.project.is_none() => ExternalInput::ProjectList {},
        ["project", "inspect", project] if invocation.project.is_none() => {
            ExternalInput::ProjectGet {
                project_id: Id::new(*project)?,
            }
        }
        ["operation", action, operation] => {
            let project_id = invocation.project.clone().ok_or(ErrorCode::InvalidInput)?;
            let operation_id = Id::new(*operation)?;
            match *action {
                "status" => ExternalInput::OperationStatus {
                    project_id,
                    operation_id,
                },
                "cancel" => ExternalInput::OperationCancel {
                    project_id,
                    operation_id,
                },
                _ => return Err(ErrorCode::InvalidInput),
            }
        }
        _ => return Err(ErrorCode::InvalidInput),
    };
    if !invocation.credential_stdin {
        return Err(ErrorCode::Unauthorized);
    }
    let credential = credential()?;
    let credential =
        ExternalCredential::from_wire(&credential).map_err(|_| ErrorCode::Unauthorized)?;
    let root = devforge_platform::app_data_dir().map_err(|_| ErrorCode::StorageUnavailable)?;
    let endpoint = Endpoint::existing(&root).map_err(|_| ErrorCode::StorageUnavailable)?;
    let mut client =
        ExternalClient::connect(endpoint, credential).map_err(|error| error.control_code())?;
    let reply = client
        .request(invocation.request.clone(), input, None)
        .map_err(|error| error.control_code())?;
    let code = reply
        .error
        .as_ref()
        .map(|fault| exit_code(fault.code))
        .unwrap_or(0);
    Ok((
        serde_json::to_value(reply).expect("external envelope"),
        code,
    ))
}

/// Legacy help alone is intercepted before old parsers acquire leases or open
/// stores. Execution still follows the unchanged legacy dispatcher.
pub fn legacy_help(args: &[String]) -> Option<Value> {
    let boundary = args
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(args.len());
    // project exec PROJECT TOOL forwards everything after TOOL, even when its
    // optional `--` separator is omitted. Those flags belong to the tool.
    let boundary = if args.first().is_some_and(|arg| arg == "project")
        && args.get(1).is_some_and(|arg| arg == "exec")
    {
        boundary.min(4)
    } else {
        boundary
    };
    if !args[..boundary]
        .iter()
        .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
    {
        return None;
    }
    let top = args.first()?.as_str();
    let (commands, example) = match top {
        "project" => (
            "inspect import list configure forget env-set db-init db-list db-serve db-provision db-check env-plan env-apply exec shell terminal doctor",
            "devforge project list",
        ),
        "runtime" => ("catalog install remove probe", "devforge runtime catalog"),
        "site" => (
            "list configure remove authority system-plan serve",
            "devforge site list",
        ),
        "inspect" => ("<project-id>", "devforge inspect PROJECT_ID"),
        "debug" => (
            "status profile source install mode",
            "devforge debug status PROJECT_ID",
        ),
        "config" => (
            "init export plan status secret setup",
            "devforge config status PROJECT_ID",
        ),
        "workspace" => (
            "list plan create restore-plan restore purge-plan purge remote-plan remote-copy cleanup-plan cleanup",
            "devforge workspace list",
        ),
        "distribution" => (
            "prerequisites check-stopped check-uninstall cleanup-plan",
            "devforge distribution prerequisites",
        ),
        "doctor" => ("", "devforge doctor"),
        _ => return None,
    };
    Some(
        json!({"command":top,"commands":commands.split_whitespace().collect::<Vec<_>>(),"example":example,"automation":"devforge api --help","mode":"legacy"}),
    )
}

#[cfg(test)]
mod tests {
    use super::legacy_help;

    #[test]
    fn legacy_help_preserves_execution_tool_arguments() {
        for args in [
            vec!["project", "exec", "project-id", "php", "--", "--help"],
            vec!["project", "exec", "project-id", "php", "--help"],
            vec!["project", "exec", "project-id", "composer", "-h"],
            vec!["project", "exec", "project-id", "npm", "run", "--help"],
            vec!["runtime", "--", "--help"],
        ] {
            let args = args.into_iter().map(str::to_owned).collect::<Vec<_>>();
            assert!(legacy_help(&args).is_none(), "{args:?}");
        }
        for args in [
            vec!["project", "--help"],
            vec!["project", "exec", "--help"],
            vec!["project", "exec", "project-id", "--help"],
        ] {
            let args = args.into_iter().map(str::to_owned).collect::<Vec<_>>();
            assert!(legacy_help(&args).is_some(), "{args:?}");
        }
    }
}
