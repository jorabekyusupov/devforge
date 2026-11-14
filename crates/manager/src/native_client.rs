//! Blocking Rust-only SDK for a future native desktop composition, not UI IPC.
//!
//! Keep this object and all credentials inside Rust. Call from a worker, never a
//! WebView/UI thread. There is no SecretStore, provider fallback, implicit launch,
//! automatic renewal, raw RPC escape hatch or credential serializer here.
use crate::{
    Client, Endpoint, PeerIdentity,
    auth::{DesktopCredential, SessionCredential, SessionInfo},
    desktop::{AccessRequest, READ_IMAGE_BYTES, READ_TTL, ReadRequest, SubjectSelector},
    launch::{BootstrappedChild, ReadyChild},
    payload::{CHUNK_BYTES, MAX_COMMAND_BYTES, MAX_SNAPSHOT_BYTES},
};
use devforge_control::{Id, OperationState};
use devforge_runtime::Cancellation as AdmissionCancellation;
use devforge_workspace::Command;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    io::Write,
    process::ExitStatus,
    time::{Duration, Instant},
};
use zeroize::{Zeroize, Zeroizing};

const EXPORT_TTL: Duration = Duration::from_secs(30);
const EXPIRY_MARGIN: Duration = Duration::from_millis(5);

#[derive(Clone, PartialEq, Eq)]
pub struct RemoteCode(String);
impl RemoteCode {
    pub fn as_str(&self) -> &str {
        &self.0
    }
    fn parse(value: String) -> Result<Self> {
        if value.len() > 96
            || !["actor_", "controller_", "manager_", "payload_"]
                .iter()
                .any(|p| value.starts_with(p))
            || !value
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err(Error::Protocol);
        }
        Ok(Self(value))
    }
}
impl fmt::Debug for RemoteCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    OwnerUnavailable,
    OwnerChanged,
    ReauthenticationRequired,
    Protocol,
    UnsupportedProtocol,
    InputLimit,
    PayloadResyncRequired,
    AdmissionUnconfirmed,
    AdmissionCancelled,
    ReadExpired,
    ResourceLimit,
    Remote(RemoteCode),
}
impl Error {
    pub fn code(&self) -> &str {
        match self {
            Self::OwnerUnavailable => "native_client_owner_unavailable",
            Self::OwnerChanged => "native_client_owner_changed",
            Self::ReauthenticationRequired => "native_client_reauthentication_required",
            Self::Protocol => "native_client_protocol",
            Self::UnsupportedProtocol => "native_client_unsupported_protocol",
            Self::InputLimit => "native_client_input_limit",
            Self::PayloadResyncRequired => "payload_resync_required",
            Self::AdmissionUnconfirmed => "native_client_admission_unconfirmed",
            Self::AdmissionCancelled => "native_client_admission_cancelled",
            Self::ReadExpired => "actor_read_expired",
            Self::ResourceLimit => "native_client_resource_limit",
            Self::Remote(code) => code.as_str(),
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerState {
    Ready,
    Quitting,
    Unavailable,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Health {
    pub api_version: u16,
    pub owner_pid: u32,
    pub state: OwnerState,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub operation_id: Id,
    pub client_id: Id,
    pub project_id: Id,
    pub state: OperationState,
    pub code: Option<String>,
}
/// Native-only progress for cancellation/active-operation handoff. Contains no
/// body, credential or raw transfer identifier and is never a renderer token.
#[derive(Clone, Debug)]
pub enum AdmissionProgress {
    Preparing,
    Uploading { sent: u64, total: u64 },
    Finalizing,
    Accepted(Receipt),
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureScope {
    Project,
    Runtime,
    Serving,
    Workspace,
    Debugger,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailureDetail {
    pub scope: FailureScope,
    pub code: String,
    pub project_id: Id,
    pub retryable: bool,
    /// Trusted worker association, separate from the receipt's subject namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_project_id: Option<Id>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationPage {
    pub items: Vec<Receipt>,
    pub next_cursor: Option<Value>,
    pub active_reads: Vec<Receipt>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadProgress {
    More { received: u64, total: u64 },
    Complete,
}
/// Native-only incremental read. Keep this outside an SDK mutex and acquire the
/// client only for each page, so another native worker can send cancel/status.
/// No body/token Debug, serde or cloning. Drop erases bytes; explicit release is
/// best-effort and the server's fixed TTL bounds an abandoned export.
pub struct ReadResult {
    descriptor: ReadDescriptor,
    cursor: Option<Cursor>,
    bytes: Zeroizing<Vec<u8>>,
    digest: [u8; 32],
    deadline: Instant,
    owner: PeerIdentity,
    root: [u8; 32],
    kind: ImageKind,
}
#[derive(Clone, Copy)]
enum ImageKind {
    Snapshot,
    Read,
    Mutation,
}
impl ImageKind {
    fn limit(self) -> usize {
        match self {
            Self::Snapshot => MAX_SNAPSHOT_BYTES,
            Self::Read | Self::Mutation => READ_IMAGE_BYTES,
        }
    }
    fn ttl(self) -> Duration {
        match self {
            Self::Snapshot => EXPORT_TTL,
            Self::Read | Self::Mutation => READ_TTL,
        }
    }
    fn expired(self) -> Error {
        match self {
            Self::Snapshot => Error::PayloadResyncRequired,
            Self::Read => Error::ReadExpired,
            Self::Mutation => Error::Remote(RemoteCode("actor_result_unavailable".into())),
        }
    }
}
/// Credential-free incremental global snapshot. Keep it outside the SDK mutex;
/// only each page step needs the client, just like an owner read image.
pub struct SnapshotRead {
    image: ReadResult,
}
impl fmt::Debug for SnapshotRead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SnapshotRead")
            .field("received", &self.image.bytes.len())
            .field("total", &self.image.descriptor.size)
            .finish_non_exhaustive()
    }
}
impl SnapshotRead {
    pub fn received_bytes(&self) -> u64 {
        self.image.received_bytes()
    }
    pub fn total_bytes(&self) -> u64 {
        self.image.total_bytes()
    }
    pub fn observed_at(&self) -> u64 {
        self.image.observed_at()
    }
}
impl fmt::Debug for ReadResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReadResult")
            .field("received", &self.bytes.len())
            .field("total", &self.descriptor.size)
            .finish_non_exhaustive()
    }
}
impl ReadResult {
    pub fn observed_at(&self) -> u64 {
        self.descriptor.observed_at
    }
    pub fn received_bytes(&self) -> u64 {
        self.bytes.len() as u64
    }
    pub fn total_bytes(&self) -> u64 {
        self.descriptor.size
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cancellation {
    Requested,
    AlreadyTerminal,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub snapshot: Value,
    pub observed_at: u64,
    pub busy: bool,
    pub stopped: bool,
    pub error: Option<String>,
    pub progress: Option<Value>,
}
impl fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Snapshot")
            .field("observed_at", &self.observed_at)
            .field("busy", &self.busy)
            .field("stopped", &self.stopped)
            .finish_non_exhaustive()
    }
}
struct Session {
    credential: SessionCredential,
    expires: Instant,
    context_deadline: Option<Instant>,
    renewable: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionMetadata {
    expires_in_ms: u64,
    context_remaining_ms: Option<u64>,
    renewable: bool,
}

/// Single native caller's owner-pinned session. No Clone/serde or secret Debug.
pub struct NativeClient {
    transport: Client,
    peer: PeerIdentity,
    root: [u8; 32],
    session: Option<Session>,
    launched: Option<ReadyChild>,
    shutdown_id: Option<String>,
}
impl fmt::Debug for NativeClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeClient")
            .field("owner_pid", &self.peer.process_id)
            .field("session_cached", &self.session.is_some())
            .finish_non_exhaustive()
    }
}
impl NativeClient {
    /// The Endpoint is native configuration, never a renderer-provided path.
    /// This method does not launch, provision, read or change native credentials.
    pub fn authenticate_explicit(
        endpoint: Endpoint,
        credential: &DesktopCredential,
    ) -> Result<Self> {
        let (peer, health) = observe(&endpoint)?;
        if health.state != OwnerState::Ready {
            return Err(Error::OwnerUnavailable);
        }
        let mut client = Self {
            root: crate::bootstrap::root_fingerprint(endpoint.root()),
            transport: Client::new(endpoint),
            peer,
            session: None,
            launched: None,
            shutdown_id: None,
        };
        let started = Instant::now();
        let response =
            client.raw(json!({"method":"authenticate","credential":credential.expose_secret()}))?;
        client.install_session(response, started, None)?;
        Ok(client)
    }
    /// Checks root and kernel PID before sending the one-use pipe credential.
    /// The credential is erased after redemption; only a credential-free child
    /// monitor is retained. Failure never kills or silently adopts another owner.
    pub fn from_bootstrapped(endpoint: Endpoint, child: BootstrappedChild) -> Result<Self> {
        let (credential, monitor) = child.into_parts();
        if credential.root_fingerprint() != &crate::bootstrap::root_fingerprint(endpoint.root()) {
            return Err(Error::OwnerChanged);
        }
        let (peer, health) = observe(&endpoint)?;
        if peer.process_id != Some(monitor.process_id()) || health.state != OwnerState::Ready {
            return Err(Error::OwnerChanged);
        }
        let mut client = Self {
            root: crate::bootstrap::root_fingerprint(endpoint.root()),
            transport: Client::new(endpoint),
            peer,
            session: None,
            launched: Some(monitor),
            shutdown_id: None,
        };
        let started = Instant::now();
        let response = client.raw(json!({"method":"redeem_bootstrap", "token":credential.expose_secret(),
            "root_fingerprint":hex(credential.root_fingerprint()), "manager_instance":hex(credential.manager_instance())}))?;
        client.install_session(response, started, None)?;
        Ok(client)
    }
    pub fn health(&mut self) -> Result<Health> {
        let data = self.data(json!({"method":"health"}))?;
        let health: Health = decode(data)?;
        if let Err(error) = validate_health(&self.peer, &health) {
            self.session = None;
            return Err(error);
        }
        Ok(health)
    }
    /// Nonsecret, conservative local lifetime metadata. No RPC or store probe.
    pub fn session_info(&mut self) -> Result<SessionInfo> {
        self.require_session()?;
        let now = Instant::now();
        let session = self
            .session
            .as_ref()
            .ok_or(Error::ReauthenticationRequired)?;
        Ok(SessionInfo {
            expires_in_ms: millis(session.expires.saturating_duration_since(now)),
            context_remaining_ms: session
                .context_deadline
                .map(|d| millis(d.saturating_duration_since(now))),
            renewable: session.renewable,
        })
    }
    /// Explicit renewal only. No expired-token renewal or native-store fallback.
    /// A lost/invalid renewal response discards the old token, since the server
    /// may already have rotated it. The caller must explicitly authenticate again.
    pub fn renew_explicit(&mut self) -> Result<SessionInfo> {
        self.require_session()?;
        let session = self
            .session
            .as_ref()
            .ok_or(Error::ReauthenticationRequired)?;
        if !session.renewable {
            return Err(Error::ReauthenticationRequired);
        }
        let previous_deadline = session.context_deadline;
        let request = json!({"method":"renew","session":session.credential.expose_secret()});
        let started = Instant::now();
        let response = self.raw(request);
        self.session = None;
        self.install_session(response?, started, previous_deadline)?;
        self.session_info()
    }
    /// Backward-compatible blocking convenience wrapper over the incremental
    /// snapshot codec. Native UI code should yield its SDK mutex between steps.
    pub fn snapshot(&mut self) -> Result<Snapshot> {
        let mut read = self.begin_snapshot()?;
        loop {
            match self.snapshot_step(&mut read) {
                Ok(ReadProgress::Complete) => return self.finish_snapshot(read),
                Ok(ReadProgress::More { .. }) => {}
                Err(error) => {
                    let _ = self.release_snapshot(read);
                    return Err(error);
                }
            }
        }
    }
    pub fn begin_snapshot(&mut self) -> Result<SnapshotRead> {
        let started = Instant::now();
        let begin: SnapshotBegin = decode(self.authenticated("snapshot_begin", json!({}))?)?;
        let descriptor = ReadDescriptor {
            snapshot_id: begin.snapshot_id,
            epoch: begin.epoch,
            size: begin.size,
            digest: begin.digest,
            observed_at: begin.observed_at,
            cursor: begin.cursor,
            expires_in_ms: millis(EXPORT_TTL),
        };
        match self.prepare_image(descriptor, started, ImageKind::Snapshot) {
            Ok(image) => Ok(SnapshotRead { image }),
            Err(error) => Err(error),
        }
    }
    pub fn snapshot_step(&mut self, read: &mut SnapshotRead) -> Result<ReadProgress> {
        self.read_result_step(&mut read.image)
    }
    pub fn finish_snapshot(&mut self, read: SnapshotRead) -> Result<Snapshot> {
        self.check_read_owner(&read.image)?;
        let result = (|| {
            self.validate_complete_image(&read.image)?;
            let snapshot: Snapshot =
                serde_json::from_slice(read.image.bytes.as_slice()).map_err(|_| Error::Protocol)?;
            if snapshot.observed_at != read.image.descriptor.observed_at
                || !snapshot.snapshot.is_object()
                || ["projects", "runtimes", "databases", "logs"]
                    .iter()
                    .any(|name| !snapshot.snapshot.get(name).is_some_and(Value::is_array))
            {
                return Err(Error::Protocol);
            }
            Ok(snapshot)
        })();
        let _ = self.release_image(
            &read.image.descriptor.snapshot_id,
            &read.image.descriptor.epoch,
        );
        result
    }
    pub fn release_snapshot(&mut self, read: SnapshotRead) -> Result<()> {
        self.release_read_result(read.image)
    }
    /// Observe only authenticated nonsecret owner health. No credential/store
    /// access or implicit launch occurs when there is no running owner.
    pub fn observe_owner(endpoint: &Endpoint) -> Result<Health> {
        observe(endpoint).map(|(_, health)| health)
    }
    /// New explicit user intent only. Never replace a retained unconfirmed key.
    pub fn new_intent_id() -> Result<Id> {
        let mut bytes = Zeroizing::new([0u8; 32]);
        getrandom::fill(bytes.as_mut()).map_err(|_| Error::ResourceLimit)?;
        Id::new(hex(bytes.as_ref())).map_err(|_| Error::Protocol)
    }
    /// Advisory native namespace only. The shared exhaustive selector copies
    /// IDs, never raw YAML/HTTP input; admission/dequeue still resolve authority.
    pub fn resolve_subject(&mut self, command: &Command) -> Result<Id> {
        self.require_session()?;
        let selector = SubjectSelector::from_command(command).map_err(|error| {
            RemoteCode::parse(error.code().to_owned()).map_or(Error::Protocol, Error::Remote)
        })?;
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Advice {
            project_id: Id,
            advisory: bool,
        }
        let advice: Advice =
            decode(self.authenticated("resolve_subject", json!({"selector":selector}))?)?;
        if !advice.advisory {
            return Err(Error::Protocol);
        }
        Ok(advice.project_id)
    }
    /// Submit one bounded ephemeral observation. A new poll uses a new key; the
    /// same key identifies the same observation until read expiry/eviction.
    pub fn desktop_read(&mut self, idempotency: &Id, request: &ReadRequest) -> Result<Receipt> {
        self.require_session()?;
        let request_value = serde_json::to_value(request).map_err(|_| Error::Protocol)?;
        let receipt: Receipt = decode(self.authenticated(
            "desktop_read",
            json!({"idempotency_key":idempotency,"request":request_value}),
        )?)?;
        if &receipt.project_id != request.project_id() {
            return Err(Error::Protocol);
        }
        Ok(receipt)
    }
    /// Durable native access, never implicit credential issuance. Secret input
    /// stays in a zeroizing request type and its transient JSON is scrubbed even
    /// when the session expires before the transport can be used.
    pub fn desktop_access(&mut self, idempotency: &Id, request: &AccessRequest) -> Result<Receipt> {
        self.require_session()?;
        let request_value = serde_json::to_value(request).map_err(|_| Error::Protocol)?;
        let value = self.authenticated(
            "desktop_access",
            json!({"idempotency_key":idempotency,"request":request_value}),
        );
        let value = match value {
            Err(Error::OwnerUnavailable | Error::Protocol) => {
                return Err(Error::AdmissionUnconfirmed);
            }
            other => other?,
        };
        let receipt: Receipt = decode(value).map_err(|_| Error::AdmissionUnconfirmed)?;
        if receipt.project_id != request.subject() {
            return Err(Error::AdmissionUnconfirmed);
        }
        Ok(receipt)
    }
    /// Read-only reconciliation after an unconfirmed durable admission. None is
    /// only a point-in-time absence: the old handler may still commit. Keep the
    /// original key; neither None nor a receipt proves new secret-body equality.
    pub fn operation_by_key(&mut self, project: &Id, idempotency: &Id) -> Result<Option<Receipt>> {
        let receipt: Option<Receipt> = decode(self.authenticated(
            "operation_by_key",
            json!({"project_id":project,"idempotency_key":idempotency}),
        )?)?;
        if receipt
            .as_ref()
            .is_some_and(|receipt| &receipt.project_id != project)
        {
            return Err(Error::Protocol);
        }
        Ok(receipt)
    }
    pub fn discovery_request(&mut self) -> Result<()> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Reply {
            requested: bool,
        }
        let reply: Reply = decode(self.authenticated("discovery_request", json!({}))?)?;
        if !reply.requested {
            return Err(Error::Protocol);
        }
        Ok(())
    }
    pub fn operations(
        &mut self,
        project: Option<&Id>,
        cursor: Option<&Value>,
        limit: u32,
    ) -> Result<OperationPage> {
        if limit == 0 || limit > 100 {
            return Err(Error::InputLimit);
        }
        if let Some(cursor) = cursor {
            validate_cursor(cursor)?;
        }
        let page: OperationPage = decode(self.authenticated(
            "operations",
            json!({"project_id":project,"cursor":cursor,"limit":limit}),
        )?)?;
        if page.items.len() > limit as usize
            || page.active_reads.len() > 2
            || project.is_some_and(|project| {
                page.items
                    .iter()
                    .chain(page.active_reads.iter())
                    .any(|receipt| &receipt.project_id != project)
            })
        {
            return Err(Error::Protocol);
        }
        if let Some(cursor) = &page.next_cursor {
            validate_cursor(cursor).map_err(|_| Error::Protocol)?;
        }
        Ok(page)
    }
    pub fn begin_read_result(&mut self, project: &Id, operation: &Id) -> Result<ReadResult> {
        self.begin_image("desktop_read_result", project, operation)
    }
    /// Explicit large mutation-result retrieval after a Succeeded receipt. This
    /// never retries execution; a missing/expired retained image stays unavailable.
    pub fn begin_result_image(&mut self, project: &Id, operation: &Id) -> Result<ReadResult> {
        self.begin_image("result_begin", project, operation)
    }
    fn begin_image(&mut self, method: &str, project: &Id, operation: &Id) -> Result<ReadResult> {
        let started = Instant::now();
        let descriptor: ReadDescriptor = decode(self.operation(method, project, operation)?)?;
        self.prepare_image(
            descriptor,
            started,
            if method == "desktop_read_result" {
                ImageKind::Read
            } else {
                ImageKind::Mutation
            },
        )
    }
    fn prepare_image(
        &mut self,
        descriptor: ReadDescriptor,
        started: Instant,
        kind: ImageKind,
    ) -> Result<ReadResult> {
        let prepared = (|| {
            if descriptor.size == 0
                || descriptor.size > kind.limit() as u64
                || descriptor.expires_in_ms == 0
                || descriptor.expires_in_ms > millis(kind.ttl())
                || descriptor.cursor.offset != 0
                || descriptor.cursor.snapshot_id != descriptor.snapshot_id
                || descriptor.cursor.epoch != descriptor.epoch
            {
                return Err(Error::Protocol);
            }
            let digest = digest_bytes(&descriptor.digest)?;
            let deadline = started
                .checked_add(Duration::from_millis(descriptor.expires_in_ms))
                .and_then(|d| d.checked_sub(EXPIRY_MARGIN))
                .ok_or(Error::Protocol)?;
            if Instant::now() >= deadline {
                return Err(kind.expired());
            }
            let mut bytes = Zeroizing::new(Vec::new());
            bytes
                .try_reserve_exact(descriptor.size as usize)
                .map_err(|_| Error::ResourceLimit)?;
            Ok((digest, deadline, bytes))
        })();
        match prepared {
            Ok((digest, deadline, bytes)) => Ok(ReadResult {
                cursor: Some(descriptor.cursor.clone()),
                descriptor,
                bytes,
                digest,
                deadline,
                owner: self.peer.clone(),
                root: self.root,
                kind,
            }),
            Err(error) => {
                let _ = self.release_image(&descriptor.snapshot_id, &descriptor.epoch);
                Err(error)
            }
        }
    }
    /// Exactly one page RPC. The native bridge may release its client mutex
    /// between calls; no background renewal or whole-job waiting is hidden here.
    pub fn read_result_step(&mut self, image: &mut ReadResult) -> Result<ReadProgress> {
        self.check_read_owner(image)?;
        let Some(cursor) = image.cursor.as_ref() else {
            return Ok(ReadProgress::Complete);
        };
        if Instant::now() >= image.deadline {
            return Err(image.kind.expired());
        }
        let page: SnapshotPage =
            decode(self.authenticated("snapshot_page", json!({"cursor":cursor}))?)?;
        if page.snapshot_id != image.descriptor.snapshot_id
            || page.epoch != image.descriptor.epoch
            || page.offset != image.bytes.len() as u64
        {
            return Err(Error::Protocol);
        }
        let decoded = decode_chunk(&page.data_hex.0)?;
        if decoded.is_empty() || decoded.len() > image.descriptor.size as usize - image.bytes.len()
        {
            return Err(Error::Protocol);
        }
        image.bytes.extend_from_slice(&decoded);
        match page.next_cursor {
            None if image.bytes.len() as u64 == image.descriptor.size => {
                image.cursor = None;
                Ok(ReadProgress::Complete)
            }
            Some(next)
                if (image.bytes.len() as u64) < image.descriptor.size
                    && next.snapshot_id == image.descriptor.snapshot_id
                    && next.epoch == image.descriptor.epoch
                    && next.offset == image.bytes.len() as u64 =>
            {
                image.cursor = Some(next);
                Ok(ReadProgress::More {
                    received: image.bytes.len() as u64,
                    total: image.descriptor.size,
                })
            }
            _ => Err(Error::Protocol),
        }
    }
    /// Complete byte/digest validation, then return the exact old owner value.
    /// Failed read jobs use failure_detail instead; no error image is fabricated.
    pub fn finish_read_result(&mut self, image: ReadResult) -> Result<Value> {
        self.check_read_owner(&image)?;
        let result = (|| {
            self.validate_complete_image(&image)?;
            let value: Value =
                serde_json::from_slice(image.bytes.as_slice()).map_err(|_| Error::Protocol)?;
            data(value)
        })();
        let _ = self.release_image(&image.descriptor.snapshot_id, &image.descriptor.epoch);
        result
    }
    pub fn release_read_result(&mut self, image: ReadResult) -> Result<()> {
        self.check_read_owner(&image)?;
        self.release_image(&image.descriptor.snapshot_id, &image.descriptor.epoch)
    }
    fn validate_complete_image(&self, image: &ReadResult) -> Result<()> {
        self.check_read_owner(image)?;
        if image.cursor.is_some()
            || image.bytes.len() as u64 != image.descriptor.size
            || <[u8; 32]>::from(Sha256::digest(image.bytes.as_slice())) != image.digest
        {
            return Err(Error::Protocol);
        }
        Ok(())
    }
    fn check_read_owner(&self, image: &ReadResult) -> Result<()> {
        if image.owner != self.peer || image.root != self.root {
            return Err(Error::OwnerChanged);
        }
        Ok(())
    }
    fn release_image(&mut self, image: &Id, epoch: &Id) -> Result<()> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Reply {
            released: bool,
        }
        let reply: Reply = decode(self.authenticated(
            "snapshot_release",
            json!({"snapshot_id":image,"epoch":epoch}),
        )?)?;
        if !reply.released {
            return Err(Error::Protocol);
        }
        Ok(())
    }
    /// Upload/reassembly is bounded; caller identity/idempotency is explicit.
    /// Finalization is sent once. Ambiguous transport failure never triggers an
    /// automatic second admission; the caller retains its idempotency key.
    pub fn admit(&mut self, project: &Id, idempotency: &Id, command: &Command) -> Result<Receipt> {
        self.admit_cancellable(
            project,
            idempotency,
            command,
            &AdmissionCancellation::default(),
        )
    }
    /// Cancellation before finalization aborts the transient upload. Once a
    /// receipt exists it is ALWAYS returned, even if cancellation raced with
    /// finalization. The caller then cancels that exact project/operation ID.
    pub fn admit_cancellable(
        &mut self,
        project: &Id,
        idempotency: &Id,
        command: &Command,
        cancel: &AdmissionCancellation,
    ) -> Result<Receipt> {
        self.admit_observed(project, idempotency, command, cancel, |_| {})
    }
    /// The native bridge can update its separate active-operation state without
    /// keeping a UI thread/SDK mutex blocked for the later job's whole lifetime.
    /// Progress callbacks are trusted Rust code, not an RPC or credential route.
    pub fn admit_observed(
        &mut self,
        project: &Id,
        idempotency: &Id,
        command: &Command,
        cancel: &AdmissionCancellation,
        mut progress: impl FnMut(AdmissionProgress),
    ) -> Result<Receipt> {
        self.require_session()?;
        check_admission_cancel(cancel)?;
        progress(AdmissionProgress::Preparing);
        check_admission_cancel(cancel)?;
        let mut writer = BoundedBytes(Zeroizing::new(Vec::new()));
        serde_json::to_writer(&mut writer, command).map_err(|_| Error::InputLimit)?;
        let bytes = writer.0;
        let digest = hex(&Sha256::digest(bytes.as_slice()));
        check_admission_cancel(cancel)?;
        let begin: UploadBegin = decode(self.authenticated("upload_begin", json!({"project_id":project,"idempotency_key":idempotency,"size":bytes.len(),"digest":digest}))?)?;
        let result = self.upload_command(&begin, project, &bytes, &digest, cancel, &mut progress);
        if result.is_err() {
            self.abort_upload(&begin);
        }
        result
    }
    fn upload_command(
        &mut self,
        begin: &UploadBegin,
        project: &Id,
        bytes: &[u8],
        digest: &str,
        cancel: &AdmissionCancellation,
        progress: &mut impl FnMut(AdmissionProgress),
    ) -> Result<Receipt> {
        if begin.size != bytes.len() as u64 || begin.digest != digest || begin.next_index != 0 {
            return Err(Error::Protocol);
        }
        let mut received = 0;
        progress(AdmissionProgress::Uploading {
            sent: 0,
            total: bytes.len() as u64,
        });
        for (index, chunk) in bytes.chunks(CHUNK_BYTES).enumerate() {
            check_admission_cancel(cancel)?;
            let reply: UploadChunk = decode(self.authenticated("upload_chunk", json!({"upload_id":begin.upload_id,"epoch":begin.epoch,"index":index,"data_hex":hex(chunk)}))?)?;
            received += chunk.len();
            if reply.upload_id != begin.upload_id
                || reply.epoch != begin.epoch
                || reply.received != received as u64
                || reply.next_index != index as u32 + 1
            {
                return Err(Error::Protocol);
            }
            progress(AdmissionProgress::Uploading {
                sent: received as u64,
                total: bytes.len() as u64,
            });
        }
        progress(AdmissionProgress::Finalizing);
        check_admission_cancel(cancel)?;
        let value = self.authenticated(
            "upload_finalize",
            json!({"upload_id":begin.upload_id,"epoch":begin.epoch}),
        );
        let value = match value {
            Err(Error::OwnerUnavailable | Error::Protocol) => {
                return Err(Error::AdmissionUnconfirmed);
            }
            other => other?,
        };
        let receipt: Receipt = decode(value).map_err(|_| Error::AdmissionUnconfirmed)?;
        if &receipt.project_id != project {
            return Err(Error::AdmissionUnconfirmed);
        }
        // Never translate a post-admission cancellation into "not admitted".
        progress(AdmissionProgress::Accepted(receipt.clone()));
        Ok(receipt)
    }
    fn abort_upload(&mut self, begin: &UploadBegin) {
        let _ = self.authenticated(
            "upload_abort",
            json!({"upload_id":begin.upload_id,"epoch":begin.epoch}),
        );
    }
    pub fn status(&mut self, project: &Id, operation: &Id) -> Result<Receipt> {
        let receipt: Receipt = decode(self.operation("status", project, operation)?)?;
        if &receipt.project_id != project || &receipt.operation_id != operation {
            return Err(Error::Protocol);
        }
        Ok(receipt)
    }
    pub fn result(&mut self, project: &Id, operation: &Id) -> Result<Value> {
        self.operation("result", project, operation)
    }
    pub fn failure_detail(&mut self, project: &Id, operation: &Id) -> Result<FailureDetail> {
        let detail: FailureDetail =
            decode(self.operation("failure_detail", project, operation)?)?;
        if &detail.project_id != project
            || detail.code.len() > 96
            || !detail
                .code
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err(Error::Protocol);
        }
        Ok(detail)
    }
    pub fn cancel(&mut self, project: &Id, operation: &Id) -> Result<Cancellation> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Reply {
            cancellation: String,
        }
        let reply: Reply = decode(self.operation("cancel", project, operation)?)?;
        match reply.cancellation.as_str() {
            "requested" => Ok(Cancellation::Requested),
            "already_terminal" => Ok(Cancellation::AlreadyTerminal),
            _ => Err(Error::Protocol),
        }
    }
    pub fn quit(&mut self) -> Result<()> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Reply {
            state: String,
            shutdown_id: String,
        }
        let reply: Reply = decode(self.authenticated("quit", json!({}))?)?;
        if reply.state != "quitting"
            || reply.shutdown_id.len() != 64
            || !reply
                .shutdown_id
                .bytes()
                .all(|value| value.is_ascii_digit() || (b'a'..=b'f').contains(&value))
        {
            return Err(Error::Protocol);
        }
        self.shutdown_id = Some(reply.shutdown_id);
        self.session = None;
        Ok(())
    }
    /// Private native-owner administration only. The one-time bearer is returned
    /// to trusted Rust composition, never an external JSON CLI or renderer.
    pub fn issue_external_grant(
        &mut self,
        project: &Id,
        scopes: &[devforge_control::Scope],
        ttl: Duration,
    ) -> Result<crate::external_auth::IssuedGrant> {
        let ttl_ms = u64::try_from(ttl.as_millis()).map_err(|_| Error::InputLimit)?;
        let mut value = self.authenticated(
            "external_grant_issue",
            json!({"project_id":project,"scopes":scopes,"ttl_ms":ttl_ms}),
        )?;
        let credential = match value.get_mut("credential") {
            Some(Value::String(value)) => Zeroizing::new(std::mem::take(value)),
            _ => return Err(Error::Protocol),
        };
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Issued {
            record: crate::external_auth::GrantRecord,
            credential: String,
        }
        let parsed: Issued = decode(value)?;
        if !parsed.credential.is_empty() || &parsed.record.project_id != project {
            return Err(Error::Protocol);
        }
        Ok(crate::external_auth::IssuedGrant {
            record: parsed.record,
            credential: crate::external_auth::ExternalCredential::from_wire(&credential)
                .map_err(|_| Error::Protocol)?,
        })
    }
    pub fn list_external_grants(&mut self) -> Result<Vec<crate::external_auth::GrantRecord>> {
        decode(self.authenticated("external_grant_list", json!({}))?)
    }
    pub fn revoke_external_grant(&mut self, grant: &Id) -> Result<()> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Revoked {
            revoked: bool,
        }
        let reply: Revoked =
            decode(self.authenticated("external_grant_revoke", json!({"grant_id":grant}))?)?;
        if reply.revoked {
            Ok(())
        } else {
            Err(Error::Protocol)
        }
    }
    /// Confirms this authenticated Quit's outcome after cleanup, without any
    /// credential access. Internally holds the legacy exclusion while reading;
    /// a missing/pending/stale record never proves successful cleanup.
    pub fn confirm_shutdown(&self) -> Result<()> {
        let owner = self
            .shutdown_id
            .as_deref()
            .ok_or_else(|| Error::Remote(RemoteCode("controller_shutdown_unconfirmed".into())))?;
        crate::controller::confirm_shutdown(self.transport.endpoint(), owner)
            .map_err(|error| Error::Remote(RemoteCode(error.code().into())))
    }
    /// Reap only an owned launched child's orderly exit. Never kills/relaunches.
    pub fn wait_for_launched_exit(&mut self) -> Result<Option<ExitStatus>> {
        self.launched
            .take()
            .map(|child| child.wait_for_exit().map_err(|_| Error::OwnerUnavailable))
            .transpose()
    }
    fn operation(&mut self, method: &str, project: &Id, operation: &Id) -> Result<Value> {
        self.authenticated(
            method,
            json!({"project_id":project,"operation_id":operation}),
        )
    }
    fn require_session(&mut self) -> Result<()> {
        let now = Instant::now();
        if self
            .session
            .as_ref()
            .is_none_or(|s| now >= s.expires || s.context_deadline.is_some_and(|d| now >= d))
        {
            self.session = None;
            return Err(Error::ReauthenticationRequired);
        }
        Ok(())
    }
    fn authenticated(&mut self, method: &str, mut fields: Value) -> Result<Value> {
        if let Err(error) = self.require_session() {
            scrub_request(&mut fields);
            return Err(error);
        }
        let object = fields.as_object_mut().ok_or(Error::Protocol)?;
        object.insert("method".into(), Value::String(method.into()));
        object.insert(
            "session".into(),
            Value::String(
                self.session
                    .as_ref()
                    .ok_or(Error::ReauthenticationRequired)?
                    .credential
                    .expose_secret()
                    .into(),
            ),
        );
        self.data(fields)
    }
    fn data(&mut self, request: Value) -> Result<Value> {
        let response = self.raw(request)?;
        match data(response) {
            Err(Error::ReauthenticationRequired) => {
                self.session = None;
                Err(Error::ReauthenticationRequired)
            }
            other => other,
        }
    }
    fn raw(&mut self, mut request: Value) -> Result<Value> {
        if let Err(error) = validate_value_size(&request, crate::MAX_REQUEST_BYTES) {
            scrub_request(&mut request);
            return Err(error);
        }
        let response = self
            .transport
            .request_with_expected_peer(&self.peer, &request);
        scrub_request(&mut request);
        match response {
            Ok(response) => Ok(response),
            Err(crate::Error::UnauthorizedPeer | crate::Error::UnsafeEndpoint) => {
                self.session = None;
                Err(Error::OwnerChanged)
            }
            Err(crate::Error::InvalidFrame | crate::Error::FrameTooLarge) => Err(Error::Protocol),
            Err(_) => Err(Error::OwnerUnavailable),
        }
    }
    fn install_session(
        &mut self,
        mut response: Value,
        started: Instant,
        previous_deadline: Option<Instant>,
    ) -> Result<()> {
        // Remove/wrap the secret before any fallible envelope/metadata parsing.
        let token = response
            .get_mut("data")
            .and_then(Value::as_object_mut)
            .and_then(|d| d.remove("session"));
        let token = match token {
            Some(Value::String(value)) => Some(Zeroizing::new(value)),
            _ => None,
        };
        let metadata: SessionMetadata = decode(data(response)?)?;
        let token = token.ok_or(Error::Protocol)?;
        if metadata.expires_in_ms == 0 || metadata.context_remaining_ms == Some(0) {
            return Err(Error::ReauthenticationRequired);
        }
        if metadata.expires_in_ms > 3_600_000
            || metadata.renewable != metadata.context_remaining_ms.is_some()
            || metadata.context_remaining_ms.is_some_and(|value| {
                value == 0 || value > 86_400_000 || value < metadata.expires_in_ms
            })
        {
            return Err(Error::Protocol);
        }
        let deadline = |ms| {
            started
                .checked_add(Duration::from_millis(ms))
                .and_then(|d| d.checked_sub(EXPIRY_MARGIN))
                .ok_or(Error::Protocol)
        };
        if previous_deadline.is_some() && !metadata.renewable {
            return Err(Error::Protocol);
        }
        let context_deadline = metadata
            .context_remaining_ms
            .map(deadline)
            .transpose()?
            .map(|d| previous_deadline.map_or(d, |old| old.min(d)));
        let token_deadline = deadline(metadata.expires_in_ms)?;
        let expires = context_deadline.map_or(token_deadline, |d| d.min(token_deadline));
        if Instant::now() >= expires {
            return Err(Error::ReauthenticationRequired);
        }
        let credential = SessionCredential::from_wire(&token).map_err(|_| Error::Protocol)?;
        self.session = Some(Session {
            credential,
            expires,
            context_deadline,
            renewable: metadata.renewable,
        });
        Ok(())
    }
}

