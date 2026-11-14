//! Authenticated private memory-only large commands and stable snapshot exports.
//! Public control frames stay64KiB. No body/header data is written to disk.
use crate::{
    Endpoint, PeerIdentity,
    actor::{ActorError, ActorHandle, OperationIdentity, SnapshotView},
    auth::{DesktopAuthority, SessionCredential},
};
use devforge_control::{Digest as WireDigest, Id};
use devforge_workspace::Command;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

pub const MAX_COMMAND_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_PENDING_UPLOADS: usize = 2;
pub const MAX_PENDING_BYTES: usize = 16 * 1024 * 1024;
pub const CHUNK_BYTES: usize = 16 * 1024;
pub const MAX_SNAPSHOT_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_SNAPSHOTS: usize = 2;
pub const MAX_SNAPSHOT_TOTAL_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Unauthorized,
    InvalidInput,
    Capacity,
    UnknownUpload,
    Expired,
    OrderMismatch,
    SizeMismatch,
    DigestMismatch,
    ResyncRequired,
    OutputLimit,
    EntropyUnavailable,
    Admission(ActorError),
}
impl Error {
    pub fn code(self) -> &'static str {
        match self {
            Self::Unauthorized => "payload_unauthorized",
            Self::InvalidInput => "payload_invalid_input",
            Self::Capacity => "payload_capacity",
            Self::UnknownUpload => "payload_unknown_upload",
            Self::Expired => "payload_expired",
            Self::OrderMismatch => "payload_order_mismatch",
            Self::SizeMismatch => "payload_size_mismatch",
            Self::DigestMismatch => "payload_digest_mismatch",
            Self::ResyncRequired => "payload_resync_required",
            Self::OutputLimit => "payload_output_limit",
            Self::EntropyUnavailable => "payload_entropy_unavailable",
            Self::Admission(error) => error.code(),
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Copy)]
pub struct Policy {
    pub ttl: Duration,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            ttl: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UploadKey {
    pub upload_id: Id,
    pub epoch: Id,
}
#[derive(Debug, Clone, Serialize)]
pub struct UploadDescriptor {
    pub upload_id: Id,
    pub epoch: Id,
    pub size: u64,
    pub digest: WireDigest,
    pub next_index: u32,
}
#[derive(Debug, Clone, Serialize)]
pub struct UploadProgress {
    pub upload_id: Id,
    pub epoch: Id,
    pub received: u64,
    pub next_index: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotCursor {
    pub snapshot_id: Id,
    pub epoch: Id,
    pub offset: u64,
}
#[derive(Debug, Clone, Serialize)]
pub struct SnapshotDescriptor {
    pub snapshot_id: Id,
    pub epoch: Id,
    pub size: u64,
    pub digest: WireDigest,
    pub observed_at: u64,
    pub cursor: SnapshotCursor,
}
/// data_hex is private data; deliberately no Debug/Deserialize or logging path.
#[derive(Serialize)]
pub struct SnapshotChunk {
    pub snapshot_id: Id,
    pub epoch: Id,
    pub offset: u64,
    pub data_hex: String,
    pub next_cursor: Option<SnapshotCursor>,
}
#[derive(Clone)]
struct Binding {
    root: PathBuf,
    peer: PeerIdentity,
    epoch: Id,
}
impl Binding {
    fn check(&self, authority: &DesktopAuthority<'_>, epoch: &Id, now: Instant) -> Result<()> {
        authority
            .check_current(now)
            .map_err(|_| Error::Unauthorized)?;
        if authority.root() != self.root || authority.peer() != &self.peer || epoch != &self.epoch {
            return Err(Error::Unauthorized);
        }
        Ok(())
    }
}
struct Upload {
    binding: Binding,
    identity: OperationIdentity,
    size: usize,
    digest: WireDigest,
    next_index: u32,
    bytes: Zeroizing<Vec<u8>>,
    expires: Instant,
}
struct Export {
    binding: Binding,
    bytes: Zeroizing<Vec<u8>>,
    expires: Instant,
}
pub struct Store {
    endpoint: Endpoint,
    epoch: Id,
    policy: Policy,
    uploads: HashMap<Id, Upload>,
    snapshots: HashMap<Id, Export>,
}
/// One-use validated command. It cannot be decoded from wire JSON or cloned.
pub struct PreparedCommand {
    binding: Binding,
    identity: OperationIdentity,
    command: Command,
    expires: Instant,
}
impl std::fmt::Debug for PreparedCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PreparedCommand([REDACTED])")
    }
}
impl PreparedCommand {
    pub fn into_parts(
        self,
        authority: &DesktopAuthority<'_>,
    ) -> Result<(OperationIdentity, Command)> {
        self.binding
            .check(authority, &self.binding.epoch, Instant::now())?;
        if Instant::now() >= self.expires {
            return Err(Error::Expired);
        }
        Ok((self.identity, self.command))
    }
}
impl Store {
    pub(crate) fn clear(&mut self) {
        self.uploads.clear();
        self.snapshots.clear();
    }
    pub fn new(endpoint: Endpoint, policy: Policy) -> Result<Self> {
        endpoint.validate().map_err(|_| Error::Unauthorized)?;
        if policy.ttl.is_zero() || policy.ttl > Duration::from_secs(120) {
            return Err(Error::InvalidInput);
        }
        Ok(Self {
            endpoint,
            epoch: random_id("epoch")?,
            policy,
            uploads: HashMap::new(),
            snapshots: HashMap::new(),
        })
    }
    fn binding(&self, authority: &DesktopAuthority<'_>, now: Instant) -> Result<Binding> {
        authority
            .check_current(now)
            .map_err(|_| Error::Unauthorized)?;
        if authority.root() != self.endpoint.root() {
            return Err(Error::Unauthorized);
        }
        Ok(Binding {
            root: authority.root().to_owned(),
            peer: authority.peer().clone(),
            epoch: self.epoch.clone(),
        })
    }
    /// Trusted maintenance only, not a wire method. Dropping expired buffers
    /// zeroizes the store-owned encoded bytes. Controller schedules this too.
    pub fn expire(&mut self, now: Instant) {
        self.uploads.retain(|_, value| now < value.expires);
        self.snapshots.retain(|_, value| now < value.expires);
    }
    pub fn begin(
        &mut self,
        authority: &DesktopAuthority<'_>,
        identity: OperationIdentity,
        size: u64,
        digest: WireDigest,
        now: Instant,
    ) -> Result<UploadDescriptor> {
        let binding = self.binding(authority, now)?;
        self.expire(now);
        let size = usize::try_from(size).map_err(|_| Error::InvalidInput)?;
        if size == 0 || size > MAX_COMMAND_BYTES {
            return Err(Error::InvalidInput);
        }
        let reserved: usize = self.uploads.values().map(|value| value.size).sum();
        if self.uploads.len() >= MAX_PENDING_UPLOADS
            || size > MAX_PENDING_BYTES.saturating_sub(reserved)
        {
            return Err(Error::Capacity);
        }
        let upload_id = random_id("upload")?;
        self.uploads.insert(
            upload_id.clone(),
            Upload {
                binding,
                identity,
                size,
                digest: digest.clone(),
                next_index: 0,
                bytes: Zeroizing::new(Vec::new()),
                expires: now + self.policy.ttl,
            },
        );
        Ok(UploadDescriptor {
            upload_id,
            epoch: self.epoch.clone(),
            size: size as u64,
            digest,
            next_index: 0,
        })
    }
    pub fn chunk(
        &mut self,
        authority: &DesktopAuthority<'_>,
        key: &UploadKey,
        index: u32,
        data_hex: &str,
        now: Instant,
    ) -> Result<UploadProgress> {
        self.binding(authority, now)?;
        let upload = self
            .uploads
            .get(&key.upload_id)
            .ok_or(Error::UnknownUpload)?;
        upload.binding.check(authority, &key.epoch, now)?;
        if now >= upload.expires {
            self.uploads.remove(&key.upload_id);
            return Err(Error::Expired);
        }
        let decoded = decode_chunk(data_hex);
        if decoded.is_err() || index != upload.next_index {
            self.uploads.remove(&key.upload_id);
            return Err(if decoded.is_err() {
                Error::InvalidInput
            } else {
                Error::OrderMismatch
            });
        }
        let decoded = decoded?;
        let upload = self
            .uploads
            .get_mut(&key.upload_id)
            .ok_or(Error::UnknownUpload)?;
        if decoded.len() > upload.size.saturating_sub(upload.bytes.len()) {
            self.uploads.remove(&key.upload_id);
            return Err(Error::SizeMismatch);
        }
        upload.bytes.extend_from_slice(&decoded);
        upload.next_index += 1;
        Ok(UploadProgress {
            upload_id: key.upload_id.clone(),
            epoch: self.epoch.clone(),
            received: upload.bytes.len() as u64,
            next_index: upload.next_index,
        })
    }
    pub fn finalize(
        &mut self,
        authority: &DesktopAuthority<'_>,
        key: &UploadKey,
        now: Instant,
    ) -> Result<PreparedCommand> {
        self.binding(authority, now)?;
        self.uploads
            .get(&key.upload_id)
            .ok_or(Error::UnknownUpload)?
            .binding
            .check(authority, &key.epoch, now)?;
        // Consume before parsing/admission. Any incomplete/tampered/replayed
        // finalize has no executable command to return.
        let upload = self
            .uploads
            .remove(&key.upload_id)
            .ok_or(Error::UnknownUpload)?;
        if now >= upload.expires {
            return Err(Error::Expired);
        }
        if upload.bytes.len() != upload.size {
            return Err(Error::SizeMismatch);
        }
        if sha256(&upload.bytes) != upload.digest {
            return Err(Error::DigestMismatch);
        }
        let command =
            serde_json::from_slice::<Command>(&upload.bytes).map_err(|_| Error::InvalidInput)?;
        let after = Instant::now().max(now);
        upload.binding.check(authority, &key.epoch, after)?;
        if after >= upload.expires {
            return Err(Error::Expired);
        }
        Ok(PreparedCommand {
            binding: upload.binding,
            identity: upload.identity,
            command,
            expires: upload.expires,
        })
    }
    pub fn abort(
        &mut self,
        authority: &DesktopAuthority<'_>,
        key: &UploadKey,
        now: Instant,
    ) -> Result<()> {
        self.binding(authority, now)?;
        self.uploads
            .get(&key.upload_id)
            .ok_or(Error::UnknownUpload)?
            .binding
            .check(authority, &key.epoch, now)?;
        self.uploads.remove(&key.upload_id);
        Ok(())
    }
    pub fn snapshot(
        &mut self,
        authority: &DesktopAuthority<'_>,
        view: &SnapshotView,
        now: Instant,
    ) -> Result<SnapshotDescriptor> {
        let binding = self.binding(authority, now)?;
        self.expire(now);
        if self.snapshots.len() >= MAX_SNAPSHOTS {
            return Err(Error::Capacity);
        }
        #[derive(Serialize)]
        struct Data<'a> {
            snapshot: &'a devforge_workspace::Snapshot,
            observed_at: u64,
            busy: bool,
            stopped: bool,
            error: Option<&'static str>,
            progress: &'a Option<devforge_workspace::setup::Progress>,
        }
        let bytes = encode(
            &Data {
                snapshot: &view.snapshot,
                observed_at: view.observed_at,
                busy: view.busy,
                stopped: view.stopped,
                error: view.error.map(|error| error.code()),
                progress: &view.progress,
            },
            MAX_SNAPSHOT_BYTES,
        )?;
        let retained: usize = self.snapshots.values().map(|value| value.bytes.len()).sum();
        if bytes.len() > MAX_SNAPSHOT_TOTAL_BYTES.saturating_sub(retained) {
            return Err(Error::Capacity);
        }
        let after = Instant::now().max(now);
        binding.check(authority, &self.epoch, after)?;
        let snapshot_id = random_id("snapshot")?;
        let descriptor = SnapshotDescriptor {
            snapshot_id: snapshot_id.clone(),
            epoch: self.epoch.clone(),
            size: bytes.len() as u64,
            digest: sha256(&bytes),
            observed_at: view.observed_at,
            cursor: SnapshotCursor {
                snapshot_id: snapshot_id.clone(),
                epoch: self.epoch.clone(),
                offset: 0,
            },
        };
        self.snapshots.insert(
            snapshot_id,
            Export {
                binding,
                bytes,
                expires: after + self.policy.ttl,
            },
        );
        Ok(descriptor)
    }
    pub fn page(
        &mut self,
        authority: &DesktopAuthority<'_>,
        cursor: &SnapshotCursor,
        now: Instant,
    ) -> Result<SnapshotChunk> {
        self.binding(authority, now)?;
        if cursor.epoch != self.epoch {
            return Err(Error::ResyncRequired);
        }
        let snapshot = self
            .snapshots
            .get(&cursor.snapshot_id)
            .ok_or(Error::ResyncRequired)?;
        snapshot.binding.check(authority, &cursor.epoch, now)?;
        if now >= snapshot.expires {
            self.snapshots.remove(&cursor.snapshot_id);
            return Err(Error::ResyncRequired);
        }
        let offset = usize::try_from(cursor.offset).map_err(|_| Error::ResyncRequired)?;
        if offset >= snapshot.bytes.len() || !offset.is_multiple_of(CHUNK_BYTES) {
            return Err(Error::ResyncRequired);
        }
        let end = (offset + CHUNK_BYTES).min(snapshot.bytes.len());
        Ok(SnapshotChunk {
            snapshot_id: cursor.snapshot_id.clone(),
            epoch: self.epoch.clone(),
            offset: cursor.offset,
            data_hex: hex(&snapshot.bytes[offset..end]),
            next_cursor: (end < snapshot.bytes.len()).then(|| SnapshotCursor {
                snapshot_id: cursor.snapshot_id.clone(),
                epoch: self.epoch.clone(),
                offset: end as u64,
            }),
        })
    }
    pub fn release(
        &mut self,
        authority: &DesktopAuthority<'_>,
        snapshot_id: &Id,
        epoch: &Id,
        now: Instant,
    ) -> Result<()> {
        self.binding(authority, now)?;
        self.snapshots
            .get(snapshot_id)
            .ok_or(Error::ResyncRequired)?
            .binding
            .check(authority, epoch, now)?;
        self.snapshots.remove(snapshot_id);
        Ok(())
    }
}

