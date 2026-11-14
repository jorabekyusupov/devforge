//! Isolated native-client bridge prerequisite. NOT registered in Tauri handlers
//! or app state: the existing GUI/default ownership is unchanged in this slice.
//!
//! All I/O methods are blocking native-worker APIs. No Workspace, Supervisor,
//! discovery owner or renderer-provided endpoint/role is constructed here.
use crate::manager_artifact::{self, BundledManager};
use devforge_control::Id;
use devforge_core::SecretStore;
use devforge_manager::{
    Endpoint,
    auth::{DesktopAuth, DesktopCredential, Policy, TrustedDesktopAction},
    controller::local_owner_user,
    desktop::{AccessRequest, ReadRequest},
    launch::{self, LaunchSpec},
    native_client::{
        self, AdmissionProgress, NativeClient, ReadProgress, ReadResult, Receipt, Snapshot,
        SnapshotRead,
    },
};
use devforge_platform::{
    APP_IDENTIFIER, NativeSecretStore, app_data_dir, distribution::NativeClientLease,
};
use devforge_runtime::Cancellation;
use devforge_workspace::Command;
use serde::Serialize;
use serde_json::Value;
use std::{
    fmt,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Disconnected,
    Starting,
    Authenticating,
    AuthenticationRequired,
    Connected,
    Quitting,
    Unavailable,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeError {
    Busy,
    InvalidRoot,
    ArtifactUnavailable,
    OwnerUnavailable,
    OwnerChanged,
    AuthenticationRequired,
    CredentialUnavailable,
    ShuttingDown,
    AdmissionCancelled,
    AdmissionUnconfirmed,
    ResultUnavailable,
    Protocol,
    RequestCode(String),
}
impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}
impl std::error::Error for BridgeError {}
impl BridgeError {
    pub fn code(&self) -> &str {
        match self {
            Self::Busy => "native_bridge_busy",
            Self::InvalidRoot => "native_bridge_invalid_root",
            Self::ArtifactUnavailable => "native_bridge_artifact_unavailable",
            Self::OwnerUnavailable => "native_bridge_owner_unavailable",
            Self::OwnerChanged => "native_bridge_owner_changed",
            Self::AuthenticationRequired => "native_bridge_authentication_required",
            Self::CredentialUnavailable => "native_bridge_credential_unavailable",
            Self::ShuttingDown => "native_bridge_shutting_down",
            Self::AdmissionCancelled => "native_client_admission_cancelled",
            Self::AdmissionUnconfirmed => "native_client_admission_unconfirmed",
            Self::ResultUnavailable => "native_bridge_result_unavailable",
            Self::Protocol => "native_bridge_protocol",
            Self::RequestCode(code) => code,
        }
    }
}
pub type Result<T> = std::result::Result<T, BridgeError>;
#[derive(Clone, Debug, Serialize)]
pub struct ConnectionView {
    pub state: ConnectionState,
    pub error: Option<BridgeError>,
}

/// The only provider entry is an explicit native user action, never a snapshot,
/// polling hook, startup, generic request flag or automatic renewal fallback.
pub trait NativeCredentialProvider: Send + Sync {
    fn credential_after_user_action(&self, endpoint: &Endpoint) -> Result<DesktopCredential>;
}
pub struct StoreCredentialProvider {
    store: Arc<dyn SecretStore + Send + Sync>,
}
impl StoreCredentialProvider {
    pub fn new(store: Arc<dyn SecretStore + Send + Sync>) -> Self {
        Self { store }
    }
    /// Constructor only. NativeSecretStore itself performs no startup probe;
    /// its get/set calls occur solely inside credential_after_user_action.
    pub fn native() -> Self {
        Self::new(Arc::new(NativeSecretStore::new(format!(
            "{APP_IDENTIFIER}.control"
        ))))
    }
}
impl NativeCredentialProvider for StoreCredentialProvider {
    fn credential_after_user_action(&self, endpoint: &Endpoint) -> Result<DesktopCredential> {
        let mut auth = DesktopAuth::new(
            endpoint.clone(),
            local_owner_user().map_err(|_| BridgeError::InvalidRoot)?,
            Policy::default(),
        )
        .map_err(|_| BridgeError::InvalidRoot)?;
        let action = TrustedDesktopAction::from_explicit_desktop_action();
        match auth
            .read_if_existing(&action, self.store.as_ref())
            .map_err(|_| BridgeError::CredentialUnavailable)?
        {
            Some(credential) => Ok(credential),
            // Creation is permitted only because this method itself is the
            // explicit user-action boundary. A malformed key is never repaired.
            None => auth
                .rotate_desktop_key(&action, self.store.as_ref(), std::time::Instant::now())
                .map_err(|_| BridgeError::CredentialUnavailable),
        }
    }
}

