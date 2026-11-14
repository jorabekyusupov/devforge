//! External environment metadata and private actor query. Core configuration
//! hashes/results stay in manager RAM, never this public projection.
use devforge_control::{
    Digest, ErrorCode, Id, ProjectBinding, limits::SafeInt, mutations::PlanReceipt,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
    time::{Duration, Instant},
};
use zeroize::Zeroizing;
pub const MAX_EXTERNAL_YAML_BYTES: usize = 16_384;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvDocumentState {
    pub project_id: Id,
    pub revision: SafeInt,
    pub yaml_exists: bool,
    pub version: Option<Digest>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvPreview {
    pub document: EnvDocumentState,
    pub receipt: Option<PlanReceipt>,
    pub write_env: bool,
    pub yaml_bytes: Option<SafeInt>,
    pub package_ids: Vec<Id>,
    pub download_ids: Vec<Id>,
    pub blockers: Vec<Id>,
}
pub(crate) struct PrivateEnvironment {
    pub binding: devforge_control::ProjectBinding,
    pub exists: bool,
    pub document_sha: Option<String>,
    pub plan: Option<Box<devforge_workspace::repro::Plan>>,
    pub plan_error: Option<String>,
}
pub(crate) struct EnvironmentIntent {
    pub data: PrivateEnvironment,
    pub expected_sha: Option<Zeroizing<String>>,
}
struct Version {
    token: Digest,
    binding: ProjectBinding,
    client: Id,
    grant: Id,
    sha: Zeroizing<String>,
    expires: Instant,
    bytes: usize,
}
#[derive(Default)]
pub(crate) struct Versions {
    entries: VecDeque<Version>,
}
pub(crate) fn opaque() -> Result<Digest, ErrorCode> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| ErrorCode::StorageUnavailable)?;
    devforge_control_adapter::sha256(&bytes)
}
impl Versions {
    pub(crate) fn record(
        &mut self,
        data: &PrivateEnvironment,
        client: &Id,
        grant: &Id,
    ) -> Result<EnvDocumentState, ErrorCode> {
        self.entries.retain(|entry| Instant::now() < entry.expires);
        let version = if let Some(sha) = data.document_sha.as_ref() {
            if let Some(existing) = self.entries.iter().find(|entry| {
                &entry.client == client
                    && &entry.grant == grant
                    && entry.binding.project_id() == data.binding.project_id()
                    && entry.binding.root() == data.binding.root()
                    && entry.binding.revision() == data.binding.revision()
                    && entry.sha.as_str() == sha
            }) {
                Some(existing.token.clone())
            } else {
                let bytes = data
                    .binding
                    .root()
                    .as_os_str()
                    .as_encoded_bytes()
                    .len()
                    .saturating_mul(4)
                    .saturating_add(1024);
                if self.entries.len() >= 128
                    || bytes
                        > 1_048_576usize.saturating_sub(self.entries.iter().map(|e| e.bytes).sum())
                {
                    return Err(ErrorCode::Busy);
                }
                let token = opaque()?;
                self.entries.push_back(Version {
                    token: token.clone(),
                    binding: data.binding.clone(),
                    client: client.clone(),
                    grant: grant.clone(),
                    sha: Zeroizing::new(sha.clone()),
                    expires: Instant::now() + Duration::from_secs(300),
                    bytes,
                });
                Some(token)
            }
        } else {
            None
        };
        Ok(EnvDocumentState {
            project_id: data.binding.project_id().clone(),
            revision: SafeInt::new(data.binding.revision())?,
            yaml_exists: data.exists,
            version,
        })
    }
    pub(crate) fn resolve(
        &mut self,
        token: Option<&Digest>,
        data: &PrivateEnvironment,
        client: &Id,
        grant: &Id,
    ) -> Result<Option<Zeroizing<String>>, ErrorCode> {
        self.entries.retain(|entry| Instant::now() < entry.expires);
        let Some(token) = token else {
            return if data.exists {
                Err(ErrorCode::ReviewMismatch)
            } else {
                Ok(None)
            };
        };
        let entry = self
            .entries
            .iter()
            .find(|entry| &entry.token == token)
            .ok_or(ErrorCode::ReviewExpired)?;
        if &entry.client != client
            || &entry.grant != grant
            || entry.binding.project_id() != data.binding.project_id()
            || entry.binding.root() != data.binding.root()
            || entry.binding.revision() != data.binding.revision()
            || data.document_sha.as_deref() != Some(entry.sha.as_str())
        {
            return Err(ErrorCode::ReviewMismatch);
        }
        Ok(Some(Zeroizing::new(entry.sha.to_string())))
    }
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }
}
type EnvironmentCheckpoint =
    Box<dyn Fn(&devforge_workspace::Workspace) -> devforge_workspace::Result<()> + Send>;
pub(crate) struct EnvironmentRead {
    pub project: Id,
    pub write_env: Option<bool>,
    pub check: EnvironmentCheckpoint,
    pub cancelled: Arc<AtomicBool>,
    pub reply: SyncSender<devforge_workspace::Result<PrivateEnvironment>>,
}
impl EnvironmentRead {
    pub(crate) fn execute(self, workspace: &devforge_workspace::Workspace) {
        let run = || {
            let check = || {
                if self.cancelled.load(Ordering::Acquire) {
                    return Err(devforge_workspace::Error {
                        scope: "workspace",
                        code: "cancelled".into(),
                        project_id: None,
                    });
                }
                (self.check)(workspace)
            };
            check()?;
            let snapshot = workspace.snapshot()?;
            let row = snapshot
                .projects
                .iter()
                .find(|row| row.project.id == self.project.as_str())
                .ok_or_else(|| devforge_workspace::Error {
                    scope: "workspace",
                    code: "project_mismatch".into(),
                    project_id: None,
                })?;
            let binding = devforge_control::ProjectBinding::from_registered(
                self.project.clone(),
                &row.project.path,
                row.project.revision,
            )
            .map_err(|_| devforge_workspace::Error {
                scope: "workspace",
                code: "project_mismatch".into(),
                project_id: None,
            })?;
            let (exists, document_sha) =
                workspace.environment_document_version(self.project.as_str())?;
            let (plan, plan_error) = if let Some(write_env) = self.write_env {
                match workspace.environment_plan_guarded(self.project.as_str(), write_env, &check) {
                    Ok(plan) => (Some(Box::new(plan)), None),
                    Err(error) => {
                        check()?;
                        (None, Some(error.code))
                    }
                }
            } else {
                (None, None)
            };
            check()?;
            Ok(PrivateEnvironment {
                binding,
                exists,
                document_sha,
                plan,
                plan_error,
            })
        };
        let _ = self.reply.send(run());
    }
}
