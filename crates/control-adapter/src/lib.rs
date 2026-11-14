//! Prepared control requests to existing domain calls. No Workspace is opened or executed.
use devforge_control::{
    Digest, ErrorCode, Id, ProjectBinding,
    mutations::{MutationInput, PreparedMutation},
};
use devforge_workspace::{Command, worktrees::Record};
use serde::Serialize;
use sha2::{Digest as _, Sha256};

/// Production SHA-256 provider for canonical request material from the contract crate.
/// The manager computes this itself; a client digest is never used as authority.
pub fn sha256(material: &[u8]) -> Result<Digest, ErrorCode> {
    Digest::new(
        Sha256::digest(material)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}

/// State loaded by a trusted manager from its plan store/owned workspace registry.
/// Never deserialize this context from an external request.
#[derive(Default)]
pub struct BackendContext<'a> {
    review: Option<&'a Digest>,
    workspace: Option<&'a Record>,
}
impl<'a> BackendContext<'a> {
    pub fn reviewed(review: &'a Digest) -> Self {
        Self {
            review: Some(review),
            workspace: None,
        }
    }
    pub fn with_workspace(mut self, record: &'a Record) -> Self {
        self.workspace = Some(record);
        self
    }
}
/// Configuration planning is a read-only domain method, not a Command mutation.
pub enum Dispatch {
    Command(Box<Command>),
    ConfigurationPlan { project_id: String, write_env: bool },
}
fn backend_review<'a>(
    prepared: &PreparedMutation,
    context: &'a BackendContext<'a>,
) -> Result<&'a str, ErrorCode> {
    let reviewed = prepared.approved_review().ok_or(ErrorCode::Unauthorized)?;
    let current = context.review.ok_or(ErrorCode::Unauthorized)?;
    if reviewed != current {
        return Err(ErrorCode::ReviewMismatch);
    }
    Ok(current.as_str())
}
fn workspace_id(
    prepared: &PreparedMutation,
    requested: &Id,
    context: &BackendContext<'_>,
) -> Result<String, ErrorCode> {
    let record = context.workspace.ok_or(ErrorCode::Unauthorized)?;
    if record.id != requested.as_str()
        || record.input.source_id != prepared.binding().project_id().as_str()
    {
        return Err(ErrorCode::ProjectMismatch);
    }
    Ok(record.id.clone())
}
/// Requires an already prepared request and *current* registry binding. Manager
/// serialization/revision checks must span this mapping and actual execution.
/// Boolean legacy domain consent is synthesized only from validated policy, never
/// copied from client JSON. No callable endpoint or authorization issuer is added.
pub fn map_mutation(
    prepared: &PreparedMutation,
    current: &ProjectBinding,
    context: &BackendContext<'_>,
) -> Result<Dispatch, ErrorCode> {
    prepared
        .request()
        .validate(current, prepared.descriptor().mutates)?;
    if prepared.binding().root() != current.root() {
        return Err(ErrorCode::ProjectMismatch);
    }
    let id = current.project_id().as_str().to_owned();
    let command = match &prepared.request().input {
        MutationInput::Start(_) => Command::Start { id },
        MutationInput::Stop(_) => Command::Stop { id },
        MutationInput::Restart(_) => Command::Restart { id },
        MutationInput::RuntimeInstall(input) => Command::Install {
            package: input.package_id.as_str().into(),
        },
        MutationInput::RuntimeRemove(input) => {
            backend_review(prepared, context)?;
            Command::RemoveRuntime {
                package: input.package_id.as_str().into(),
            }
        }
        MutationInput::Configure(input) => {
            // Core Configure may stop an active session; reject invalid names before
            // constructing that side-effectful command, using the domain validator.
            devforge_projects::project::validate_name(input.name.as_str())
                .map_err(|_| ErrorCode::InvalidInput)?;
            Command::Configure {
                id,
                revision: prepared
                    .request()
                    .expected_revision
                    .ok_or(ErrorCode::InvalidInput)?,
                name: input.name.as_str().into(),
                php: input.php.as_str().into(),
                node: input.node.as_str().into(),
                domain: input.domain.as_str().into(),
                https: input.https,
                vite: input.vite,
                queue: input.queue,
            }
        }
        MutationInput::EnvironmentPlan(input) => {
            return Ok(Dispatch::ConfigurationPlan {
                project_id: id,
                write_env: input.write_env,
            });
        }
        MutationInput::EnvironmentApply(input) => Command::ConfigurationApply {
            id,
            review: backend_review(prepared, context)?.into(),
            trusted: true,
            write_env: input.write_env,
        },
        MutationInput::YamlSave(input) => {
            backend_review(prepared, context)?;
            Command::ConfigurationSave {
                id,
                expected: input.expected.as_ref().map(|v| v.as_str().into()),
                text: input.text.as_str().into(),
                consent: true,
            }
        }
        MutationInput::DatabaseBind(input) => {
            backend_review(prepared, context)?;
            Command::SwitchDatabase {
                id,
                instance: input.instance_id.as_str().into(),
                consent: true,
            }
        }
        MutationInput::DatabaseRestore(input) => {
            backend_review(prepared, context)?;
            Command::RestoreBackup {
                id,
                backup: input.backup_id.as_str().into(),
                consent: true,
            }
        }
        MutationInput::WorkspaceArchive(input) => Command::WorktreeCleanup {
            id: workspace_id(prepared, &input.workspace_id, context)?,
            review: backend_review(prepared, context)?.into(),
            consent: true,
        },
        MutationInput::WorkspaceRestore(input) => Command::WorktreeArchive {
            id: workspace_id(prepared, &input.workspace_id, context)?,
            operation: "restore".into(),
            review: backend_review(prepared, context)?.into(),
            consent: true,
        },
        MutationInput::WorkspacePurge(input) => Command::WorktreeArchive {
            id: workspace_id(prepared, &input.workspace_id, context)?,
            operation: "purge".into(),
            review: backend_review(prepared, context)?.into(),
            consent: true,
        },
    };
    Ok(Dispatch::Command(Box::new(command)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DomainScope {
    Project,
    Runtime,
    Serving,
    Workspace,
    Debugger,
}
#[derive(Debug, Serialize)]
pub struct DomainFailure {
    pub scope: DomainScope,
    pub code: String,
    pub project_id: Id,
    pub retryable: bool,
}
/// Keeps stable domain codes, not Display/debug text, paths or foreign IDs.
/// Unknown scope/malformed code becomes a generic control storage failure.
pub fn map_failure(
    error: &devforge_workspace::Error,
    binding: &ProjectBinding,
) -> Result<DomainFailure, ErrorCode> {
    let scope = match error.scope {
        "project" => DomainScope::Project,
        "runtime" => DomainScope::Runtime,
        "serving" => DomainScope::Serving,
        "workspace" => DomainScope::Workspace,
        "debugger" => DomainScope::Debugger,
        _ => return Err(ErrorCode::StorageUnavailable),
    };
    if error
        .project_id
        .as_deref()
        .is_some_and(|id| id != binding.project_id().as_str())
    {
        return Err(ErrorCode::ProjectMismatch);
    }
    if error.code.is_empty()
        || error.code.len() > 128
        || !error
            .code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err(ErrorCode::StorageUnavailable);
    }
    Ok(DomainFailure {
        scope,
        code: error.code.clone(),
        project_id: binding.project_id().clone(),
        retryable: matches!(
            error.code.as_str(),
            "busy"
                | "storage_unavailable"
                | "manifest_busy"
                | "download_failed"
                | "connection_failed"
        ),
    })
}
