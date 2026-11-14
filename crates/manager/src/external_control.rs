//! The M13.2 external-grant surface. No native sessions, command admission,
//! content exports, grant issuance or supervisor construction are accepted here.
use crate::{
    Owner, PeerIdentity,
    actor::{ActorError, ActorHandle, CancelOutcome},
    auth::DesktopAuthority,
    external_auth::{ExternalCredential, ExternalRegistry, GrantRecord, IssuedGrant},
};
use devforge_control::{
    ErrorCode, Fault, Id, Origin, ProjectBinding, Scope, limits::SafeInt, tools::*,
};
use serde::{Deserialize, Serialize};
use std::{sync::Mutex, time::Duration};
use zeroize::Zeroizing;

pub const LIVE_TOOLS: [&str; 6] = [
    "health.health",
    "services.status",
    "projects.list",
    "projects.get",
    "operations.status",
    "operations.cancel",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tool", content = "arguments", deny_unknown_fields)]
pub enum ExternalInput {
    #[serde(rename = "health.health")]
    Health {},
    #[serde(rename = "services.status")]
    Status {},
    #[serde(rename = "projects.list")]
    ProjectList {},
    #[serde(rename = "projects.get")]
    ProjectGet { project_id: Id },
    #[serde(rename = "operations.status")]
    OperationStatus { project_id: Id, operation_id: Id },
    #[serde(rename = "operations.cancel")]
    OperationCancel { project_id: Id, operation_id: Id },
}
impl ExternalInput {
    pub fn tool(&self) -> &'static str {
        match self {
            Self::Health {} => LIVE_TOOLS[0],
            Self::Status {} => LIVE_TOOLS[1],
            Self::ProjectList {} => LIVE_TOOLS[2],
            Self::ProjectGet { .. } => LIVE_TOOLS[3],
            Self::OperationStatus { .. } => LIVE_TOOLS[4],
            Self::OperationCancel { .. } => LIVE_TOOLS[5],
        }
    }
    fn project(&self) -> Option<&Id> {
        match self {
            Self::ProjectGet { project_id }
            | Self::OperationStatus { project_id, .. }
            | Self::OperationCancel { project_id, .. } => Some(project_id),
            _ => None,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CancelOutput {
    pub operation_id: Id,
    pub cancellation: CancellationState,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancellationState {
    Requested,
    AlreadyTerminal,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tool", content = "data", deny_unknown_fields)]
pub enum ExternalOutput {
    #[serde(rename = "health.health")]
    Health(HealthOutput),
    #[serde(rename = "services.status")]
    Status(ServiceOutput),
    #[serde(rename = "projects.list")]
    ProjectList(ProjectOutput),
    #[serde(rename = "projects.get")]
    ProjectGet(ProjectOutput),
    #[serde(rename = "operations.status")]
    OperationStatus(OperationOutput),
    #[serde(rename = "operations.cancel")]
    OperationCancel(CancelOutput),
}
impl ExternalOutput {
    pub fn tool(&self) -> &'static str {
        match self {
            Self::Health(_) => LIVE_TOOLS[0],
            Self::Status(_) => LIVE_TOOLS[1],
            Self::ProjectList(_) => LIVE_TOOLS[2],
            Self::ProjectGet(_) => LIVE_TOOLS[3],
            Self::OperationStatus(_) => LIVE_TOOLS[4],
            Self::OperationCancel(_) => LIVE_TOOLS[5],
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalReply {
    pub api_version: u16,
    pub request_id: Id,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<Id>,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<ExternalOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Fault>,
    pub warnings: Vec<ErrorCode>,
}
impl ExternalReply {
    fn result(request_id: Id, result: Result<ExternalOutput, ErrorCode>) -> Self {
        match result {
            Ok(data) => Self {
                api_version: 1,
                request_id,
                operation_id: None,
                ok: true,
                data: Some(data),
                error: None,
                warnings: vec![],
            },
            Err(code) => Self {
                api_version: 1,
                request_id,
                operation_id: None,
                ok: false,
                data: None,
                error: Some(Fault {
                    code,
                    retryable: matches!(code, ErrorCode::Busy | ErrorCode::StorageUnavailable),
                }),
                warnings: vec![],
            },
        }
    }
}
pub(crate) fn invalid_reply(wire: &serde_json::Value) -> ExternalReply {
    let request = wire.get("request");
    let id = request
        .and_then(|r| r.get("request_id"))
        .and_then(serde_json::Value::as_str)
        .and_then(|id| Id::new(id).ok())
        .unwrap_or_else(|| Id::new("invalid-request").expect("fixed id"));
    let code = if request
        .and_then(|r| r.get("api_version"))
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|v| v != 1)
    {
        ErrorCode::UnsupportedVersion
    } else if request
        .and_then(|r| r.get("input"))
        .and_then(|r| r.get("tool"))
        .and_then(serde_json::Value::as_str)
        .is_some_and(|tool| !LIVE_TOOLS.contains(&tool))
    {
        ErrorCode::UnsupportedCapability
    } else {
        ErrorCode::InvalidInput
    };
    ExternalReply::result(id, Err(code))
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExternalRequest {
    pub api_version: u16,
    pub request_id: Id,
    pub expected_revision: Option<u64>,
    pub input: ExternalInput,
}
struct Token(Zeroizing<String>);
impl<'de> Deserialize<'de> for Token {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        let value = Zeroizing::new(String::deserialize(decoder)?);
        if value.len() != 64 {
            return Err(serde::de::Error::custom("invalid_credential"));
        }
        Ok(Self(value))
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireRequest {
    method: String,
    credential: Token,
    request: ExternalRequest,
}

pub(crate) struct Server {
    pub(crate) registry: Mutex<Option<ExternalRegistry>>,
}
impl Server {
    pub(crate) fn new(owner: &Owner) -> crate::external_auth::Result<Self> {
        Ok(Self {
            registry: Mutex::new(Some(ExternalRegistry::open(owner)?)),
        })
    }
    pub(crate) fn close(&self) {
        self.registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }
    pub(crate) fn issue(
        &self,
        authority: &DesktopAuthority<'_>,
        actor: &ActorHandle,
        project: &Id,
        scopes: &[Scope],
        ttl: Duration,
    ) -> Result<IssuedGrant, ErrorCode> {
        if !scopes.contains(&Scope::Observer)
            || (scopes.contains(&Scope::ExecutionTest) && !scopes.contains(&Scope::ProjectManage))
            || scopes.iter().any(|s| {
                !matches!(
                    s,
                    Scope::Observer | Scope::ProjectManage | Scope::ExecutionTest
                )
            })
        {
            return Err(ErrorCode::InvalidInput);
        }
        let view = actor.external_snapshot().map_err(actor_error)?;
        let trusted_binding = binding(&view, project)?;
        let mut lock = self.registry.try_lock().map_err(|_| ErrorCode::Busy)?;
        let registry = lock.as_mut().ok_or(ErrorCode::Interrupted)?;
        let issued = registry
            .issue(authority, &trusted_binding, scopes, &[], ttl)
            .map_err(grant_error)?;
        // SQLite may wait. Verify the registered filesystem identity again
        // before privately delivering a credential; never rebind a replaced
        // directory during that wait. A failed check delivers no bearer.
        if binding(&view, project).is_err() {
            let _ = registry.revoke(authority, &issued.record.grant_id);
            return Err(ErrorCode::ProjectMismatch);
        }
        Ok(issued)
    }
    pub(crate) fn list(
        &self,
        authority: &DesktopAuthority<'_>,
    ) -> Result<Vec<GrantRecord>, ErrorCode> {
        self.registry
            .try_lock()
            .map_err(|_| ErrorCode::Busy)?
            .as_mut()
            .ok_or(ErrorCode::Interrupted)?
            .list(authority)
            .map_err(grant_error)
    }
    pub(crate) fn revoke(
        &self,
        authority: &DesktopAuthority<'_>,
        grant: &Id,
    ) -> Result<(), ErrorCode> {
        self.registry
            .try_lock()
            .map_err(|_| ErrorCode::Busy)?
            .as_mut()
            .ok_or(ErrorCode::Interrupted)?
            .revoke(authority, grant)
            .map_err(grant_error)
    }
    pub(crate) fn request(
        &self,
        peer: PeerIdentity,
        wire: WireRequest,
        actor: &ActorHandle,
        quitting: bool,
    ) -> ExternalReply {
        let id = wire.request.request_id.clone();
        if quitting {
            return ExternalReply::result(id, Err(ErrorCode::Interrupted));
        }
        ExternalReply::result(id, self.dispatch(&peer, wire, actor))
    }
    fn dispatch(
        &self,
        peer: &PeerIdentity,
        wire: WireRequest,
        actor: &ActorHandle,
    ) -> Result<ExternalOutput, ErrorCode> {
        if wire.method != "external" {
            return Err(ErrorCode::InvalidInput);
        }
        if wire.request.api_version != 1 {
            return Err(ErrorCode::UnsupportedVersion);
        }
        let credential = ExternalCredential::from_wire(&wire.credential.0).map_err(grant_error)?;
        let mut lock = self.registry.try_lock().map_err(|_| ErrorCode::Busy)?;
        let registry = lock.as_mut().ok_or(ErrorCode::Interrupted)?;
        let project = registry
            .credential_project(peer, &credential)
            .map_err(grant_error)?;
        let view = actor.external_snapshot().map_err(actor_error)?;
        let binding = binding(&view, &project)?;
        let required = if matches!(wire.request.input, ExternalInput::OperationCancel { .. }) {
            &[Scope::Observer, Scope::ProjectManage][..]
        } else {
            &[Scope::Observer][..]
        };
        let proof = registry
            .authorize(
                peer,
                &credential,
                &binding,
                required,
                &[],
                wire.request.expected_revision,
            )
            .map_err(grant_error)?;
        if wire
            .request
            .input
            .project()
            .is_some_and(|id| id != &project)
        {
            return Err(ErrorCode::ProjectMismatch);
        }
        let row = view
            .snapshot
            .projects
            .iter()
            .find(|r| r.project.id == project.as_str())
            .ok_or(ErrorCode::ProjectMismatch)?;
        let freshness = || -> Result<ProjectionFreshness, ErrorCode> {
            Ok(ProjectionFreshness {
                origin: Origin::Cached,
                observed_at: SafeInt::new(view.observed_at)?,
                revision: SafeInt::new(binding.revision())?,
            })
        };
        let output = match wire.request.input {
            ExternalInput::Health {} => ExternalOutput::Health(HealthOutput {
                freshness: freshness()?,
                items: vec![HealthItem {
                    code: Id::new(if view.error.is_some() || view.stopped {
                        "metadata_unavailable"
                    } else {
                        "metadata_cached"
                    })?,
                    retryable: view.error.is_some(),
                    next_action: None,
                }],
                next_cursor: None,
            }),
            ExternalInput::Status {} => {
                if row.processes.len() > 128 {
                    return Err(ErrorCode::OutputLimitExceeded);
                }
                let items = row
                    .processes
                    .iter()
                    .map(|(id, state)| {
                        Ok(ServiceItem {
                            service_id: Id::new(id.clone())?,
                            state: match state.observed {
                                devforge_runtime::process::Observed::Starting => {
                                    ServiceState::Starting
                                }
                                devforge_runtime::process::Observed::Ready => ServiceState::Ready,
                                devforge_runtime::process::Observed::Backoff => {
                                    ServiceState::Backoff
                                }
                                devforge_runtime::process::Observed::Stopping => {
                                    ServiceState::Stopping
                                }
                                devforge_runtime::process::Observed::Stopped => {
                                    ServiceState::Stopped
                                }
                                devforge_runtime::process::Observed::Succeeded => {
                                    ServiceState::Succeeded
                                }
                                devforge_runtime::process::Observed::Failed => ServiceState::Failed,
                            },
                            ready: Some(matches!(
                                state.observed,
                                devforge_runtime::process::Observed::Ready
                            )),
                        })
                    })
                    .collect::<Result<Vec<_>, ErrorCode>>()?;
                ExternalOutput::Status(ServiceOutput {
                    freshness: freshness()?,
                    items,
                    next_cursor: None,
                })
            }
            ExternalInput::ProjectList {} | ExternalInput::ProjectGet { .. } => {
                let item = ProjectItem {
                    project_id: project.clone(),
                    name: devforge_control::limits::Text::new(row.project.name.clone())?,
                    php: Id::new(row.project.php.clone())?,
                    node: Id::new(row.project.node.clone())?,
                    composer: Id::new(row.project.composer.clone())?,
                    revision: SafeInt::new(row.project.revision)?,
                };
                let value = ProjectOutput {
                    freshness: freshness()?,
                    items: vec![item],
                    next_cursor: None,
                };
                if matches!(wire.request.input, ExternalInput::ProjectList {}) {
                    ExternalOutput::ProjectList(value)
                } else {
                    ExternalOutput::ProjectGet(value)
                }
            }
            ExternalInput::OperationStatus { operation_id, .. } => {
                let receipt = actor
                    .external_status(proof.client_id(), &project, &operation_id)
                    .map_err(actor_error)?;
                ExternalOutput::OperationStatus(OperationOutput {
                    freshness: ProjectionFreshness {
                        origin: Origin::Persisted,
                        observed_at: SafeInt::new(
                            std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .map_err(|_| ErrorCode::StorageUnavailable)?
                                .as_millis()
                                .try_into()
                                .map_err(|_| ErrorCode::StorageUnavailable)?,
                        )?,
                        revision: SafeInt::new(binding.revision())?,
                    },
                    items: vec![OperationItem {
                        operation_id,
                        state: receipt.state,
                        cancel_supported: !receipt.state.terminal(),
                    }],
                    next_cursor: None,
                })
            }
            ExternalInput::OperationCancel { operation_id, .. } => {
                let outcome = actor
                    .external_cancel(proof.client_id(), &project, &operation_id, || {
                        registry
                            .authorize(
                                peer,
                                &credential,
                                &binding,
                                required,
                                &[],
                                wire.request.expected_revision,
                            )
                            .map(|_| ())
                            .map_err(|_| ActorError::Unauthorized)
                    })
                    .map_err(actor_error)?;
                ExternalOutput::OperationCancel(CancelOutput {
                    operation_id,
                    cancellation: match outcome {
                        CancelOutcome::Requested => CancellationState::Requested,
                        CancelOutcome::AlreadyTerminal => CancellationState::AlreadyTerminal,
                    },
                })
            }
        };
        // Reads may wait for the operation ledger. Expiry/revoke checks precede
        // the response as well; cancellation rechecks before its actual bit.
        registry
            .authorize(
                peer,
                &credential,
                &binding,
                required,
                &[],
                wire.request.expected_revision,
            )
            .map_err(grant_error)?;
        Ok(output)
    }
}
pub(crate) fn binding(
    view: &crate::actor::SnapshotView,
    project: &Id,
) -> Result<ProjectBinding, ErrorCode> {
    let row = view
        .snapshot
        .projects
        .iter()
        .find(|r| r.project.id == project.as_str() && r.project.active)
        .ok_or(ErrorCode::ProjectMismatch)?;
    let actual = devforge_projects::discovery::identity(&row.project.path)
        .map_err(|_| ErrorCode::ProjectMismatch)?;
    if row.project.filesystem_identity.as_deref() != Some(actual.as_str()) {
        return Err(ErrorCode::ProjectMismatch);
    }
    ProjectBinding::from_registered(project.clone(), &row.project.path, row.project.revision)
}
pub(crate) fn grant_error(error: crate::external_auth::Error) -> ErrorCode {
    use crate::external_auth::Error as E;
    match error {
        E::Expired => ErrorCode::GrantExpired,
        E::Revoked => ErrorCode::GrantRevoked,
        E::ProjectMismatch => ErrorCode::ProjectMismatch,
        E::RevisionConflict => ErrorCode::RevisionConflict,
        E::InvalidInput => ErrorCode::InvalidInput,
        E::Capacity => ErrorCode::Busy,
        E::Unauthorized => ErrorCode::Unauthorized,
        _ => ErrorCode::StorageUnavailable,
    }
}
fn actor_error(error: ActorError) -> ErrorCode {
    match error {
        ActorError::Busy => ErrorCode::Busy,
        ActorError::OperationNotFound | ActorError::Unauthorized => ErrorCode::Unauthorized,
        ActorError::InvalidInput => ErrorCode::InvalidInput,
        ActorError::ShuttingDown => ErrorCode::Interrupted,
        _ => ErrorCode::StorageUnavailable,
    }
}
