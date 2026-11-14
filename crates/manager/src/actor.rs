//! A single real Workspace worker behind a one-shot owner permit.
//!
//! This is an in-process desktop composition boundary, not a public dispatcher
//! or grant issuer. Every admission/read/cancel needs a fresh DesktopAuthority.
//! Previously admitted jobs survive session revocation/disconnect; execution-time
//! grant revalidation belongs to the later external-client composition.
use crate::desktop::{self, AccessRequest, ReadImage, ReadRequest};
use crate::{WorkspacePermit, auth::DesktopAuthority};
use devforge_control::{Id, OperationState};
use devforge_control_adapter::DomainScope;
use devforge_core::{SecretStore, SettingsRepository};
use devforge_runtime::Cancellation;
use devforge_workspace::{Command, Snapshot, Workspace, setup::Progress};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
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
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

pub const QUEUE_CAPACITY: usize = 32;
pub const RESULT_BYTES: usize = 65_536;
pub const RESULT_ROWS: usize = 128;
pub const RESULTS_TOTAL_BYTES: usize = 1_048_576;
pub const COMMAND_BYTES: usize = 8 * 1024 * 1024;
pub const QUEUE_BYTES: usize = 16 * 1024 * 1024;
pub const SNAPSHOT_BYTES: usize = 4_194_304;
// Reserved heap budget for either finite environment or database read request.
const ENV_QUERY_BYTES: usize = 4096;
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
    IdempotencyUnverifiable,
    ReadExpired,
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
            Self::IdempotencyUnverifiable => "actor_idempotency_unverifiable",
            Self::ReadExpired => "actor_read_expired",
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
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationCursor {
    pub client_id: Id,
    pub project_id: Option<Id>,
    pub ceiling: u64,
    pub before: u64,
}
#[derive(Serialize)]
pub struct OperationPage {
    pub items: Vec<Receipt>,
    pub next_cursor: Option<OperationCursor>,
    pub active_reads: Vec<Receipt>,
}
/// Validated transient domain metadata only; no Display/message, path or payload.
/// A global operation's subject is ROOT_SUBJECT, not a fabricated project binding.
#[derive(Debug, Clone, Serialize)]
pub struct FailureDetail {
    pub scope: DomainScope,
    pub code: String,
    pub project_id: Id,
    pub retryable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_project_id: Option<Id>,
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
    reads: VecDeque<SavedRead>,
    volatile_fingerprints: VecDeque<(Id, Zeroizing<String>)>,
    read_tickets: VecDeque<ReadTicket>,
}
struct ReadTicket {
    receipt: Receipt,
    key: Id,
    fingerprint: String,
    expires: Instant,
    cancellation: Cancellation,
}
struct SavedRead {
    operation_id: Id,
    image: Arc<ReadImage>,
}
struct WaitingCommand {
    work: Work,
    bytes: usize,
    cancellation: Cancellation,
}
enum Work {
    Command(Box<Command>),
    Read(Box<ReadRequest>),
    Access(Box<AccessRequest>),
    Service(Box<crate::service_control::ServiceJob>),
}
impl Work {
    fn is_read(&self) -> bool {
        matches!(self, Self::Read(_))
    }
    fn is_secret(&self) -> bool {
        // These known free-text fields may contain low-entropy credentials.
        // A persisted unkeyed request hash would be an offline verifier. This
        // finite policy is not general DLP for arbitrary project metadata.
        matches!(self,Self::Access(request)if matches!(request.as_ref(),AccessRequest::ConfigurationSecret{..}))
            || matches!(self,Self::Command(command)if matches!(command.as_ref(),Command::DebugHttp{..}|Command::DebugRequest{..}|Command::ConfigurationSave{..}))
    }
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
    active_read: AtomicBool,
    mutation_active: AtomicBool,
    mutation_queued: AtomicUsize,
    queued: AtomicUsize,
    read_budget: Arc<desktop::ImageBudget>,
    discovery: Mutex<Option<devforge_workspace::discovery::Service>>,
    environment_queued: AtomicUsize,
    environment_reads: AtomicUsize,
}
struct Job {
    receipt: Receipt,
    cancellation: Cancellation,
}
enum Message {
    Job(Box<Job>),
    Environment(Box<crate::environment_control::EnvironmentRead>),
    Database(Box<crate::database_control::DatabaseRead>),
    Domain(Box<crate::domain_control::DomainRead>),
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
        Self::start_with_terminal_arguments(permit, secrets, Vec::new())
    }
    pub fn start_with_terminal_arguments(
        permit: WorkspacePermit,
        secrets: Arc<dyn SecretStore + Send + Sync>,
        terminal_arguments: Vec<std::ffi::OsString>,
    ) -> Result<Self> {
        devforge_projects::terminal::validate_launch_arguments(&terminal_arguments)
            .map_err(|_| ActorError::InvalidInput)?;
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
            active_read: AtomicBool::new(false),
            mutation_active: AtomicBool::new(false),
            mutation_queued: AtomicUsize::new(0),
            queued: AtomicUsize::new(0),
            read_budget: Arc::new(desktop::ImageBudget::default()),
            discovery: Mutex::new(None),
            environment_queued: AtomicUsize::new(0),
            environment_reads: AtomicUsize::new(0),
        });
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker_shared = shared.clone();
        let worker = thread::Builder::new()
            .name("devforge-workspace-owner".into())
            .spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    worker_loop(
                        &permit,
                        secrets,
                        &worker_shared,
                        receiver,
                        ready_tx,
                        terminal_arguments,
                    )
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
                worker_shared.active_read.store(false, Ordering::Release);
                worker_shared
                    .mutation_active
                    .store(false, Ordering::Release);
                worker_shared.mutation_queued.store(0, Ordering::Release);
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
                    state.reads.clear();
                    state.volatile_fingerprints.clear();
                    state.read_tickets.clear();
                }
                worker_shared
                    .discovery
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take();
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
    pub(crate) fn external_fingerprint_matches(
        &self,
        operation: &Id,
        fingerprint: &str,
    ) -> Result<bool> {
        let state = self
            .shared
            .state
            .lock()
            .map_err(|_| ActorError::StorageUnavailable)?;
        let expected = state
            .volatile_fingerprints
            .iter()
            .find(|(id, _)| id == operation)
            .ok_or(ActorError::IdempotencyUnverifiable)?;
        Ok(bool::from(
            expected.1.as_bytes().ct_eq(fingerprint.as_bytes()),
        ))
    }
    pub(crate) fn queue_environment(
        &self,
        query: crate::environment_control::EnvironmentRead,
    ) -> Result<()> {
        self.queue_private(Message::Environment(Box::new(query)))
    }
    pub(crate) fn queue_database(
        &self,
        query: crate::database_control::DatabaseRead,
    ) -> Result<()> {
        self.queue_private(Message::Database(Box::new(query)))
    }
    pub(crate) fn queue_domain(&self, query: crate::domain_control::DomainRead) -> Result<()> {
        self.queue_private(Message::Domain(Box::new(query)))
    }
    fn queue_private(&self, message: Message) -> Result<()> {
        let state = self
            .shared
            .state
            .lock()
            .map_err(|_| ActorError::StorageUnavailable)?;
        if self.shared.stopping.load(Ordering::Acquire) {
            return Err(ActorError::ShuttingDown);
        }
        let queued = self.shared.environment_queued.load(Ordering::Acquire);
        let reads = state.waiting.values().filter(|v| v.work.is_read()).count()
            + usize::from(self.shared.active_read.load(Ordering::Acquire))
            + self.shared.environment_reads.load(Ordering::Acquire);
        if state.pending + queued >= QUEUE_CAPACITY
            || state
                .waiting_bytes
                .saturating_add((queued + 1) * ENV_QUERY_BYTES)
                > QUEUE_BYTES
            || reads >= desktop::READ_RESULTS
        {
            return Err(ActorError::Busy);
        }
        self.shared
            .environment_queued
            .fetch_add(1, Ordering::AcqRel);
        self.shared.environment_reads.fetch_add(1, Ordering::AcqRel);
        if self.sender.try_send(message).is_err() {
            self.shared
                .environment_queued
                .fetch_sub(1, Ordering::AcqRel);
            self.shared.environment_reads.fetch_sub(1, Ordering::AcqRel);
            return Err(ActorError::Busy);
        }
        Ok(())
    }
    pub(crate) fn external_find(
        &self,
        client: &Id,
        project: &Id,
        key: &Id,
    ) -> Result<Option<(Receipt, String, u64)>> {
        let state = self
            .shared
            .state
            .lock()
            .map_err(|_| ActorError::StorageUnavailable)?;
        let found = state.find(
            client,
            &OperationIdentity {
                project_id: project.clone(),
                idempotency_key: key.clone(),
            },
        )?;
        found
            .map(|(receipt, fingerprint)| {
                let revision: i64 = state
                    .db
                    .query_row(
                        "SELECT revision FROM service_operations WHERE operation_id=?1",
                        [receipt.operation_id.as_str()],
                        |row| row.get(0),
                    )
                    .map_err(|_| ActorError::StorageUnavailable)?;
                Ok((
                    receipt,
                    fingerprint,
                    u64::try_from(revision).map_err(|_| ActorError::StorageUnavailable)?,
                ))
            })
            .transpose()
    }
    pub(crate) fn submit_service(
        &self,
        job: crate::service_control::ServiceJob,
        registry: &mut crate::external_auth::ExternalRegistry,
    ) -> devforge_control::Result<Receipt> {
        use devforge_control::ErrorCode as E;
        let client = job.client().clone();
        let identity = OperationIdentity {
            project_id: job.request().project_id.clone(),
            idempotency_key: job
                .request()
                .idempotency_key
                .clone()
                .ok_or(E::InvalidInput)?,
        };
        let fingerprint = job.fingerprint().to_owned();
        let sensitive = job.sensitive();
        let bytes = job.encoded_bytes();
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| E::StorageUnavailable)?;
        if let Some((receipt, old)) = state
            .find(&client, &identity)
            .map_err(|_| E::StorageUnavailable)?
        {
            if sensitive && old.starts_with("volatile-secret-v1:") {
                let expected = state
                    .volatile_fingerprints
                    .iter()
                    .find(|(id, _)| id == &receipt.operation_id)
                    .ok_or(E::IdempotencyUnverifiable)?;
                if !bool::from(expected.1.as_bytes().ct_eq(fingerprint.as_bytes())) {
                    return Err(E::IdempotencyConflict);
                }
                return Ok(receipt);
            }
            if old != fingerprint || sensitive {
                return Err(E::IdempotencyConflict);
            }
            return Ok(receipt);
        }
        if self.shared.stopping.load(Ordering::Acquire) {
            return Err(E::Interrupted);
        }
        state.purge_cancelled_payloads();
        publish_queue(&self.shared, &state);
        if state.pending + self.shared.environment_queued.load(Ordering::Acquire) >= QUEUE_CAPACITY
            || bytes
                > QUEUE_BYTES.saturating_sub(
                    state.waiting_bytes
                        + self.shared.environment_queued.load(Ordering::Acquire) * ENV_QUERY_BYTES,
                )
        {
            return Err(E::Busy);
        }
        let snapshot = self
            .external_snapshot()
            .map_err(|_| E::StorageUnavailable)?;
        let prepared = job.validate_admission(&snapshot.snapshot, registry)?;
        let receipt = Receipt {
            operation_id: random_id().map_err(|_| E::StorageUnavailable)?,
            client_id: client.clone(),
            project_id: identity.project_id.clone(),
            state: OperationState::Queued,
            code: None,
        };
        let output = devforge_control::mutations::MutationReceipt {
            project_id: receipt.project_id.clone(),
            operation_id: receipt.operation_id.clone(),
            state: receipt.state,
            revision: devforge_control::limits::SafeInt::new(
                job.request().expected_revision.ok_or(E::InvalidInput)?,
            )?,
        };
        use devforge_control::mutations::{MutationInput, MutationOutput};
        prepared.check_output(&match job.request().input {
            MutationInput::Start(_) => MutationOutput::Start(output),
            MutationInput::Stop(_) => MutationOutput::Stop(output),
            MutationInput::Restart(_) => MutationOutput::Restart(output),
            MutationInput::RuntimeInstall(_) => MutationOutput::RuntimeInstall(output),
            MutationInput::Configure(_) => MutationOutput::Configure(output),
            MutationInput::EnvironmentApply(_) => MutationOutput::EnvironmentApply(output),
            MutationInput::YamlSave(_) => MutationOutput::YamlSave(output),
            MutationInput::DatabaseBind(_) => MutationOutput::DatabaseBind(output),
            MutationInput::DatabaseRestore(_) => MutationOutput::DatabaseRestore(output),
            MutationInput::DomainApply(_) => MutationOutput::DomainApply(output),
            MutationInput::DomainCleanup(_) => MutationOutput::DomainCleanup(output),
            _ => return Err(E::UnsupportedCapability),
        })?;
        let transaction = state
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|_| E::StorageUnavailable)?;
        job.claim(&receipt.operation_id)?;
        let persisted_fingerprint = if sensitive {
            format!(
                "volatile-secret-v1:{}",
                random_id().map_err(|_| E::StorageUnavailable)?.as_str()
            )
        } else {
            fingerprint.clone()
        };
        let insertion = (|| {
            job.validate_admission(&snapshot.snapshot, registry)?;
            transaction.execute("INSERT INTO operations(operation_id,client_id,project_id,idempotency_key,fingerprint,state,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,'queued',?6,?6)",params![receipt.operation_id.as_str(),client.as_str(),identity.project_id.as_str(),identity.idempotency_key.as_str(),persisted_fingerprint,now() as i64]).map_err(|_|E::StorageUnavailable)?;
            transaction
                .execute(
                    "INSERT INTO service_operations(operation_id,revision) VALUES(?1,?2)",
                    params![
                        receipt.operation_id.as_str(),
                        job.request().expected_revision.ok_or(E::InvalidInput)? as i64
                    ],
                )
                .map_err(|_| E::StorageUnavailable)?;
            job.validate_admission(&snapshot.snapshot, registry)?;
            transaction.commit().map_err(|_| E::StorageUnavailable)
        })();
        if let Err(error) = insertion {
            job.release_claim(&receipt.operation_id);
            return Err(error);
        }
        if sensitive {
            if state.volatile_fingerprints.len() >= 128 {
                state.volatile_fingerprints.pop_front();
            }
            state
                .volatile_fingerprints
                .push_back((receipt.operation_id.clone(), Zeroizing::new(fingerprint)));
        }
        let cancellation = job.cancellation.clone();
        let mut cancellations = match self.shared.cancellations.lock() {
            Ok(lock) => lock,
            Err(_) => {
                state
                    .update(
                        &receipt.operation_id,
                        OperationState::Interrupted,
                        Some("admission_interrupted"),
                    )
                    .map_err(|_| E::AdmissionUnconfirmed)?;
                return Err(E::AdmissionUnconfirmed);
            }
        };
        cancellations.insert(
            receipt.operation_id.clone(),
            CancellationEntry {
                client,
                project: identity.project_id,
                cancellation: cancellation.clone(),
            },
        );
        drop(cancellations);
        state.pending += 1;
        state.waiting_bytes += bytes;
        state.waiting.insert(
            receipt.operation_id.clone(),
            WaitingCommand {
                work: Work::Service(Box::new(job)),
                bytes,
                cancellation: cancellation.clone(),
            },
        );
        publish_queue(&self.shared, &state);
        if self
            .sender
            .try_send(Message::Job(Box::new(Job {
                receipt: receipt.clone(),
                cancellation,
            })))
            .is_err()
        {
            state.pending -= 1;
            state.take_payload(&receipt.operation_id);
            publish_queue(&self.shared, &state);
            self.shared
                .cancellations
                .lock()
                .map_err(|_| E::StorageUnavailable)?
                .remove(&receipt.operation_id);
            state
                .update(
                    &receipt.operation_id,
                    OperationState::Interrupted,
                    Some("admission_interrupted"),
                )
                .map_err(|_| E::StorageUnavailable)?;
            return Err(E::AdmissionUnconfirmed);
        }
        Ok(receipt)
    }
    pub fn resolve_subject(
        &self,
        authority: &DesktopAuthority<'_>,
        selector: &crate::desktop::SubjectSelector,
    ) -> Result<Id> {
        self.client(authority)?;
        let cache = self
            .shared
            .cache
            .lock()
            .map_err(|_| ActorError::StorageUnavailable)?;
        let subject = crate::subjects::resolve_selector(selector, &cache.snapshot)
            .map_err(|_| ActorError::SubjectMismatch)?;
        self.client(authority)?;
        Ok(subject)
    }
    pub fn operation_by_key(
        &self,
        authority: &DesktopAuthority<'_>,
        project: Id,
        key: Id,
    ) -> Result<Option<Receipt>> {
        let client = self.client(authority)?;
        let state = self
            .shared
            .state
            .lock()
            .map_err(|_| ActorError::StorageUnavailable)?;
        self.client(authority)?;
        let found = state
            .find(
                &client,
                &OperationIdentity {
                    project_id: project,
                    idempotency_key: key,
                },
            )?
            .map(|(receipt, _)| receipt);
        self.client(authority)?;
        Ok(found)
    }
    pub fn operations(
        &self,
        authority: &DesktopAuthority<'_>,
        project: Option<Id>,
        cursor: Option<OperationCursor>,
        limit: u32,
    ) -> Result<OperationPage> {
        if !(1..=100).contains(&limit) {
            return Err(ActorError::InvalidInput);
        }
        let client = self.client(authority)?;
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| ActorError::StorageUnavailable)?;
        self.client(authority)?;
        state.expire_reads();
        let (ceiling, before) = if let Some(cursor) = cursor {
            if cursor.client_id != client
                || cursor.project_id != project
                || cursor.ceiling > 9_007_199_254_740_990
                || cursor.before == 0
                || cursor.before > cursor.ceiling + 1
            {
                return Err(ActorError::InvalidInput);
            }
            (cursor.ceiling, cursor.before)
        } else {
            let ceiling=state.db.query_row::<i64,_,_>("SELECT COALESCE(MAX(rowid),0) FROM operations WHERE client_id=?1 AND (?2 IS NULL OR project_id=?2)",params![client.as_str(),project.as_ref().map(Id::as_str)],|row|row.get(0)).map_err(|_|ActorError::StorageUnavailable)?;
            if !(0..=9_007_199_254_740_990).contains(&ceiling) {
                return Err(ActorError::OutputLimitExceeded);
            }
            (ceiling as u64, ceiling as u64 + 1)
        };
        let mut statement=state.db.prepare("SELECT operation_id,client_id,project_id,state,code,rowid FROM operations WHERE client_id=?1 AND (?2 IS NULL OR project_id=?2) AND rowid<=?3 AND rowid<?4 ORDER BY rowid DESC LIMIT ?5").map_err(|_|ActorError::StorageUnavailable)?;
        let rows = statement
            .query_map(
                params![
                    client.as_str(),
                    project.as_ref().map(Id::as_str),
                    ceiling as i64,
                    before as i64,
                    limit + 1
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                },
            )
            .map_err(|_| ActorError::StorageUnavailable)?;
        let mut items = Vec::new();
        let mut last = before;
        let mut more = false;
        for row in rows {
            let row = row.map_err(|_| ActorError::StorageUnavailable)?;
            if items.len() == limit as usize {
                more = true;
                break;
            }
            last = u64::try_from(row.5).map_err(|_| ActorError::StorageUnavailable)?;
            items.push(receipt(row.0, row.1, row.2, row.3, row.4)?);
        }
        let active_reads = state
            .read_tickets
            .iter()
            .filter(|entry| {
                entry.receipt.client_id == client
                    && project
                        .as_ref()
                        .is_none_or(|id| id == &entry.receipt.project_id)
                    && !entry.receipt.state.terminal()
                    && !entry.cancellation.is_cancelled()
            })
            .map(|entry| entry.receipt.clone())
            .collect::<Vec<_>>();
        if active_reads.len() > desktop::READ_RESULTS {
            return Err(ActorError::StorageUnavailable);
        }
        self.client(authority)?;
        Ok(OperationPage {
            items,
            next_cursor: more.then_some(OperationCursor {
                client_id: client,
                project_id: project,
                ceiling,
                before: last,
            }),
            active_reads,
        })
    }
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
        self.external_snapshot()
    }
    /// Crate-private cache access for the separately authorized external facade.
    /// This never makes a DesktopAuthority or admits external commands.
    pub(crate) fn external_snapshot(&self) -> Result<SnapshotView> {
        let cache = self
            .shared
            .cache
            .lock()
            .map_err(|_| ActorError::StorageUnavailable)?;
        Ok(SnapshotView {
            snapshot: cache.snapshot.clone(),
            observed_at: cache.observed_at,
            busy: self.shared.mutation_active.load(Ordering::Acquire)
                || self.shared.mutation_queued.load(Ordering::Acquire) != 0,
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
        self.submit_work(authority, identity, Work::Command(Box::new(command)))
    }
    pub fn desktop_read(
        &self,
        authority: &DesktopAuthority<'_>,
        idempotency_key: Id,
        request: ReadRequest,
    ) -> Result<Receipt> {
        if !request.valid() {
            return Err(ActorError::InvalidInput);
        }
        let identity = OperationIdentity {
            project_id: request.project_id().clone(),
            idempotency_key,
        };
        self.submit_work(authority, identity, Work::Read(Box::new(request)))
    }
    pub fn desktop_access(
        &self,
        authority: &DesktopAuthority<'_>,
        idempotency_key: Id,
        request: AccessRequest,
    ) -> Result<Receipt> {
        if !request.valid() {
            return Err(ActorError::InvalidInput);
        }
        let identity = OperationIdentity {
            project_id: request.subject(),
            idempotency_key,
        };
        self.submit_work(authority, identity, Work::Access(Box::new(request)))
    }
    pub fn discovery_request(&self, authority: &DesktopAuthority<'_>) -> Result<()> {
        self.client(authority)?;
        if self.shared.stopping.load(Ordering::Acquire) {
            return Err(ActorError::ShuttingDown);
        }
        let lock = self
            .shared
            .discovery
            .lock()
            .map_err(|_| ActorError::WorkerUnavailable)?;
        lock.as_ref()
            .ok_or(ActorError::WorkerUnavailable)?
            .request();
        Ok(())
    }
    pub fn read_image(
        &self,
        authority: &DesktopAuthority<'_>,
        project: &Id,
        operation: &Id,
    ) -> Result<Arc<ReadImage>> {
        let client = self.client(authority)?;
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| ActorError::StorageUnavailable)?;
        self.client(authority)?;
        let receipt = state.get(&client, project, operation)?;
        if !receipt.state.terminal() {
            return Err(ActorError::NotReady);
        }
        if receipt.state != OperationState::Succeeded {
            return Err(ActorError::OperationFailed);
        }
        if receipt.code.as_deref() == Some("result_output_limit") {
            return Err(ActorError::OutputLimitExceeded);
        }
        state.expire_reads();
        state
            .reads
            .iter()
            .find(|entry| &entry.operation_id == operation)
            .map(|entry| entry.image.clone())
            .ok_or(if operation.as_str().starts_with("read-") {
                ActorError::ReadExpired
            } else {
                ActorError::ResultUnavailable
            })
    }
    fn submit_work(
        &self,
        authority: &DesktopAuthority<'_>,
        identity: OperationIdentity,
        work: Work,
    ) -> Result<Receipt> {
        let client = self.client(authority)?;
        if let Work::Command(command) = &work {
            let cache = self
                .shared
                .cache
                .lock()
                .map_err(|_| ActorError::StorageUnavailable)?;
            validate_subject(&identity.project_id, command, &cache.snapshot)?;
        }
        let secret = work.is_secret();
        let read = work.is_read();
        let (fingerprint, encoded_bytes) = work_fingerprint(&identity.project_id, &work)?;
        let mut state = self
            .shared
            .state
            .lock()
            .map_err(|_| ActorError::StorageUnavailable)?;
        self.client(authority)?;
        state.expire_reads();
        let existing = if read {
            state.find_read(&client, &identity)
        } else {
            state.find(&client, &identity)?
        };
        if let Some((receipt, previous)) = existing {
            self.client(authority)?;
            if secret && previous.starts_with("volatile-secret-v1:") {
                let Some((_, expected)) = state
                    .volatile_fingerprints
                    .iter()
                    .find(|(operation, _)| operation == &receipt.operation_id)
                else {
                    return Err(ActorError::IdempotencyUnverifiable);
                };
                if !bool::from(expected.as_bytes().ct_eq(fingerprint.as_bytes())) {
                    return Err(ActorError::IdempotencyConflict);
                }
                return Ok(receipt);
            }
            if previous != fingerprint || secret {
                return Err(ActorError::IdempotencyConflict);
            }
            return Ok(receipt);
        }
        if self.shared.stopping.load(Ordering::Acquire) {
            return Err(ActorError::ShuttingDown);
        }
        state.purge_cancelled_payloads();
        publish_queue(&self.shared, &state);
        state.expire_reads();
        if state.pending + self.shared.environment_queued.load(Ordering::Acquire) >= QUEUE_CAPACITY
            || encoded_bytes
                > QUEUE_BYTES.saturating_sub(
                    state.waiting_bytes
                        + self.shared.environment_queued.load(Ordering::Acquire) * ENV_QUERY_BYTES,
                )
        {
            return Err(ActorError::Busy);
        }
        if read
            && (state
                .waiting
                .values()
                .filter(|item| item.work.is_read())
                .count()
                + usize::from(self.shared.active_read.load(Ordering::Acquire))
                + self.shared.environment_reads.load(Ordering::Acquire))
                >= desktop::READ_RESULTS
        {
            return Err(ActorError::Busy);
        }
        let receipt = Receipt {
            operation_id: if read {
                Id::new(format!("read-{}", random_id()?.as_str()))
                    .map_err(|_| ActorError::InvalidInput)?
            } else {
                random_id()?
            },
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
        let persisted_fingerprint = if secret {
            format!("volatile-secret-v1:{}", random_id()?.as_str())
        } else {
            fingerprint.clone()
        };
        let insertion = if read {
            state.insert_read(
                &receipt,
                &identity.idempotency_key,
                &fingerprint,
                cancellation.clone(),
                authority,
            )
        } else {
            state.insert(
                &receipt,
                &identity.idempotency_key,
                &persisted_fingerprint,
                authority,
            )
        };
        if let Err(error) = insertion {
            if let Ok(mut entries) = self.shared.cancellations.lock() {
                entries.remove(&receipt.operation_id);
            }
            return Err(error);
        }
        state.pending += 1;
        if secret {
            if state.volatile_fingerprints.len() >= 128 {
                state.volatile_fingerprints.pop_front();
            }
            state
                .volatile_fingerprints
                .push_back((receipt.operation_id.clone(), Zeroizing::new(fingerprint)));
        }
        state.waiting_bytes += encoded_bytes;
        state.waiting.insert(
            receipt.operation_id.clone(),
            WaitingCommand {
                work,
                bytes: encoded_bytes,
                cancellation: cancellation.clone(),
            },
        );
        self.shared.queued.store(state.pending, Ordering::Release);
        publish_queue(&self.shared, &state);
        let job = Message::Job(Box::new(Job {
            receipt: receipt.clone(),
            cancellation,
        }));
        if let Err(error) = self.sender.try_send(job) {
            state.pending -= 1;
            state.take_payload(&receipt.operation_id);
            publish_queue(&self.shared, &state);
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
                    publish_queue(&self.shared, &state);
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
    pub(crate) fn external_status(
        &self,
        client: &Id,
        project: &Id,
        operation: &Id,
    ) -> Result<Receipt> {
        self.shared
            .state
            .lock()
            .map_err(|_| ActorError::StorageUnavailable)?
            .get(client, project, operation)
    }
    /// Called with the external grant registry serialized. Revalidate the exact
    /// original client grant after the cancellation lock wait, before the bit.
    pub(crate) fn external_cancel<F: FnOnce() -> Result<()>>(
        &self,
        client: &Id,
        project: &Id,
        operation: &Id,
        authorize: F,
    ) -> Result<CancelOutcome> {
        let entries = self
            .shared
            .cancellations
            .lock()
            .map_err(|_| ActorError::StorageUnavailable)?;
        if let Some(entry) = entries.get(operation) {
            if &entry.client != client || &entry.project != project {
                return Err(ActorError::OperationNotFound);
            }
            authorize()?;
            entry.cancellation.cancel();
            drop(entries);
            if let Ok(mut state) = self.shared.state.try_lock() {
                state.take_payload(operation);
                publish_queue(&self.shared, &state);
                if let Ok(receipt) = state.get(client, project, operation)
                    && receipt.state == OperationState::Running
                {
                    let _ = state.update(operation, OperationState::CancelRequested, None);
                }
            }
            return Ok(CancelOutcome::Requested);
        }
        drop(entries);
        let receipt = self.external_status(client, project, operation)?;
        authorize()?;
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
    terminal_arguments: Vec<std::ffi::OsString>,
) -> Result<()> {
    let mut workspace = match Workspace::open(permit.root(), secrets) {
        Ok(value) => value,
        Err(_) => {
            let _ = ready.send(Err(ActorError::WorkspaceUnavailable));
            return Err(ActorError::WorkspaceUnavailable);
        }
    };
    workspace.progress = shared.progress.clone();
    workspace
        .set_terminal_arguments(terminal_arguments)
        .map_err(|_| ActorError::InvalidInput)?;
    {
        let service = devforge_workspace::discovery::Service::new();
        if let Ok(settings) = devforge_platform::SqliteSettings::open(permit.root())
            .and_then(|settings| settings.load())
            && let Some(folder) = settings.project_folder
        {
            let path = PathBuf::from(folder);
            if path.is_absolute() {
                service.set_root(path);
            }
        }
        *shared
            .discovery
            .lock()
            .map_err(|_| ActorError::WorkerUnavailable)? = Some(service);
    }
    sync_discovery(shared, &mut workspace)?;
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
                    Message::Environment(query) => {
                        shared.environment_queued.fetch_sub(1, Ordering::AcqRel);
                        shared.environment_reads.fetch_sub(1, Ordering::AcqRel);
                        let _ = query.reply.send(Err(devforge_workspace::Error {
                            scope: "workspace",
                            code: "interrupted".into(),
                            project_id: None,
                        }));
                    }
                    Message::Database(query) => {
                        shared.environment_queued.fetch_sub(1, Ordering::AcqRel);
                        shared.environment_reads.fetch_sub(1, Ordering::AcqRel);
                        let _ = query.reply.send(Err(devforge_workspace::Error {
                            scope: "workspace",
                            code: "interrupted".into(),
                            project_id: None,
                        }));
                    }
                    Message::Domain(query) => {
                        shared.environment_queued.fetch_sub(1, Ordering::AcqRel);
                        shared.environment_reads.fetch_sub(1, Ordering::AcqRel);
                        let _ = query.reply.send(Err(devforge_workspace::Error {
                            scope: "workspace",
                            code: "interrupted".into(),
                            project_id: None,
                        }));
                    }
                }
            }
            shared
                .discovery
                .lock()
                .map_err(|_| ActorError::WorkerUnavailable)?
                .take();
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
                    let _ = sync_discovery(shared, &mut workspace);
                    if let Ok(mut state) = shared.state.lock() {
                        state.expire_reads();
                    }
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
            Message::Environment(query) => {
                shared.environment_queued.fetch_sub(1, Ordering::AcqRel);
                shared.active.store(true, Ordering::Release);
                query.execute(&workspace);
                shared.active.store(false, Ordering::Release);
                shared.environment_reads.fetch_sub(1, Ordering::AcqRel);
            }
            Message::Database(query) => {
                shared.environment_queued.fetch_sub(1, Ordering::AcqRel);
                shared.active.store(true, Ordering::Release);
                query.execute(&workspace);
                shared.active.store(false, Ordering::Release);
                shared.environment_reads.fetch_sub(1, Ordering::AcqRel);
            }
            Message::Domain(query) => {
                shared.environment_queued.fetch_sub(1, Ordering::AcqRel);
                shared.active.store(true, Ordering::Release);
                query.execute(&workspace);
                shared.active.store(false, Ordering::Release);
                shared.environment_reads.fetch_sub(1, Ordering::AcqRel);
            }
            Message::Job(job) => {
                if job.cancellation.is_cancelled() || shared.stopping.load(Ordering::Acquire) {
                    finish_cancelled(shared, &job)?;
                    continue;
                }
                let work = {
                    let mut state = shared
                        .state
                        .lock()
                        .map_err(|_| ActorError::StorageUnavailable)?;
                    state.pending -= 1;
                    shared.queued.store(state.pending, Ordering::Release);
                    let payload = state.take_payload(&job.receipt.operation_id);
                    if job.cancellation.is_cancelled() || payload.is_none() {
                        publish_queue(shared, &state);
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
                    shared.mutation_active.store(
                        payload.as_ref().is_some_and(|value| !value.work.is_read()),
                        Ordering::Release,
                    );
                    publish_queue(shared, &state);
                    shared.active_read.store(
                        payload.as_ref().is_some_and(|value| value.work.is_read()),
                        Ordering::Release,
                    );
                    state.update(&job.receipt.operation_id, OperationState::Running, None)?;
                    payload.ok_or(ActorError::WorkerUnavailable)?.work
                };
                let read = work.is_read();
                let attempt = if let Work::Command(command) = &work
                    && let Command::CreateProject { input } = command.as_ref()
                {
                    workspace.snapshot().ok().and_then(|before| {
                        crate::subjects::CreateAttempt::capture(
                            &shared.root,
                            &job.receipt.operation_id,
                            input,
                            &before,
                        )
                    })
                } else {
                    None
                };
                let worktree_attempt = if let Work::Command(command) = &work
                    && let Command::WorktreeCreate { input, review, .. } = command.as_ref()
                {
                    workspace.snapshot().ok().and_then(|before| {
                        crate::subjects::WorktreeAttempt::capture(
                            &shared.root,
                            &job.receipt.operation_id,
                            input,
                            review,
                            &before,
                        )
                    })
                } else {
                    None
                };
                let mut outcome = match work {
                    Work::Command(command) => {
                        if resource_command(&command) {
                            match workspace
                                .snapshot()
                                .map_err(|_| ActorError::WorkspaceUnavailable)
                                .and_then(|snapshot| {
                                    validate_subject(&job.receipt.project_id, &command, &snapshot)
                                }) {
                                Ok(()) => workspace.execute(*command, &job.cancellation),
                                Err(_) => Err(devforge_workspace::Error {
                                    scope: "workspace",
                                    code: "subject_mismatch".into(),
                                    project_id: Some(job.receipt.project_id.as_str().into()),
                                }),
                            }
                        } else {
                            workspace.execute(*command, &job.cancellation)
                        }
                    }
                    Work::Read(request) => request.execute(&workspace),
                    Work::Access(request) => {
                        if matches!(request.as_ref(), AccessRequest::DiscoveryRoot { .. }) {
                            let lock = shared
                                .discovery
                                .lock()
                                .map_err(|_| ActorError::WorkerUnavailable)?;
                            request.execute(&mut workspace, &shared.root, lock.as_ref())
                        } else {
                            request.execute(&mut workspace, &shared.root, None)
                        }
                    }
                    Work::Service(job) => job.execute(&mut workspace),
                };
                if read {
                    let expired = {
                        let state = shared
                            .state
                            .lock()
                            .map_err(|_| ActorError::StorageUnavailable)?;
                        state
                            .read_tickets
                            .iter()
                            .find(|entry| entry.receipt.operation_id == job.receipt.operation_id)
                            .is_none_or(|entry| Instant::now() >= entry.expires)
                    };
                    if expired || job.cancellation.is_cancelled() {
                        outcome = Err(devforge_workspace::Error {
                            scope: "workspace",
                            code: "cancelled".into(),
                            project_id: Some(job.receipt.project_id.as_str().into()),
                        });
                    }
                }
                let recovery = if let Err(error) = &outcome
                    && let Some(error_project) = error.project_id.as_deref()
                {
                    workspace.snapshot().ok().and_then(|after| {
                        attempt
                            .as_ref()
                            .and_then(|attempt| {
                                attempt.recover_project(
                                    &shared.root,
                                    &job.receipt.operation_id,
                                    &after,
                                    error_project,
                                    |path| devforge_projects::discovery::identity(path).ok(),
                                )
                            })
                            .or_else(|| {
                                worktree_attempt.as_ref().and_then(|attempt| {
                                    attempt.recover_project(
                                        &shared.root,
                                        &job.receipt.operation_id,
                                        &after,
                                        error_project,
                                        &error.code,
                                        |path| devforge_projects::discovery::identity(path).ok(),
                                    )
                                })
                            })
                    })
                } else {
                    None
                };
                let needs_large = read
                    || outcome
                        .as_ref()
                        .is_ok_and(|value| !small_result_fits(value));
                let large_image = if needs_large && let Ok(value) = &outcome {
                    {
                        let mut state = shared
                            .state
                            .lock()
                            .map_err(|_| ActorError::StorageUnavailable)?;
                        state.expire_reads();
                        if state.reads.len() >= desktop::READ_RESULTS {
                            state.reads.pop_front();
                        }
                    }
                    desktop::encode_read(value, shared.read_budget.clone(), now()).ok()
                } else {
                    None
                };
                let _ = sync_discovery(shared, &mut workspace);
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
                        let retained = if needs_large {
                            if let Some(image) = large_image {
                                state.reads.push_back(SavedRead {
                                    operation_id: job.receipt.operation_id.clone(),
                                    image,
                                });
                                true
                            } else {
                                false
                            }
                        } else {
                            state.remember(job.receipt.operation_id.clone(), value)
                        };
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
                        if let Some(detail) = validated_failure_with_recovery(
                            &error,
                            &job.receipt.project_id,
                            recovery,
                        ) {
                            state.remember_outcome(
                                job.receipt.operation_id.clone(),
                                RetainedOutcome::Failure(detail),
                            );
                        }
                        if cancelled {
                            state.record_cancellation(
                                &job.receipt,
                                read,
                                stable_code(error.scope, &error.code),
                            )?;
                        } else {
                            state.update(
                                &job.receipt.operation_id,
                                OperationState::Failed,
                                Some(stable_code(error.scope, &error.code)),
                            )?;
                        }
                    }
                }
                drop(state);
                shared
                    .cancellations
                    .lock()
                    .map_err(|_| ActorError::StorageUnavailable)?
                    .remove(&job.receipt.operation_id);
                shared.active.store(false, Ordering::Release);
                shared.mutation_active.store(false, Ordering::Release);
                shared.active_read.store(false, Ordering::Release);
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
    publish_queue(shared, &state);
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
fn publish_queue(shared: &Shared, state: &State) {
    shared.queued.store(state.pending, Ordering::Release);
    shared.mutation_queued.store(
        state
            .waiting
            .values()
            .filter(|value| !value.work.is_read())
            .count(),
        Ordering::Release,
    );
}
fn sync_discovery(shared: &Shared, workspace: &mut Workspace) -> Result<()> {
    let (scan, status) = {
        let lock = shared
            .discovery
            .lock()
            .map_err(|_| ActorError::WorkerUnavailable)?;
        let service = lock.as_ref().ok_or(ActorError::WorkerUnavailable)?;
        (service.take(), service.status())
    };
    if let Some(scan) = scan {
        workspace
            .reconcile_discovery(scan)
            .map_err(|_| ActorError::WorkspaceUnavailable)?;
    }
    workspace.discovery_status(status.root, status.scanning, status.degraded);
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
        if schema > 2 {
            return Err(ActorError::StorageUnavailable);
        }
        db.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;")
            .map_err(|_| ActorError::StorageUnavailable)?;
        if schema == 0 {
            db.execute_batch("BEGIN IMMEDIATE; CREATE TABLE operations(operation_id TEXT PRIMARY KEY,client_id TEXT NOT NULL,project_id TEXT NOT NULL,idempotency_key TEXT NOT NULL,fingerprint TEXT NOT NULL,state TEXT NOT NULL,code TEXT,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL,UNIQUE(client_id,project_id,idempotency_key)); PRAGMA user_version=1; COMMIT;").map_err(|_| ActorError::StorageUnavailable)?;
        }
        if schema < 2 {
            db.execute_batch("BEGIN IMMEDIATE; CREATE TABLE service_operations(operation_id TEXT PRIMARY KEY REFERENCES operations(operation_id),revision INTEGER NOT NULL CHECK(revision>=0)); PRAGMA user_version=2; COMMIT;").map_err(|_|ActorError::StorageUnavailable)?;
        }
        let state = Self {
            db,
            _file: file,
            pending: 0,
            waiting: HashMap::new(),
            waiting_bytes: 0,
            results: VecDeque::new(),
            result_bytes: 0,
            reads: VecDeque::new(),
            volatile_fingerprints: VecDeque::new(),
            read_tickets: VecDeque::new(),
        };
        state.interrupt_pending()?;
        Ok(state)
    }
    fn interrupt_pending(&self) -> Result<()> {
        self.db.execute("UPDATE operations SET state=CASE WHEN state='queued' THEN 'interrupted' ELSE 'unknown' END,code='owner_interrupted',updated_at=?1 WHERE state IN ('queued','running','cancel_requested')", [now() as i64]).map_err(|_| ActorError::StorageUnavailable)?;
        Ok(())
    }
    fn expire_reads(&mut self) {
        let now = Instant::now();
        self.reads.retain(|entry| now < entry.image.expires);
        self.read_tickets.retain(|entry| {
            if now >= entry.expires {
                entry.cancellation.cancel();
                !entry.receipt.state.terminal()
            } else {
                true
            }
        });
    }
    fn find_read(&self, client: &Id, identity: &OperationIdentity) -> Option<(Receipt, String)> {
        self.read_tickets
            .iter()
            .find(|entry| {
                &entry.receipt.client_id == client
                    && entry.receipt.project_id == identity.project_id
                    && entry.key == identity.idempotency_key
                    && Instant::now() < entry.expires
            })
            .map(|entry| (entry.receipt.clone(), entry.fingerprint.clone()))
    }
    fn insert_read(
        &mut self,
        receipt: &Receipt,
        key: &Id,
        fingerprint: &str,
        cancellation: Cancellation,
        authority: &DesktopAuthority<'_>,
    ) -> Result<()> {
        authority
            .check_current(Instant::now())
            .map_err(|_| ActorError::Unauthorized)?;
        if self.read_tickets.len() >= 128 {
            let Some(index) = self
                .read_tickets
                .iter()
                .position(|entry| entry.receipt.state.terminal())
            else {
                return Err(ActorError::Busy);
            };
            self.read_tickets.remove(index);
        }
        self.read_tickets.push_back(ReadTicket {
            receipt: receipt.clone(),
            key: key.clone(),
            fingerprint: fingerprint.into(),
            expires: Instant::now() + desktop::READ_TTL,
            cancellation,
        });
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
        if operation.as_str().starts_with("read-") {
            let entry = self
                .read_tickets
                .iter()
                .find(|entry| &entry.receipt.operation_id == operation)
                .ok_or(ActorError::ReadExpired)?;
            if &entry.receipt.client_id != client || &entry.receipt.project_id != project {
                return Err(ActorError::OperationNotFound);
            }
            if Instant::now() >= entry.expires {
                return Err(ActorError::ReadExpired);
            }
            return Ok(entry.receipt.clone());
        }
        let row = self.db.query_row("SELECT operation_id,client_id,project_id,state,code FROM operations WHERE operation_id=?1 AND client_id=?2 AND project_id=?3", params![operation.as_str(),client.as_str(),project.as_str()], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,String>(3)?,row.get::<_,Option<String>>(4)?))).optional().map_err(|_| ActorError::StorageUnavailable)?.ok_or(ActorError::OperationNotFound)?;
        receipt(row.0, row.1, row.2, row.3, row.4)
    }
    fn record_cancellation(&mut self, receipt: &Receipt, read: bool, code: &str) -> Result<()> {
        let running = if read {
            self.read_tickets.iter().any(|entry| {
                entry.receipt.operation_id == receipt.operation_id
                    && entry.receipt.state == OperationState::Running
            })
        } else {
            self.get(
                &receipt.client_id,
                &receipt.project_id,
                &receipt.operation_id,
            )?
            .state
                == OperationState::Running
        };
        if running {
            self.update(&receipt.operation_id, OperationState::CancelRequested, None)?;
        }
        self.update(&receipt.operation_id, OperationState::Cancelled, Some(code))
    }
    fn update(&mut self, operation: &Id, state: OperationState, code: Option<&str>) -> Result<()> {
        if operation.as_str().starts_with("read-") {
            let entry = self
                .read_tickets
                .iter_mut()
                .find(|entry| &entry.receipt.operation_id == operation)
                .ok_or(ActorError::ReadExpired)?;
            entry
                .receipt
                .state
                .transition(state)
                .map_err(|_| ActorError::InvalidInput)?;
            entry.receipt.state = state;
            entry.receipt.code = code.map(str::to_owned);
            return Ok(());
        }
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
                | "debugger_failed"
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
fn validate_subject(subject: &Id, command: &Command, snapshot: &Snapshot) -> Result<()> {
    let selector = crate::desktop::SubjectSelector::from_command(command)?;
    let expected = match &selector {
        crate::desktop::SubjectSelector::Root {} => {
            Id::new(ROOT_SUBJECT).map_err(|_| ActorError::InvalidInput)?
        }
        crate::desktop::SubjectSelector::Project { project_id } => project_id.clone(),
        crate::desktop::SubjectSelector::Worktree { .. }
        | crate::desktop::SubjectSelector::Cache { .. } => {
            crate::subjects::resolve_selector(&selector, snapshot)
                .map_err(|_| ActorError::SubjectMismatch)?
        }
    };
    if &expected != subject {
        return Err(ActorError::SubjectMismatch);
    }
    Ok(())
}
fn resource_command(command: &Command) -> bool {
    matches!(
        command,
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
            | Command::StopCache { .. }
    )
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
        "debugger" => "debugger_failed",
        _ => "workspace_failed",
    }
}
#[cfg(test)]
fn validated_failure(error: &devforge_workspace::Error, subject: &Id) -> Option<FailureDetail> {
    validated_failure_with_recovery(error, subject, None)
}
fn validated_failure_with_recovery(
    error: &devforge_workspace::Error,
    subject: &Id,
    recovery: Option<Id>,
) -> Option<FailureDetail> {
    let scope = match error.scope {
        "project" => DomainScope::Project,
        "runtime" => DomainScope::Runtime,
        "serving" => DomainScope::Serving,
        "workspace" => DomainScope::Workspace,
        "debugger" => DomainScope::Debugger,
        _ => return None,
    };
    if error.project_id.as_deref().is_some_and(|project| {
        project != subject.as_str() && recovery.as_ref().is_none_or(|id| id.as_str() != project)
    }) || error.code.is_empty()
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
        recovery_project_id: recovery,
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
fn small_result_fits(value: &Value) -> bool {
    #[derive(Serialize)]
    #[serde(tag = "kind", content = "data", rename_all = "snake_case")]
    enum Outcome<'a> {
        Success(&'a Value),
    }
    measure(&Outcome::Success(value), RESULT_BYTES).is_ok()
}
fn work_fingerprint(project: &Id, work: &Work) -> Result<(String, usize)> {
    match work {
        Work::Command(command) => fingerprint(project, command),
        Work::Read(request) => named_fingerprint(project, "desktop_read", request),
        Work::Access(request) => named_fingerprint(project, "desktop_access", request),
        Work::Service(job) => Ok((job.fingerprint().to_owned(), job.encoded_bytes())),
    }
}
fn named_fingerprint<T: Serialize>(
    project: &Id,
    kind: &str,
    request: &T,
) -> Result<(String, usize)> {
    let bytes = measure(request, COMMAND_BYTES).map_err(|_| ActorError::InvalidInput)?;
    let mut writer = LimitedWriter {
        bytes: 0,
        limit: COMMAND_BYTES + 1024,
        hash: Some(Sha256::new()),
    };
    serde_json::to_writer(&mut writer, &(project, kind, request))
        .map_err(|_| ActorError::InvalidInput)?;
    Ok((
        hex(&writer.hash.ok_or(ActorError::InvalidInput)?.finalize()),
        bytes,
    ))
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
use crate::external_auth::test_support;
#[cfg(test)]
mod retention_tests {
    use super::*;
    #[test]
    fn durable_running_cancellation_without_prior_bit_persistence_finishes_and_keeps_store_usable()
    {
        let root = test_support::root("df-active-cancel-");
        let owner = test_support::acquire(root.path()).unwrap();
        let mut state = State::open(owner.endpoint().directory.join("operations.sqlite3")).unwrap();
        for (operation, status) in [
            ("running-cancel", "running"),
            ("requested-cancel", "cancel_requested"),
        ] {
            state
                .db
                .execute(
                    "INSERT INTO operations VALUES(?1,'client','project',?1,'fixture',?2,NULL,1,1)",
                    params![operation, status],
                )
                .unwrap();
            let receipt = state
                .get(
                    &Id::new("client").unwrap(),
                    &Id::new("project").unwrap(),
                    &Id::new(operation).unwrap(),
                )
                .unwrap();
            state
                .record_cancellation(&receipt, false, "cancelled")
                .unwrap();
            assert_eq!(
                state
                    .get(
                        &receipt.client_id,
                        &receipt.project_id,
                        &receipt.operation_id
                    )
                    .unwrap()
                    .state,
                OperationState::Cancelled
            );
        }
        state.db.execute("INSERT INTO operations VALUES('next','client','project','next','fixture','running',NULL,1,1)",[]).unwrap();
        state
            .update(&Id::new("next").unwrap(), OperationState::Succeeded, None)
            .unwrap();
        assert_eq!(
            state
                .get(
                    &Id::new("client").unwrap(),
                    &Id::new("project").unwrap(),
                    &Id::new("next").unwrap()
                )
                .unwrap()
                .state,
            OperationState::Succeeded
        );
    }
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
