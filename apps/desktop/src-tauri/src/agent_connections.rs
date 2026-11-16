//! Native owner frontdoor. Bearers and native sessions never cross this invoke
//! contract. Paths, project metadata and masked exact-review summaries may.
use crate::{
    native_bridge::{BridgeError, ConnectionState, NativeBridge},
    workspace,
};
use devforge_control::{Digest, Id, Scope};
use devforge_manager::{
    auth::TrustedDesktopAction,
    external_auth::{GrantRecord, IssuedGrant},
    native_client::{NativeClient, Receipt},
    service_control::{PendingReview, ReviewState},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

#[derive(Debug, Serialize)]
pub struct Error {
    code: String,
}
type Result<T> = std::result::Result<T, Error>;
fn error(code: impl Into<String>) -> Error {
    Error { code: code.into() }
}
fn bridge_error(value: BridgeError) -> Error {
    error(value.code())
}
#[derive(Default)]
pub struct State {
    operations: Mutex<Vec<Operation>>,
}
#[derive(Clone, Serialize)]
pub struct Owner {
    state: ConnectionState,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
}
#[derive(Clone, Serialize)]
pub struct Project {
    id: String,
    name: String,
}
#[derive(Clone, Serialize)]
pub struct Grant {
    grant_id: Id,
    client_id: Id,
    project_id: Id,
    scopes: Vec<Scope>,
    created_at: u64,
    expires_at: u64,
    revoked: bool,
    expired: bool,
}
impl From<GrantRecord> for Grant {
    fn from(value: GrantRecord) -> Self {
        Self {
            grant_id: value.grant_id,
            client_id: value.client_id,
            project_id: value.project_id,
            scopes: value.scopes,
            created_at: value.created_at,
            expires_at: value.expires_at,
            revoked: value.revoked,
            expired: value.expired,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionContext {
    environment: bool,
    database: bool,
    cache: bool,
    mail: bool,
    test_isolated: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    Execution {
        tool: devforge_control::mutations::ExecutionTool,
        arg_count: u64,
        managed_test: bool,
        timeout_ms: u64,
        context: ExecutionContext,
        safe_arguments: Vec<String>,
    },
    Service {
        action: String,
    },
    RuntimeInstall {
        package_id: Id,
    },
    RuntimeSelect {
        php: Id,
        node: Id,
    },
    EnvironmentApply {
        write_env: bool,
    },
    YamlSave {
        byte_count: u64,
    },
    DatabaseBind {
        instance_id: Id,
    },
    DatabaseRestore {
        backup_id: Id,
    },
    Domain {
        action: String,
    },
}
#[derive(Clone, Serialize)]
#[serde(untagged)]
pub enum DetailValue {
    Text(String),
    Number(u64),
    Bool(bool),
    Texts(Vec<String>),
}
#[derive(Clone, Serialize)]
pub struct Detail {
    key: String,
    value: DetailValue,
}
#[derive(Clone, Serialize)]
pub struct Plan {
    digest: Digest,
    project_id: Id,
    project_name: String,
    project_root: String,
    grant_id: Id,
    client_id: Id,
    tool: String,
    revision: u64,
    expires_at: u64,
    state: ReviewState,
    effects: Vec<String>,
    required_scopes: Vec<Scope>,
    target: Target,
    revalidation_required: bool,
    machine_confirmation_required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    operation_id: Option<Id>,
    details: Vec<Detail>,
}
#[derive(Clone, Serialize)]
pub struct Operation {
    grant_id: Id,
    receipt: Receipt,
}
#[derive(Serialize)]
pub struct Snapshot {
    owner: Owner,
    projects: Vec<Project>,
    grants: Vec<Grant>,
    plans: Vec<Plan>,
    operations: Vec<Operation>,
}
#[derive(Serialize)]
pub struct Config {
    grant_id: Id,
    credential_path: String,
    config_path: String,
    config_snippet: String,
}
#[derive(Serialize)]
pub struct IssueResult {
    grant: Grant,
    config: Config,
}
#[derive(Serialize)]
pub struct LookupResult {
    grant_id: Id,
    receipt: Option<Receipt>,
    point_in_time: bool,
}
#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    State {},
    Connect {},
    Issue {
        project_id: Id,
        scopes: Vec<Scope>,
        ttl_seconds: u64,
    },
    Revoke {
        grant_id: Id,
    },
    Config {
        grant_id: Id,
    },
    Approve {
        digest: Digest,
    },
    ConfirmMachine {
        digest: Digest,
    },
    Status {
        grant_id: Id,
        operation_id: Id,
    },
    Cancel {
        grant_id: Id,
        operation_id: Id,
    },
    Reconcile {
        grant_id: Id,
        idempotency_key: Id,
    },
}
fn read<T>(
    bridge: &NativeBridge,
    mut action: impl FnMut(&mut NativeClient) -> devforge_manager::native_client::Result<T>,
) -> Result<T> {
    retry_busy(|| bridge.agent_owner_action(|client| action(client))).map_err(bridge_error)
}
fn retry_busy<T>(
    mut action: impl FnMut() -> crate::native_bridge::Result<T>,
) -> crate::native_bridge::Result<T> {
    let start = Instant::now();
    loop {
        match action() {
            Err(BridgeError::Busy) if start.elapsed() < Duration::from_secs(5) => {
                std::thread::sleep(Duration::from_millis(10))
            }
            other => return other,
        }
    }
}
fn compensate_export_failure(
    failure: Error,
    revoke: impl FnMut() -> crate::native_bridge::Result<()>,
    revoked: impl FnMut() -> crate::native_bridge::Result<bool>,
) -> Error {
    // Revoking this fixed grant ID is idempotent. Even a lost revoke reply
    // requires an authoritative metadata check before reporting compensation.
    let _ = retry_busy(revoke);
    if matches!(retry_busy(revoked), Ok(true)) {
        failure
    } else {
        error("export_failed_revoke_unconfirmed")
    }
}
impl State {
    fn track(&self, operation: Operation) -> Result<Operation> {
        let mut list = self
            .operations
            .lock()
            .map_err(|_| error("agent_state_unavailable"))?;
        list.retain(|old| {
            old.grant_id != operation.grant_id
                || old.receipt.operation_id != operation.receipt.operation_id
        });
        if list.len() == 32 {
            list.remove(0);
        }
        list.push(operation.clone());
        Ok(operation)
    }
    fn snapshot(&self, workspace: &workspace::State) -> Result<Snapshot> {
        let bridge = workspace.agent_bridge();
        let view = bridge.connection_view();
        let owner = Owner {
            state: view.state,
            code: view.error.map(|value| value.code().into()),
        };
        if owner.state != ConnectionState::Connected {
            return Ok(Snapshot {
                owner,
                projects: vec![],
                grants: vec![],
                plans: vec![],
                operations: vec![],
            });
        }
        let snapshot = workspace
            .agent_snapshot()
            .map_err(|value| error(value.code))?;
        let projects = snapshot
            .snapshot
            .get("projects")
            .and_then(Value::as_array)
            .ok_or_else(|| error("agent_protocol"))?
            .iter()
            .map(|row| {
                let project = &row["project"];
                Ok(Project {
                    id: project["id"]
                        .as_str()
                        .ok_or_else(|| error("agent_protocol"))?
                        .into(),
                    name: project["name"]
                        .as_str()
                        .ok_or_else(|| error("agent_protocol"))?
                        .into(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let grants = read(&bridge, NativeClient::list_external_grants)?
            .into_iter()
            .map(Grant::from)
            .collect();
        let reviews = read(&bridge, NativeClient::pending_reviews)?;
        let mut plans = vec![];
        for review in reviews {
            if let Some(operation) = &review.operation_id {
                let receipt = read(&bridge, |client| {
                    client.external_operation_status(&review.grant_id, operation)
                })?;
                self.track(Operation {
                    grant_id: review.grant_id.clone(),
                    receipt,
                })?;
            }
            plans.push(plan(review)?);
        }
        let tracked = self
            .operations
            .lock()
            .map_err(|_| error("agent_state_unavailable"))?
            .clone();
        let mut operations = vec![];
        for operation in tracked {
            let receipt = read(&bridge, |client| {
                client
                    .external_operation_status(&operation.grant_id, &operation.receipt.operation_id)
            })?;
            operations.push(self.track(Operation {
                grant_id: operation.grant_id,
                receipt,
            })?);
        }
        Ok(Snapshot {
            owner,
            projects,
            grants,
            plans,
            operations,
        })
    }
    fn dispatch(&self, workspace: &workspace::State, request: Request) -> Result<Value> {
        let bridge = workspace.agent_bridge();
        match request {
            Request::State {} => encoded(self.snapshot(workspace)?),
            Request::Connect {} => {
                workspace
                    .agent_connect()
                    .map_err(|value| error(value.code))?;
                encoded(self.snapshot(workspace)?)
            }
            Request::Issue {
                project_id,
                scopes,
                ttl_seconds,
            } => {
                validate_issue(&scopes, ttl_seconds)?;
                let (root, artifact) = bridge.agent_native_paths();
                let artifact =
                    artifact.ok_or_else(|| error("native_bridge_artifact_unavailable"))?;
                verify_artifact(&artifact)?;
                let issued = bridge
                    .agent_owner_action(|client| {
                        client.issue_external_grant(
                            &project_id,
                            &scopes,
                            Duration::from_secs(ttl_seconds),
                        )
                    })
                    .map_err(bridge_error)?;
                match export_new(&root, &artifact, &issued) {
                    Ok(config) => encoded(IssueResult {
                        grant: issued.record.into(),
                        config,
                    }),
                    Err(failure) => Err(compensate_export_failure(
                        failure,
                        || {
                            bridge.agent_owner_action(|client| {
                                client.revoke_external_grant(&issued.record.grant_id)
                            })
                        },
                        || {
                            bridge.agent_owner_action(|client| {
                                client.list_external_grants().map(|grants| {
                                    grants.iter().any(|grant| {
                                        grant.grant_id == issued.record.grant_id && grant.revoked
                                    })
                                })
                            })
                        },
                    )),
                }
            }
            Request::Revoke { grant_id } => {
                bridge
                    .agent_owner_action(|client| client.revoke_external_grant(&grant_id))
                    .map_err(bridge_error)?;
                encoded(self.snapshot(workspace)?)
            }
            Request::Config { grant_id } => {
                let grants = read(&bridge, NativeClient::list_external_grants)?;
                let grant = grants
                    .into_iter()
                    .find(|grant| grant.grant_id == grant_id && !grant.revoked && !grant.expired)
                    .ok_or_else(|| error("agent_config_unavailable"))?;
                let (root, artifact) = bridge.agent_native_paths();
                let artifact =
                    artifact.ok_or_else(|| error("native_bridge_artifact_unavailable"))?;
                verify_artifact(&artifact)?;
                encoded(existing_config(&root, &artifact, &grant)?)
            }
            Request::Approve { digest } => {
                bridge
                    .agent_owner_action(|client| client.approve_service_plan(&digest))
                    .map_err(bridge_error)?;
                encoded(self.snapshot(workspace)?)
            }
            Request::ConfirmMachine { digest } => {
                let pending = read(&bridge, NativeClient::pending_reviews)?;
                if !pending.iter().any(|plan| {
                    plan.digest == digest
                        && plan.machine_confirmation_required
                        && plan.state == ReviewState::Approved
                }) {
                    return Err(error("agent_machine_confirmation_unavailable"));
                }
                let action = TrustedDesktopAction::from_explicit_desktop_action();
                bridge
                    .agent_owner_action(|client| client.confirm_domain_change(&digest, &action))
                    .map_err(bridge_error)?;
                encoded(self.snapshot(workspace)?)
            }
            Request::Status {
                grant_id,
                operation_id,
            } => {
                let receipt = read(&bridge, |client| {
                    client.external_operation_status(&grant_id, &operation_id)
                })?;
                encoded(self.track(Operation { grant_id, receipt })?)
            }
            Request::Cancel {
                grant_id,
                operation_id,
            } => {
                bridge
                    .agent_owner_action(|client| {
                        client.external_operation_cancel(&grant_id, &operation_id)
                    })
                    .map_err(bridge_error)?;
                let receipt = read(&bridge, |client| {
                    client.external_operation_status(&grant_id, &operation_id)
                })?;
                encoded(self.track(Operation { grant_id, receipt })?)
            }
            Request::Reconcile {
                grant_id,
                idempotency_key,
            } => {
                let receipt = read(&bridge, |client| {
                    client.external_operation_by_key(&grant_id, &idempotency_key)
                })?;
                if let Some(receipt) = &receipt {
                    self.track(Operation {
                        grant_id: grant_id.clone(),
                        receipt: receipt.clone(),
                    })?;
                }
                encoded(LookupResult {
                    grant_id,
                    receipt,
                    point_in_time: true,
                })
            }
        }
    }
}
fn validate_issue(scopes: &[Scope], ttl: u64) -> Result<()> {
    if scopes.is_empty()
        || scopes.len() > 4
        || !scopes.contains(&Scope::Observer)
        || !(60..=2592000).contains(&ttl)
    {
        return Err(error("agent_invalid_grant"));
    }
    for (index, scope) in scopes.iter().enumerate() {
        if scopes[..index].contains(scope) {
            return Err(error("agent_invalid_grant"));
        }
    }
    if scopes
        .iter()
        .any(|scope| matches!(scope, Scope::ExecutionTest | Scope::SensitiveOperation))
        && !scopes.contains(&Scope::ProjectManage)
    {
        return Err(error("agent_invalid_grant"));
    }
    Ok(())
}
fn encoded(value: impl Serialize) -> Result<Value> {
    serde_json::to_value(value).map_err(|_| error("agent_protocol"))
}
fn verify_artifact(artifact: &crate::manager_artifact::BundledManager) -> Result<()> {
    devforge_manager::launch::verify_pinned_executable(
        &artifact.executable,
        &artifact.expected_sha256,
    )
    .map_err(|_| error("native_bridge_artifact_unavailable"))
}
fn export_paths(root: &Path, grant: &Id) -> (PathBuf, PathBuf, PathBuf) {
    let directory = root.join(format!("agent-connection-{}", grant.as_str()));
    (
        directory.clone(),
        directory.join("credential.json"),
        directory.join("mcp.json"),
    )
}
fn config(
    root: &Path,
    artifact: &crate::manager_artifact::BundledManager,
    grant: &GrantRecord,
) -> Result<Config> {
    let (_, credential, file) = export_paths(root, &grant.grant_id);
    let mut args = vec![
        "mcp",
        "serve",
        "--credential-file",
        credential
            .to_str()
            .ok_or_else(|| error("agent_config_unavailable"))?,
    ];
    if grant.scopes.contains(&Scope::ProjectManage) {
        args.extend(["--allow-service-actions", "--allow-runtime-actions"]);
    }
    if grant.scopes.contains(&Scope::ExecutionTest) {
        args.push("--allow-execution-actions");
    }
    if grant.scopes.contains(&Scope::SensitiveOperation) {
        args.extend([
            "--allow-environment-actions",
            "--allow-database-actions",
            "--allow-domain-actions",
        ]);
    }
    let snippet=serde_json::to_string_pretty(&json!({"mcpServers":{format!("devforge-{}",grant.grant_id.as_str()):{"command":artifact.executable,"args":args,"env":{"DEVFORGE_DATA_DIR":root}}}})).map_err(|_|error("agent_config_unavailable"))?;
    Ok(Config {
        grant_id: grant.grant_id.clone(),
        credential_path: credential.to_string_lossy().into_owned(),
        config_path: file.to_string_lossy().into_owned(),
        config_snippet: snippet,
    })
}
fn export_new(
    root: &Path,
    artifact: &crate::manager_artifact::BundledManager,
    issued: &IssuedGrant,
) -> Result<Config> {
    verify_artifact(artifact)?;
    let action = TrustedDesktopAction::from_explicit_desktop_action();
    let (directory, credential, file) = export_paths(root, &issued.record.grant_id);
    devforge_manager::credential_file::create_new_private_export_dir(&directory, &action)
        .map_err(|_| error("agent_config_export_failed"))?;
    let bytes=serde_json::to_vec(&json!({"schema":1,"project_id":issued.record.project_id,"credential":issued.credential.expose_secret()})).map_err(|_|error("agent_config_export_failed"))?;
    // The one-time secret is Rust-owned and written only to this fresh private file.
    let mut bytes = bytes;
    let result = devforge_manager::credential_file::write_new_private_export_file(
        &credential,
        &bytes,
        &action,
    );
    bytes.fill(0);
    result.map_err(|_| error("agent_config_export_failed"))?;
    let config = config(root, artifact, &issued.record)?;
    devforge_manager::credential_file::write_new_private_export_file(
        &file,
        config.config_snippet.as_bytes(),
        &action,
    )
    .map_err(|_| error("agent_config_export_failed"))?;
    Ok(config)
}
fn existing_config(
    root: &Path,
    artifact: &crate::manager_artifact::BundledManager,
    grant: &GrantRecord,
) -> Result<Config> {
    let (_, credential, _) = export_paths(root, &grant.grant_id);
    let loaded = devforge_manager::credential_file::CredentialFile::load(&credential)
        .map_err(|_| error("agent_config_unavailable"))?;
    if loaded.project_id() != &grant.project_id {
        return Err(error("agent_config_unavailable"));
    }
    let mut config = config(root, artifact, grant)?;
    let metadata =
        fs::symlink_metadata(&config.config_path).map_err(|_| error("agent_config_unavailable"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 65536 {
        return Err(error("agent_config_unavailable"));
    }
    let stored =
        fs::read_to_string(&config.config_path).map_err(|_| error("agent_config_unavailable"))?;
    let candidate: Value =
        serde_json::from_str(&stored).map_err(|_| error("agent_config_unavailable"))?;
    let mut expected: Value = serde_json::from_str(&config.config_snippet)
        .map_err(|_| error("agent_config_unavailable"))?;
    let key = format!("devforge-{}", grant.grant_id.as_str());
    let actual_args = candidate["mcpServers"][&key]["args"]
        .as_array()
        .ok_or_else(|| error("agent_config_unavailable"))?;
    let allowed_args = expected["mcpServers"][&key]["args"]
        .as_array()
        .ok_or_else(|| error("agent_config_unavailable"))?;
    // Previously exported immutable configs may select fewer surfaces. Their
    // fixed executable, credential path, root and server key remain exact.
    let mut selected = std::collections::BTreeSet::new();
    if actual_args.len() < 4
        || actual_args[..4] != allowed_args[..4]
        || actual_args[4..].iter().any(|arg| {
            !allowed_args[4..].contains(arg) || arg.as_str().is_none_or(|arg| !selected.insert(arg))
        })
    {
        return Err(error("agent_config_unavailable"));
    }
    expected["mcpServers"][&key]["args"] = Value::Array(actual_args.clone());
    if candidate != expected
        || serde_json::to_string_pretty(&candidate)
            .map_err(|_| error("agent_config_unavailable"))?
            != stored
    {
        return Err(error("agent_config_unavailable"));
    }
    config.config_snippet = stored;
    Ok(config)
}
fn plan(value: PendingReview) -> Result<Plan> {
    let target_value = serde_json::to_value(&value.target).map_err(|_| error("agent_protocol"))?;
    let target =
        serde_json::from_value(target_value.clone()).map_err(|_| error("agent_protocol"))?;
    let details = review_details(&target_value);
    Ok(Plan {
        digest: value.digest,
        project_id: value.project_id,
        project_name: value.project_name,
        project_root: value.project_root,
        grant_id: value.grant_id,
        client_id: value.client_id,
        tool: value.tool,
        revision: value.revision.get(),
        expires_at: value.expires_at.get(),
        state: value.state,
        effects: value.effects,
        required_scopes: value.required_scopes,
        target,
        revalidation_required: value.revalidation_required,
        machine_confirmation_required: value.machine_confirmation_required,
        operation_id: value.operation_id,
        details,
    })
}
fn review_details(target: &Value) -> Vec<Detail> {
    let mut output = vec![];
    let summary = target.get("summary").unwrap_or(&Value::Null);
    for path in [
        "/runtimes/php",
        "/runtimes/node",
        "/runtimes/composer",
        "/domain",
        "/https",
        "/vite",
        "/queue",
        "/database/engine",
        "/database/version",
        "/redis/version",
        "/redis/dotnet",
        "/cache_driver",
        "/queue_driver",
        "/mail",
        "/preparation/composer",
        "/preparation/npm",
        "/preparation/create_env",
        "/preparation/generate_key",
        "/preparation/predis",
        "/environment_keys",
        "/environment_key_count",
        "/pin_count",
        "/php_setting_names",
        "/hosts/added",
        "/hosts/removed",
        "/hosts/retained",
        "/ca_fingerprint",
        "/shared_ca_preserved",
        "/remaining_ms",
    ] {
        if let Some(value) = summary.pointer(path)
            && let Some(value) = detail_value(value)
        {
            output.push(Detail {
                key: path.trim_start_matches('/').replace('/', "."),
                value,
            });
        }
    }
    for key in ["package_ids", "download_ids"] {
        if let Some(value) = target.get(key).and_then(detail_value) {
            output.push(Detail {
                key: key.into(),
                value,
            });
        }
    }
    if target.get("kind").and_then(Value::as_str) == Some("environment_apply") {
        for path in [
            "/database_action",
            "/preparation/composer",
            "/preparation/npm",
            "/preparation/create_env",
            "/preparation/generate_key",
            "/preparation/predis",
        ] {
            if let Some(value) = target.pointer(path).and_then(detail_value) {
                output.push(Detail {
                    key: path.trim_start_matches('/').replace('/', "."),
                    value,
                });
            }
        }
    }
    if let Some(steps) = summary.get("steps").and_then(Value::as_array) {
        for (index, step) in steps.iter().enumerate() {
            for key in ["operation", "target"] {
                if let Some(value) = step.get(key).and_then(detail_value) {
                    output.push(Detail {
                        key: format!("steps.{index}.{key}"),
                        value,
                    });
                }
            }
        }
    }
    output
}
fn detail_value(value: &Value) -> Option<DetailValue> {
    match value {
        Value::String(value) => Some(DetailValue::Text(value.clone())),
        Value::Bool(value) => Some(DetailValue::Bool(*value)),
        Value::Number(value) => value.as_u64().map(DetailValue::Number),
        Value::Array(values) => values
            .iter()
            .map(|value| value.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
            .map(DetailValue::Texts),
        _ => None,
    }
}
#[tauri::command]
pub async fn agent_connections(
    request: Request,
    workspace: tauri::State<'_, workspace::State>,
    state: tauri::State<'_, std::sync::Arc<State>>,
) -> Result<Value> {
    let workspace = workspace.inner().clone();
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.dispatch(&workspace, request))
        .await
        .map_err(|_| error("agent_worker_failed"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_compensation_retries_busy_and_requires_confirmed_revocation() {
        let mut attempts = 0;
        let failure = compensate_export_failure(
            error("export_failed"),
            || {
                attempts += 1;
                if attempts < 3 {
                    Err(BridgeError::Busy)
                } else {
                    Ok(())
                }
            },
            || Ok(true),
        );
        assert_eq!(attempts, 3);
        assert_eq!(failure.code, "export_failed");
        for confirmed in [Ok(false), Err(BridgeError::OwnerUnavailable)] {
            let failure = compensate_export_failure(
                error("export_failed"),
                || Err(BridgeError::OwnerUnavailable),
                || confirmed.clone(),
            );
            assert_eq!(failure.code, "export_failed_revoke_unconfirmed");
        }
    }
    fn id(value: &str) -> Id {
        Id::new(value).unwrap()
    }
    fn artifact(root: &Path) -> crate::manager_artifact::BundledManager {
        let executable = root.join(if cfg!(windows) {
            "fixture-manager.exe"
        } else {
            "fixture-manager"
        });
        fs::write(&executable, b"agent export fixture").unwrap();
        crate::manager_artifact::BundledManager {
            executable,
            target: "fixture",
            expected_sha256: [
                0x63, 0x7e, 0xc1, 0xcf, 0xee, 0xbb, 0x91, 0xe7, 0xa1, 0x45, 0x28, 0x93, 0x16, 0x97,
                0xe2, 0xf6, 0x2d, 0x40, 0x30, 0x15, 0x0b, 0x90, 0x89, 0x14, 0x36, 0x11, 0x07, 0xec,
                0xf6, 0x8f, 0x0a, 0x66,
            ],
        }
    }
    #[test]
    fn fresh_export_is_protected_path_only_and_never_overwrites_or_adopts() {
        let directory = crate::native_bridge::fixture_root::root("df-agent-export-");
        let root = directory.path().canonicalize().unwrap();
        let artifact = artifact(root.as_path());
        let token = "0123456789abcdef".repeat(4);
        let grant = GrantRecord {
            grant_id: id("grant-fixture"),
            client_id: id("client-fixture"),
            project_id: id("project-fixture"),
            scopes: vec![
                Scope::Observer,
                Scope::ProjectManage,
                Scope::ExecutionTest,
                Scope::SensitiveOperation,
            ],
            information: vec![],
            created_at: 1,
            expires_at: 60001,
            revoked: false,
            expired: false,
        };
        let issued = IssuedGrant {
            record: grant.clone(),
            credential: devforge_manager::external_auth::ExternalCredential::from_wire(&token)
                .unwrap(),
        };
        let config = export_new(root.as_path(), &artifact, &issued).unwrap();
        let wire = serde_json::to_string(&config).unwrap();
        assert!(!wire.contains(&token));
        assert!(!config.config_snippet.contains(&token));
        let snippet: Value = serde_json::from_str(&config.config_snippet).unwrap();
        let server = &snippet["mcpServers"]["devforge-grant-fixture"];
        assert_eq!(
            server["command"],
            artifact.executable.to_string_lossy().as_ref()
        );
        assert!(server.get("credential").is_none());
        assert_eq!(server["env"].as_object().unwrap().len(), 1);
        assert_eq!(server["args"][3], config.credential_path);
        let loaded = devforge_manager::credential_file::CredentialFile::load(Path::new(
            &config.credential_path,
        ))
        .unwrap();
        assert_eq!(loaded.project_id(), &grant.project_id);
        let (_, loaded) = loaded.into_parts();
        assert!(loaded.expose_secret() == token);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&config.credential_path)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
            assert_eq!(
                fs::metadata(Path::new(&config.credential_path).parent().unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
        }
        assert!(export_new(root.as_path(), &artifact, &issued).is_err());
        assert!(existing_config(root.as_path(), &artifact, &grant).is_ok());
        let mut legacy: Value = serde_json::from_str(&config.config_snippet).unwrap();
        let args = legacy["mcpServers"]["devforge-grant-fixture"]["args"]
            .as_array_mut()
            .unwrap();
        assert!(args.contains(&json!("--allow-execution-actions")));
        args.retain(|arg| arg != "--allow-execution-actions");
        let legacy = serde_json::to_string_pretty(&legacy).unwrap();
        fs::write(&config.config_path, &legacy).unwrap();
        assert_eq!(
            existing_config(root.as_path(), &artifact, &grant)
                .unwrap()
                .config_snippet,
            legacy
        );
        assert_eq!(fs::read_to_string(&config.config_path).unwrap(), legacy);
        let mut unknown: Value = serde_json::from_str(&legacy).unwrap();
        unknown["mcpServers"]["devforge-grant-fixture"]["args"]
            .as_array_mut()
            .unwrap()
            .push(json!("--allow-unrestricted-shell"));
        fs::write(
            &config.config_path,
            serde_json::to_string_pretty(&unknown).unwrap(),
        )
        .unwrap();
        assert!(existing_config(root.as_path(), &artifact, &grant).is_err());
        fs::write(&config.config_path, b"changed").unwrap();
        assert!(existing_config(root.as_path(), &artifact, &grant).is_err());
        let result = IssueResult {
            grant: grant.into(),
            config,
        };
        assert!(!serde_json::to_string(&result).unwrap().contains(&token));
    }
    #[test]
    fn renderer_contract_and_review_projection_exclude_authority_and_secret_values() {
        for request in [
            json!({"action":"state","session":"never-js"}),
            json!({"action":"connect","role":"desktop"}),
            json!({"action":"issue","project_id":"p","scopes":["observer"],"ttl_seconds":60,"information":["mail_content"]}),
            json!({"action":"confirm_machine","digest":"a".repeat(64),"elevate":true}),
        ] {
            assert!(serde_json::from_value::<Request>(request).is_err());
        }
        assert!(validate_issue(&[Scope::Observer, Scope::SensitiveOperation], 60).is_err());
        assert!(validate_issue(&[Scope::Observer], 2592001).is_err());
        assert!(validate_issue(&[Scope::Observer, Scope::ProjectManage], 60).is_ok());
        let raw = json!({"kind":"yaml_save","byte_count":120,"raw_yaml":"raw-yaml-canary","summary":{"runtimes":{"php":"php-id","node":"node-id","composer":"composer-id"},"environment_keys":["APP_SECRET"],"environment_values":{"APP_SECRET":"secret-value-canary"},"domain":"reviewed.devforge.test","preparation":{"create_env":true}}});
        let target: Target = serde_json::from_value(raw.clone()).unwrap();
        let details = review_details(&raw);
        let safe = serde_json::to_string(&(target, details)).unwrap();
        assert!(safe.contains("APP_SECRET"));
        assert!(!safe.contains("secret-value-canary") && !safe.contains("raw-yaml-canary"));
        let machine = json!({"kind":"domain","action":"hosts","summary":{"steps":[{"operation":"hosts_update","target":"system_hosts"}],"hosts":{"added":["a.devforge.test"],"removed":[],"retained":["b.devforge.test"]},"remaining_ms":0,"shared_ca_preserved":true,"private_key":"native-private-key-canary"}});
        let safe = serde_json::to_string(&review_details(&machine)).unwrap();
        assert!(safe.contains("system_hosts") && safe.contains("b.devforge.test"));
        assert!(!safe.contains("native-private-key-canary"));
        let execution = json!({"kind":"execution","tool":"php","arg_count":2,"managed_test":true,"timeout_ms":1000,"context":{"environment":false,"database":false,"cache":false,"mail":false,"test_isolated":true},"safe_arguments":["[REDACTED]","owned-script.php"],"raw_arguments":["secret-execution-raw-canary"]});
        let target: Target = serde_json::from_value(execution).unwrap();
        let safe = serde_json::to_string(&target).unwrap();
        assert!(safe.contains("owned-script.php") && safe.contains("test_isolated"));
        assert!(!safe.contains("secret-execution-raw-canary"));
    }
}
