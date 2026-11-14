//! Versioned control-plane contracts shared by future CLI, desktop and MCP adapters.
//!
//! This crate does not authenticate a peer, run a manager or expose a transport.
//! Adapters must resolve registry-owned project bindings and server-issued grants;
//! neither a client root nor a boolean confirmation establishes authority.
use serde::{Deserialize, Serialize};
use std::{
    fmt,
    path::{Path, PathBuf},
};

pub const API_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidInput,
    CursorExpired,
    OutputLimitExceeded,
    TimedOut,
    UnsupportedVersion,
    Unauthorized,
    GrantExpired,
    GrantRevoked,
    ProjectMismatch,
    RevisionConflict,
    ReviewExpired,
    ReviewMismatch,
    IdempotencyConflict,
    InvalidTransition,
    UnsupportedCapability,
    Busy,
    Interrupted,
    StorageUnavailable,
    AdmissionUnconfirmed,
    PreconditionFailed,
}
impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ErrorCode {}
pub type Result<T> = std::result::Result<T, ErrorCode>;

/// Opaque identifiers, never paths, command text or authorization tokens.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct Id(String);
impl Id {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-.:".contains(&c))
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::new(String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ApiVersion(u16);
impl Default for ApiVersion {
    fn default() -> Self {
        Self(API_VERSION)
    }
}
impl<'de> Deserialize<'de> for ApiVersion {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let version = u16::deserialize(d)?;
        if version != API_VERSION {
            return Err(serde::de::Error::custom("unsupported_version"));
        }
        Ok(Self(version))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request<T> {
    pub api_version: ApiVersion,
    pub request_id: Id,
    pub project_id: Id,
    pub expected_revision: Option<u64>,
    pub idempotency_key: Option<Id>,
    pub input: T,
}
impl<T> Request<T> {
    /// Peer/grant verification remains mandatory before dispatch.
    pub fn validate(&self, binding: &ProjectBinding, mutating: bool) -> Result<()> {
        if self.project_id != binding.project_id {
            return Err(ErrorCode::ProjectMismatch);
        }
        if self
            .expected_revision
            .is_some_and(|r| r != binding.revision)
        {
            return Err(ErrorCode::RevisionConflict);
        }
        if mutating && (self.expected_revision.is_none() || self.idempotency_key.is_none()) {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(())
    }
}

/// Built by trusted registry code, never deserialized from an IPC request.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectBinding {
    project_id: Id,
    canonical_root: PathBuf,
    revision: u64,
}
impl ProjectBinding {
    /// Read-only filesystem resolution of a registry path. The caller must first
    /// retrieve this path from its registry, not trust a renderer/client root.
    pub fn from_registered(project_id: Id, root: &Path, revision: u64) -> Result<Self> {
        if !root.is_absolute() {
            return Err(ErrorCode::InvalidInput);
        }
        let canonical_root = root.canonicalize().map_err(|_| ErrorCode::InvalidInput)?;
        if !canonical_root.is_dir() {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            project_id,
            canonical_root,
            revision,
        })
    }
    pub fn project_id(&self) -> &Id {
        &self.project_id
    }
    pub fn root(&self) -> &Path {
        &self.canonical_root
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Guard for an existing, registry-scoped resource. This is path validation,
    /// not permission to expose all project files. An adapter still checks its
    /// resource allowlist and grant before calling it.
    pub fn resolve_existing(&self, relative: &Path) -> Result<PathBuf> {
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(ErrorCode::InvalidInput);
        }
        let resolved = self
            .canonical_root
            .join(relative)
            .canonicalize()
            .map_err(|_| ErrorCode::InvalidInput)?;
        if !resolved.starts_with(&self.canonical_root) {
            return Err(ErrorCode::ProjectMismatch);
        }
        Ok(resolved)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Observer,
    ProjectManage,
    ExecutionTest,
    SensitiveOperation,
}

/// Independent grant bits: project management never implies execution or sensitive access.
/// Construction is for a trusted consent/store boundary, not a tool input handler.
/// Grant/review expiry and `now` arguments use Unix milliseconds.
#[derive(Debug, Clone, Serialize)]
pub struct Grant {
    client_id: Id,
    project_id: Id,
    scopes: Vec<Scope>,
    expires_at: u64,
    revoked: bool,
}
impl Grant {
    pub fn issue(
        client_id: Id,
        project_id: Id,
        scopes: Vec<Scope>,
        expires_at: u64,
    ) -> Result<Self> {
        if scopes.is_empty() || scopes.len() > 4 || expires_at == 0 {
            return Err(ErrorCode::InvalidInput);
        }
        for (i, scope) in scopes.iter().enumerate() {
            if scopes[..i].contains(scope) {
                return Err(ErrorCode::InvalidInput);
            }
        }
        Ok(Self {
            client_id,
            project_id,
            scopes,
            expires_at,
            revoked: false,
        })
    }
    pub fn revoke(&mut self) {
        self.revoked = true;
    }
    pub fn authorize(
        &self,
        authenticated_client: &Id,
        project: &ProjectBinding,
        scope: Scope,
        now: u64,
    ) -> Result<()> {
        if self.revoked {
            return Err(ErrorCode::GrantRevoked);
        }
        if now >= self.expires_at {
            return Err(ErrorCode::GrantExpired);
        }
        if authenticated_client != &self.client_id {
            return Err(ErrorCode::Unauthorized);
        }
        if &self.project_id != project.project_id() {
            return Err(ErrorCode::ProjectMismatch);
        }
        if !self.scopes.contains(&scope) {
            return Err(ErrorCode::Unauthorized);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Digest(String);
impl Digest {
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// The hashing implementation belongs to the plan/request producer. This
    /// validates canonical SHA-256 hex, not the authenticity of supplied content.
    pub fn new(value: String) -> Result<Self> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self(value))
    }
}
impl<'de> Deserialize<'de> for Digest {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Self::new(String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

/// Trusted stored review. A request cannot manufacture this from confirm:true.
#[derive(Debug, Clone, Serialize)]
pub struct ReviewedPlan {
    project_id: Id,
    revision: u64,
    action: Id,
    digest: Digest,
    expires_at: u64,
}
impl ReviewedPlan {
    pub fn record(
        binding: &ProjectBinding,
        action: Id,
        digest: Digest,
        expires_at: u64,
    ) -> Result<Self> {
        if expires_at == 0 {
            return Err(ErrorCode::InvalidInput);
        }
        Ok(Self {
            project_id: binding.project_id.clone(),
            revision: binding.revision,
            action,
            digest,
            expires_at,
        })
    }
    pub fn verify(
        &self,
        current: &ProjectBinding,
        action: &Id,
        actual_digest: &Digest,
        now: u64,
    ) -> Result<()> {
        if self.project_id != current.project_id {
            return Err(ErrorCode::ProjectMismatch);
        }
        if self.revision != current.revision {
            return Err(ErrorCode::RevisionConflict);
        }
        if now >= self.expires_at {
            return Err(ErrorCode::ReviewExpired);
        }
        if &self.action != action || &self.digest != actual_digest {
            return Err(ErrorCode::ReviewMismatch);
        }
        Ok(())
    }
}

/// Stored key identity includes authenticated client and project, not key alone.
#[derive(Debug, Clone, Serialize)]
pub struct IdempotencyRecord {
    client_id: Id,
    project_id: Id,
    key: Id,
    request_digest: Digest,
    operation_id: Id,
}
impl IdempotencyRecord {
    pub fn new(
        client_id: Id,
        project_id: Id,
        key: Id,
        request_digest: Digest,
        operation_id: Id,
    ) -> Self {
        Self {
            client_id,
            project_id,
            key,
            request_digest,
            operation_id,
        }
    }
    pub fn replay(&self, client: &Id, project: &Id, key: &Id, digest: &Digest) -> Result<&Id> {
        if client != &self.client_id
            || project != &self.project_id
            || key != &self.key
            || digest != &self.request_digest
        {
            return Err(ErrorCode::IdempotencyConflict);
        }
        Ok(&self.operation_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Queued,
    Running,
    CancelRequested,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
    Unknown,
}
impl OperationState {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Interrupted | Self::Unknown
        )
    }
    pub fn transition(self, next: Self) -> Result<Self> {
        use OperationState::*;
        let valid = matches!(
            (self, next),
            (Queued, Running | Cancelled | Failed | Interrupted | Unknown)
                | (
                    Running,
                    CancelRequested | Succeeded | Failed | Interrupted | Unknown
                )
                | (
                    CancelRequested,
                    Cancelled | Succeeded | Failed | Interrupted | Unknown
                )
        );
        if valid {
            Ok(next)
        } else {
            Err(ErrorCode::InvalidTransition)
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Live,
    Persisted,
    Cached,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Existing internal snapshot timestamps use Unix milliseconds.
pub struct Freshness {
    pub origin: Origin,
    pub observed_at: u64,
    pub revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fault {
    pub code: ErrorCode,
    pub retryable: bool,
}
/// Construction prevents contradictory ok/data/error combinations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "T: Deserialize<'de>"))]
pub struct Response<T> {
    api_version: ApiVersion,
    request_id: Id,
    #[serde(skip_serializing_if = "Option::is_none")]
    operation_id: Option<Id>,
    ok: bool,
    #[serde(
        default,
        deserialize_with = "present_data",
        skip_serializing_if = "Option::is_none"
    )]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Fault>,
    warnings: Vec<ErrorCode>,
}
fn present_data<'de, T: Deserialize<'de>, D: serde::Deserializer<'de>>(
    decoder: D,
) -> std::result::Result<Option<T>, D::Error> {
    T::deserialize(decoder).map(Some)
}
impl<T> Response<T> {
    pub fn is_ok(&self) -> bool {
        self.ok
    }
    pub fn data(&self) -> Option<&T> {
        self.data.as_ref()
    }
    pub fn error(&self) -> Option<&Fault> {
        self.error.as_ref()
    }
    pub fn request_id(&self) -> &Id {
        &self.request_id
    }
    pub fn operation_id(&self) -> Option<&Id> {
        self.operation_id.as_ref()
    }
    pub fn success(request_id: Id, operation_id: Option<Id>, data: T) -> Self {
        Self {
            api_version: ApiVersion::default(),
            request_id,
            operation_id,
            ok: true,
            data: Some(data),
            error: None,
            warnings: vec![],
        }
    }
    pub fn failure(request_id: Id, operation_id: Option<Id>, fault: Fault) -> Self {
        Self {
            api_version: ApiVersion::default(),
            request_id,
            operation_id,
            ok: false,
            data: None,
            error: Some(fault),
            warnings: vec![],
        }
    }
}

pub mod inventory;

pub mod export;
pub mod limits;
pub mod tools;

impl ErrorCode {
    /// Stable transport mapping; no raw path, payload or serde failure text.
    pub fn fault(self) -> Fault {
        Fault {
            code: self,
            retryable: matches!(self, Self::Busy | Self::StorageUnavailable | Self::TimedOut),
        }
    }
}

pub mod mutations;
