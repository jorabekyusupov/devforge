//! A single real Workspace worker behind a one-shot owner permit.
//!
//! This is an in-process desktop composition boundary, not a public dispatcher
//! or grant issuer. Every admission/read/cancel needs a fresh DesktopAuthority.
//! Previously admitted jobs survive session revocation/disconnect; execution-time
//! grant revalidation belongs to the later external-client composition.
use crate::{WorkspacePermit, auth::DesktopAuthority};
use devforge_control::{Id, OperationState};
use devforge_control_adapter::DomainScope;
use devforge_core::SecretStore;
use devforge_runtime::Cancellation;
use devforge_workspace::{Command, Snapshot, Workspace, setup::Progress};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const QUEUE_CAPACITY: usize = 32;
pub const RESULT_BYTES: usize = 65_536;
pub const RESULT_ROWS: usize = 128;
pub const RESULTS_TOTAL_BYTES: usize = 1_048_576;
pub const COMMAND_BYTES: usize = 8 * 1024 * 1024;
pub const QUEUE_BYTES: usize = 16 * 1024 * 1024;
pub const SNAPSHOT_BYTES: usize = 4_194_304;
/// Receipt namespace for genuinely global desktop operations, never a project.
pub const ROOT_SUBJECT: &str = "desktop-root";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorError {
    Unauthorized,
    Busy,
    ShuttingDown,
    InvalidInput,
    IdempotencyConflict,
    OperationNotFound,
    NotReady,
    ResultUnavailable,
    OperationFailed,
    OperationCancelled,
    OutputLimitExceeded,
    StorageUnavailable,
    WorkspaceUnavailable,
    WorkerUnavailable,
    ShutdownFailed,
    EntropyUnavailable,
    SubjectMismatch,
    UnsupportedSubject,
}
impl std::fmt::Display for ActorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}
impl std::error::Error for ActorError {}
impl ActorError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Unauthorized => "actor_unauthorized",
            Self::Busy => "actor_busy",
            Self::ShuttingDown => "actor_shutting_down",
            Self::InvalidInput => "actor_invalid_input",
            Self::IdempotencyConflict => "actor_idempotency_conflict",
            Self::OperationNotFound => "actor_operation_not_found",
            Self::NotReady => "actor_not_ready",
            Self::ResultUnavailable => "actor_result_unavailable",
            Self::OperationFailed => "actor_operation_failed",
            Self::OperationCancelled => "actor_operation_cancelled",
            Self::OutputLimitExceeded => "actor_output_limit",
            Self::StorageUnavailable => "actor_storage_unavailable",
            Self::WorkspaceUnavailable => "actor_workspace_unavailable",
            Self::WorkerUnavailable => "actor_worker_unavailable",
            Self::ShutdownFailed => "actor_shutdown_failed",
            Self::EntropyUnavailable => "actor_entropy_unavailable",
            Self::SubjectMismatch => "actor_subject_mismatch",
            Self::UnsupportedSubject => "actor_unsupported_subject",
        }
    }
}
pub type Result<T> = std::result::Result<T, ActorError>;

