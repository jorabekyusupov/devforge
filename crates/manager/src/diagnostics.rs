//! Four metadata projections only. Raw events/mail and content search are never
//! accepted by this adapter. Pagination retains bounded observed safe rows.
use crate::{
    PeerIdentity,
    actor::ActorHandle,
    external_auth::ExternalCredential,
    external_control::{Server as Grants, grant_error},
};
use devforge_control::{
    Digest, ErrorCode, Id, ProjectBinding, Request, Response,
    limits::{Cursor, CursorContext, SafeInt},
    tools::*,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::SyncSender,
    },
    time::{Duration, Instant},
};
use zeroize::Zeroizing;
const TTL: Duration = Duration::from_secs(120);
const WINDOW_BYTES: usize = 8 * 1024 * 1024;
#[derive(Clone)]
pub(crate) enum ReadMode {
    Registration,
    Window(devforge_workspace::activity_metadata::ActivityKind),
    WindowAfter(
        devforge_workspace::activity_metadata::ActivityKind,
        u64,
        Instant,
    ),
    Epoch(devforge_workspace::activity_metadata::ActivityKind),
}
type Check = Box<dyn Fn(&devforge_workspace::Workspace) -> devforge_workspace::Result<()> + Send>;
pub(crate) struct MetadataRead {
    pub project: Id,
    pub mode: ReadMode,
    pub check: Check,
    pub cancelled: Arc<AtomicBool>,
    pub deadline_bound: bool,
    pub reply: SyncSender<devforge_workspace::Result<ReadResult>>,
}
pub(crate) struct ReadResult {
    pub binding: ProjectBinding,
    pub window: Option<devforge_workspace::activity_metadata::MetadataWindow>,
    pub epoch: Option<String>,
}
impl MetadataRead {
    pub(crate) fn execute(self, workspace: &devforge_workspace::Workspace) {
        let run = || {
            let check = || {
                if self.cancelled.load(Ordering::Acquire) {
                    return Err(crate::service_control::ws_error(ErrorCode::Interrupted));
                }
                (self.check)(workspace)
            };
            // Deadline-bound activity reads already run the same scoped check
            // immediately before and after source IO inside the workspace.
            // Avoid duplicating those checks on the single actor thread.
            if self.deadline_bound {
                if self.cancelled.load(Ordering::Acquire) {
                    return Err(crate::service_control::ws_error(ErrorCode::Interrupted));
                }
            } else {
                check()?;
            }
            let binding =
                crate::service_control::domain_registration_binding(workspace, &self.project)
                    .map_err(crate::service_control::ws_error)?;
            let (window, epoch) = match &self.mode {
                ReadMode::Registration => (None, None),
                ReadMode::Window(kind) => (
                    Some(workspace.activity_metadata(self.project.as_str(), kind, &check)?),
                    None,
                ),
                ReadMode::WindowAfter(kind, anchor, _) => (
                    Some(workspace.activity_metadata_with_anchor(
                        self.project.as_str(),
                        kind,
                        *anchor,
                        &check,
                    )?),
                    None,
                ),
                ReadMode::Epoch(kind) => (
                    None,
                    Some(workspace.activity_metadata_epoch(self.project.as_str(), kind, &check)?),
                ),
            };
            // Registration has no workspace source wrapper to perform the
            // authorization; normal non-deadline callers retain their old gate.
            if !self.deadline_bound || matches!(self.mode, ReadMode::Registration) {
                check()?;
            }
            Ok(ReadResult {
                binding,
                window,
                epoch,
            })
        };
        let _ = self.reply.send(run());
    }
}
struct ObservedWindow {
    id: Id,
    client: Id,
    grant: Id,
    project: Id,
    revision: u64,
    tool: Id,
    // Public cursor carries a random opaque value; the deterministic filter
    // digest stays private, preventing offline guessing of mail addresses.
    filter: Digest,
    filter_binding: Digest,
    source: devforge_workspace::activity_metadata::MetadataWindow,
    expires: Instant,
    expires_at: u64,
    bytes: usize,
}
struct Handle {
    cursor: Cursor,
    window: Arc<ObservedWindow>,
}
#[derive(Default)]
struct Windows {
    windows: VecDeque<Arc<ObservedWindow>>,
    handles: VecDeque<Handle>,
    bytes: usize,
}
impl Windows {
    fn prune(&mut self) {
        self.windows.retain(|w| Instant::now() < w.expires);
        self.bytes = self.windows.iter().map(|w| w.bytes).sum();
        self.handles
            .retain(|h| self.windows.iter().any(|w| w.id == h.window.id));
    }
    fn insert(&mut self, window: Arc<ObservedWindow>) -> Result<(), ErrorCode> {
        self.prune();
        if window.bytes > WINDOW_BYTES {
            return Err(ErrorCode::OutputLimitExceeded);
        }
        while self.windows.len() >= 16 || self.bytes + window.bytes > WINDOW_BYTES {
            let old = self.windows.pop_front().ok_or(ErrorCode::Busy)?;
            self.bytes -= old.bytes;
            self.handles.retain(|h| h.window.id != old.id);
        }
        self.bytes += window.bytes;
        self.windows.push_back(window);
        Ok(())
    }
    fn evict_window(&mut self, id: &Id) {
        self.windows.retain(|window| &window.id != id);
        self.handles.retain(|handle| &handle.window.id != id);
        self.bytes = self.windows.iter().map(|window| window.bytes).sum();
    }
    fn evict_grant(&mut self, grant: &Id) {
        self.windows.retain(|window| &window.grant != grant);
        self.handles.retain(|handle| &handle.window.grant != grant);
        self.bytes = self.windows.iter().map(|window| window.bytes).sum();
    }
    fn get(&mut self, cursor: &Cursor) -> Result<Arc<ObservedWindow>, ErrorCode> {
        self.prune();
        let context = cursor.context.as_ref().ok_or(ErrorCode::CursorExpired)?;
        let handle = self
            .handles
            .iter()
            .find(|h| {
                h.cursor
                    .context
                    .as_ref()
                    .is_some_and(|c| c.handle == context.handle)
            })
            .ok_or(ErrorCode::CursorExpired)?;
        if &handle.cursor != cursor {
            return Err(ErrorCode::CursorExpired);
        }
        Ok(handle.window.clone())
    }
    fn issue(&mut self, window: Arc<ObservedWindow>, before: u64) -> Result<Cursor, ErrorCode> {
        let handle = opaque_id()?;
        let cursor = Cursor {
            project_id: window.project.clone(),
            tool: window.tool.clone(),
            revision: SafeInt::new(window.revision)?,
            offset: SafeInt::new(before)?,
            expires_at: SafeInt::new(window.expires_at)?,
            context: Some(CursorContext {
                client_id: window.client.clone(),
                grant_id: window.grant.clone(),
                filter: window.filter.clone(),
                source_epoch: Digest::new(window.source.epoch.clone())?,
                high_water: SafeInt::new(window.source.high_water)?,
                handle,
            }),
        };
        if self.handles.len() >= 128 {
            self.handles.pop_front();
        }
        self.handles.push_back(Handle {
            cursor: cursor.clone(),
            window,
        });
        Ok(cursor)
    }
}
#[derive(Clone, Default)]
struct MailWatchFilter {
    subject: Option<String>,
    recipient: Option<String>,
    sender: Option<String>,
    since: Option<u64>,
}
impl Drop for MailWatchFilter {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.subject.zeroize();
        self.recipient.zeroize();
        self.sender.zeroize();
    }
}
impl MailWatchFilter {
    fn sensitive(&self) -> bool {
        self.subject.is_some() || self.recipient.is_some() || self.sender.is_some()
    }
    fn empty(&self) -> bool {
        !self.sensitive() && self.since.is_none()
    }
}
#[derive(Clone)]
struct ActivityBookmark {
    id: Id,
    client: Id,
    grant: Id,
    project: Id,
    revision: u64,
    stream: ActivityStream,
    detail: bool,
    context: Option<Id>,
    mail: MailWatchFilter,
    epoch: String,
    high_water: u64,
    expires: Instant,
}
struct WaitPermit<'a>(&'a AtomicUsize);
impl Drop for WaitPermit<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
struct SupportArtifact {
    id: Id,
    client: Id,
    grant: Id,
    project: Id,
    revision: u64,
    bytes: Vec<u8>,
    expires: Instant,
    expires_at: u64,
}
impl SupportArtifact {
    fn live_at(&self, now: u64) -> bool {
        Instant::now() < self.expires && now < self.expires_at
    }
}
pub(crate) struct Diagnostics {
    grants: Arc<Grants>,
    windows: Mutex<Windows>,
    activity: Mutex<VecDeque<ActivityBookmark>>,
    support: Mutex<VecDeque<SupportArtifact>>,
    waiters: AtomicUsize,
    stopping: AtomicBool,
}
impl Diagnostics {
    pub(crate) fn new(grants: Arc<Grants>) -> Self {
        Self {
            grants,
            windows: Mutex::new(Default::default()),
            activity: Mutex::new(Default::default()),
            support: Mutex::new(Default::default()),
            waiters: AtomicUsize::new(0),
            stopping: AtomicBool::new(false),
        }
    }
    pub(crate) fn support_bytes(
        &self,
        artifact_id: &Id,
        client: &Id,
        grant: &Id,
        project: &Id,
        revision: u64,
        now: u64,
    ) -> Result<Vec<u8>, ErrorCode> {
        let mut artifacts = self
            .support
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        artifacts.retain(|a| Instant::now() < a.expires);
        artifacts
            .iter()
            .find(|a| {
                &a.id == artifact_id
                    && &a.client == client
                    && &a.grant == grant
                    && &a.project == project
                    && a.revision == revision
                    && a.live_at(now)
            })
            .map(|a| a.bytes.clone())
            .ok_or(ErrorCode::CursorExpired)
    }
    pub(crate) fn interrupt_waits(&self) {
        self.stopping.store(true, Ordering::Release);
    }
    /// Reclaim expired sensitive mail windows and bookmark filters even when idle.
    /// Nonblocking: a busy cache is revisited by the next maintenance tick.
    pub(crate) fn evict_grant_windows(&self, grant: &Id) {
        if let Ok(mut artifacts) = self.support.lock() {
            artifacts.retain(|a| &a.grant != grant);
        }
        if let Ok(mut windows) = self.windows.lock() {
            windows.evict_grant(grant);
        }
    }
    pub(crate) fn expire_cached_state(&self) {
        if let Ok(mut artifacts) = self.support.try_lock() {
            artifacts.retain(|a| Instant::now() < a.expires);
        }
        if let Ok(mut windows) = self.windows.try_lock() {
            windows.prune();
        }
        if let Ok(mut activity) = self.activity.try_lock() {
            activity.retain(|v| Instant::now() < v.expires);
        }
    }
    pub(crate) fn clear(&self) {
        if let Ok(mut artifacts) = self.support.lock() {
            artifacts.clear();
        }
        *self
            .windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Windows::default();
        self.activity
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
    }
    pub(crate) fn request(
        self: &Arc<Self>,
        peer: PeerIdentity,
        value: Value,
        actor: &ActorHandle,
        rejected: Option<ErrorCode>,
    ) -> Value {
        let id = value
            .get("request")
            .and_then(|v| v.get("request_id"))
            .and_then(Value::as_str)
            .and_then(|v| Id::new(v).ok())
            .unwrap_or_else(|| Id::new("invalid-request").expect("fixedid"));
        let result = if let Some(code) = rejected {
            Err(code)
        } else if value
            .get("request")
            .and_then(|v| v.get("api_version"))
            .and_then(Value::as_u64)
            .is_some_and(|v| v != 1)
        {
            Err(ErrorCode::UnsupportedVersion)
        } else {
            serde_json::from_value::<Wire>(value)
                .map_err(|_| ErrorCode::InvalidInput)
                .and_then(|wire| self.read(&peer, wire, actor))
        };
        serde_json::to_value(match result{Ok(data)=>Response::success(id,None,data),Err(code)=>Response::failure(id,None,code.fault())}).unwrap_or_else(|_|json!({"api_version":1,"request_id":"invalid-request","ok":false,"error":{"code":"storage_unavailable","retryable":true},"warnings":[]}))
    }
    fn query(
        self: &Arc<Self>,
        actor: &ActorHandle,
        peer: &PeerIdentity,
        credential: &ExternalCredential,
        project: &Id,
        mode: ReadMode,
        read_policy: (Option<devforge_control::InformationScope>, Option<u64>),
    ) -> Result<ReadResult, ErrorCode> {
        self.query_until(
            actor,
            peer,
            credential,
            project,
            mode,
            (read_policy.0, read_policy.1, None),
        )
    }
    fn query_until(
        self: &Arc<Self>,
        actor: &ActorHandle,
        peer: &PeerIdentity,
        credential: &ExternalCredential,
        project: &Id,
        mode: ReadMode,
        read_policy: (
            Option<devforge_control::InformationScope>,
            Option<u64>,
            Option<Instant>,
        ),
    ) -> Result<ReadResult, ErrorCode> {
        let mut saw_busy = false;
        loop {
            if matches!(&mode, ReadMode::WindowAfter(_, _, deadline) if Instant::now() >= *deadline)
            {
                return Err(if saw_busy {
                    ErrorCode::Busy
                } else {
                    ErrorCode::TimedOut
                });
            }
            if read_policy.2.is_some() && self.stopping.load(Ordering::Acquire) {
                return Err(ErrorCode::Interrupted);
            }
            match self.query_once(actor, peer, credential, project, mode.clone(), read_policy) {
                Err(ErrorCode::Busy) if read_policy.2.is_some_and(|d| Instant::now() < d) => {
                    saw_busy = true;
                    std::thread::sleep(Duration::from_millis(10));
                }
                result => return result,
            }
        }
    }
    fn query_once(
        self: &Arc<Self>,
        actor: &ActorHandle,
        peer: &PeerIdentity,
        credential: &ExternalCredential,
        project: &Id,
        mode: ReadMode,
        read_policy: (
            Option<devforge_control::InformationScope>,
            Option<u64>,
            Option<Instant>,
        ),
    ) -> Result<ReadResult, ErrorCode> {
        let (content, revision, deadline) = read_policy;
        let policy = self.clone();
        let peer = peer.clone();
        let credential =
            ExternalCredential::from_wire(credential.expose_secret()).map_err(grant_error)?;
        let subject = project.clone();
        let check = Box::new(move |workspace: &devforge_workspace::Workspace| {
            if deadline.is_some() && policy.stopping.load(Ordering::Acquire) {
                return Err(crate::service_control::ws_error(ErrorCode::Interrupted));
            }
            let binding = crate::service_control::domain_registration_binding(workspace, &subject)
                .map_err(crate::service_control::ws_error)?;
            let mut registry =
                if deadline.is_some() {
                    policy
                        .grants
                        .registry
                        .try_lock()
                        .map_err(|_| crate::service_control::ws_error(ErrorCode::Busy))?
                } else {
                    policy.grants.registry.lock().map_err(|_| {
                        crate::service_control::ws_error(ErrorCode::StorageUnavailable)
                    })?
                };
            registry
                .as_mut()
                .ok_or_else(|| crate::service_control::ws_error(ErrorCode::Interrupted))?
                .authorize_read(&peer, &credential, &binding, content, revision)
                .map_err(|e| crate::service_control::ws_error(grant_error(e)))?;
            Ok(())
        });
        let (reply, receiver) = std::sync::mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        actor
            .queue_metadata(MetadataRead {
                project: project.clone(),
                mode,
                check,
                cancelled: cancelled.clone(),
                deadline_bound: deadline.is_some(),
                reply,
            })
            .map_err(|_| ErrorCode::Busy)?;
        let limit = deadline
            .map(|end| {
                end.saturating_duration_since(Instant::now())
                    .min(Duration::from_secs(5))
            })
            .unwrap_or(Duration::from_secs(5));
        match receiver.recv_timeout(limit) {
            Ok(result) => result.map_err(crate::service_control::environment_error),
            Err(_) => {
                cancelled.store(true, Ordering::Release);
                Err(ErrorCode::TimedOut)
            }
        }
    }
    /// The owner thread serves the request; only brief scoped reads are queued
    /// on the actor. No SQLite transaction or actor work is held while sleeping.
    pub(crate) fn activity_request(
        self: &Arc<Self>,
        peer: PeerIdentity,
        value: Value,
        actor: &ActorHandle,
        rejected: Option<ErrorCode>,
    ) -> Value {
        let id = value
            .get("request")
            .and_then(|v| v.get("request_id"))
            .and_then(Value::as_str)
            .and_then(|v| Id::new(v).ok())
            .unwrap_or_else(|| Id::new("invalid-request").unwrap());
        let result = if let Some(code) = rejected {
            Err(code)
        } else if self.stopping.load(Ordering::Acquire) {
            Err(ErrorCode::Interrupted)
        } else if value
            .get("request")
            .and_then(|v| v.get("api_version"))
            .and_then(Value::as_u64)
            .is_some_and(|v| v != 1)
        {
            Err(ErrorCode::UnsupportedVersion)
        } else {
            self.activity_inner(&peer, value, actor)
        };
        serde_json::to_value(match result {
            Ok(data) => Response::success(id, None, data),
            Err(code) => Response::failure(id, None, code.fault()),
        }).unwrap_or_else(|_| json!({"api_version":1,"request_id":"invalid-request","ok":false,"error":{"code":"storage_unavailable","retryable":true},"warnings":[]}))
    }
    fn activity_inner(
        self: &Arc<Self>,
        peer: &PeerIdentity,
        value: Value,
        actor: &ActorHandle,
    ) -> Result<Value, ErrorCode> {
        let method = value
            .get("method")
            .and_then(Value::as_str)
            .ok_or(ErrorCode::InvalidInput)?;
        match method {
            "support_preview" | "support_read" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct SupportWire {
                    method: String,
                    credential: Token,
                    request: Request<Value>,
                }
                let wire: SupportWire =
                    serde_json::from_value(value).map_err(|_| ErrorCode::InvalidInput)?;
                if wire.request.idempotency_key.is_some() {
                    return Err(ErrorCode::InvalidInput);
                }
                let artifact_id = if wire.method == "support_preview" {
                    if wire.request.input != json!({}) {
                        return Err(ErrorCode::InvalidInput);
                    }
                    None
                } else {
                    let object = wire
                        .request
                        .input
                        .as_object()
                        .ok_or(ErrorCode::InvalidInput)?;
                    if object.len() != 1 {
                        return Err(ErrorCode::InvalidInput);
                    }
                    Some(Id::new(
                        object
                            .get("artifact_id")
                            .and_then(Value::as_str)
                            .ok_or(ErrorCode::InvalidInput)?,
                    )?)
                };
                let credential =
                    ExternalCredential::from_wire(&wire.credential.0).map_err(grant_error)?;
                if self.activity_credential_project(
                    peer,
                    &credential,
                    Instant::now() + Duration::from_secs(4),
                )? != wire.request.project_id
                {
                    return Err(ErrorCode::ProjectMismatch);
                }
                let initial = self.query(
                    actor,
                    peer,
                    &credential,
                    &wire.request.project_id,
                    ReadMode::Registration,
                    (None, wire.request.expected_revision),
                )?;
                let candidate = if artifact_id.is_none() {
                    let ToolOutput::Health(health) = doctor(actor, &initial.binding)? else {
                        return Err(ErrorCode::StorageUnavailable);
                    };
                    let item = health.items.first().ok_or(ErrorCode::StorageUnavailable)?;
                    Some(
                        devforge_workspace::support_bundle::render(
                            item.code.as_str(),
                            health.freshness.observed_at.get(),
                            health.freshness.revision.get(),
                        )
                        .ok_or(ErrorCode::StorageUnavailable)?,
                    )
                } else {
                    None
                };
                let current = self.query(
                    actor,
                    peer,
                    &credential,
                    &wire.request.project_id,
                    ReadMode::Registration,
                    (None, Some(initial.binding.revision())),
                )?;
                if current.binding.root() != initial.binding.root() {
                    return Err(ErrorCode::ProjectMismatch);
                }
                let mut grants = self
                    .grants
                    .registry
                    .lock()
                    .map_err(|_| ErrorCode::StorageUnavailable)?;
                let proof = grants
                    .as_mut()
                    .ok_or(ErrorCode::Interrupted)?
                    .authorize_read(
                        peer,
                        &credential,
                        &current.binding,
                        None,
                        Some(initial.binding.revision()),
                    )
                    .map_err(grant_error)?;
                let mut artifacts = self
                    .support
                    .lock()
                    .map_err(|_| ErrorCode::StorageUnavailable)?;
                artifacts.retain(|a| Instant::now() < a.expires);
                let (id, bytes, expires_at) = if let Some(id) = artifact_id {
                    let a = artifacts
                        .iter()
                        .find(|a| {
                            a.id == id
                                && a.client == *proof.client_id()
                                && a.grant == *proof.grant_id()
                                && a.project == wire.request.project_id
                                && a.revision == current.binding.revision()
                                && a.live_at(proof.observed_at())
                        })
                        .ok_or(ErrorCode::CursorExpired)?;
                    (a.id.clone(), a.bytes.clone(), a.expires_at)
                } else {
                    let id = opaque_id()?;
                    let bytes = candidate.ok_or(ErrorCode::StorageUnavailable)?;
                    let expires_at = proof
                        .observed_at()
                        .saturating_add(TTL.as_millis() as u64)
                        .min(proof.expires_at());
                    if artifacts.len() >= 128 {
                        artifacts.pop_front();
                    }
                    artifacts.push_back(SupportArtifact {
                        id: id.clone(),
                        client: proof.client_id().clone(),
                        grant: proof.grant_id().clone(),
                        project: wire.request.project_id,
                        revision: current.binding.revision(),
                        bytes: bytes.clone(),
                        expires: Instant::now() + TTL,
                        expires_at,
                    });
                    (id, bytes, expires_at)
                };
                let preview: Value =
                    serde_json::from_slice(&bytes).map_err(|_| ErrorCode::StorageUnavailable)?;
                Ok(json!({"artifact_id": id, "preview": preview, "expires_at": expires_at}))
            }
            "activity_mark" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct MarkWire {
                    method: String,
                    credential: Token,
                    request: Request<ActivityMarkInput>,
                }
                let wire: MarkWire =
                    serde_json::from_value(value).map_err(|_| ErrorCode::InvalidInput)?;
                if wire.method != "activity_mark" || wire.request.idempotency_key.is_some() {
                    return Err(ErrorCode::InvalidInput);
                }
                let credential =
                    ExternalCredential::from_wire(&wire.credential.0).map_err(grant_error)?;
                let input = wire.request.input;
                let mail = MailWatchFilter {
                    subject: input.subject.as_ref().map(|v| v.as_str().to_owned()),
                    recipient: input.recipient.as_ref().map(|v| v.as_str().to_owned()),
                    sender: input.sender.as_ref().map(|v| v.as_str().to_owned()),
                    since: input.since.as_ref().map(|v| v.get()),
                };
                let mut kind =
                    activity_kind(input.stream, input.detail, input.context_id.as_ref(), &mail)?;
                // A mark needs only a high-water/epoch; never parse earlier MIME
                // or retain old verification links merely to start watching.
                if let devforge_workspace::activity_metadata::ActivityKind::MailContent {
                    include_detail,
                    ..
                } = &mut kind
                {
                    *include_detail = false;
                }
                let content = activity_scope(input.stream, input.detail, &mail);
                let deadline = Instant::now() + Duration::from_secs(4);
                // Resolve the credential-bound project before asking the actor
                // anything about a client-supplied project identifier. This
                // makes existing and nonexistent foreign project IDs identical.
                let bound = self.activity_credential_project(peer, &credential, deadline)?;
                if bound != wire.request.project_id {
                    return Err(ErrorCode::ProjectMismatch);
                }
                let registration = self.query_until(
                    actor,
                    peer,
                    &credential,
                    &wire.request.project_id,
                    ReadMode::Registration,
                    (content, wire.request.expected_revision, Some(deadline)),
                )?;
                let read = self.query_until(
                    actor,
                    peer,
                    &credential,
                    &wire.request.project_id,
                    ReadMode::Window(kind),
                    (
                        content,
                        Some(registration.binding.revision()),
                        Some(deadline),
                    ),
                )?;
                if read.binding.root() != registration.binding.root() {
                    return Err(ErrorCode::ProjectMismatch);
                }
                let source = read.window.ok_or(ErrorCode::StorageUnavailable)?;
                if source.missing {
                    return Err(ErrorCode::PreconditionFailed);
                }
                let current = self.query_until(
                    actor,
                    peer,
                    &credential,
                    &wire.request.project_id,
                    ReadMode::Registration,
                    (
                        content,
                        Some(registration.binding.revision()),
                        Some(deadline),
                    ),
                )?;
                if current.binding.root() != registration.binding.root() {
                    return Err(ErrorCode::ProjectMismatch);
                }
                let (client, grant) = self.activity_identity(
                    peer,
                    &credential,
                    &current.binding,
                    content,
                    Some(registration.binding.revision()),
                    Some(deadline),
                )?;
                let bookmark = ActivityBookmark {
                    id: opaque_id()?,
                    client,
                    grant,
                    project: wire.request.project_id,
                    revision: read.binding.revision(),
                    stream: input.stream,
                    detail: input.detail,
                    context: input.context_id,
                    mail,
                    epoch: source.epoch,
                    high_water: source.high_water,
                    expires: Instant::now() + TTL,
                };
                if self.stopping.load(Ordering::Acquire) {
                    return Err(ErrorCode::Interrupted);
                }
                let id = bookmark.id.clone();
                self.save_activity(bookmark)?;
                serde_json::to_value(ActivityMarkOutput { bookmark: id })
                    .map_err(|_| ErrorCode::StorageUnavailable)
            }
            "activity_wait" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct WaitWire {
                    method: String,
                    credential: Token,
                    request: Request<ActivityWaitInput>,
                }
                let wire: WaitWire =
                    serde_json::from_value(value).map_err(|_| ErrorCode::InvalidInput)?;
                if wire.method != "activity_wait" || wire.request.idempotency_key.is_some() {
                    return Err(ErrorCode::InvalidInput);
                }
                if self
                    .waiters
                    .try_update(Ordering::AcqRel, Ordering::Acquire, |v| {
                        (v < 4).then_some(v + 1)
                    })
                    .is_err()
                {
                    return Err(ErrorCode::Busy);
                }
                let _permit = WaitPermit(&self.waiters);
                let mut bookmark = {
                    let mut all = self.activity.try_lock().map_err(|_| ErrorCode::Busy)?;
                    all.retain(|v| Instant::now() < v.expires);
                    all.iter()
                        .find(|v| v.id == wire.request.input.bookmark)
                        .cloned()
                        .ok_or(ErrorCode::CursorExpired)?
                };
                if bookmark.project != wire.request.project_id {
                    return Err(ErrorCode::ProjectMismatch);
                }
                if wire
                    .request
                    .expected_revision
                    .is_some_and(|v| v != bookmark.revision)
                {
                    return Err(ErrorCode::RevisionConflict);
                }
                let narrowed = match (&bookmark.context, &wire.request.input.context_id) {
                    (Some(old), Some(new)) if old != new => return Err(ErrorCode::CursorExpired),
                    (None, Some(new)) => {
                        bookmark.context = Some(new.clone());
                        true
                    }
                    _ => false,
                };
                let credential =
                    ExternalCredential::from_wire(&wire.credential.0).map_err(grant_error)?;
                let kind = activity_kind(
                    bookmark.stream,
                    bookmark.detail,
                    bookmark.context.as_ref(),
                    &bookmark.mail,
                )?;
                let content = activity_scope(bookmark.stream, bookmark.detail, &bookmark.mail);
                let deadline =
                    Instant::now() + Duration::from_millis(wire.request.input.timeout_ms.get());
                // Leave time for an in-flight read and the fresh actor/grant
                // checks after the observation interval. Windows filesystem and
                // SQLite checks can take longer than a 250ms final budget under
                // load. With the 3s maximum and registry's 250ms SQLite busy
                // bound, the nominal worst case is 4.25s (transport limit 5s).
                let hard_deadline = deadline + Duration::from_millis(1000);
                let read_deadline = deadline + Duration::from_millis(500);
                // Verify the original client/grant BEFORE any activity source IO.
                let registration = self.query_until(
                    actor,
                    peer,
                    &credential,
                    &bookmark.project,
                    ReadMode::Registration,
                    (content, Some(bookmark.revision), Some(hard_deadline)),
                )?;
                let (client, grant) = self.activity_identity(
                    peer,
                    &credential,
                    &registration.binding,
                    content,
                    Some(bookmark.revision),
                    Some(hard_deadline),
                )?;
                if client != bookmark.client || grant != bookmark.grant {
                    return Err(ErrorCode::Unauthorized);
                }
                let mut observed = false;
                loop {
                    if self.stopping.load(Ordering::Acquire) {
                        return Err(ErrorCode::Interrupted);
                    }
                    if Instant::now() >= bookmark.expires {
                        return Err(ErrorCode::CursorExpired);
                    }
                    if Instant::now() >= deadline {
                        if !observed {
                            return Err(ErrorCode::TimedOut);
                        }
                        let current = self.query_until(
                            actor,
                            peer,
                            &credential,
                            &bookmark.project,
                            ReadMode::Registration,
                            (content, Some(bookmark.revision), Some(hard_deadline)),
                        )?;
                        let (client, grant) = self.activity_identity(
                            peer,
                            &credential,
                            &current.binding,
                            content,
                            Some(bookmark.revision),
                            Some(hard_deadline),
                        )?;
                        if client != bookmark.client || grant != bookmark.grant {
                            return Err(ErrorCode::Unauthorized);
                        }
                        if self.stopping.load(Ordering::Acquire) {
                            return Err(ErrorCode::Interrupted);
                        }
                        if Instant::now() >= bookmark.expires {
                            return Err(ErrorCode::CursorExpired);
                        }
                        self.ensure_activity_live(&wire.request.input.bookmark)?;
                        if narrowed {
                            bookmark.id = opaque_id()?;
                            self.save_activity(bookmark.clone())?;
                        }
                        return serde_json::to_value(ActivityWaitOutput {
                            bookmark: bookmark.id,
                            event: None,
                            timed_out: true,
                            continuity: ActivityContinuity::Unverified,
                        })
                        .map_err(|_| ErrorCode::StorageUnavailable);
                    }
                    let read = match self.query_until(
                        actor,
                        peer,
                        &credential,
                        &bookmark.project,
                        ReadMode::WindowAfter(kind.clone(), bookmark.high_water, deadline),
                        (content, Some(bookmark.revision), Some(read_deadline)),
                    ) {
                        Err(ErrorCode::TimedOut) if observed => continue,
                        result => result?,
                    };
                    let window = read.window.ok_or(ErrorCode::StorageUnavailable)?;
                    observed = true;
                    if window.missing
                        || window.epoch != bookmark.epoch
                        || window.anchor_present != Some(true)
                        || window.high_water < bookmark.high_water
                    {
                        return Err(ErrorCode::CursorExpired);
                    }
                    let (client, grant) = self.activity_identity(
                        peer,
                        &credential,
                        &read.binding,
                        content,
                        Some(bookmark.revision),
                        Some(hard_deadline),
                    )?;
                    if client != bookmark.client || grant != bookmark.grant {
                        return Err(ErrorCode::Unauthorized);
                    }
                    if window
                        .first_omitted
                        .is_some_and(|sequence| sequence > bookmark.high_water)
                    {
                        return Err(ErrorCode::OutputLimitExceeded);
                    }
                    if let Some(row) = window
                        .rows
                        .iter()
                        .filter(|row| row.sequence > bookmark.high_water)
                        .min_by_key(|row| row.sequence)
                    {
                        let event = activity_event(&row.item, bookmark.detail)?;
                        let current = self.query_until(
                            actor,
                            peer,
                            &credential,
                            &bookmark.project,
                            ReadMode::Registration,
                            (content, Some(bookmark.revision), Some(hard_deadline)),
                        )?;
                        let (client, grant) = self.activity_identity(
                            peer,
                            &credential,
                            &current.binding,
                            content,
                            Some(bookmark.revision),
                            Some(hard_deadline),
                        )?;
                        if client != bookmark.client || grant != bookmark.grant {
                            return Err(ErrorCode::Unauthorized);
                        }
                        if self.stopping.load(Ordering::Acquire) {
                            return Err(ErrorCode::Interrupted);
                        }
                        if Instant::now() >= bookmark.expires {
                            return Err(ErrorCode::CursorExpired);
                        }
                        self.ensure_activity_live(&wire.request.input.bookmark)?;
                        let mut advanced = bookmark.clone();
                        advanced.id = opaque_id()?;
                        advanced.high_water = row.sequence;
                        let id = advanced.id.clone();
                        self.save_activity(advanced)?;
                        return serde_json::to_value(ActivityWaitOutput {
                            bookmark: id,
                            event: Some(event),
                            timed_out: false,
                            continuity: ActivityContinuity::Unverified,
                        })
                        .map_err(|_| ErrorCode::StorageUnavailable);
                    }
                    std::thread::sleep(
                        Duration::from_millis(100)
                            .min(deadline.saturating_duration_since(Instant::now())),
                    );
                }
            }
            _ => Err(ErrorCode::InvalidInput),
        }
    }
    /// This is only an internal peer/token lookup, not authorization. All
    /// subsequent registration/source/output grant checks remain mandatory.
    fn activity_credential_project(
        &self,
        peer: &PeerIdentity,
        credential: &ExternalCredential,
        deadline: Instant,
    ) -> Result<Id, ErrorCode> {
        loop {
            if self.stopping.load(Ordering::Acquire) {
                return Err(ErrorCode::Interrupted);
            }
            match self.grants.registry.try_lock() {
                Ok(registry) => {
                    return registry
                        .as_ref()
                        .ok_or(ErrorCode::Interrupted)?
                        .credential_project(peer, credential)
                        .map_err(grant_error);
                }
                Err(std::sync::TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(std::sync::TryLockError::WouldBlock) => return Err(ErrorCode::Busy),
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    return Err(ErrorCode::StorageUnavailable);
                }
            }
        }
    }
    fn activity_identity(
        &self,
        peer: &PeerIdentity,
        credential: &ExternalCredential,
        binding: &ProjectBinding,
        content: Option<devforge_control::InformationScope>,
        revision: Option<u64>,
        deadline: Option<Instant>,
    ) -> Result<(Id, Id), ErrorCode> {
        loop {
            if self.stopping.load(Ordering::Acquire) {
                return Err(ErrorCode::Interrupted);
            }
            match self.grants.registry.try_lock() {
                Ok(mut registry) => {
                    let proof = registry
                        .as_mut()
                        .ok_or(ErrorCode::Interrupted)?
                        .authorize_read(peer, credential, binding, content, revision)
                        .map_err(grant_error)?;
                    return Ok((proof.client_id().clone(), proof.grant_id().clone()));
                }
                Err(std::sync::TryLockError::WouldBlock)
                    if deadline.is_some_and(|d| Instant::now() < d) =>
                {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(std::sync::TryLockError::WouldBlock) => return Err(ErrorCode::Busy),
                Err(std::sync::TryLockError::Poisoned(_)) => {
                    return Err(ErrorCode::StorageUnavailable);
                }
            }
        }
    }
    fn ensure_activity_live(&self, id: &Id) -> Result<(), ErrorCode> {
        let all = self.activity.try_lock().map_err(|_| ErrorCode::Busy)?;
        if all
            .iter()
            .any(|bookmark| &bookmark.id == id && Instant::now() < bookmark.expires)
        {
            Ok(())
        } else {
            Err(ErrorCode::CursorExpired)
        }
    }
    fn save_activity(&self, bookmark: ActivityBookmark) -> Result<(), ErrorCode> {
        let mut all = self.activity.try_lock().map_err(|_| ErrorCode::Busy)?;
        all.retain(|v| Instant::now() < v.expires);
        if all.len() >= 128 {
            all.pop_front();
        }
        all.push_back(bookmark);
        Ok(())
    }
    fn read(
        self: &Arc<Self>,
        peer: &PeerIdentity,
        wire: Wire,
        actor: &ActorHandle,
    ) -> Result<ToolOutput, ErrorCode> {
        if wire.method != "diagnostics_read" {
            return Err(ErrorCode::InvalidInput);
        }
        check_input(&wire.request.input)?;
        let content = match &wire.request.input {
            ToolInput::Log(p) if p.content == Some(true) || p.query.is_some() => {
                Some(devforge_control::InformationScope::LogContent)
            }
            ToolInput::Inspector(p) if p.detail == Some(true) => {
                Some(devforge_control::InformationScope::InspectorQueries)
            }
            ToolInput::Mail(p)
                if p.detail == Some(true)
                    || p.mail_id.is_some()
                    || p.subject.is_some()
                    || p.recipient.is_some()
                    || p.sender.is_some() =>
            {
                Some(devforge_control::InformationScope::MailContent)
            }
            _ => None,
        };
        let credential = ExternalCredential::from_wire(&wire.credential.0).map_err(grant_error)?;
        if matches!(wire.request.input, ToolInput::Mail(_)) {
            let bound = self.activity_credential_project(
                peer,
                &credential,
                Instant::now() + Duration::from_secs(4),
            )?;
            if bound != wire.request.project_id {
                return Err(ErrorCode::ProjectMismatch);
            }
        }
        let registration = self.query(
            actor,
            peer,
            &credential,
            &wire.request.project_id,
            ReadMode::Registration,
            (content, wire.request.expected_revision),
        )?;
        let mut grants = self
            .grants
            .registry
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        let registry = grants.as_mut().ok_or(ErrorCode::Interrupted)?;
        let proof = registry
            .authorize_read(
                peer,
                &credential,
                &registration.binding,
                content,
                wire.request.expected_revision,
            )
            .map_err(grant_error)?;
        let grant = proof.control_grant()?;
        let client = proof.client_id().clone();
        let grant_id = proof.grant_id().clone();
        let now = proof.observed_at();
        let prepared = prepare_read(
            &serde_json::to_vec(&wire.request).map_err(|_| ErrorCode::InvalidInput)?,
            &registration.binding,
            &grant,
            &client,
            now,
            None,
        )?;
        drop(grants);
        let mut inserted_mail_window = None;
        let result = (|| -> Result<ToolOutput, ErrorCode> {
            let output = if matches!(wire.request.input, ToolInput::Health(_)) {
                if wire.request.input.page().cursor.is_some() {
                    return Err(ErrorCode::CursorExpired);
                }
                doctor(actor, &registration.binding)?
            } else {
                let page = wire.request.input.page();
                let filter = filter_digest(&wire.request.input)?;
                let (window, before, continued) = if let Some(cursor) = &page.cursor {
                    let window = self
                        .windows
                        .lock()
                        .map_err(|_| ErrorCode::StorageUnavailable)?
                        .get(cursor)?;
                    if window.client != client
                        || window.grant != grant_id
                        || window.project != wire.request.project_id
                        || window.revision != registration.binding.revision()
                        || window.tool.as_str() != wire.request.input.tool()
                        || window.filter_binding != filter
                    {
                        return Err(ErrorCode::CursorExpired);
                    }
                    let epoch = self
                        .query(
                            actor,
                            peer,
                            &credential,
                            &wire.request.project_id,
                            ReadMode::Epoch(kind(&wire.request.input)?),
                            (content, Some(registration.binding.revision())),
                        )?
                        .epoch
                        .ok_or(ErrorCode::CursorExpired)?;
                    if epoch != window.source.epoch {
                        return Err(ErrorCode::CursorExpired);
                    }
                    (window, cursor.offset.get(), true)
                } else {
                    let read = self.query(
                        actor,
                        peer,
                        &credential,
                        &wire.request.project_id,
                        ReadMode::Window(kind(&wire.request.input)?),
                        (content, Some(registration.binding.revision())),
                    )?;
                    if read.binding.revision() != registration.binding.revision()
                        || read.binding.root() != registration.binding.root()
                    {
                        return Err(ErrorCode::RevisionConflict);
                    }
                    let source = read.window.ok_or(ErrorCode::StorageUnavailable)?;
                    let bytes = bounded_size(&source)?;
                    let window = Arc::new(ObservedWindow {
                        id: opaque_id()?,
                        client: client.clone(),
                        grant: grant_id.clone(),
                        project: wire.request.project_id.clone(),
                        revision: registration.binding.revision(),
                        tool: Id::new(wire.request.input.tool())?,
                        filter: crate::environment_control::opaque()?,
                        filter_binding: filter,
                        expires: Instant::now() + TTL,
                        expires_at: now + TTL.as_millis() as u64,
                        bytes,
                        source,
                    });
                    self.windows
                        .lock()
                        .map_err(|_| ErrorCode::StorageUnavailable)?
                        .insert(window.clone())?;
                    if content == Some(devforge_control::InformationScope::MailContent) {
                        inserted_mail_window = Some(window.id.clone());
                    }
                    let before = window.source.high_water.saturating_add(1);
                    (window, before, false)
                };
                let rows = window
                    .source
                    .rows
                    .iter()
                    .filter(|r| r.sequence < before && r.sequence <= window.source.high_water)
                    .take(page.limit.get() as usize)
                    .collect::<Vec<_>>();
                let last = rows.last().map(|r| r.sequence);
                let more =
                    last.is_some_and(|last| window.source.rows.iter().any(|r| r.sequence < last));
                let cursor = if more {
                    Some(
                        self.windows
                            .lock()
                            .map_err(|_| ErrorCode::StorageUnavailable)?
                            .issue(window.clone(), last.ok_or(ErrorCode::StorageUnavailable)?)?,
                    )
                } else {
                    None
                };
                project(&wire.request.input, &window, &rows, cursor, continued)?
            };
            prepared.check_deadline()?;
            // Re-resolve the registry-owned binding before returning sensitive data;
            // an MCP request need not supply an expected revision itself.
            if content.is_some() {
                let current = self.query(
                    actor,
                    peer,
                    &credential,
                    &wire.request.project_id,
                    ReadMode::Registration,
                    (content, Some(registration.binding.revision())),
                )?;
                if current.binding.root() != registration.binding.root() {
                    return Err(ErrorCode::ProjectMismatch);
                }
            }
            let mut grants = self
                .grants
                .registry
                .lock()
                .map_err(|_| ErrorCode::StorageUnavailable)?;
            let proof = grants
                .as_mut()
                .ok_or(ErrorCode::Interrupted)?
                .authorize_read(
                    peer,
                    &credential,
                    &registration.binding,
                    content,
                    wire.request.expected_revision,
                )
                .map_err(grant_error)?;
            prepared.check_output(&output, &registration.binding, proof.observed_at())?;
            Ok(output)
        })();
        // Revocation may race a new cache insert. On any failed read, reclaim
        // the exact newly inserted sensitive window after all grant locks drop.
        if result.is_err()
            && let Some(id) = inserted_mail_window
            && let Ok(mut windows) = self.windows.lock()
        {
            windows.evict_window(&id);
        }
        result
    }
}
fn activity_scope(
    stream: ActivityStream,
    detail: bool,
    mail: &MailWatchFilter,
) -> Option<devforge_control::InformationScope> {
    if stream == ActivityStream::Mail && mail.sensitive() {
        return Some(devforge_control::InformationScope::MailContent);
    }
    if !detail {
        return None;
    }
    Some(match stream {
        ActivityStream::Inspector => devforge_control::InformationScope::InspectorQueries,
        ActivityStream::Logs => devforge_control::InformationScope::LogContent,
        ActivityStream::Mail => devforge_control::InformationScope::MailContent,
    })
}
fn activity_kind(
    stream: ActivityStream,
    detail: bool,
    context: Option<&Id>,
    mail: &MailWatchFilter,
) -> Result<devforge_workspace::activity_metadata::ActivityKind, ErrorCode> {
    use devforge_workspace::activity_metadata::ActivityKind;
    let context = context.map(|v| v.as_str().to_owned());
    let kind = match (stream, detail) {
        (ActivityStream::Inspector, false) if mail.empty() => ActivityKind::Inspector { context },
        (ActivityStream::Inspector, true) if mail.empty() => {
            ActivityKind::InspectorDetail { context }
        }
        (ActivityStream::Logs, detail) if mail.empty() => {
            ActivityKind::WatchLogs { context, detail }
        }
        (ActivityStream::Mail, _) if context.is_some() => return Err(ErrorCode::InvalidInput),
        (ActivityStream::Mail, _) if detail || mail.sensitive() => ActivityKind::MailContent {
            subject: mail.subject.clone(),
            recipient: mail.recipient.clone(),
            sender: mail.sender.clone(),
            since: mail.since,
            mail_id: None,
            include_detail: detail,
        },
        (ActivityStream::Mail, _) => match mail.since {
            Some(since) => ActivityKind::MailSince { since },
            None => ActivityKind::Mail,
        },
        _ => return Err(ErrorCode::InvalidInput),
    };
    kind.validate().map_err(|_| ErrorCode::InvalidInput)?;
    Ok(kind)
}
fn mail_item(
    item: &devforge_workspace::activity_metadata::MetadataItem,
    include_detail: bool,
) -> Result<MailItem, ErrorCode> {
    use devforge_workspace::activity_metadata::MetadataItem;
    let (mail_id, received_at, attachments, detail) = match item {
        MetadataItem::Mail {
            mail_id,
            received_at,
            attachments,
        } => (mail_id, received_at, attachments, None),
        MetadataItem::MailContent {
            mail_id,
            received_at,
            attachments,
            detail,
        } => (mail_id, received_at, attachments, detail.as_ref()),
        _ => return Err(ErrorCode::StorageUnavailable),
    };
    if include_detail && detail.is_none() {
        return Err(ErrorCode::StorageUnavailable);
    }
    let detail = if include_detail {
        detail
            .map(|d| {
                Ok::<_, ErrorCode>(MailContentDetail {
                    subject: devforge_control::limits::Text::new(d.subject.clone())?,
                    from: devforge_control::limits::Text::new(d.from.clone())?,
                    to: devforge_control::limits::Text::new(d.to.clone())?,
                    text: devforge_control::limits::Text::new(d.text.clone())?,
                    links: d
                        .links
                        .iter()
                        .map(|v| devforge_control::limits::Text::new(v.clone()))
                        .collect::<Result<_, _>>()?,
                    attachment_metadata: d
                        .attachment_metadata
                        .iter()
                        .map(|v| {
                            Ok(MailAttachmentMetadata {
                                name: devforge_control::limits::Text::new(v.name.clone())?,
                                mime: devforge_control::limits::Text::new(v.mime.clone())?,
                                size: SafeInt::new(v.size)?,
                            })
                        })
                        .collect::<Result<_, ErrorCode>>()?,
                    truncated: d.truncated,
                })
            })
            .transpose()?
    } else {
        None
    };
    Ok(MailItem {
        mail_id: Id::new(mail_id)?,
        received_at: SafeInt::new(*received_at)?,
        attachments: SafeInt::new(*attachments)?,
        detail,
    })
}
fn activity_event(
    item: &devforge_workspace::activity_metadata::MetadataItem,
    include_detail: bool,
) -> Result<ActivityEvent, ErrorCode> {
    use devforge_workspace::activity_metadata::MetadataItem;
    match item {
        MetadataItem::Inspector {
            context_id,
            method,
            status,
            queries,
            elapsed_ms,
        } => Ok(ActivityEvent::Inspector {
            data: InspectorItem {
                context_id: Id::new(context_id)?,
                method: devforge_control::limits::Text::new(method.clone())?,
                status: *status,
                queries: SafeInt::new(*queries)?,
                elapsed_ms: SafeInt::new(*elapsed_ms)?,
                detail: None,
            },
        }),
        MetadataItem::InspectorDetail {
            context_id,
            method,
            status,
            queries,
            elapsed_ms,
            detail,
        } => Ok(ActivityEvent::Inspector {
            data: InspectorItem {
                context_id: Id::new(context_id)?,
                method: devforge_control::limits::Text::new(method.clone())?,
                status: *status,
                queries: SafeInt::new(*queries)?,
                elapsed_ms: SafeInt::new(*elapsed_ms)?,
                detail: Some(InspectorDetail {
                    kind: Id::new(&detail.kind)?,
                    observed_at: SafeInt::new(detail.observed_at)?,
                    text: devforge_control::limits::Text::new(detail.text.clone())?,
                    duration_ms: detail.duration_ms.map(SafeInt::new).transpose()?,
                    truncated: detail.truncated,
                    capture_truncated: detail.capture_truncated,
                    repeated_count: detail.repeated_count.map(SafeInt::new).transpose()?,
                    sql_ms: detail.sql_ms.map(SafeInt::new).transpose()?,
                    peak_memory_bytes: detail.peak_memory_bytes.map(SafeInt::new).transpose()?,
                }),
            },
        }),
        MetadataItem::WatchLog {
            context_id,
            event_id,
            level,
            code,
            observed_at,
            message,
        } => {
            let message = message
                .as_ref()
                .map(|v| devforge_control::limits::Text::new(v.clone()))
                .transpose()?;
            let (message, exception) = if code == "captured_exception" {
                (None, message)
            } else {
                (message, None)
            };
            Ok(ActivityEvent::Logs {
                context_id: Id::new(context_id)?,
                data: LogItem {
                    event_id: Id::new(event_id)?,
                    level: devforge_control::limits::Text::new(level.clone())?,
                    code: Id::new(code)?,
                    observed_at: SafeInt::new(*observed_at)?,
                    message,
                    exception,
                },
            })
        }
        MetadataItem::Mail { .. } | MetadataItem::MailContent { .. } => Ok(ActivityEvent::Mail {
            data: mail_item(item, include_detail)?,
        }),
        _ => Err(ErrorCode::StorageUnavailable),
    }
}
fn check_input(input: &ToolInput) -> Result<(), ErrorCode> {
    match input {
        ToolInput::Health(_) => Ok(()),
        ToolInput::Log(_) => Ok(()),
        ToolInput::Inspector(_) => Ok(()),
        ToolInput::Mail(_) => Ok(()),
        _ => Err(ErrorCode::UnsupportedCapability),
    }
}
fn kind(
    input: &ToolInput,
) -> Result<devforge_workspace::activity_metadata::ActivityKind, ErrorCode> {
    use devforge_workspace::activity_metadata::ActivityKind;
    match input {
        ToolInput::Inspector(p) if p.detail == Some(true) => Ok(ActivityKind::InspectorDetail {
            context: p.context_id.as_ref().map(|v| v.as_str().into()),
        }),
        ToolInput::Inspector(p) => Ok(ActivityKind::Inspector {
            context: p.context_id.as_ref().map(|v| v.as_str().into()),
        }),
        ToolInput::Log(p) if p.content == Some(true) || p.query.is_some() => {
            Ok(ActivityKind::LogContent {
                level: p.level.as_ref().map(|v| v.as_str().into()),
                query: p.query.as_ref().map(|v| v.as_str().into()),
                since: p.since.as_ref().map(|v| v.get()),
            })
        }
        ToolInput::Log(p) => Ok(ActivityKind::Logs {
            level: p.level.as_ref().map(|v| v.as_str().into()),
            since: p.since.as_ref().map(|v| v.get()),
        }),
        ToolInput::Mail(p)
            if p.detail == Some(true)
                || p.mail_id.is_some()
                || p.subject.is_some()
                || p.recipient.is_some()
                || p.sender.is_some() =>
        {
            Ok(ActivityKind::MailContent {
                subject: p.subject.as_ref().map(|v| v.as_str().to_owned()),
                recipient: p.recipient.as_ref().map(|v| v.as_str().to_owned()),
                sender: p.sender.as_ref().map(|v| v.as_str().to_owned()),
                mail_id: p.mail_id.as_ref().map(|v| v.as_str().to_owned()),
                since: p.since.as_ref().map(|v| v.get()),
                include_detail: p.detail == Some(true),
            })
        }
        ToolInput::Mail(p) => Ok(match &p.since {
            Some(since) => ActivityKind::MailSince { since: since.get() },
            None => ActivityKind::Mail,
        }),
        _ => Err(ErrorCode::UnsupportedCapability),
    }
}
fn filter_digest(input: &ToolInput) -> Result<Digest, ErrorCode> {
    let bytes = match input {
        ToolInput::Log(p) => {
            serde_json::to_vec(&(input.tool(), &p.level, &p.content, &p.query, &p.since))
        }
        ToolInput::Inspector(p) => serde_json::to_vec(&(input.tool(), &p.context_id, &p.detail)),
        ToolInput::Mail(p) => serde_json::to_vec(&(
            input.tool(),
            &p.subject,
            &p.recipient,
            &p.sender,
            &p.since,
            &p.detail,
            &p.mail_id,
        )),
        _ => return Err(ErrorCode::UnsupportedCapability),
    }
    .map_err(|_| ErrorCode::InvalidInput)?;
    let bytes = zeroize::Zeroizing::new(bytes);
    devforge_control_adapter::sha256(&bytes)
}
fn opaque_id() -> Result<Id, ErrorCode> {
    let digest = crate::environment_control::opaque()?;
    Id::new(format!("meta-{}", digest.as_str()))
}
fn bounded_size(value: &impl Serialize) -> Result<usize, ErrorCode> {
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, v: &[u8]) -> std::io::Result<usize> {
            if v.len() > WINDOW_BYTES.saturating_sub(self.0) {
                return Err(std::io::Error::other("metadatalimit"));
            }
            self.0 += v.len();
            Ok(v.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut count = Count(0);
    serde_json::to_writer(&mut count, value).map_err(|_| ErrorCode::OutputLimitExceeded)?;
    Ok(count.0)
}
fn project(
    input: &ToolInput,
    window: &ObservedWindow,
    rows: &[&devforge_workspace::activity_metadata::MetadataRow],
    next_cursor: Option<Cursor>,
    continued: bool,
) -> Result<ToolOutput, ErrorCode> {
    use devforge_workspace::activity_metadata::MetadataItem;
    let freshness = || {
        Ok::<_, ErrorCode>(ProjectionFreshness {
            origin: if continued || window.source.missing {
                devforge_control::Origin::Cached
            } else {
                devforge_control::Origin::Persisted
            },
            observed_at: SafeInt::new(window.source.observed_at)?,
            revision: SafeInt::new(window.revision)?,
            window: Some(ProjectionWindow {
                epoch: Digest::new(window.source.epoch.clone())?,
                high_water: SafeInt::new(window.source.high_water)?,
                low_water: SafeInt::new(window.source.low_water)?,
                row_truncated: window.source.row_truncated,
                byte_truncated: window.source.byte_truncated,
                bounded: true,
                source_missing: window.source.missing,
            }),
        })
    };
    match input {
        ToolInput::Inspector(_) => {
            let items = rows
                .iter()
                .map(|row| {
                    let (context_id, method, status, queries, elapsed_ms, detail) = match &row.item
                    {
                        MetadataItem::Inspector {
                            context_id,
                            method,
                            status,
                            queries,
                            elapsed_ms,
                        } => (context_id, method, status, queries, elapsed_ms, None),
                        MetadataItem::InspectorDetail {
                            context_id,
                            method,
                            status,
                            queries,
                            elapsed_ms,
                            detail,
                        } => {
                            let value = InspectorDetail {
                                kind: Id::new(&detail.kind)?,
                                observed_at: SafeInt::new(detail.observed_at)?,
                                text: devforge_control::limits::Text::new(detail.text.clone())?,
                                duration_ms: detail.duration_ms.map(SafeInt::new).transpose()?,
                                truncated: detail.truncated,
                                capture_truncated: detail.capture_truncated,
                                sql_ms: detail.sql_ms.map(SafeInt::new).transpose()?,
                                peak_memory_bytes: detail
                                    .peak_memory_bytes
                                    .map(SafeInt::new)
                                    .transpose()?,
                                repeated_count: detail
                                    .repeated_count
                                    .map(SafeInt::new)
                                    .transpose()?,
                            };
                            (context_id, method, status, queries, elapsed_ms, Some(value))
                        }
                        _ => return Err(ErrorCode::StorageUnavailable),
                    };
                    Ok(InspectorItem {
                        context_id: Id::new(context_id)?,
                        method: devforge_control::limits::Text::new(method.clone())?,
                        status: *status,
                        queries: SafeInt::new(*queries)?,
                        elapsed_ms: SafeInt::new(*elapsed_ms)?,
                        detail,
                    })
                })
                .collect::<Result<_, _>>()?;
            Ok(ToolOutput::Inspector(InspectorOutput {
                freshness: freshness()?,
                items,
                next_cursor,
            }))
        }
        ToolInput::Log(_) => {
            let items = rows
                .iter()
                .map(|row| {
                    let (event_id, level, code, observed_at, message) = match &row.item {
                        MetadataItem::Log {
                            event_id,
                            level,
                            code,
                            observed_at,
                        } => (event_id, level, code, observed_at, None),
                        MetadataItem::LogContent {
                            event_id,
                            level,
                            code,
                            observed_at,
                            message,
                        } => (
                            event_id,
                            level,
                            code,
                            observed_at,
                            Some(devforge_control::limits::Text::new(message.clone())?),
                        ),
                        _ => return Err(ErrorCode::StorageUnavailable),
                    };
                    let (message, exception) = if code == "captured_exception" {
                        (None, message)
                    } else {
                        (message, None)
                    };
                    Ok(LogItem {
                        event_id: Id::new(event_id)?,
                        level: devforge_control::limits::Text::new(level.clone())?,
                        code: Id::new(code)?,
                        observed_at: SafeInt::new(*observed_at)?,
                        message,
                        exception,
                    })
                })
                .collect::<Result<_, _>>()?;
            Ok(ToolOutput::Log(LogOutput {
                freshness: freshness()?,
                items,
                next_cursor,
            }))
        }
        ToolInput::Mail(input) => {
            let items = rows
                .iter()
                .map(|row| mail_item(&row.item, input.detail == Some(true)))
                .collect::<Result<_, _>>()?;
            Ok(ToolOutput::Mail(MailOutput {
                freshness: freshness()?,
                items,
                next_cursor,
            }))
        }
        _ => Err(ErrorCode::UnsupportedCapability),
    }
}
fn doctor(actor: &ActorHandle, binding: &ProjectBinding) -> Result<ToolOutput, ErrorCode> {
    let view = actor
        .external_snapshot()
        .map_err(|_| ErrorCode::StorageUnavailable)?;
    let row = view
        .snapshot
        .projects
        .iter()
        .find(|r| r.project.id == binding.project_id().as_str())
        .ok_or(ErrorCode::StorageUnavailable)?;
    let (code, retryable, next) = if view.error.is_some() || view.stopped {
        ("cached_metadata_unavailable", true, Some("health.doctor"))
    } else if row.project.revision != binding.revision() {
        ("cached_metadata_stale", true, Some("health.doctor"))
    } else if [&row.project.php, &row.project.node].into_iter().any(|id| {
        !view
            .snapshot
            .runtimes
            .iter()
            .any(|r| &r.id == id && r.installed)
    }) {
        ("runtime_missing", false, Some("runtimes.install"))
    } else if row.state == "failed" {
        ("service_failed", true, Some("services.status"))
    } else if row.state == "stopped" {
        ("service_stopped", false, Some("services.start"))
    } else {
        ("cached_metadata_available", false, None)
    };
    Ok(ToolOutput::Health(HealthOutput {
        freshness: ProjectionFreshness {
            origin: devforge_control::Origin::Cached,
            observed_at: SafeInt::new(view.observed_at)?,
            revision: SafeInt::new(row.project.revision)?,
            window: None,
        },
        items: vec![HealthItem {
            code: Id::new(code)?,
            retryable,
            next_action: next.map(Id::new).transpose()?,
            evidence: Some(Id::new(match code {
                "runtime_missing" => "runtime_not_confirmed_in_snapshot",
                "service_failed" => "cached_service_failed",
                "service_stopped" => "cached_service_stopped",
                "cached_metadata_stale" => "revision_mismatch",
                "cached_metadata_unavailable" => "snapshot_unavailable",
                _ => "cached_project_snapshot",
            })?),
            impact: Some(Id::new(match code {
                "runtime_missing" => "runtime_may_be_unavailable",
                "service_failed" | "service_stopped" => "project_service_unavailable",
                "cached_metadata_stale" | "cached_metadata_unavailable" => "diagnosis_incomplete",
                _ => "no_known_issue",
            })?),
        }],
        next_cursor: None,
    }))
}
struct Token(Zeroizing<String>);
impl<'de> Deserialize<'de> for Token {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = Zeroizing::new(String::deserialize(d)?);
        if v.len() != 64 {
            return Err(serde::de::Error::custom("invalidcredential"));
        }
        Ok(Self(v))
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    method: String,
    credential: Token,
    request: Request<ToolInput>,
}

#[cfg(test)]
mod mail_cache_tests {
    use super::*;
    use devforge_workspace::activity_metadata::{
        MailContentDetail, MetadataItem, MetadataRow, MetadataWindow,
    };
    #[test]
    fn support_artifact_expiry_obeys_wall_and_monotonic_clocks() {
        let mut artifact = SupportArtifact {
            id: Id::new("artifact").unwrap(),
            client: Id::new("client").unwrap(),
            grant: Id::new("grant").unwrap(),
            project: Id::new("project").unwrap(),
            revision: 1,
            bytes: vec![],
            expires: Instant::now() + Duration::from_secs(60),
            expires_at: 120,
        };
        assert!(artifact.live_at(119));
        assert!(!artifact.live_at(120));
        artifact.expires = Instant::now() - Duration::from_millis(1);
        assert!(!artifact.live_at(119));
    }
    #[test]
    fn maintenance_prune_releases_expired_sensitive_windows_and_handles() {
        let id = |v| Id::new(v).unwrap();
        let mut windows = Windows::default();
        let source = MetadataWindow {
            rows: vec![MetadataRow {
                sequence: 1,
                item: MetadataItem::MailContent {
                    mail_id: "a".repeat(32),
                    received_at: 1,
                    attachments: 0,
                    detail: Some(MailContentDetail {
                        subject: "subject".into(),
                        from: "from".into(),
                        to: "to".into(),
                        text: "text".into(),
                        links: vec!["https://example.test/?token=PRIVATE".into()],
                        attachment_metadata: vec![],
                        truncated: false,
                    }),
                },
            }],
            epoch: "0".repeat(64),
            high_water: 1,
            low_water: 1,
            observed_at: 1,
            row_truncated: false,
            byte_truncated: false,
            missing: false,
            origin: "fixture",
            source_rows: 1,
            source_bytes: 1,
            anchor_present: None,
            first_omitted: None,
        };
        let window = Arc::new(ObservedWindow {
            id: id("window"),
            client: id("client"),
            grant: id("grant"),
            project: id("project"),
            revision: 1,
            tool: id("mail.list"),
            filter: Digest::new("1".repeat(64)).unwrap(),
            filter_binding: Digest::new("2".repeat(64)).unwrap(),
            source,
            expires: Instant::now() - Duration::from_millis(1),
            expires_at: 1,
            bytes: 256,
        });
        let weak = Arc::downgrade(&window);
        windows.windows.push_back(window.clone());
        windows.bytes = 256;
        windows.issue(window.clone(), 1).unwrap();
        drop(window);
        assert!(weak.upgrade().is_some());
        windows.prune();
        assert!(weak.upgrade().is_none());
        assert!(windows.windows.is_empty() && windows.handles.is_empty());
        assert_eq!(windows.bytes, 0);
        let active = Arc::new(ObservedWindow {
            id: id("active"),
            client: id("client"),
            grant: id("grant"),
            project: id("project"),
            revision: 1,
            tool: id("mail.list"),
            filter: Digest::new("3".repeat(64)).unwrap(),
            filter_binding: Digest::new("4".repeat(64)).unwrap(),
            source: MetadataWindow {
                rows: vec![],
                epoch: "0".repeat(64),
                high_water: 1,
                low_water: 1,
                observed_at: 1,
                row_truncated: false,
                byte_truncated: false,
                missing: false,
                origin: "fixture",
                source_rows: 1,
                source_bytes: 1,
                anchor_present: None,
                first_omitted: None,
            },
            expires: Instant::now() + Duration::from_secs(10),
            expires_at: 10,
            bytes: 128,
        });
        let weak = Arc::downgrade(&active);
        windows.windows.push_back(active.clone());
        windows.bytes = 128;
        windows.issue(active.clone(), 1).unwrap();
        drop(active);
        windows.evict_grant(&id("other"));
        assert!(weak.upgrade().is_some());
        windows.evict_grant(&id("grant"));
        assert!(weak.upgrade().is_none());
        assert!(windows.handles.is_empty() && windows.windows.is_empty());
        assert_eq!(windows.bytes, 0);
    }
}