struct TrackedOperation {
    subject: Id,
    key: Id,
    cancel: Cancellation,
    receipt: Option<Receipt>,
    phase: AdmissionPhase,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdmissionPhase {
    Prepared,
    Sending,
    Admitted,
    Unconfirmed,
    Rejected,
}
#[derive(Clone)]
pub struct GuiIntent {
    subject: Id,
    key: Id,
    cancel: Cancellation,
}
impl GuiIntent {
    pub fn project_id(&self) -> &Id {
        &self.subject
    }
    pub fn idempotency_key(&self) -> &Id {
        &self.key
    }
}
#[derive(Clone, Debug)]
pub struct OperationView {
    pub project_id: Id,
    pub idempotency_key: Id,
    pub receipt: Option<Receipt>,
    pub cancellation_requested: bool,
    pub unconfirmed: bool,
    pub phase: AdmissionPhase,
}

struct Inner {
    root: PathBuf,
    artifact: Option<BundledManager>,
    _lease: NativeClientLease,
    provider: Arc<dyn NativeCredentialProvider>,
    sdk: Mutex<Option<NativeClient>>,
    view: Mutex<ConnectionView>,
    connecting: AtomicBool,
    generation: AtomicU64,
    quitting: AtomicBool,
    operation: Mutex<Option<TrackedOperation>>,
}
#[derive(Clone)]
pub struct NativeBridge {
    inner: Arc<Inner>,
}
impl fmt::Debug for NativeBridge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeBridge")
            .field("connection", &self.connection_view())
            .finish_non_exhaustive()
    }
}
struct Connecting<'a>(&'a AtomicBool);
impl Drop for Connecting<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
impl NativeBridge {
    /// Production root/executable binding come only from native configuration.
    /// No path, endpoint or role parameter can be supplied by a renderer.
    pub fn from_native_configuration(provider: Arc<dyn NativeCredentialProvider>) -> Result<Self> {
        let root = app_data_dir().map_err(|_| BridgeError::InvalidRoot)?;
        #[cfg(feature = "e2e")]
        {
            let fixture =
                crate::fixture_root::from_environment().map_err(|_| BridgeError::InvalidRoot)?;
            if fixture.root()
                != std::fs::canonicalize(&root).map_err(|_| BridgeError::InvalidRoot)?
            {
                return Err(BridgeError::InvalidRoot);
            }
        }

        let artifact =
            manager_artifact::bundled_launch().map_err(|_| BridgeError::ArtifactUnavailable)?;
        Self::build(root, artifact, provider)
    }
    fn build(
        root: PathBuf,
        artifact: Option<BundledManager>,
        provider: Arc<dyn NativeCredentialProvider>,
    ) -> Result<Self> {
        let lease = NativeClientLease::acquire(&root).map_err(|error| {
            if error.kind() == std::io::ErrorKind::WouldBlock {
                BridgeError::Busy
            } else {
                BridgeError::InvalidRoot
            }
        })?;
        let root = std::fs::canonicalize(root).map_err(|_| BridgeError::InvalidRoot)?;
        Ok(Self {
            inner: Arc::new(Inner {
                root,
                artifact,
                _lease: lease,
                provider,
                sdk: Mutex::new(None),
                view: Mutex::new(ConnectionView {
                    state: ConnectionState::Disconnected,
                    error: None,
                }),
                connecting: AtomicBool::new(false),
                generation: AtomicU64::new(0),
                quitting: AtomicBool::new(false),
                operation: Mutex::new(None),
            }),
        })
    }
    #[cfg(test)]
    pub(crate) fn from_fixture(
        root: PathBuf,
        provider: Arc<dyn NativeCredentialProvider>,
    ) -> Result<Self> {
        Self::build(root, None, provider)
    }
    pub fn connection_view(&self) -> ConnectionView {
        self.inner
            .view
            .lock()
            .map(|view| view.clone())
            .unwrap_or(ConnectionView {
                state: ConnectionState::Unavailable,
                error: Some(BridgeError::Protocol),
            })
    }
    fn view(&self, state: ConnectionState, error: Option<BridgeError>) {
        if let Ok(mut view) = self.inner.view.lock() {
            *view = ConnectionView { state, error };
        }
    }
    fn connecting(&self) -> Result<Connecting<'_>> {
        if self.inner.quitting.load(Ordering::Acquire) {
            return Err(BridgeError::ShuttingDown);
        }
        self.inner
            .connecting
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| BridgeError::Busy)?;
        Ok(Connecting(&self.inner.connecting))
    }
    fn endpoint(&self) -> Result<Endpoint> {
        Endpoint::existing(&self.inner.root).map_err(|_| BridgeError::OwnerUnavailable)
    }
    /// Explicit cold user launch only. Existing owners are preserved and need a
    /// separate explicit authentication action. This NEVER invokes the provider.
    pub fn connect_cold_user_launch(&self) -> Result<()> {
        let _attempt = self.connecting()?;
        let generation = self.inner.generation.load(Ordering::Acquire);
        self.view(ConnectionState::Starting, None);
        if let Ok(endpoint) = self.endpoint() {
            match NativeClient::observe_owner(&endpoint) {
                Ok(health) if health.state == native_client::OwnerState::Ready => {
                    self.view(
                        ConnectionState::AuthenticationRequired,
                        Some(BridgeError::AuthenticationRequired),
                    );
                    return Err(BridgeError::AuthenticationRequired);
                }
                Ok(health) => {
                    let error = if health.state == native_client::OwnerState::Quitting {
                        BridgeError::ShuttingDown
                    } else {
                        BridgeError::OwnerUnavailable
                    };
                    self.view(ConnectionState::Unavailable, Some(error.clone()));
                    return Err(error);
                }
                Err(native_client::Error::OwnerUnavailable) => {}
                Err(_) => {
                    self.view(
                        ConnectionState::Unavailable,
                        Some(BridgeError::OwnerChanged),
                    );
                    return Err(BridgeError::OwnerChanged);
                }
            }
        }
        let artifact = self.inner.artifact.as_ref().ok_or_else(|| {
            self.view(
                ConnectionState::Unavailable,
                Some(BridgeError::ArtifactUnavailable),
            );
            BridgeError::ArtifactUnavailable
        })?;
        let arguments = vec![
            "manager".into(),
            "serve".into(),
            "--private-desktop-bootstrap".into(),
        ];
        #[cfg(feature = "e2e")]
        let arguments = {
            let mut arguments = arguments;
            let fixture =
                crate::fixture_root::from_environment().map_err(|_| BridgeError::InvalidRoot)?;
            if fixture.root() != self.inner.root {
                return Err(BridgeError::InvalidRoot);
            }
            arguments.push("--internal-fixture-store".into());
            arguments
        };
        let spec = LaunchSpec {
            executable: artifact.executable.clone(),
            arguments,
            data_root: self.inner.root.clone(),
            environment: native_environment(),
        };
        let child = launch::spawn_with_private_bootstrap(&spec, &artifact.expected_sha256)
            .map_err(|_| BridgeError::OwnerUnavailable)?;
        let root = self.inner.root.clone();
        let ready = child
            .wait_ready(
                Duration::from_secs(30),
                move |pid| match Endpoint::existing(&root)
                    .ok()
                    .and_then(|endpoint| NativeClient::observe_owner(&endpoint).ok())
                {
                    Some(health) if health.owner_pid == pid => Ok(true),
                    Some(_) => Ok(false),
                    None => Err(devforge_manager::bootstrap::BootstrapError::NotReady),
                },
            )
            .map_err(|_| BridgeError::OwnerUnavailable)?;
        let sdk = NativeClient::from_bootstrapped(self.endpoint()?, ready).map_err(map_sdk)?;
        if generation != self.inner.generation.load(Ordering::Acquire) {
            return Err(BridgeError::ShuttingDown);
        }
        self.publish(sdk)
    }
    /// Caller invokes this only for an explicit native user action. No SDK mutex
    /// is held while an OS provider may block; polling remains non-probing.
    pub fn connect_user_action(&self) -> Result<()> {
        let _attempt = self.connecting()?;
        if self
            .with_sdk(|sdk| {
                if sdk.health()?.state != native_client::OwnerState::Ready {
                    return Err(native_client::Error::OwnerUnavailable);
                }
                sdk.session_info().map(|_| ())
            })
            .is_ok()
        {
            self.view(ConnectionState::Connected, None);
            return Ok(());
        }
        let generation = self.inner.generation.load(Ordering::Acquire);
        let endpoint = self.endpoint()?;
        let health = NativeClient::observe_owner(&endpoint).map_err(map_sdk)?;
        if health.state != native_client::OwnerState::Ready {
            let error = if health.state == native_client::OwnerState::Quitting {
                BridgeError::ShuttingDown
            } else {
                BridgeError::OwnerUnavailable
            };
            self.view(ConnectionState::Unavailable, Some(error.clone()));
            return Err(error);
        }
        self.view(ConnectionState::Authenticating, None);
        let credential = self
            .inner
            .provider
            .credential_after_user_action(&endpoint)
            .inspect_err(|error| {
                self.view(ConnectionState::AuthenticationRequired, Some(error.clone()));
            })?;
        let sdk = NativeClient::authenticate_explicit(endpoint, &credential).map_err(map_sdk)?;
        if generation != self.inner.generation.load(Ordering::Acquire)
            || self.inner.quitting.load(Ordering::Acquire)
        {
            return Err(BridgeError::ShuttingDown);
        }
        self.publish(sdk)
    }
    fn publish(&self, sdk: NativeClient) -> Result<()> {
        if self.inner.quitting.load(Ordering::Acquire) {
            return Err(BridgeError::ShuttingDown);
        }
        *self.inner.sdk.lock().map_err(|_| BridgeError::Protocol)? = Some(sdk);
        self.view(ConnectionState::Connected, None);
        Ok(())
    }
    fn with_sdk<T>(
        &self,
        f: impl FnOnce(&mut NativeClient) -> native_client::Result<T>,
    ) -> Result<T> {
        let mut slot = self.inner.sdk.try_lock().map_err(|_| BridgeError::Busy)?;
        let result = f(slot.as_mut().ok_or(BridgeError::AuthenticationRequired)?);
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                let error = map_sdk(error);
                if matches!(
                    error,
                    BridgeError::AuthenticationRequired | BridgeError::OwnerChanged
                ) {
                    *slot = None;
                    self.view(ConnectionState::AuthenticationRequired, Some(error.clone()));
                }
                Err(error)
            }
        }
    }
    // Owned image buffers cannot be retried after a try_lock failure (the move
    // would drop them). These worker-only finalizers wait for the bounded RPC
    // currently holding the client; no native credential prompt holds this lock.
    fn with_owned_image<T>(
        &self,
        f: impl FnOnce(&mut NativeClient) -> native_client::Result<T>,
    ) -> Result<T> {
        let mut slot = self.inner.sdk.lock().map_err(|_| BridgeError::Protocol)?;
        f(slot.as_mut().ok_or(BridgeError::AuthenticationRequired)?).map_err(map_sdk)
    }
    /// Separate native lifecycle maintenance, e.g. a trusted timer before token
    /// expiry. Never a polling side effect or provider call; the SDK refuses an
    /// expired/revoked/nonrenewable token and preserves the hard context deadline.
    /// Native lifecycle scheduler only; no provider, no expired resurrection.
    pub fn maintain_connected_context(&self) -> Result<()> {
        let info = self.with_sdk(NativeClient::session_info)?;
        if info.renewable && info.expires_in_ms < 60_000 {
            self.renew_connected_context()?;
        }
        Ok(())
    }
    pub fn renew_connected_context(&self) -> Result<()> {
        self.with_sdk(|sdk| sdk.renew_explicit().map(|_| ()))
    }
    pub fn health(&self) -> Result<native_client::Health> {
        self.with_sdk(NativeClient::health)
    }
    pub fn begin_snapshot(&self) -> Result<SnapshotRead> {
        self.with_sdk(NativeClient::begin_snapshot)
    }
    pub fn snapshot_step(&self, read: &mut SnapshotRead) -> Result<ReadProgress> {
        self.with_sdk(|sdk| sdk.snapshot_step(read))
    }
    pub fn finish_snapshot(&self, read: SnapshotRead) -> Result<Snapshot> {
        self.with_owned_image(|sdk| sdk.finish_snapshot(read))
    }
    pub fn release_snapshot(&self, read: SnapshotRead) -> Result<()> {
        self.with_owned_image(|sdk| sdk.release_snapshot(read))
    }
    pub fn begin_read(&self, key: &Id, request: &ReadRequest) -> Result<Receipt> {
        self.with_sdk(|sdk| sdk.desktop_read(key, request))
    }
    pub fn begin_read_result(&self, project: &Id, operation: &Id) -> Result<ReadResult> {
        self.with_sdk(|sdk| sdk.begin_read_result(project, operation))
    }
    pub fn begin_result_image(&self, project: &Id, operation: &Id) -> Result<ReadResult> {
        self.with_sdk(|sdk| sdk.begin_result_image(project, operation))
    }
    pub fn read_step(&self, read: &mut ReadResult) -> Result<ReadProgress> {
        self.with_sdk(|sdk| sdk.read_result_step(read))
    }
    pub fn finish_read(&self, read: ReadResult) -> Result<Value> {
        self.with_owned_image(|sdk| sdk.finish_read_result(read))
    }
    pub fn release_read(&self, read: ReadResult) -> Result<()> {
        self.with_owned_image(|sdk| sdk.release_read_result(read))
    }
    pub fn access_user_action(&self, key: &Id, request: &AccessRequest) -> Result<Receipt> {
        self.with_sdk(|sdk| sdk.desktop_access(key, request))
    }
    pub fn discovery_request(&self) -> Result<()> {
        self.with_sdk(NativeClient::discovery_request)
    }
    pub fn operations(
        &self,
        project: Option<&Id>,
        cursor: Option<&Value>,
        limit: u32,
    ) -> Result<native_client::OperationPage> {
        self.with_sdk(|sdk| sdk.operations(project, cursor, limit))
    }
    pub fn failure_detail(
        &self,
        project: &Id,
        operation: &Id,
    ) -> Result<native_client::FailureDetail> {
        self.with_sdk(|sdk| sdk.failure_detail(project, operation))
    }
    /// Subject is native manager-advised state, not a renderer-supplied authority.
    /// The future adapter resolves it centrally before calling this primitive.
    pub fn new_gui_intent(&self, command: &Command) -> Result<GuiIntent> {
        if self.inner.quitting.load(Ordering::Acquire) {
            return Err(BridgeError::ShuttingDown);
        }
        let subject = self.with_sdk(|sdk| sdk.resolve_subject(command))?;
        self.new_intent(subject)
    }
    pub fn new_gui_access_intent(&self, request: &AccessRequest) -> Result<GuiIntent> {
        if self.inner.quitting.load(Ordering::Acquire) {
            return Err(BridgeError::ShuttingDown);
        }
        self.new_intent(request.subject())
    }
    fn new_intent(&self, subject: Id) -> Result<GuiIntent> {
        let mut active = self
            .inner
            .operation
            .lock()
            .map_err(|_| BridgeError::Protocol)?;
        if active.is_some() {
            return Err(BridgeError::Busy);
        }
        let key = NativeClient::new_intent_id().map_err(map_sdk)?;
        let cancel = Cancellation::default();
        *active = Some(TrackedOperation {
            subject: subject.clone(),
            key: key.clone(),
            cancel: cancel.clone(),
            receipt: None,
            phase: AdmissionPhase::Prepared,
        });
        Ok(GuiIntent {
            subject,
            key,
            cancel,
        })
    }
    /// Track every durable GUI access just like command admission. Do not lose
    /// the original key when a secret/open/settings request has an unconfirmed
    /// reply; read-only reconciliation remains possible without re-execution.
    pub fn admit_gui_access(&self, intent: &GuiIntent, request: &AccessRequest) -> Result<Receipt> {
        {
            let mut slot = self
                .inner
                .operation
                .lock()
                .map_err(|_| BridgeError::Protocol)?;
            let active = slot.as_ref().ok_or(BridgeError::Protocol)?;
            if active.key != intent.key
                || active.subject != intent.subject
                || request.subject() != intent.subject
            {
                return Err(BridgeError::Protocol);
            }
            if !matches!(
                active.phase,
                AdmissionPhase::Prepared | AdmissionPhase::Rejected
            ) {
                return Err(BridgeError::Busy);
            }
            if intent.cancel.is_cancelled() {
                *slot = None;
                return Err(BridgeError::AdmissionCancelled);
            }
            slot.as_mut().ok_or(BridgeError::Protocol)?.phase = AdmissionPhase::Sending;
        }
        let result = self.with_sdk(|sdk| sdk.desktop_access(&intent.key, request));
        if let Ok(mut active) = self.inner.operation.lock()
            && let Some(active) = active.as_mut()
            && active.key == intent.key
        {
            match &result {
                Ok(receipt) => {
                    active.receipt = Some(receipt.clone());
                    active.phase = AdmissionPhase::Admitted;
                }
                Err(BridgeError::AdmissionUnconfirmed) => {
                    active.phase = AdmissionPhase::Unconfirmed
                }
                Err(_) => active.phase = AdmissionPhase::Rejected,
            }
        }
        if result.is_ok() && intent.cancel.is_cancelled() {
            let _ = self.cancel_gui_operation();
        }
        result
    }
    pub fn admit_gui_command(&self, intent: &GuiIntent, command: &Command) -> Result<Receipt> {
        {
            let mut active = self
                .inner
                .operation
                .lock()
                .map_err(|_| BridgeError::Protocol)?;
            if active
                .as_ref()
                .is_none_or(|active| active.key != intent.key || active.subject != intent.subject)
            {
                return Err(BridgeError::Protocol);
            }
            if let Some(active) = active.as_mut() {
                if matches!(
                    active.phase,
                    AdmissionPhase::Sending
                        | AdmissionPhase::Admitted
                        | AdmissionPhase::Unconfirmed
                ) {
                    return Err(BridgeError::Busy);
                }
                active.phase = AdmissionPhase::Sending;
            }
        }
        let result = self.with_sdk(|sdk| {
            sdk.admit_observed(
                &intent.subject,
                &intent.key,
                command,
                &intent.cancel,
                |progress| {
                    if let AdmissionProgress::Accepted(receipt) = progress
                        && let Ok(mut active) = self.inner.operation.lock()
                        && let Some(active) = active.as_mut()
                        && active.key == intent.key
                    {
                        active.receipt = Some(receipt);
                        active.phase = AdmissionPhase::Admitted;
                    }
                },
            )
        });
        if let Ok(mut active) = self.inner.operation.lock()
            && let Some(active) = active.as_mut()
            && active.key == intent.key
        {
            match &result {
                Err(BridgeError::AdmissionUnconfirmed) => {
                    active.phase = AdmissionPhase::Unconfirmed
                }
                Err(_) if active.receipt.is_none() => active.phase = AdmissionPhase::Rejected,
                _ => {}
            }
        }
        if matches!(result, Err(BridgeError::AdmissionCancelled))
            && let Ok(mut active) = self.inner.operation.lock()
            && active.as_ref().is_some_and(|active| {
                active.key == intent.key
                    && active.receipt.is_none()
                    && active.phase != AdmissionPhase::Unconfirmed
            })
        {
            *active = None;
        }
        if result.is_ok() && intent.cancel.is_cancelled() {
            // Keep the accepted receipt even if delivery of the already
            // requested cancellation is temporarily Busy/unavailable.
            let _ = self.cancel_gui_operation();
        }
        result
    }
    /// Explicitly abandon only a completed, definitely rejected admission. A
    /// sending/unconfirmed/accepted intent can never be silently replaced.
    pub fn discard_rejected_gui_intent(&self, intent: &GuiIntent) -> Result<()> {
        let mut active = self
            .inner
            .operation
            .lock()
            .map_err(|_| BridgeError::Protocol)?;
        if active.as_ref().is_none_or(|active| {
            active.key != intent.key
                || active.subject != intent.subject
                || active.phase != AdmissionPhase::Rejected
                || active.receipt.is_some()
        }) {
            return Err(BridgeError::Busy);
        }
        *active = None;
        Ok(())
    }
    pub fn operation_view(&self) -> Option<OperationView> {
        self.inner
            .operation
            .lock()
            .ok()?
            .as_ref()
            .map(|op| OperationView {
                project_id: op.subject.clone(),
                idempotency_key: op.key.clone(),
                receipt: op.receipt.clone(),
                cancellation_requested: op.cancel.is_cancelled(),
                unconfirmed: op.phase == AdmissionPhase::Unconfirmed,
                phase: op.phase,
            })
    }
    /// The flag is set before any SDK lock/IO. Receipt-less cancellation aborts
    /// admission; an accepted receipt routes only to that GUI operation.
    pub fn cancel_gui_operation(&self) -> Result<()> {
        let receipt = {
            let active = self
                .inner
                .operation
                .lock()
                .map_err(|_| BridgeError::Protocol)?;
            let Some(active) = active.as_ref() else {
                return Ok(());
            };
            active.cancel.cancel();
            active.receipt.clone()
        };
        if let Some(receipt) = receipt {
            self.with_sdk(|sdk| {
                sdk.cancel(&receipt.project_id, &receipt.operation_id)
                    .map(|_| ())
            })?;
        }
        Ok(())
    }
    pub fn reconcile_gui_operation(&self) -> Result<Option<Receipt>> {
        let intent = self.operation_view().ok_or(BridgeError::Protocol)?;
        let receipt =
            self.with_sdk(|sdk| sdk.operation_by_key(&intent.project_id, &intent.idempotency_key))?;
        if let Some(receipt) = &receipt
            && let Ok(mut active) = self.inner.operation.lock()
            && let Some(active) = active.as_mut()
            && active.key == intent.idempotency_key
        {
            active.receipt = Some(receipt.clone());
            active.phase = AdmissionPhase::Admitted;
        }
        if receipt.is_some() && intent.cancellation_requested {
            let _ = self.cancel_gui_operation();
        }
        Ok(receipt)
    }
    pub fn status(&self, project: &Id, operation: &Id) -> Result<Receipt> {
        self.with_sdk(|sdk| sdk.status(project, operation))
    }
    pub fn result(&self, project: &Id, operation: &Id) -> Result<Value> {
        self.with_sdk(|sdk| sdk.result(project, operation))
    }
    pub fn finish_gui_operation(&self, receipt: &Receipt) -> Result<()> {
        if !receipt.state.terminal() {
            return Err(BridgeError::Busy);
        }
        let actual = self.status(&receipt.project_id, &receipt.operation_id)?;
        if !actual.state.terminal() {
            return Err(BridgeError::Busy);
        }
        let mut active = self
            .inner
            .operation
            .lock()
            .map_err(|_| BridgeError::Protocol)?;
        if active
            .as_ref()
            .and_then(|a| a.receipt.as_ref())
            .is_none_or(|r| {
                r.operation_id != receipt.operation_id || r.project_id != receipt.project_id
            })
        {
            return Err(BridgeError::Protocol);
        }
        *active = None;
        Ok(())
    }
    pub fn quit_owner_user_action(&self) -> Result<()> {
        if self.inner.quitting.swap(true, Ordering::AcqRel) {
            return Err(BridgeError::ShuttingDown);
        }
        self.inner.generation.fetch_add(1, Ordering::AcqRel);
        self.view(ConnectionState::Quitting, None);
        match self.with_sdk(NativeClient::quit) {
            Ok(()) => {
                self.view(ConnectionState::Disconnected, None);
                Ok(())
            }
            Err(error) => {
                self.inner.quitting.store(false, Ordering::Release);
                self.view(ConnectionState::Unavailable, Some(error.clone()));
                Err(error)
            }
        }
    }
    /// Separate post-ACK cleanup confirmation; works after session revocation.
    /// The SDK checks the matching owner outcome while holding the legacy lease.
    pub fn confirm_owner_shutdown(&self) -> Result<()> {
        self.with_sdk(|sdk| sdk.confirm_shutdown())
    }
}
fn native_environment() -> std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString> {
    // Native process context only; the manager executable is separately fixed
    // and hash-verified. No loader flags, project env or auth token is inherited.
    let mut environment = std::collections::BTreeMap::new();
    for key in [
        "SystemRoot",
        "WINDIR",
        "HOME",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "TEMP",
        "TMP",
        "SSH_AUTH_SOCK",
    ] {
        if let Some(value) = std::env::var_os(key) {
            environment.insert(key.into(), value);
        }
    }
    environment.insert("LANG".into(), "C".into());
    environment.insert("LC_ALL".into(), "C".into());
    // Used only by project-owned system-tool discovery; the manager candidate
    // never uses PATH and remains the fixed compiled-hash executable above.
    if let Some(path) = std::env::var_os("PATH") {
        let paths = std::env::split_paths(&path)
            .filter(|path| path.is_absolute())
            .collect::<Vec<_>>();
        if let Ok(path) = std::env::join_paths(paths) {
            environment.insert("PATH".into(), path);
        }
    }
    environment
}
fn map_sdk(error: native_client::Error) -> BridgeError {
    match error {
        native_client::Error::OwnerUnavailable => BridgeError::OwnerUnavailable,
        native_client::Error::OwnerChanged => BridgeError::OwnerChanged,
        native_client::Error::ReauthenticationRequired => BridgeError::AuthenticationRequired,
        native_client::Error::AdmissionCancelled => BridgeError::AdmissionCancelled,
        native_client::Error::AdmissionUnconfirmed => BridgeError::AdmissionUnconfirmed,
        other => BridgeError::RequestCode(other.code().to_owned()),
    }
}