pub fn encode_command(command: &Command) -> Result<Zeroizing<Vec<u8>>> {
    encode(command, MAX_COMMAND_BYTES)
}
pub fn sha256(bytes: &[u8]) -> WireDigest {
    WireDigest::new(hex(&Sha256::digest(bytes))).expect("owned SHA256 shape")
}
pub fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 15) as usize] as char);
    }
    out
}
pub fn decode_chunk(value: &str) -> Result<Zeroizing<Vec<u8>>> {
    if value.is_empty() || value.len() > CHUNK_BYTES * 2 || !value.len().is_multiple_of(2) {
        return Err(Error::InvalidInput);
    }
    let mut out = Zeroizing::new(Vec::with_capacity(value.len() / 2));
    for pair in value.as_bytes().as_chunks::<2>().0 {
        let digit = |byte| match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            _ => None,
        };
        out.push(
            (digit(pair[0]).ok_or(Error::InvalidInput)? << 4)
                | digit(pair[1]).ok_or(Error::InvalidInput)?,
        );
    }
    Ok(out)
}
fn random_id(prefix: &str) -> Result<Id> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| Error::EntropyUnavailable)?;
    Id::new(format!("{prefix}-{}", hex(&bytes))).map_err(|_| Error::InvalidInput)
}
struct Buffer {
    bytes: Zeroizing<Vec<u8>>,
    limit: usize,
}
impl Write for Buffer {
    fn write(&mut self, input: &[u8]) -> std::io::Result<usize> {
        if input.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other("payload limit"));
        }
        self.bytes.extend_from_slice(input);
        Ok(input.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn encode<T: Serialize>(data: &T, limit: usize) -> Result<Zeroizing<Vec<u8>>> {
    let mut buffer = Buffer {
        bytes: Zeroizing::new(Vec::new()),
        limit,
    };
    serde_json::to_writer(&mut buffer, data).map_err(|_| Error::OutputLimit)?;
    Ok(buffer.bytes)
}

pub(crate) struct Session(Zeroizing<String>);
pub(crate) struct HexData(Zeroizing<String>);
impl<'de> Deserialize<'de> for HexData {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let value = Zeroizing::new(String::deserialize(deserializer)?);
        if value.is_empty()
            || value.len() > CHUNK_BYTES * 2
            || !value.len().is_multiple_of(2)
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(serde::de::Error::custom("invalid chunk"));
        }
        Ok(Self(value))
    }
}
impl<'de> Deserialize<'de> for Session {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let value = Zeroizing::new(String::deserialize(deserializer)?);
        SessionCredential::from_wire(&value)
            .map_err(|_| serde::de::Error::custom("invalid session"))?;
        Ok(Self(value))
    }
}
/// Private DTOs only. Controller supplies native session authority and Actor.
#[derive(Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Request {
    UploadBegin {
        session: Session,
        project_id: Id,
        idempotency_key: Id,
        size: u64,
        digest: WireDigest,
    },
    UploadChunk {
        session: Session,
        upload_id: Id,
        epoch: Id,
        index: u32,
        data_hex: HexData,
    },
    UploadFinalize {
        session: Session,
        upload_id: Id,
        epoch: Id,
    },
    UploadAbort {
        session: Session,
        upload_id: Id,
        epoch: Id,
    },
    SnapshotBegin {
        session: Session,
    },
    SnapshotPage {
        session: Session,
        cursor: SnapshotCursor,
    },
    SnapshotRelease {
        session: Session,
        snapshot_id: Id,
        epoch: Id,
    },
}
impl Request {
    pub(crate) fn handles(method: &str) -> bool {
        matches!(
            method,
            "upload_begin"
                | "upload_chunk"
                | "upload_finalize"
                | "upload_abort"
                | "snapshot_begin"
                | "snapshot_page"
                | "snapshot_release"
        )
    }
    pub(crate) fn credential(&self) -> Result<SessionCredential> {
        let session = match self {
            Self::UploadBegin { session, .. }
            | Self::UploadChunk { session, .. }
            | Self::UploadFinalize { session, .. }
            | Self::UploadAbort { session, .. }
            | Self::SnapshotBegin { session }
            | Self::SnapshotPage { session, .. }
            | Self::SnapshotRelease { session, .. } => session,
        };
        SessionCredential::from_wire(&session.0).map_err(|_| Error::Unauthorized)
    }
    pub(crate) fn dispatch(
        self,
        authority: &DesktopAuthority<'_>,
        store: &mut Store,
        actor: &ActorHandle,
    ) -> Result<Value> {
        let now = Instant::now();
        let value = match self {
            Self::UploadBegin {
                project_id,
                idempotency_key,
                size,
                digest,
                ..
            } => serde_json::to_value(store.begin(
                authority,
                OperationIdentity {
                    project_id,
                    idempotency_key,
                },
                size,
                digest,
                now,
            )?),
            Self::UploadChunk {
                upload_id,
                epoch,
                index,
                data_hex,
                ..
            } => serde_json::to_value(store.chunk(
                authority,
                &UploadKey { upload_id, epoch },
                index,
                &data_hex.0,
                now,
            )?),
            Self::UploadFinalize {
                upload_id, epoch, ..
            } => {
                let (identity, command) = store
                    .finalize(authority, &UploadKey { upload_id, epoch }, now)?
                    .into_parts(authority)?;
                serde_json::to_value(
                    actor
                        .submit_desktop(authority, identity, command)
                        .map_err(Error::Admission)?,
                )
            }
            Self::UploadAbort {
                upload_id, epoch, ..
            } => {
                store.abort(authority, &UploadKey { upload_id, epoch }, now)?;
                Ok(serde_json::json!({"aborted":true}))
            }
            Self::SnapshotBegin { .. } => {
                let view = actor.cached_snapshot(authority).map_err(Error::Admission)?;
                serde_json::to_value(store.snapshot(authority, &view, now)?)
            }
            Self::SnapshotPage { cursor, .. } => {
                serde_json::to_value(store.page(authority, &cursor, now)?)
            }
            Self::SnapshotRelease {
                snapshot_id, epoch, ..
            } => {
                store.release(authority, &snapshot_id, &epoch, now)?;
                Ok(serde_json::json!({"released":true}))
            }
        };
        value.map_err(|_| Error::InvalidInput)
    }
}
