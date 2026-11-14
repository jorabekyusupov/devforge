//! Blocking Rust-only SDK for a future native desktop composition, not UI IPC.
//!
//! Keep this object and all credentials inside Rust. Call from a worker, never a
//! WebView/UI thread. There is no SecretStore, provider fallback, implicit launch,
//! automatic renewal, raw RPC escape hatch or credential serializer here.
use crate::{
    Client, Endpoint, PeerIdentity,
    auth::{DesktopCredential, SessionCredential, SessionInfo},
    launch::{BootstrappedChild, ReadyChild},
    payload::{CHUNK_BYTES, MAX_COMMAND_BYTES, MAX_SNAPSHOT_BYTES},
};
use devforge_control::{Id, OperationState};
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
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureScope {
    Project,
    Runtime,
    Serving,
    Workspace,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FailureDetail {
    pub scope: FailureScope,
    pub code: String,
    pub project_id: Id,
    pub retryable: bool,
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
    session: Option<Session>,
    launched: Option<ReadyChild>,
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
            transport: Client::new(endpoint),
            peer,
            session: None,
            launched: None,
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
            transport: Client::new(endpoint),
            peer,
            session: None,
            launched: Some(monitor),
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
    pub fn snapshot(&mut self) -> Result<Snapshot> {
        let started = Instant::now();
        let begin: SnapshotBegin = decode(self.authenticated("snapshot_begin", json!({}))?)?;
        let result = self.collect_snapshot(&begin, started);
        self.release_snapshot(&begin);
        result
    }
    fn collect_snapshot(&mut self, begin: &SnapshotBegin, started: Instant) -> Result<Snapshot> {
        let size = bounded_size(begin.size)?;
        let expected_digest = digest_bytes(&begin.digest)?;
        if begin.cursor.snapshot_id != begin.snapshot_id
            || begin.cursor.epoch != begin.epoch
            || begin.cursor.offset != 0
        {
            return Err(Error::Protocol);
        }
        let mut bytes = Zeroizing::new(Vec::with_capacity(size));
        let mut cursor = begin.cursor.clone();
        loop {
            if started.elapsed() >= EXPORT_TTL {
                return Err(Error::PayloadResyncRequired);
            }
            let page: SnapshotPage =
                decode(self.authenticated("snapshot_page", json!({"cursor":cursor}))?)?;
            if page.snapshot_id != begin.snapshot_id
                || page.epoch != begin.epoch
                || page.offset != bytes.len() as u64
            {
                return Err(Error::Protocol);
            }
            let decoded = decode_chunk(&page.data_hex.0)?;
            if decoded.is_empty() || decoded.len() > size.saturating_sub(bytes.len()) {
                return Err(Error::Protocol);
            }
            bytes.extend_from_slice(&decoded);
            match page.next_cursor {
                None if bytes.len() == size => break,
                Some(next)
                    if bytes.len() < size
                        && next.snapshot_id == begin.snapshot_id
                        && next.epoch == begin.epoch
                        && next.offset == bytes.len() as u64 =>
                {
                    cursor = next
                }
                _ => return Err(Error::Protocol),
            }
        }
        if <[u8; 32]>::from(Sha256::digest(bytes.as_slice())) != expected_digest {
            return Err(Error::Protocol);
        }
        // Decode only after complete byte reassembly; UTF-8 may split in a page.
        let snapshot: Snapshot =
            serde_json::from_slice(bytes.as_slice()).map_err(|_| Error::Protocol)?;
        if snapshot.observed_at != begin.observed_at
            || !snapshot.snapshot.is_object()
            || ["projects", "runtimes", "databases", "logs"]
                .iter()
                .any(|name| !snapshot.snapshot.get(name).is_some_and(Value::is_array))
        {
            return Err(Error::Protocol);
        }
        Ok(snapshot)
    }
    fn release_snapshot(&mut self, begin: &SnapshotBegin) {
        // Export TTL is fixed/bounded. Preserve the original error or verified
        // snapshot if best-effort release cannot be delivered; never renew here.
        let _ = self.authenticated(
            "snapshot_release",
            json!({"snapshot_id":begin.snapshot_id,"epoch":begin.epoch}),
        );
    }
    /// Upload/reassembly is bounded; caller identity/idempotency is explicit.
    /// Finalization is sent once. Ambiguous transport failure never triggers an
    /// automatic second admission; the caller retains its idempotency key.
    pub fn admit(&mut self, project: &Id, idempotency: &Id, command: &Command) -> Result<Receipt> {
        self.require_session()?;
        let mut writer = BoundedBytes(Zeroizing::new(Vec::new()));
        serde_json::to_writer(&mut writer, command).map_err(|_| Error::InputLimit)?;
        let bytes = writer.0;
        let digest = hex(&Sha256::digest(bytes.as_slice()));
        let begin: UploadBegin = decode(self.authenticated("upload_begin", json!({"project_id":project,"idempotency_key":idempotency,"size":bytes.len(),"digest":digest}))?)?;
        let result = self.upload_command(&begin, project, &bytes, &digest);
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
    ) -> Result<Receipt> {
        if begin.size != bytes.len() as u64 || begin.digest != digest || begin.next_index != 0 {
            return Err(Error::Protocol);
        }
        let mut received = 0;
        for (index, chunk) in bytes.chunks(CHUNK_BYTES).enumerate() {
            let reply: UploadChunk = decode(self.authenticated("upload_chunk", json!({"upload_id":begin.upload_id,"epoch":begin.epoch,"index":index,"data_hex":hex(chunk)}))?)?;
            received += chunk.len();
            if reply.upload_id != begin.upload_id
                || reply.epoch != begin.epoch
                || reply.received != received as u64
                || reply.next_index != index as u32 + 1
            {
                return Err(Error::Protocol);
            }
        }
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
        }
        let reply: Reply = decode(self.authenticated("quit", json!({}))?)?;
        if reply.state != "quitting" {
            return Err(Error::Protocol);
        }
        self.session = None;
        Ok(())
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
        self.require_session()?;
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
    }
}
fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
fn bounded_size(size: u64) -> Result<usize> {
    if size == 0 || size > MAX_SNAPSHOT_BYTES as u64 {
        return Err(Error::Protocol);
    }
    usize::try_from(size).map_err(|_| Error::Protocol)
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