#[cfg(test)]
#[path = "../../../../crates/manager/tests/common/mod.rs"]
pub(crate) mod fixture_root;
#[cfg(test)]
mod tests {
    use super::*;
    use devforge_core::ErrorCode;
    use devforge_manager::controller::{Controller, Options, StartIntent, Stores};
    use std::{collections::BTreeMap, sync::atomic::AtomicUsize, thread, time::Instant};
    #[derive(Default)]
    struct MemoryStore {
        values: Mutex<BTreeMap<String, String>>,
        gets: AtomicUsize,
        sets: AtomicUsize,
    }
    impl SecretStore for MemoryStore {
        fn get(&self, key: &str) -> std::result::Result<Option<String>, ErrorCode> {
            self.gets.fetch_add(1, Ordering::SeqCst);
            Ok(self.values.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &str, value: &str) -> std::result::Result<(), ErrorCode> {
            self.sets.fetch_add(1, Ordering::SeqCst);
            self.values.lock().unwrap().insert(key.into(), value.into());
            Ok(())
        }
        fn delete(&self, key: &str) -> std::result::Result<(), ErrorCode> {
            self.values.lock().unwrap().remove(key);
            Ok(())
        }
    }
    struct NoWorkspaceSecrets;
    impl SecretStore for NoWorkspaceSecrets {
        fn get(&self, _: &str) -> std::result::Result<Option<String>, ErrorCode> {
            panic!("metadata fixture cannot read workspace/native credentials")
        }
        fn set(&self, _: &str, _: &str) -> std::result::Result<(), ErrorCode> {
            panic!("metadata fixture cannot write workspace/native credentials")
        }
        fn delete(&self, _: &str) -> std::result::Result<(), ErrorCode> {
            panic!("metadata fixture cannot delete workspace/native credentials")
        }
    }
    #[derive(Default)]
    struct DeniedProvider {
        calls: AtomicUsize,
    }
    impl NativeCredentialProvider for DeniedProvider {
        fn credential_after_user_action(&self, _: &Endpoint) -> Result<DesktopCredential> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(BridgeError::CredentialUnavailable)
        }
    }
    fn start(
        root: &std::path::Path,
        store: Arc<MemoryStore>,
        options: Options,
    ) -> (
        thread::JoinHandle<devforge_manager::controller::Result<()>>,
        devforge_manager::controller::OsQuitHandle,
    ) {
        let controller = Controller::with_options(
            root,
            StartIntent::ExplicitUserStart,
            Stores {
                workspace: Arc::new(NoWorkspaceSecrets),
                desktop: store,
            },
            options,
        )
        .unwrap();
        let endpoint = controller.endpoint().clone();
        let quitter = controller.quitter();
        let worker = thread::spawn(move || controller.serve(None));
        let started = Instant::now();
        while NativeClient::observe_owner(&endpoint).is_err() {
            assert!(started.elapsed() < Duration::from_secs(5));
            thread::sleep(Duration::from_millis(5));
        }
        (worker, quitter)
    }
    #[test]
    fn lease_and_disconnected_polling_never_probe_provider_or_fall_back() {
        let root = fixture_root::root("df-bridge-");
        let provider = Arc::new(DeniedProvider::default());
        let bridge = NativeBridge::build(root.path().to_owned(), None, provider.clone()).unwrap();
        assert_eq!(
            bridge.connection_view().state,
            ConnectionState::Disconnected
        );
        assert_eq!(
            bridge.begin_snapshot().unwrap_err(),
            BridgeError::AuthenticationRequired
        );
        assert_eq!(
            bridge.connect_cold_user_launch().unwrap_err(),
            BridgeError::ArtifactUnavailable
        );
        assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
        assert!(matches!(
            NativeBridge::build(root.path().to_owned(), None, provider),
            Err(BridgeError::Busy)
        ));
        assert!(devforge_platform::distribution::OperationLease::exclusive(root.path()).is_err());
        drop(bridge);
        assert!(devforge_platform::distribution::OperationLease::exclusive(root.path()).is_ok());
    }
    #[test]
    fn existing_owner_connects_only_on_explicit_action_and_preserves_gui_operation_ids() {
        let root = fixture_root::root("df-bridge-owner-");
        let store = Arc::new(MemoryStore::default());
        let (worker, _) = start(root.path(), store.clone(), Options::default());
        let bridge = NativeBridge::build(
            root.path().to_owned(),
            None,
            Arc::new(StoreCredentialProvider::new(store.clone())),
        )
        .unwrap();
        assert_eq!(
            bridge.connect_cold_user_launch().unwrap_err(),
            BridgeError::AuthenticationRequired
        );
        assert_eq!(store.gets.load(Ordering::SeqCst), 0);
        assert_eq!(store.sets.load(Ordering::SeqCst), 0);
        bridge.connect_user_action().unwrap();
        let gets = store.gets.load(Ordering::SeqCst);
        assert_eq!(gets, 2);
        assert_eq!(store.sets.load(Ordering::SeqCst), 1);
        bridge.connect_user_action().unwrap();
        assert_eq!(
            store.gets.load(Ordering::SeqCst),
            gets,
            "valid connected state must not prompt again"
        );
        bridge.renew_connected_context().unwrap();
        assert_eq!(
            store.gets.load(Ordering::SeqCst),
            gets,
            "separate lifecycle renewal must not call provider/store"
        );
        assert!(
            devforge_platform::distribution::AppLease::acquire(root.path()).is_err(),
            "manager still excludes the legacy GUI"
        );
        let mut read = bridge.begin_snapshot().unwrap();
        loop {
            let progress = bridge.snapshot_step(&mut read).unwrap();
            bridge.health().unwrap();
            if matches!(progress, ReadProgress::Complete) {
                break;
            }
        }
        assert!(
            bridge.finish_snapshot(read).unwrap().snapshot["projects"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let command = Command::RefreshRuntimes;
        let intent = bridge.new_gui_intent(&command).unwrap();
        bridge.cancel_gui_operation().unwrap();
        assert!(matches!(
            bridge.admit_gui_command(&intent, &command),
            Err(BridgeError::AdmissionCancelled)
        ));
        assert!(bridge.operation_view().is_none());
        let rejected = bridge.new_gui_intent(&command).unwrap();
        let mismatched = Command::Favorite {
            id: "a".repeat(32),
            favorite: true,
        };
        assert!(
            matches!(bridge.admit_gui_command(&rejected,&mismatched),Err(BridgeError::RequestCode(code))if code=="actor_subject_mismatch")
        );
        assert_eq!(
            bridge.operation_view().unwrap().phase,
            AdmissionPhase::Rejected
        );
        assert!(matches!(
            bridge.new_gui_intent(&command),
            Err(BridgeError::Busy)
        ));
        bridge.discard_rejected_gui_intent(&rejected).unwrap();
        let intent = bridge.new_gui_intent(&command).unwrap();
        let receipt = bridge.admit_gui_command(&intent, &command).unwrap();
        assert_eq!(
            bridge
                .operation_view()
                .unwrap()
                .receipt
                .unwrap()
                .operation_id,
            receipt.operation_id
        );
        let started = Instant::now();
        let terminal = loop {
            let status = bridge
                .status(&receipt.project_id, &receipt.operation_id)
                .unwrap();
            if status.state.terminal() {
                break status;
            }
            assert!(started.elapsed() < Duration::from_secs(5));
            thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(terminal.state, devforge_control::OperationState::Succeeded);
        assert!(
            bridge
                .result(&receipt.project_id, &receipt.operation_id)
                .unwrap()
                .is_object()
        );
        bridge.finish_gui_operation(&terminal).unwrap();
        assert!(bridge.operation_view().is_none());
        let discovery = root.path().join("discovery");
        std::fs::create_dir(&discovery).unwrap();
        let access = AccessRequest::DiscoveryRoot {
            path: discovery.to_string_lossy().into_owned(),
        };
        let cancelled_access = bridge.new_gui_access_intent(&access).unwrap();
        bridge.cancel_gui_operation().unwrap();
        assert_eq!(
            bridge
                .admit_gui_access(&cancelled_access, &access)
                .unwrap_err(),
            BridgeError::AdmissionCancelled
        );
        assert!(bridge.operation_view().is_none());
        let access_intent = bridge.new_gui_access_intent(&access).unwrap();
        let access_receipt = bridge.admit_gui_access(&access_intent, &access).unwrap();
        let started = Instant::now();
        let access_terminal = loop {
            let status = bridge
                .status(&access_receipt.project_id, &access_receipt.operation_id)
                .unwrap();
            if status.state.terminal() {
                break status;
            }
            assert!(started.elapsed() < Duration::from_secs(5));
            thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(
            access_terminal.state,
            devforge_control::OperationState::Succeeded
        );
        bridge.finish_gui_operation(&access_terminal).unwrap();
        assert_eq!(store.gets.load(Ordering::SeqCst), gets);
        bridge.quit_owner_user_action().unwrap();
        worker.join().unwrap().unwrap();
    }
    #[test]
    fn denied_explicit_provider_never_creates_local_owner_or_fake_snapshot() {
        let root = fixture_root::root("df-bridge-denied-");
        let store = Arc::new(MemoryStore::default());
        let (worker, quit) = start(root.path(), store, Options::default());
        let provider = Arc::new(DeniedProvider::default());
        let bridge = NativeBridge::build(root.path().to_owned(), None, provider.clone()).unwrap();
        assert_eq!(
            bridge.connect_user_action().unwrap_err(),
            BridgeError::CredentialUnavailable
        );
        assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            bridge.begin_snapshot().unwrap_err(),
            BridgeError::AuthenticationRequired
        );
        assert!(devforge_manager::Owner::acquire(root.path()).is_err());
        assert!(NativeClient::observe_owner(&Endpoint::existing(root.path()).unwrap()).is_ok());
        quit.request_explicit().unwrap();
        worker.join().unwrap().unwrap();
    }
    #[test]
    fn expired_polling_does_not_read_store_and_requires_explicit_reconnect() {
        let root = fixture_root::root("df-bridge-expiry-");
        let store = Arc::new(MemoryStore::default());
        let options = Options {
            auth: Policy {
                session_lifetime: Duration::from_millis(100),
                store_interval: Duration::from_secs(1),
                ..Policy::default()
            },
            ..Options::default()
        };
        let (worker, quit) = start(root.path(), store.clone(), options);
        let bridge = NativeBridge::build(
            root.path().to_owned(),
            None,
            Arc::new(StoreCredentialProvider::new(store.clone())),
        )
        .unwrap();
        bridge.connect_user_action().unwrap();
        let gets = store.gets.load(Ordering::SeqCst);
        thread::sleep(Duration::from_millis(150));
        assert_eq!(
            bridge.begin_snapshot().unwrap_err(),
            BridgeError::AuthenticationRequired
        );
        assert_eq!(store.gets.load(Ordering::SeqCst), gets);
        thread::sleep(Duration::from_millis(1_000));
        bridge.connect_user_action().unwrap();
        assert_eq!(store.gets.load(Ordering::SeqCst), gets + 2);
        quit.request_explicit().unwrap();
        worker.join().unwrap().unwrap();
    }
}