/// No caller-supplied client ID. All private desktop sessions represent the same
/// root/user owner intent; external clients require a different future authority.
#[derive(Clone)]
pub struct OperationIdentity {
    pub project_id: Id,
    pub idempotency_key: Id,
}
#[derive(Debug, Clone, Serialize)]
pub struct Receipt {
    pub operation_id: Id,
    pub client_id: Id,
    pub project_id: Id,
    pub state: OperationState,
    pub code: Option<String>,
}
/// Validated transient domain metadata only; no Display/message, path or payload.
/// A global operation's subject is ROOT_SUBJECT, not a fabricated project binding.
#[derive(Debug, Clone, Serialize)]
pub struct FailureDetail {
    pub scope: DomainScope,
    pub code: String,
    pub project_id: Id,
    pub retryable: bool,
}
pub struct SnapshotView {
    pub snapshot: Arc<Snapshot>,
    pub observed_at: u64,
    pub busy: bool,
    pub stopped: bool,
    pub error: Option<ActorError>,
    pub progress: Option<Progress>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelOutcome {
    Requested,
    AlreadyTerminal,
}

struct Cached {
    snapshot: Arc<Snapshot>,
    observed_at: u64,
    error: Option<ActorError>,
}
struct SavedResult {
    operation_id: Id,
    outcome: RetainedOutcome,
    bytes: usize,
}
#[derive(Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
enum RetainedOutcome {
    Success(Value),
    Failure(FailureDetail),
}
struct State {
    db: Connection,
    _file: File,
    pending: usize,
    waiting: HashMap<Id, WaitingCommand>,
    waiting_bytes: usize,
    results: VecDeque<SavedResult>,
    result_bytes: usize,
}
struct WaitingCommand {
    command: Command,
    bytes: usize,
    cancellation: Cancellation,
}
#[derive(Debug, Clone, Copy)]
pub struct QueueUsage {
    pub waiting_items: usize,
    pub waiting_bytes: usize,
    pub active: bool,
}
struct CancellationEntry {
    client: Id,
    project: Id,
    cancellation: Cancellation,
}
struct Shared {
    root: PathBuf,
    state: Mutex<State>,
    cache: Mutex<Cached>,
    progress: Arc<Mutex<Option<Progress>>>,
    cancellations: Mutex<HashMap<Id, CancellationEntry>>,
    stopping: AtomicBool,
    stopped: AtomicBool,
    active: AtomicBool,
    queued: AtomicUsize,
}
struct Job {
    receipt: Receipt,
    cancellation: Cancellation,
}
enum Message {
    Job(Box<Job>),
}

/// Owns and joins exactly one real Workspace thread. Dropping a Handle does not
/// stop services. Dropping this owner requests cancellation and joins cleanup.
pub struct WorkspaceActor {
    handle: ActorHandle,
    worker: Option<JoinHandle<Result<()>>>,
}
#[derive(Clone)]
pub struct ActorHandle {
    shared: Arc<Shared>,
    sender: SyncSender<Message>,
}
impl WorkspaceActor {
    pub fn start(
        permit: WorkspacePermit,
        secrets: Arc<dyn SecretStore + Send + Sync>,
    ) -> Result<Self> {
        permit
            .endpoint()
            .validate()
            .map_err(|_| ActorError::StorageUnavailable)?;
        let state = State::open(permit.endpoint().directory.join("operations.sqlite3"))?;
        let shared = Arc::new(Shared {
            root: permit.root().to_path_buf(),
            state: Mutex::new(state),
            cache: Mutex::new(Cached {
                snapshot: Arc::new(Snapshot::default()),
                observed_at: 0,
                error: None,
            }),
            progress: Arc::new(Mutex::new(None)),
            cancellations: Mutex::new(HashMap::new()),
            stopping: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            active: AtomicBool::new(false),
            queued: AtomicUsize::new(0),
        });
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker_shared = shared.clone();
        let worker = thread::Builder::new()
            .name("devforge-workspace-owner".into())
            .spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    worker_loop(&permit, secrets, &worker_shared, receiver, ready_tx)
                }));
                let result = outcome.unwrap_or(Err(ActorError::WorkerUnavailable));
                if let Err(error) = result
                    && let Ok(mut cache) = worker_shared.cache.lock()
                {
                    cache.error = Some(error);
                }
                worker_shared.stopping.store(true, Ordering::Release);
                worker_shared.stopped.store(true, Ordering::Release);
                worker_shared.active.store(false, Ordering::Release);
                worker_shared.queued.store(0, Ordering::Release);
                if let Ok(mut entries) = worker_shared.cancellations.lock() {
                    entries.clear();
                }
                if let Ok(mut state) = worker_shared.state.lock() {
                    if result.is_err() {
                        let _ = state.interrupt_pending();
                    }
                    state.waiting.clear();
                    state.waiting_bytes = 0;
                    state.pending = 0;
                }
                // Workspace has been dropped (and its process owners joined) before
                // this permit can release the singleton/install leases.
                drop(permit);
                result
            })
            .map_err(|_| ActorError::WorkerUnavailable)?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                handle: ActorHandle { shared, sender },
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(_) => {
                let _ = worker.join();
                Err(ActorError::WorkerUnavailable)
            }
        }
    }
    pub fn handle(&self) -> ActorHandle {
        self.handle.clone()
    }
    pub fn shutdown(mut self) -> Result<()> {
        self.finish()
    }
    fn finish(&mut self) -> Result<()> {
        self.handle.shared.stopping.store(true, Ordering::Release);
        if let Ok(entries) = self.handle.shared.cancellations.lock() {
            for entry in entries.values() {
                entry.cancellation.cancel();
            }
        }
        self.worker
            .take()
            .map(|worker| worker.join().unwrap_or(Err(ActorError::WorkerUnavailable)))
            .unwrap_or(Ok(()))
    }
}
impl Drop for WorkspaceActor {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

impl ActorHandle {
    pub fn queue_usage(&self, authority: &DesktopAuthority<'_>) -> Result<QueueUsage> {
        self.client(authority)?;
        let state = self
            .shared
            .state
            .lock()
            .map_err(|_| ActorError::StorageUnavailable)?;
        self.client(authority)?;
        Ok(QueueUsage {
            waiting_items: state.pending,
            waiting_bytes: state.waiting_bytes,
            active: self.shared.active.load(Ordering::Acquire),
        })
    }
    /// Bootstrap composition only: this is an actual initialized owner, not a
    /// caller-supplied ready flag or permission to read project metadata.
    pub(crate) fn bootstrap_ready_root(&self) -> Result<&Path> {
        if self.shared.stopping.load(Ordering::Acquire)
            || self.shared.stopped.load(Ordering::Acquire)
        {
            return Err(ActorError::WorkerUnavailable);
        }
        let cache = self
            .shared
            .cache
            .lock()
            .map_err(|_| ActorError::StorageUnavailable)?;
        if cache.observed_at == 0 || cache.error.is_some() {
            return Err(ActorError::WorkspaceUnavailable);
        }
        Ok(&self.shared.root)
    }
    fn client(&self, authority: &DesktopAuthority<'_>) -> Result<Id> {
        authority
            .check_current(Instant::now())
            .map_err(|_| ActorError::Unauthorized)?;
        if authority.root() != self.shared.root {
            return Err(ActorError::Unauthorized);
        }
        let mut hash = Sha256::new();
        native_path_hash(&mut hash, authority.root());
        hash.update([0]);
        hash.update(authority.peer().user.as_bytes());
        Id::new(format!("desktop-{}", hex(&hash.finalize()))).map_err(|_| ActorError::InvalidInput)
    }
    pub fn cached_snapshot(&self, authority: &DesktopAuthority<'_>) -> Result<SnapshotView> {
        self.client(authority)?;
        let cache = self
            .shared
            .cache
            .lock()
            .map_err(|_| ActorError::StorageUnavailable)?;
        Ok(SnapshotView {
            snapshot: cache.snapshot.clone(),
            observed_at: cache.observed_at,
            busy: self.shared.active.load(Ordering::Acquire)
                || self.shared.queued.load(Ordering::Acquire) != 0,
            stopped: self.shared.stopped.load(Ordering::Acquire),
            error: cache.error,
            progress: self
                .shared
                .progress
                .lock()
                .map_err(|_| ActorError::StorageUnavailable)?
                .clone(),
        })
    }
    pub fn submit_desktop(
        &self,
        authority: &DesktopAuthority<'_>,
        identity: OperationIdentity,
        command: Command,
    ) -> Result<Receipt> {
        let client = self.client(authority)?;
        validate_subject(&identity.project_id, &command)?;
        let (fingerprint, encoded_bytes) = fingerprint(&identity.project_id, &command)?;
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| ActorError::StorageUnavailable)?;
        self.client(authority)?;
        if let Some((receipt, previous)) = state.find(&client, &identity)? {
            self.client(authority)?;
            if previous != fingerprint {
                return Err(ActorError::IdempotencyConflict);
            }
            return Ok(receipt);
        }
        if self.shared.stopping.load(Ordering::Acquire) {
            return Err(ActorError::ShuttingDown);
        }
        state.purge_cancelled_payloads();
        if state.pending >= QUEUE_CAPACITY
            || encoded_bytes > QUEUE_BYTES.saturating_sub(state.waiting_bytes)
        {
            return Err(ActorError::Busy);
        }
        let receipt = Receipt {
            operation_id: random_id()?,
            client_id: client.clone(),
            project_id: identity.project_id.clone(),
            state: OperationState::Queued,
            code: None,
        };
        let cancellation = Cancellation::default();
        // Admission is serialized with queue accounting. No capacity-rejected
        // request writes a Queued row. Only the worker consumes this channel.
        self.shared
            .cancellations
            .lock()
            .map_err(|_| ActorError::StorageUnavailable)?
            .insert(
                receipt.operation_id.clone(),
                CancellationEntry {
                    client,
                    project: identity.project_id,
                    cancellation: cancellation.clone(),
                },
            );
        if let Err(error) =
            state.insert(&receipt, &identity.idempotency_key, &fingerprint, authority)
        {
            if let Ok(mut entries) = self.shared.cancellations.lock() {
                entries.remove(&receipt.operation_id);
            }
            return Err(error);
        }
        state.pending += 1;
        state.waiting_bytes += encoded_bytes;
        state.waiting.insert(
            receipt.operation_id.clone(),
            WaitingCommand {
                command,
                bytes: encoded_bytes,
                cancellation: cancellation.clone(),
            },
        );
        self.shared.queued.store(state.pending, Ordering::Release);
        let job = Message::Job(Box::new(Job {
            receipt: receipt.clone(),
            cancellation,
        }));
        if let Err(error) = self.sender.try_send(job) {
            state.pending -= 1;
            state.take_payload(&receipt.operation_id);
            self.shared.queued.store(state.pending, Ordering::Release);
            self.shared
                .cancellations
                .lock()
                .map_err(|_| ActorError::StorageUnavailable)?
                .remove(&receipt.operation_id);
            state.update(
                &receipt.operation_id,
                OperationState::Interrupted,
                Some("admission_interrupted"),
            )?;
            return Err(match error {
                TrySendError::Full(_) => ActorError::Busy,
                TrySendError::Disconnected(_) => ActorError::WorkerUnavailable,
            });
        }
        Ok(receipt)
    }
    pub fn status(
        &self,
        authority: &DesktopAuthority<'_>,
        project: &Id,
        operation: &Id,
    ) -> Result<Receipt> {
        let client = self.client(authority)?;
        let state = self
            .shared
            .state
            .lock()
            .map_err(|_| ActorError::StorageUnavailable)?;
        self.client(authority)?;
        let receipt = state.get(&client, project, operation)?;
        self.client(authority)?;
        Ok(receipt)
    }
    pub fn result(
        &self,
        authority: &DesktopAuthority<'_>,
        project: &Id,
        operation: &Id,
    ) -> Result<Value> {
        let client = self.client(authority)?;
        let state = self
            .shared
            .state
            .lock()
            .map_err(|_| ActorError::StorageUnavailable)?;
        self.client(authority)?;
        let receipt = state.get(&client, project, operation)?;
        self.client(authority)?;
        match receipt.state {
            OperationState::Succeeded => state
                .results
                .iter()
                .find(|r| &r.operation_id == operation)
                .and_then(|r| match &r.outcome {
                    RetainedOutcome::Success(value) => Some(value.clone()),
                    _ => None,
                })
                .ok_or(ActorError::ResultUnavailable),
            OperationState::Cancelled => Err(ActorError::OperationCancelled),
            OperationState::Failed => Err(ActorError::OperationFailed),
            OperationState::Interrupted | OperationState::Unknown => {
                Err(ActorError::ResultUnavailable)
            }
            _ => Err(ActorError::NotReady),
        }
    }
    pub fn failure_detail(
        &self,
        authority: &DesktopAuthority<'_>,
        project: &Id,
        operation: &Id,
    ) -> Result<FailureDetail> {
        let client = self.client(authority)?;
        let state = self
            .shared
            .state
            .lock()
            .map_err(|_| ActorError::StorageUnavailable)?;
        self.client(authority)?;
        let receipt = state.get(&client, project, operation)?;
        self.client(authority)?;
        if !receipt.state.terminal() {
            return Err(ActorError::NotReady);
        }
        state
            .results
            .iter()
            .find(|entry| &entry.operation_id == operation)
            .and_then(|entry| match &entry.outcome {
                RetainedOutcome::Failure(detail) => Some(detail.clone()),
                _ => None,
            })
            .ok_or(ActorError::ResultUnavailable)
    }
    /// The cancellation bit is independent of the busy Workspace worker and DB
    /// mutex. CancelRequested persistence is best-effort here; the worker always
    /// persists the eventual terminal observation. Completion can win the race.
    pub fn cancel(
        &self,
        authority: &DesktopAuthority<'_>,
        project: &Id,
        operation: &Id,
    ) -> Result<CancelOutcome> {
        let client = self.client(authority)?;
        {
            let entries = self
                .shared
                .cancellations
                .lock()
                .map_err(|_| ActorError::StorageUnavailable)?;
            self.client(authority)?;
            if let Some(entry) = entries.get(operation) {
                if entry.client != client || &entry.project != project {
                    return Err(ActorError::OperationNotFound);
                }
                entry.cancellation.cancel();
                drop(entries);
                if let Ok(mut state) = self.shared.state.try_lock() {
                    state.take_payload(operation);
                    if let Ok(receipt) = state.get(&client, project, operation)
                        && receipt.state == OperationState::Running
                    {
                        let _ = state.update(operation, OperationState::CancelRequested, None);
                    }
                }
                return Ok(CancelOutcome::Requested);
            }
        }
        let receipt = self.status(authority, project, operation)?;
        if receipt.state.terminal() {
            Ok(CancelOutcome::AlreadyTerminal)
        } else {
            Err(ActorError::WorkerUnavailable)
        }
    }
}

