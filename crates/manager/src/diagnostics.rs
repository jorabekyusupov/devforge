//! Four metadata projections only. Raw events/mail and content search are never
//! accepted by this adapter. Pagination retains bounded observed safe rows.
use crate::{
    PeerIdentity,
    actor::ActorHandle,
    external_auth::ExternalCredential,
    external_control::{Server as Grants, grant_error},
};
use devforge_control::{
    Digest, ErrorCode, Id, ProjectBinding, Request, Response, Scope,
    limits::{Cursor, CursorContext, SafeInt},
    tools::*,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
    time::{Duration, Instant},
};
use zeroize::Zeroizing;
const TTL: Duration = Duration::from_secs(120);
const WINDOW_BYTES: usize = 8 * 1024 * 1024;
pub(crate) enum ReadMode {
    Registration,
    Window(devforge_workspace::activity_metadata::ActivityKind),
    Epoch(devforge_workspace::activity_metadata::ActivityKind),
}
type Check = Box<dyn Fn(&devforge_workspace::Workspace) -> devforge_workspace::Result<()> + Send>;
pub(crate) struct MetadataRead {
    pub project: Id,
    pub mode: ReadMode,
    pub check: Check,
    pub cancelled: Arc<AtomicBool>,
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
            check()?;
            let binding =
                crate::service_control::domain_registration_binding(workspace, &self.project)
                    .map_err(crate::service_control::ws_error)?;
            let (window, epoch) = match &self.mode {
                ReadMode::Registration => (None, None),
                ReadMode::Window(kind) => (
                    Some(workspace.activity_metadata(self.project.as_str(), kind, &check)?),
                    None,
                ),
                ReadMode::Epoch(kind) => (
                    None,
                    Some(workspace.activity_metadata_epoch(self.project.as_str(), kind, &check)?),
                ),
            };
            check()?;
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
    filter: Digest,
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
pub(crate) struct Diagnostics {
    grants: Arc<Grants>,
    windows: Mutex<Windows>,
}
impl Diagnostics {
    pub(crate) fn new(grants: Arc<Grants>) -> Self {
        Self {
            grants,
            windows: Mutex::new(Default::default()),
        }
    }
    pub(crate) fn clear(&self) {
        *self
            .windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Windows::default();
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
    ) -> Result<ReadResult, ErrorCode> {
        let policy = self.clone();
        let peer = peer.clone();
        let credential =
            ExternalCredential::from_wire(credential.expose_secret()).map_err(grant_error)?;
        let subject = project.clone();
        let check = Box::new(move |workspace: &devforge_workspace::Workspace| {
            let binding = crate::service_control::domain_registration_binding(workspace, &subject)
                .map_err(crate::service_control::ws_error)?;
            policy
                .grants
                .registry
                .lock()
                .map_err(|_| crate::service_control::ws_error(ErrorCode::StorageUnavailable))?
                .as_mut()
                .ok_or_else(|| crate::service_control::ws_error(ErrorCode::Interrupted))?
                .authorize(&peer, &credential, &binding, &[Scope::Observer], &[], None)
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
                reply,
            })
            .map_err(|_| ErrorCode::Busy)?;
        match receiver.recv_timeout(Duration::from_secs(5)) {
            Ok(result) => result.map_err(crate::service_control::environment_error),
            Err(_) => {
                cancelled.store(true, Ordering::Release);
                Err(ErrorCode::TimedOut)
            }
        }
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
        let credential = ExternalCredential::from_wire(&wire.credential.0).map_err(grant_error)?;
        let registration = self.query(
            actor,
            peer,
            &credential,
            &wire.request.project_id,
            ReadMode::Registration,
        )?;
        let mut grants = self
            .grants
            .registry
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        let registry = grants.as_mut().ok_or(ErrorCode::Interrupted)?;
        let proof = registry
            .authorize(
                peer,
                &credential,
                &registration.binding,
                &[Scope::Observer],
                &[],
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
                    || window.filter != filter
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
                    filter,
                    expires: Instant::now() + TTL,
                    expires_at: now + TTL.as_millis() as u64,
                    bytes,
                    source,
                });
                self.windows
                    .lock()
                    .map_err(|_| ErrorCode::StorageUnavailable)?
                    .insert(window.clone())?;
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
        let mut grants = self
            .grants
            .registry
            .lock()
            .map_err(|_| ErrorCode::StorageUnavailable)?;
        let proof = grants
            .as_mut()
            .ok_or(ErrorCode::Interrupted)?
            .authorize(
                peer,
                &credential,
                &registration.binding,
                &[Scope::Observer],
                &[],
                wire.request.expected_revision,
            )
            .map_err(grant_error)?;
        prepared.check_output(&output, &registration.binding, proof.observed_at())?;
        Ok(output)
    }
}
fn check_input(input: &ToolInput) -> Result<(), ErrorCode> {
    match input {
        ToolInput::Health(_) => Ok(()),
        ToolInput::Log(input) if input.query.is_none() => Ok(()),
        ToolInput::Inspector(_) => Ok(()),
        ToolInput::Mail(input) if input.subject.is_none() && input.recipient.is_none() => Ok(()),
        ToolInput::Log(_) | ToolInput::Mail(_) => Err(ErrorCode::Unauthorized),
        _ => Err(ErrorCode::UnsupportedCapability),
    }
}
fn kind(
    input: &ToolInput,
) -> Result<devforge_workspace::activity_metadata::ActivityKind, ErrorCode> {
    use devforge_workspace::activity_metadata::ActivityKind;
    match input {
        ToolInput::Inspector(p) => Ok(ActivityKind::Inspector {
            context: p.context_id.as_ref().map(|v| v.as_str().into()),
        }),
        ToolInput::Log(p) => Ok(ActivityKind::Logs {
            level: p.level.as_ref().map(|v| v.as_str().into()),
        }),
        ToolInput::Mail(_) => Ok(ActivityKind::Mail),
        _ => Err(ErrorCode::UnsupportedCapability),
    }
}
fn filter_digest(input: &ToolInput) -> Result<Digest, ErrorCode> {
    let value = match input {
        ToolInput::Log(p) => json!({"tool":input.tool(),"level":p.level}),
        ToolInput::Inspector(p) => json!({"tool":input.tool(),"context":p.context_id}),
        ToolInput::Mail(_) => json!({"tool":input.tool()}),
        _ => return Err(ErrorCode::UnsupportedCapability),
    };
    devforge_control_adapter::sha256(
        &serde_json::to_vec(&value).map_err(|_| ErrorCode::InvalidInput)?,
    )
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
                    let MetadataItem::Inspector {
                        context_id,
                        method,
                        status,
                        queries,
                        elapsed_ms,
                    } = &row.item
                    else {
                        return Err(ErrorCode::StorageUnavailable);
                    };
                    Ok(InspectorItem {
                        context_id: Id::new(context_id)?,
                        method: devforge_control::limits::Text::new(method.clone())?,
                        status: *status,
                        queries: SafeInt::new(*queries)?,
                        elapsed_ms: SafeInt::new(*elapsed_ms)?,
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
                    let MetadataItem::Log {
                        event_id,
                        level,
                        code,
                        observed_at,
                    } = &row.item
                    else {
                        return Err(ErrorCode::StorageUnavailable);
                    };
                    Ok(LogItem {
                        event_id: Id::new(event_id)?,
                        level: devforge_control::limits::Text::new(level.clone())?,
                        code: Id::new(code)?,
                        observed_at: SafeInt::new(*observed_at)?,
                    })
                })
                .collect::<Result<_, _>>()?;
            Ok(ToolOutput::Log(LogOutput {
                freshness: freshness()?,
                items,
                next_cursor,
            }))
        }
        ToolInput::Mail(_) => {
            let items = rows
                .iter()
                .map(|row| {
                    let MetadataItem::Mail {
                        mail_id,
                        received_at,
                        attachments,
                    } = &row.item
                    else {
                        return Err(ErrorCode::StorageUnavailable);
                    };
                    Ok(MailItem {
                        mail_id: Id::new(mail_id)?,
                        received_at: SafeInt::new(*received_at)?,
                        attachments: SafeInt::new(*attachments)?,
                    })
                })
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
