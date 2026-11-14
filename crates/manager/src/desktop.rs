//! Typed private desktop facade. No Workspace construction, native credential
//! issuer, GUI binding or generic filesystem/shell endpoint lives here.
use crate::{actor::ROOT_SUBJECT, payload};
use devforge_control::Id;
use devforge_core::{LocaleRegistry, SettingsPatch};
use devforge_workspace::Command;
use devforge_workspace::{LogFilter, Workspace};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::{
    io::Write,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

pub const READ_IMAGE_BYTES: usize = 64 * 1024 * 1024;
pub const READ_TOTAL_BYTES: usize = 128 * 1024 * 1024;
pub const READ_RESULTS: usize = 2;
pub const READ_TTL: Duration = Duration::from_secs(120);

/// Advisory identifiers only. Never serialize a raw command, HTTP payload,
/// query path, YAML or native secret into the subject lookup protocol.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SubjectSelector {
    Root {},
    Project { project_id: Id },
    Worktree { worktree_id: Id },
    Cache { instance_id: Id },
}
impl SubjectSelector {
    pub fn from_command(command: &Command) -> crate::actor::Result<Self> {
        let project = |id: &str| -> crate::actor::Result<Self> {
            let id = Id::new(id).map_err(|_| crate::actor::ActorError::InvalidInput)?;
            if id.as_str() == ROOT_SUBJECT {
                return Err(crate::actor::ActorError::SubjectMismatch);
            }
            Ok(Self::Project { project_id: id })
        };
        match command {
            Command::ConfigurationSave { id, .. }
            | Command::ConfigurationApply { id, .. }
            | Command::DebugInstall { id, .. }
            | Command::DebugConfigure { id, .. }
            | Command::DebugBreakpoints { id, .. }
            | Command::DebugControl { id, .. }
            | Command::DebugRequest { id, .. }
            | Command::DebugHttp { id, .. }
            | Command::DebugProfileDelete { id, .. }
            | Command::DebugProfileExport { id, .. }
            | Command::PrepareProject { id, .. }
            | Command::PhpDiagnostics { id, .. }
            | Command::PhpSettings { id, .. }
            | Command::Favorite { id, .. }
            | Command::Visit { id, .. }
            | Command::HttpsPreview { id, .. }
            | Command::HttpsExport { id, .. }
            | Command::HttpsApply { id, .. }
            | Command::InspectorInstall { id, .. }
            | Command::InspectorCapture { id, .. }
            | Command::InspectorClear { id, .. }
            | Command::InspectorProbe { id, .. }
            | Command::Configure { id, .. }
            | Command::Forget { id, .. }
            | Command::MailConfigure { id, .. }
            | Command::MailDelete { id, .. }
            | Command::MailSave { id, .. }
            | Command::Start { id, .. }
            | Command::Stop { id, .. }
            | Command::Restart { id, .. }
            | Command::BindDatabase { id, .. }
            | Command::TestDatabase { id, .. }
            | Command::ActivateDatabaseGeneration { id, .. }
            | Command::UnbindDatabase { id, .. }
            | Command::InstallRedisClient { id, .. }
            | Command::SwitchDatabase { id, .. }
            | Command::CreateCache { id, .. }
            | Command::ConfigureServices { id, .. }
            | Command::CreateBackup { id, .. }
            | Command::ExportBackup { id, .. }
            | Command::ImportBackup { id, .. }
            | Command::RestoreBackup { id, .. } => project(id),
            Command::WorktreePlan { input } | Command::WorktreeCreate { input, .. } => {
                project(&input.source_id)
            }
            Command::WorktreeArchivePlan { id, .. }
            | Command::WorktreeArchive { id, .. }
            | Command::WorktreeRemotePlan { id, .. }
            | Command::WorktreeRemoteCopy { id, .. }
            | Command::WorktreeOpen { id }
            | Command::WorktreeCleanupPlan { id }
            | Command::WorktreeCleanup { id, .. } => Ok(Self::Worktree {
                worktree_id: Id::new(id).map_err(|_| crate::actor::ActorError::InvalidInput)?,
            }),
            Command::StartCache { instance } | Command::StopCache { instance } => Ok(Self::Cache {
                instance_id: Id::new(instance)
                    .map_err(|_| crate::actor::ActorError::InvalidInput)?,
            }),
            Command::Import { .. }
            | Command::CreateProject { .. }
            | Command::DiscoveryRestore { .. }
            | Command::RefreshRuntimes
            | Command::Install { .. }
            | Command::RemoveRuntime { .. }
            | Command::CreateDatabase { .. }
            | Command::StartDatabase { .. }
            | Command::StopDatabase { .. } => Ok(Self::Root {}),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReadRequest {
    Report {
        request: crate::report_reads::ReportReadRequest,
    },
    Inspector {
        project_id: Id,
        context: Option<Id>,
    },
    Inbox {
        project_id: Id,
        query: String,
        page: u32,
    },
    MailMessage {
        project_id: Id,
        message: Id,
    },
    MailPart {
        project_id: Id,
        message: Id,
        index: u32,
    },
    Logs {
        project_id: Id,
        filter: LogFilter,
    },
    DebugView {
        project_id: Id,
    },
    DebugProfile {
        project_id: Id,
        profile: Id,
    },
    DebugSource {
        project_id: Id,
        file: String,
    },
    ConfigurationDocument {
        project_id: Id,
    },
    ConfigurationPlan {
        project_id: Id,
        write_env: bool,
    },
}
impl ReadRequest {
    pub fn project_id(&self) -> &Id {
        match self {
            Self::Report { request } => request.subject(),
            Self::Inspector { project_id, .. }
            | Self::Inbox { project_id, .. }
            | Self::MailMessage { project_id, .. }
            | Self::MailPart { project_id, .. }
            | Self::Logs { project_id, .. }
            | Self::DebugView { project_id }
            | Self::DebugProfile { project_id, .. }
            | Self::DebugSource { project_id, .. }
            | Self::ConfigurationDocument { project_id }
            | Self::ConfigurationPlan { project_id, .. } => project_id,
        }
    }
    pub(crate) fn valid(&self) -> bool {
        if let Self::Report { request } = self {
            return request.valid();
        }
        if self.project_id().as_str() == ROOT_SUBJECT {
            return false;
        }
        match self {
            Self::Report { .. } => unreachable!("report validation handled above"),
            Self::Inbox { query, page, .. } => query.len() <= 512 && *page <= 10000,
            Self::MailPart { index, .. } => *index < 50,
            Self::Logs { filter, .. } => {
                filter.query.len() <= 512
                    && filter.level.len() <= 32
                    && filter.kind.len() <= 32
                    && filter.page <= 10000
                    && filter.since <= 9_007_199_254_740_991
            }
            Self::DebugSource { file, .. } => {
                !file.is_empty() && file.len() <= 4096 && !file.chars().any(char::is_control)
            }
            _ => true,
        }
    }
    pub(crate) fn execute(&self, workspace: &Workspace) -> devforge_workspace::Result<Value> {
        let id = self.project_id().as_str();
        match self {
            Self::Report { request } => request.execute(workspace),
            Self::Inspector { context, .. } => {
                workspace.inspector_context(id, context.as_ref().map(Id::as_str))
            }
            Self::Inbox { query, page, .. } => workspace.mail_view(id, query, *page as usize),
            Self::MailMessage { message, .. } => workspace.mail_message(id, message.as_str()),
            Self::MailPart { message, index, .. } => {
                workspace.mail_part(id, message.as_str(), *index as usize)
            }
            Self::Logs { filter, .. } => workspace.logs_view(id, filter),
            Self::DebugView { .. } => workspace.debug_view(id),
            Self::DebugProfile { profile, .. } => workspace.debug_profile(id, profile.as_str()),
            Self::DebugSource { file, .. } => workspace.debug_source(id, file),
            Self::ConfigurationDocument { .. } => workspace.configuration_document(id),
            Self::ConfigurationPlan { write_env, .. } => {
                workspace.configuration_plan(id, *write_env)
            }
        }
    }
}
/// Sensitive native input. No Debug, cleartext persistence or result projection.
#[derive(Clone)]
pub struct SecretValue(Zeroizing<String>);
impl SecretValue {
    pub fn new(value: &str) -> Option<Self> {
        (value.len() <= 4096).then(|| Self(Zeroizing::new(value.to_owned())))
    }
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}
impl Serialize for SecretValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}
impl<'de> Deserialize<'de> for SecretValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Zeroizing::new(String::deserialize(deserializer)?);
        if value.len() > 4096 {
            return Err(serde::de::Error::custom("secret limit"));
        }
        Ok(Self(value))
    }
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenTarget {
    Browser,
    Editor,
    Folder,
    Terminal,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AccessRequest {
    ConfigurationSecret {
        project_id: Id,
        reference: String,
        value: SecretValue,
    },
    OpenTarget {
        project_id: Id,
        target: OpenTarget,
    },
    DiscoveryRoot {
        path: String,
    },
}
impl AccessRequest {
    pub fn subject(&self) -> Id {
        self.project_id()
    }
    pub fn project_id(&self) -> Id {
        match self {
            Self::ConfigurationSecret { project_id, .. } | Self::OpenTarget { project_id, .. } => {
                project_id.clone()
            }
            Self::DiscoveryRoot { .. } => Id::new(ROOT_SUBJECT).expect("fixed root ID"),
        }
    }
    pub(crate) fn valid(&self) -> bool {
        match self {
            Self::ConfigurationSecret {
                project_id,
                reference,
                ..
            } => {
                project_id.as_str() != ROOT_SUBJECT
                    && !reference.is_empty()
                    && reference.len() <= 64
                    && !reference.chars().any(char::is_control)
            }
            Self::OpenTarget { project_id, .. } => project_id.as_str() != ROOT_SUBJECT,
            Self::DiscoveryRoot { path } => {
                !path.is_empty() && path.len() <= 4096 && !path.chars().any(char::is_control)
            }
        }
    }
    pub(crate) fn execute(
        &self,
        workspace: &mut Workspace,
        root: &Path,
        discovery: Option<&devforge_workspace::discovery::Service>,
    ) -> devforge_workspace::Result<Value> {
        match self {
            Self::ConfigurationSecret {
                project_id,
                reference,
                value,
            } => {
                workspace.configuration_secret(
                    project_id.as_str(),
                    reference,
                    value.expose_secret(),
                )?;
                Ok(serde_json::json!({"saved":true}))
            }
            Self::OpenTarget { project_id, target } => {
                let target = match target {
                    OpenTarget::Browser => "browser",
                    OpenTarget::Editor => "editor",
                    OpenTarget::Folder => "folder",
                    OpenTarget::Terminal => "terminal",
                };
                workspace.open_target(project_id.as_str(), target)?;
                Ok(serde_json::json!({"opened":true}))
            }
            Self::DiscoveryRoot { path } => {
                let home = devforge_platform::home_dir()
                    .map_err(|_| domain("workspace", "discovery_unavailable", None))?;
                let path = devforge_platform::resolve_project_folder(path, &home)
                    .map_err(|_| domain("workspace", "discovery_unavailable", None))?;
                let mut settings = devforge_platform::SqliteSettings::open(root)
                    .map_err(|_| domain("workspace", "storage_unavailable", None))?;
                let registry = LocaleRegistry::from_json(include_str!(
                    "../../../shared/locales/registry.json"
                ))
                .map_err(|_| domain("workspace", "storage_unavailable", None))?;
                settings
                    .patch(
                        SettingsPatch {
                            language: None,
                            theme: None,
                            project_folder: Some(path.to_string_lossy().into_owned()),
                        },
                        &registry,
                    )
                    .map_err(|_| domain("workspace", "storage_unavailable", None))?;
                discovery
                    .ok_or_else(|| domain("workspace", "discovery_unavailable", None))?
                    .set_root(path);
                Ok(serde_json::json!({"saved":true}))
            }
        }
    }
}
fn domain(scope: &'static str, code: &str, project: Option<&str>) -> devforge_workspace::Error {
    devforge_workspace::Error {
        scope,
        code: code.into(),
        project_id: project.map(str::to_owned),
    }
}

/// Aggregate encoded-image quota shared by actor cache and pinned page exports.
/// Quota is released only after the last Arc image disappears, not on eviction
/// while a client export still retains the same bytes.
#[derive(Default)]
pub(crate) struct ImageBudget {
    used: AtomicUsize,
}
impl ImageBudget {
    fn reserve(&self, bytes: usize) -> bool {
        self.used
            .try_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes)
                    .filter(|next| *next <= READ_TOTAL_BYTES)
            })
            .is_ok()
    }
    fn release(&self, bytes: usize) {
        self.used.fetch_sub(bytes, Ordering::AcqRel);
    }
}
pub struct ReadImage {
    bytes: Zeroizing<Vec<u8>>,
    budget: Arc<ImageBudget>,
    charge: usize,
    pub(crate) expires: Instant,
    pub observed_at: u64,
}
impl ReadImage {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn expires_in(&self, now: Instant) -> Duration {
        self.expires.saturating_duration_since(now)
    }
}
impl Drop for ReadImage {
    fn drop(&mut self) {
        let bytes = std::mem::replace(&mut self.bytes, Zeroizing::new(Vec::new()));
        drop(bytes);
        self.budget.release(self.charge);
    }
}
struct Writer {
    bytes: Zeroizing<Vec<u8>>,
    budget: Arc<ImageBudget>,
    charge: usize,
    transferred: bool,
}
impl Write for Writer {
    fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
        if input.len() > READ_IMAGE_BYTES.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other("read image limit"));
        }
        let needed = self.bytes.len() + input.len();
        if needed > self.bytes.capacity() {
            let target = needed
                .max(self.bytes.capacity().max(4096).saturating_mul(2))
                .min(READ_IMAGE_BYTES);
            let extra = target - self.charge;
            if !self.budget.reserve(extra) {
                return Err(std::io::Error::other("read image aggregate limit"));
            }
            let additional = target - self.bytes.len();
            if self.bytes.try_reserve_exact(additional).is_err() {
                self.budget.release(extra);
                return Err(std::io::Error::other("read image allocation"));
            }
            self.charge = target;
        }
        self.bytes.extend_from_slice(input);
        Ok(input.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        if !self.transferred {
            let bytes = std::mem::replace(&mut self.bytes, Zeroizing::new(Vec::new()));
            drop(bytes);
            self.budget.release(self.charge);
        }
    }
}
pub(crate) fn encode_read(
    value: &Value,
    budget: Arc<ImageBudget>,
    observed_at: u64,
) -> Result<Arc<ReadImage>, payload::Error> {
    let mut writer = Writer {
        bytes: Zeroizing::new(Vec::new()),
        budget: budget.clone(),
        charge: 0,
        transferred: false,
    };
    #[derive(Serialize)]
    struct Outcome<'a> {
        ok: bool,
        data: &'a Value,
    }
    serde_json::to_writer(
        &mut writer,
        &Outcome {
            ok: true,
            data: value,
        },
    )
    .map_err(|_| payload::Error::OutputLimit)?;
    writer.transferred = true;
    let bytes = std::mem::replace(&mut writer.bytes, Zeroizing::new(Vec::new()));
    Ok(Arc::new(ReadImage {
        bytes,
        budget,
        charge: writer.charge,
        expires: Instant::now() + READ_TTL,
        observed_at,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn subject_selector_never_contains_raw_http_query_or_yaml_fields() {
        let canary = "selector-privacy-fixture";
        let commands=[Command::DebugRequest{id:"project-a".into(),path:format!("/path?password={canary}")},Command::ConfigurationSave{id:"project-a".into(),expected:None,text:format!("# {canary}"),consent:true},serde_json::from_value::<Command>(serde_json::json!({"action":"debug_http","id":"project-a","request":{"method":"POST","path":"/login","headers":{"authorization":canary},"body":canary}})).unwrap()];
        for command in commands {
            let selector = SubjectSelector::from_command(&command).unwrap();
            let encoded = serde_json::to_value(selector).unwrap();
            assert_eq!(
                encoded,
                serde_json::json!({"kind":"project","project_id":"project-a"})
            );
            assert!(!encoded.to_string().contains(canary));
        }
        assert!(
            serde_json::from_value::<SubjectSelector>(
                serde_json::json!({"kind":"root","role":"desktop"})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<SubjectSelector>(
                serde_json::json!({"kind":"project","project_id":"project-a","body":canary})
            )
            .is_err()
        );
    }
    #[test]
    fn aggregate_quota_denies_before_growth_and_last_pinned_reference_releases_claim() {
        let budget = Arc::new(ImageBudget::default());
        assert!(budget.reserve(READ_TOTAL_BYTES - 4096));
        assert!(encode_read(&serde_json::json!({"value":"small"}), budget.clone(), 1).is_err());
        assert_eq!(budget.used.load(Ordering::Acquire), READ_TOTAL_BYTES - 4096);
        budget.release(READ_TOTAL_BYTES - 4096);
        let image = encode_read(&serde_json::json!({"value":"small"}), budget.clone(), 1).unwrap();
        let pin = image.clone();
        let charged = budget.used.load(Ordering::Acquire);
        assert!(charged > 0);
        drop(image);
        assert_eq!(budget.used.load(Ordering::Acquire), charged);
        drop(pin);
        assert_eq!(budget.used.load(Ordering::Acquire), 0);
    }
    #[test]
    fn private_inputs_keep_secret_and_query_limits_strict() {
        assert!(SecretValue::new(&"x".repeat(4097)).is_none());
        assert!(SecretValue::new(&"x".repeat(4096)).is_some());
        let project = Id::new("p").unwrap();
        assert!(
            !ReadRequest::Inbox {
                project_id: project,
                query: "x".repeat(513),
                page: 0
            }
            .valid()
        );
    }
}