fn worker_loop(
    permit: &WorkspacePermit,
    secrets: Arc<dyn SecretStore + Send + Sync>,
    shared: &Arc<Shared>,
    receiver: Receiver<Message>,
    ready: SyncSender<Result<()>>,
) -> Result<()> {
    let mut workspace = match Workspace::open(permit.root(), secrets) {
        Ok(value) => value,
        Err(_) => {
            let _ = ready.send(Err(ActorError::WorkspaceUnavailable));
            return Err(ActorError::WorkspaceUnavailable);
        }
    };
    workspace.progress = shared.progress.clone();
    if let Err(error) = refresh(shared, &workspace) {
        let _ = ready.send(Err(error));
        return Err(error);
    }
    let _ = ready.send(Ok(()));
    let mut last_refresh = Instant::now();
    loop {
        if shared.stopping.load(Ordering::Acquire) {
            while let Ok(message) = receiver.try_recv() {
                match message {
                    Message::Job(job) => finish_cancelled(shared, &job)?,
                }
            }
            let result = workspace.shutdown().map_err(|_| ActorError::ShutdownFailed);
            if let Err(error) = refresh(shared, &workspace)
                && let Ok(mut cache) = shared.cache.lock()
            {
                cache.error = Some(error);
            }
            drop(workspace);
            return result;
        }
        let message = match receiver.recv_timeout(Duration::from_millis(50)) {
            Ok(value) => value,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if last_refresh.elapsed() >= Duration::from_millis(250) {
                    if let Err(error) = refresh(shared, &workspace)
                        && let Ok(mut cache) = shared.cache.lock()
                    {
                        cache.error = Some(error);
                    }
                    last_refresh = Instant::now();
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                shared.stopping.store(true, Ordering::Release);
                continue;
            }
        };
        match message {
            Message::Job(job) => {
                if job.cancellation.is_cancelled() || shared.stopping.load(Ordering::Acquire) {
                    finish_cancelled(shared, &job)?;
                    continue;
                }
                let command = {
                    let mut state = shared
                        .state
                        .lock()
                        .map_err(|_| ActorError::StorageUnavailable)?;
                    state.pending -= 1;
                    shared.queued.store(state.pending, Ordering::Release);
                    let payload = state.take_payload(&job.receipt.operation_id);
                    if job.cancellation.is_cancelled() || payload.is_none() {
                        state.update(
                            &job.receipt.operation_id,
                            OperationState::Cancelled,
                            Some("cancelled"),
                        )?;
                        drop(state);
                        shared
                            .cancellations
                            .lock()
                            .map_err(|_| ActorError::StorageUnavailable)?
                            .remove(&job.receipt.operation_id);
                        continue;
                    }
                    shared.active.store(true, Ordering::Release);
                    state.update(&job.receipt.operation_id, OperationState::Running, None)?;
                    payload.ok_or(ActorError::WorkerUnavailable)?.command
                };
                let outcome = workspace.execute(command, &job.cancellation);
                if let Err(error) = refresh(shared, &workspace)
                    && let Ok(mut cache) = shared.cache.lock()
                {
                    cache.error = Some(error);
                }
                last_refresh = Instant::now();
                let mut state = shared
                    .state
                    .lock()
                    .map_err(|_| ActorError::StorageUnavailable)?;
                match outcome {
                    Ok(value) => {
                        // A successful side effect stays Succeeded even if its
                        // output cannot be retained or a late cancel raced it.
                        let retained = state.remember(job.receipt.operation_id.clone(), value);
                        state.update(
                            &job.receipt.operation_id,
                            OperationState::Succeeded,
                            if retained {
                                None
                            } else {
                                Some("result_output_limit")
                            },
                        )?;
                    }
                    Err(error) => {
                        let cancelled =
                            matches!(error.code.as_str(), "cancelled" | "setup_cancelled");
                        if let Some(detail) = validated_failure(&error, &job.receipt.project_id) {
                            state.remember_outcome(
                                job.receipt.operation_id.clone(),
                                RetainedOutcome::Failure(detail),
                            );
                        }
                        state.update(
                            &job.receipt.operation_id,
                            if cancelled {
                                OperationState::Cancelled
                            } else {
                                OperationState::Failed
                            },
                            Some(stable_code(error.scope, &error.code)),
                        )?;
                    }
                }
                drop(state);
                shared
                    .cancellations
                    .lock()
                    .map_err(|_| ActorError::StorageUnavailable)?
                    .remove(&job.receipt.operation_id);
                shared.active.store(false, Ordering::Release);
            }
        }
    }
}
fn finish_cancelled(shared: &Shared, job: &Job) -> Result<()> {
    let mut state = shared
        .state
        .lock()
        .map_err(|_| ActorError::StorageUnavailable)?;
    state.pending -= 1;
    state.take_payload(&job.receipt.operation_id);
    shared.queued.store(state.pending, Ordering::Release);
    state.update(
        &job.receipt.operation_id,
        OperationState::Cancelled,
        Some("cancelled"),
    )?;
    shared
        .cancellations
        .lock()
        .map_err(|_| ActorError::StorageUnavailable)?
        .remove(&job.receipt.operation_id);
    Ok(())
}
fn refresh(shared: &Shared, workspace: &Workspace) -> Result<()> {
    let snapshot = workspace
        .snapshot()
        .map_err(|_| ActorError::WorkspaceUnavailable)?;
    measure(&snapshot, SNAPSHOT_BYTES)?;
    let mut cache = shared
        .cache
        .lock()
        .map_err(|_| ActorError::StorageUnavailable)?;
    *cache = Cached {
        snapshot: Arc::new(snapshot),
        observed_at: now(),
        error: None,
    };
    Ok(())
}