fn check_admission_cancel(cancel: &AdmissionCancellation) -> Result<()> {
    if cancel.is_cancelled() {
        Err(Error::AdmissionCancelled)
    } else {
        Ok(())
    }
}

fn observe(endpoint: &Endpoint) -> Result<(PeerIdentity, Health)> {
    let (peer, response) = Client::new(endpoint.clone())
        .request_with_peer(&json!({"method":"health"}))
        .map_err(|_| Error::OwnerUnavailable)?;
    let health: Health = decode(data(response)?)?;
    validate_health(&peer, &health)?;
    Ok((peer, health))
}
fn validate_health(peer: &PeerIdentity, health: &Health) -> Result<()> {
    if health.api_version != 1 {
        return Err(Error::UnsupportedProtocol);
    }
    if peer.process_id.is_none()
        || peer.process_id == Some(0)
        || peer.process_id != Some(health.owner_pid)
    {
        return Err(Error::OwnerChanged);
    }
    Ok(())
}
fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| Error::Protocol)
}
fn data(mut response: Value) -> Result<Value> {
    let object = response.as_object_mut().ok_or(Error::Protocol)?;
    match object.remove("ok") {
        Some(Value::Bool(true)) if object.len() == 1 => {
            object.remove("data").ok_or(Error::Protocol)
        }
        Some(Value::Bool(false)) if object.len() == 1 => {
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Fault {
                code: String,
            }
            let fault: Fault = decode(object.remove("error").ok_or(Error::Protocol)?)?;
            let code = RemoteCode::parse(fault.code)?;
            match code.as_str() {
                "manager_auth_unauthorized"
                | "manager_auth_not_provisioned"
                | "manager_auth_bootstrap_unavailable" => Err(Error::ReauthenticationRequired),
                "payload_resync_required" => Err(Error::PayloadResyncRequired),
                "actor_read_expired" => Err(Error::ReadExpired),
                _ => Err(Error::Remote(code)),
            }
        }
        _ => Err(Error::Protocol),
    }
}
fn scrub_request(request: &mut Value) {
    if let Some(object) = request.as_object_mut() {
        for key in ["session", "credential", "token", "data_hex"] {
            if let Some(Value::String(value)) = object.get_mut(key) {
                value.zeroize();
            }
        }
        if let Some(Value::Object(request)) = object.get_mut("request")
            && let Some(Value::String(value)) = request.get_mut("value")
        {
            value.zeroize();
        }
    }
}
fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(HEX[(byte >> 4) as usize] as char);
        text.push(HEX[(byte & 15) as usize] as char);
    }
    text
}
fn digest_bytes(text: &str) -> Result<[u8; 32]> {
    let bytes = decode_hex(text, 32)?;
    bytes.as_slice().try_into().map_err(|_| Error::Protocol)
}
fn decode_chunk(text: &str) -> Result<Zeroizing<Vec<u8>>> {
    decode_hex(text, CHUNK_BYTES)
}
fn decode_hex(text: &str, max: usize) -> Result<Zeroizing<Vec<u8>>> {
    if !text.len().is_multiple_of(2) || text.len() / 2 > max {
        return Err(Error::Protocol);
    }
    let mut bytes = Zeroizing::new(Vec::with_capacity(text.len() / 2));
    for pair in text.as_bytes().as_chunks::<2>().0 {
        let digit = |b| match b {
            b'0'..=b'9' => Ok(b - b'0'),
            b'a'..=b'f' => Ok(b - b'a' + 10),
            _ => Err(Error::Protocol),
        };
        bytes.push((digit(pair[0])? << 4) | digit(pair[1])?);
    }
    Ok(bytes)
}
struct BoundedBytes(Zeroizing<Vec<u8>>);
impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_COMMAND_BYTES.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other("native command limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
struct WireHex(Zeroizing<String>);
impl<'de> Deserialize<'de> for WireHex {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let value = Zeroizing::new(String::deserialize(deserializer)?);
        if value.len() > CHUNK_BYTES * 2 {
            return Err(serde::de::Error::custom("chunk limit"));
        }
        Ok(Self(value))
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    snapshot_id: Id,
    epoch: Id,
    offset: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotBegin {
    snapshot_id: Id,
    epoch: Id,
    size: u64,
    digest: String,
    observed_at: u64,
    cursor: Cursor,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SnapshotPage {
    snapshot_id: Id,
    epoch: Id,
    offset: u64,
    data_hex: WireHex,
    next_cursor: Option<Cursor>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UploadBegin {
    upload_id: Id,
    epoch: Id,
    size: u64,
    digest: String,
    next_index: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UploadChunk {
    upload_id: Id,
    epoch: Id,
    received: u64,
    next_index: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadDescriptor {
    snapshot_id: Id,
    epoch: Id,
    size: u64,
    digest: String,
    observed_at: u64,
    cursor: Cursor,
    expires_in_ms: u64,
}
fn validate_cursor(cursor: &Value) -> Result<()> {
    if !cursor.is_object() {
        return Err(Error::InputLimit);
    }
    validate_value_size(cursor, 2048)
}
fn validate_value_size(value: &Value, limit: usize) -> Result<()> {
    struct Count {
        used: usize,
        limit: usize,
    }
    impl Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.used) {
                return Err(std::io::Error::other("cursor limit"));
            }
            self.used += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(&mut Count { used: 0, limit }, value).map_err(|_| Error::InputLimit)
}