impl State {
    fn open(path: PathBuf) -> Result<Self> {
        for suffix in ["", "-journal", "-wal", "-shm"] {
            let mut name = path.as_os_str().to_os_string();
            name.push(suffix);
            let sibling = PathBuf::from(name);
            if fs::symlink_metadata(&sibling).is_ok() {
                let mut options = OpenOptions::new();
                options.read(true).write(true);
                crate::transport::os::secure_lock_options(&mut options);
                let file = options
                    .open(sibling)
                    .map_err(|_| ActorError::StorageUnavailable)?;
                crate::transport::os::validate_lock(&file)
                    .map_err(|_| ActorError::StorageUnavailable)?;
            }
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        crate::transport::os::secure_lock_options(&mut options);
        let file = options
            .open(&path)
            .map_err(|_| ActorError::StorageUnavailable)?;
        crate::transport::os::validate_lock(&file).map_err(|_| ActorError::StorageUnavailable)?;
        let db = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(|_| ActorError::StorageUnavailable)?;
        db.busy_timeout(Duration::from_millis(250))
            .map_err(|_| ActorError::StorageUnavailable)?;
        let schema: u32 = db
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .map_err(|_| ActorError::StorageUnavailable)?;
        if schema > 1 {
            return Err(ActorError::StorageUnavailable);
        }
        db.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;")
            .map_err(|_| ActorError::StorageUnavailable)?;
        if schema == 0 {
            db.execute_batch("BEGIN IMMEDIATE; CREATE TABLE operations(operation_id TEXT PRIMARY KEY,client_id TEXT NOT NULL,project_id TEXT NOT NULL,idempotency_key TEXT NOT NULL,fingerprint TEXT NOT NULL,state TEXT NOT NULL,code TEXT,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,UNIQUE(client_id,project_id,idempotency_key)); PRAGMA user_version=1; COMMIT;").map_err(|_| ActorError::StorageUnavailable)?;
        }
        let state = Self {
            db,
            _file: file,
            pending: 0,
            waiting: HashMap::new(),
            waiting_bytes: 0,
            results: VecDeque::new(),
            result_bytes: 0,
        };
        state.interrupt_pending()?;
        Ok(state)
    }
    fn interrupt_pending(&self) -> Result<()> {
        self.db.execute("UPDATE operations SET state=CASE WHEN state='queued' THEN 'interrupted' ELSE 'unknown' END,code='owner_interrupted',updated_at=?1 WHERE state IN ('queued','running','cancel_requested')", [now() as i64]).map_err(|_| ActorError::StorageUnavailable)?;
        Ok(())
    }
    fn take_payload(&mut self, operation: &Id) -> Option<WaitingCommand> {
        let value = self.waiting.remove(operation)?;
        self.waiting_bytes -= value.bytes;
        Some(value)
    }
    fn purge_cancelled_payloads(&mut self) {
        let mut released = 0;
        self.waiting.retain(|_, value| {
            if value.cancellation.is_cancelled() {
                released += value.bytes;
                false
            } else {
                true
            }
        });
        self.waiting_bytes -= released;
    }
    fn insert(
        &mut self,
        receipt: &Receipt,
        key: &Id,
        fingerprint: &str,
        authority: &DesktopAuthority<'_>,
    ) -> Result<()> {
        let transaction = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|_| ActorError::StorageUnavailable)?;
        authority
            .check_current(Instant::now())
            .map_err(|_| ActorError::Unauthorized)?;
        transaction.execute("INSERT INTO operations(operation_id,client_id,project_id,idempotency_key,fingerprint,state,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,'queued',?6,?6)", params![receipt.operation_id.as_str(),receipt.client_id.as_str(),receipt.project_id.as_str(),key.as_str(),fingerprint,now() as i64]).map_err(|_| ActorError::StorageUnavailable)?;
        authority
            .check_current(Instant::now())
            .map_err(|_| ActorError::Unauthorized)?;
        transaction
            .commit()
            .map_err(|_| ActorError::StorageUnavailable)?;
        Ok(())
    }
    fn find(&self, client: &Id, identity: &OperationIdentity) -> Result<Option<(Receipt, String)>> {
        self.db.query_row("SELECT operation_id,client_id,project_id,state,code,fingerprint FROM operations WHERE client_id=?1 AND project_id=?2 AND idempotency_key=?3", params![client.as_str(),identity.project_id.as_str(),identity.idempotency_key.as_str()], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,String>(3)?,row.get::<_,Option<String>>(4)?,row.get::<_,String>(5)?))).optional().map_err(|_| ActorError::StorageUnavailable)?.map(|(op,client,project,state,code,fingerprint)| Ok((receipt(op,client,project,state,code)?,fingerprint))).transpose()
    }
    fn get(&self, client: &Id, project: &Id, operation: &Id) -> Result<Receipt> {
        let row = self.db.query_row("SELECT operation_id,client_id,project_id,state,code FROM operations WHERE operation_id=?1 AND client_id=?2 AND project_id=?3", params![operation.as_str(),client.as_str(),project.as_str()], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,String>(3)?,row.get::<_,Option<String>>(4)?))).optional().map_err(|_| ActorError::StorageUnavailable)?.ok_or(ActorError::OperationNotFound)?;
        receipt(row.0, row.1, row.2, row.3, row.4)
    }
    fn update(&self, operation: &Id, state: OperationState, code: Option<&str>) -> Result<()> {
        let previous: String = self
            .db
            .query_row(
                "SELECT state FROM operations WHERE operation_id=?1",
                [operation.as_str()],
                |row| row.get(0),
            )
            .map_err(|_| ActorError::StorageUnavailable)?;
        let previous: OperationState = serde_json::from_value(Value::String(previous))
            .map_err(|_| ActorError::StorageUnavailable)?;
        previous
            .transition(state)
            .map_err(|_| ActorError::StorageUnavailable)?;
        let state = serde_json::to_value(state).map_err(|_| ActorError::StorageUnavailable)?;
        self.db
            .execute(
                "UPDATE operations SET state=?1,code=?2,updated_at=?3 WHERE operation_id=?4",
                params![state.as_str(), code, now() as i64, operation.as_str()],
            )
            .map_err(|_| ActorError::StorageUnavailable)?;
        Ok(())
    }
    fn remember(&mut self, operation_id: Id, value: Value) -> bool {
        self.remember_outcome(operation_id, RetainedOutcome::Success(value))
    }
    fn remember_outcome(&mut self, operation_id: Id, outcome: RetainedOutcome) -> bool {
        let Ok(bytes) = measure(&outcome, RESULT_BYTES) else {
            return false;
        };
        while self.results.len() >= RESULT_ROWS || self.result_bytes + bytes > RESULTS_TOTAL_BYTES {
            if let Some(previous) = self.results.pop_front() {
                self.result_bytes -= previous.bytes;
            } else {
                break;
            }
        }
        self.result_bytes += bytes;
        self.results.push_back(SavedResult {
            operation_id,
            outcome,
            bytes,
        });
        true
    }
}
fn receipt(
    op: String,
    client: String,
    project: String,
    state: String,
    code: Option<String>,
) -> Result<Receipt> {
    if code.as_deref().is_some_and(|value| {
        !matches!(
            value,
            "cancelled"
                | "project_failed"
                | "runtime_failed"
                | "serving_failed"
                | "workspace_failed"
                | "result_output_limit"
                | "owner_interrupted"
                | "admission_interrupted"
        )
    }) {
        return Err(ActorError::StorageUnavailable);
    }
    let id = |value| Id::new(value).map_err(|_| ActorError::StorageUnavailable);
    Ok(Receipt {
        operation_id: id(op)?,
        client_id: id(client)?,
        project_id: id(project)?,
        state: serde_json::from_value(Value::String(state))
            .map_err(|_| ActorError::StorageUnavailable)?,
        code,
    })
}
fn random_id() -> Result<Id> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| ActorError::EntropyUnavailable)?;
    Id::new(format!("op-{}", hex(&bytes))).map_err(|_| ActorError::InvalidInput)
}
fn validate_subject(subject: &Id, command: &Command) -> Result<()> {
    let (expected, global) = match command {
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
        | Command::RestoreBackup { id, .. } => (id.as_str(), false),
        Command::Import { .. }
        | Command::CreateProject { .. }
        | Command::DiscoveryRestore { .. }
        | Command::RefreshRuntimes
        | Command::Install { .. }
        | Command::RemoveRuntime { .. }
        | Command::CreateDatabase { .. }
        | Command::StartDatabase { .. }
        | Command::StopDatabase { .. } => (ROOT_SUBJECT, true),
        // Worktree IDs are not project IDs; cache instance ownership also needs
        // trusted resolution. No raw label is accepted until that mapping exists.
        Command::WorktreeArchivePlan { .. }
        | Command::WorktreeArchive { .. }
        | Command::WorktreeRemotePlan { .. }
        | Command::WorktreeRemoteCopy { .. }
        | Command::WorktreeOpen { .. }
        | Command::WorktreePlan { .. }
        | Command::WorktreeCreate { .. }
        | Command::WorktreeCleanupPlan { .. }
        | Command::WorktreeCleanup { .. }
        | Command::StartCache { .. }
        | Command::StopCache { .. } => return Err(ActorError::UnsupportedSubject),
    };
    if subject.as_str() != expected || (!global && expected == ROOT_SUBJECT) {
        return Err(ActorError::SubjectMismatch);
    }
    Ok(())
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn stable_code(scope: &str, code: &str) -> &'static str {
    if matches!(code, "cancelled" | "setup_cancelled") {
        return "cancelled";
    }
    match scope {
        "project" => "project_failed",
        "runtime" => "runtime_failed",
        "serving" => "serving_failed",
        _ => "workspace_failed",
    }
}
fn validated_failure(error: &devforge_workspace::Error, subject: &Id) -> Option<FailureDetail> {
    let scope = match error.scope {
        "project" => DomainScope::Project,
        "runtime" => DomainScope::Runtime,
        "serving" => DomainScope::Serving,
        "workspace" => DomainScope::Workspace,
        _ => return None,
    };
    if error
        .project_id
        .as_deref()
        .is_some_and(|project| project != subject.as_str())
        || error.code.is_empty()
        || error.code.len() > 128
        || !error
            .code
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return None;
    }
    Some(FailureDetail {
        scope,
        code: error.code.clone(),
        project_id: subject.clone(),
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
fn native_path_hash(hash: &mut Sha256, root: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        hash.update(root.as_os_str().as_bytes());
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        for value in root.as_os_str().encode_wide() {
            hash.update(value.to_le_bytes());
        }
    }
}
struct LimitedWriter {
    bytes: usize,
    limit: usize,
    hash: Option<Sha256>,
}
impl Write for LimitedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes) {
            return Err(std::io::Error::other("actor output limit"));
        }
        self.bytes += bytes.len();
        if let Some(hash) = self.hash.as_mut() {
            hash.update(bytes);
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn measure<T: Serialize>(value: &T, limit: usize) -> Result<usize> {
    let mut writer = LimitedWriter {
        bytes: 0,
        limit,
        hash: None,
    };
    serde_json::to_writer(&mut writer, value).map_err(|_| ActorError::OutputLimitExceeded)?;
    Ok(writer.bytes)
}
fn fingerprint(project: &Id, command: &Command) -> Result<(String, usize)> {
    // Preserve the established tuple hash while bounding/counting only command
    // bytes. Id contains ASCII without JSON escapes; tuple overhead is exact.
    let overhead = project.as_str().len() + 5;
    let mut writer = LimitedWriter {
        bytes: 0,
        limit: COMMAND_BYTES + overhead,
        hash: Some(Sha256::new()),
    };
    serde_json::to_writer(&mut writer, &(project, command))
        .map_err(|_| ActorError::InvalidInput)?;
    let bytes = writer.bytes - overhead;
    Ok((
        hex(&writer.hash.ok_or(ActorError::InvalidInput)?.finalize()),
        bytes,
    ))
}

#[cfg(test)]
mod retention_tests {
    use super::*;
    #[test]
    fn malformed_or_foreign_failure_metadata_is_not_projected() {
        let subject = Id::new("project-a").unwrap();
        for error in [
            devforge_workspace::Error {
                scope: "unknown",
                code: "invalid_project".into(),
                project_id: None,
            },
            devforge_workspace::Error {
                scope: "project",
                code: "path/secret-value".into(),
                project_id: None,
            },
            devforge_workspace::Error {
                scope: "project",
                code: "invalid_project".into(),
                project_id: Some("project-b".into()),
            },
            devforge_workspace::Error {
                scope: "project",
                code: "x".repeat(129),
                project_id: None,
            },
        ] {
            assert!(validated_failure(&error, &subject).is_none());
        }
    }
    #[test]
    fn retained_results_obey_row_and_byte_caps_without_persisting_payloads() {
        #[cfg(unix)]
        let temporary = tempfile::Builder::new()
            .prefix("df-ret-")
            .tempdir_in("/tmp")
            .unwrap();
        #[cfg(not(unix))]
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("owned-user-root");
        crate::transport::os::ensure_private_dir(&root).unwrap();
        let owner = crate::Owner::acquire(&root).unwrap();
        let path = owner.endpoint().directory.join("operations.sqlite3");
        let mut state = State::open(path.clone()).unwrap();
        for n in 0..(RESULT_ROWS + 1) {
            assert!(state.remember(
                Id::new(format!("small-{n}")).unwrap(),
                serde_json::json!({"value":n})
            ));
        }
        assert_eq!(state.results.len(), RESULT_ROWS);
        assert!(
            state
                .results
                .iter()
                .all(|entry| entry.operation_id.as_str() != "small-0")
        );
        for n in 0..32 {
            assert!(state.remember(
                Id::new(format!("large-{n}")).unwrap(),
                serde_json::json!({"private_payload":"retention-only-no-ledger".repeat(2000)})
            ));
            assert!(state.result_bytes <= RESULTS_TOTAL_BYTES);
        }
        let rows = state.results.len();
        let bytes = state.result_bytes;
        assert!(!state.remember(
            Id::new("oversized").unwrap(),
            serde_json::json!({"private_payload":"x".repeat(RESULT_BYTES)})
        ));
        assert_eq!((state.results.len(), state.result_bytes), (rows, bytes));
        assert!(rows < RESULT_ROWS);
        drop(state);
        assert!(
            !String::from_utf8_lossy(&fs::read(path).unwrap()).contains("retention-only-no-ledger")
        );
    }
}
