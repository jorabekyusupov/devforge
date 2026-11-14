//! Private legacy-standalone owner composition. No public agent grant issuer or
//! GUI migration: legacy desktop exclusion is intentionally retained for now.
use crate::{
    Endpoint, Owner, PeerIdentity, Server, StopSignal,
    actor::{ActorHandle, OperationIdentity, WorkspaceActor},
    auth::{
        BootstrapCredential, DesktopAuth, DesktopCredential, NativeGuiPolicy, Policy,
        SessionCredential,
    },
    bootstrap::{BootstrapWriter, ReadyPermit},
};
use devforge_control::Id;
use devforge_core::SecretStore;
use devforge_platform::distribution::AppLease;
use devforge_workspace::Command;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Busy,
    LegacyDesktopActive,
    StartInhibited,
    PersistenceUnavailable,
    OwnerUnavailable,
    ActorUnavailable,
    AuthenticationUnavailable,
    RuntimeUnavailable,
    BootstrapUnavailable,
    Unauthorized,
    PayloadUnavailable,
    ShutdownFailed,
    ShutdownUnconfirmed,
}
impl Error {
    pub fn code(self) -> &'static str {
        match self {
            Self::Busy => "controller_busy",
            Self::LegacyDesktopActive => "controller_legacy_desktop_active",
            Self::StartInhibited => "controller_start_inhibited",
            Self::PersistenceUnavailable => "controller_persistence_unavailable",
            Self::OwnerUnavailable => "controller_owner_unavailable",
            Self::ActorUnavailable => "controller_actor_unavailable",
            Self::AuthenticationUnavailable => "controller_auth_unavailable",
            Self::RuntimeUnavailable => "controller_runtime_unavailable",
            Self::BootstrapUnavailable => "controller_bootstrap_unavailable",
            Self::Unauthorized => "manager_auth_unauthorized",
            Self::PayloadUnavailable => "controller_payload_unavailable",
            Self::ShutdownFailed => "controller_shutdown_failed",
            Self::ShutdownUnconfirmed => "controller_shutdown_unconfirmed",
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

pub struct Stores {
    pub workspace: Arc<dyn SecretStore + Send + Sync>,
    pub desktop: Arc<dyn SecretStore + Send + Sync>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartIntent {
    AutoStart,
    ExplicitUserStart,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    LegacyStandalone,
}
#[derive(Default)]
pub struct Options {
    pub mode: Mode,
    pub auth: Policy,
    pub native_gui: NativeGuiPolicy,
    /// Trusted native launch configuration only; no renderer/RPC setter.
    pub terminal_arguments: Vec<std::ffi::OsString>,
}

/// Native OS context for trusted local composition; never a wire method/field.
pub fn local_owner_user() -> Result<String> {
    crate::transport::os::current_user().map_err(|_| Error::OwnerUnavailable)
}

pub struct Controller {
    server: Option<Server>,
    actor: Option<WorkspaceActor>,
    context: Arc<Context>,
    // Keep this after actor: legacy GUI exclusion lasts through service cleanup.
    _legacy_gui: AppLease,
}
struct Context {
    actor: ActorHandle,
    auth: Mutex<DesktopAuth>,
    auth_store: Arc<dyn SecretStore + Send + Sync>,
    endpoint: Endpoint,
    native_policy: NativeGuiPolicy,
    quit_gate: Mutex<()>,
    quitting: AtomicBool,
    owner_active: AtomicBool,
    payloads: Mutex<crate::payload::Store>,
    stop: StopSignal,
    shutdown_id: String,
    external: Arc<crate::external_control::Server>,
    services: Arc<crate::service_control::Services>,
}
/// Trusted OS-signal composition, never a wire role or request field. This may
/// wait for persistence; call it on the signal library's dedicated worker.
#[derive(Clone)]
pub struct OsQuitHandle {
    context: Arc<Context>,
}
impl OsQuitHandle {
    pub fn request_explicit(&self) -> Result<()> {
        let _gate = self
            .context
            .quit_gate
            .lock()
            .map_err(|_| Error::PersistenceUnavailable)?;
        self.context.commit_quit(|| Ok(()))
    }
}
impl Controller {
    pub fn start(root: &Path, intent: StartIntent, stores: Stores) -> Result<Self> {
        Self::with_options(root, intent, stores, Options::default())
    }
    pub fn with_options(
        root: &Path,
        intent: StartIntent,
        stores: Stores,
        options: Options,
    ) -> Result<Self> {
        let owner = Owner::acquire(root).map_err(|error| {
            if matches!(error, crate::Error::Busy) {
                Error::Busy
            } else {
                Error::OwnerUnavailable
            }
        })?;
        let endpoint = owner.endpoint().clone();
        let legacy = match options.mode {
            Mode::LegacyStandalone => AppLease::acquire(endpoint.root()).map_err(|error| {
                if error.kind() == std::io::ErrorKind::WouldBlock {
                    Error::LegacyDesktopActive
                } else {
                    Error::OwnerUnavailable
                }
            })?,
        };
        match intent {
            StartIntent::AutoStart if inhibition(&endpoint)? => return Err(Error::StartInhibited),
            StartIntent::ExplicitUserStart => persist_inhibition(&endpoint, false)?,
            _ => {}
        }
        let mut nonce = [0u8; 32];
        getrandom::fill(&mut nonce).map_err(|_| Error::PersistenceUnavailable)?;
        let shutdown_id: String = nonce.iter().map(|byte| format!("{byte:02x}")).collect();
        let mut lifecycle = read_inhibition(&endpoint)?;
        lifecycle.shutdown = Some(ShutdownRecord {
            owner: shutdown_id.clone(),
            outcome: ShutdownOutcome::Pending,
        });
        persist_lifecycle_checked(&endpoint, &lifecycle, || Ok(()))?;
        let auth = DesktopAuth::new(endpoint.clone(), local_owner_user()?, options.auth)
            .map_err(|_| Error::AuthenticationUnavailable)?;
        let external = Arc::new(
            crate::external_control::Server::new(&owner)
                .map_err(|_| Error::AuthenticationUnavailable)?,
        );
        let services = Arc::new(crate::service_control::Services::new(external.clone()));
        let actor = WorkspaceActor::start_with_terminal_arguments(
            owner
                .claim_workspace()
                .map_err(|_| Error::ActorUnavailable)?,
            stores.workspace,
            options.terminal_arguments,
        )
        .map_err(|_| Error::ActorUnavailable)?;
        let context = Arc::new(Context {
            actor: actor.handle(),
            auth: Mutex::new(auth),
            auth_store: stores.desktop,
            payloads: Mutex::new(
                crate::payload::Store::new(endpoint.clone(), crate::payload::Policy::default())
                    .map_err(|_| Error::PayloadUnavailable)?,
            ),
            endpoint,
            native_policy: options.native_gui,
            quit_gate: Mutex::new(()),
            quitting: AtomicBool::new(false),
            owner_active: AtomicBool::new(true),
            stop: StopSignal::default(),
            shutdown_id,
            external,
            services,
        });
        Ok(Self {
            server: Some(Server::new(owner)),
            actor: Some(actor),
            context,
            _legacy_gui: legacy,
        })
    }
    pub fn endpoint(&self) -> &Endpoint {
        &self.context.endpoint
    }
    pub fn quitter(&self) -> OsQuitHandle {
        OsQuitHandle {
            context: self.context.clone(),
        }
    }
    /// Optional bootstrap writer comes only from a validated inherited private
    /// pipe captured by the launch entrypoint, never from a wire request.
    pub fn serve(mut self, bootstrap: Option<BootstrapWriter>) -> Result<()> {
        if self.context.quitting.load(Ordering::Acquire) {
            let cleanup = self
                .actor
                .take()
                .ok_or(Error::ActorUnavailable)?
                .shutdown()
                .map_err(|_| Error::ActorUnavailable);
            return self.context.finish_shutdown(cleanup);
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| Error::RuntimeUnavailable)?;
        let server = self.server.take().ok_or(Error::OwnerUnavailable)?;
        let ready_context = self.context.clone();
        let context = self.context.clone();
        let maintenance = self.context.clone();
        let expiry = runtime.spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                if maintenance.quitting.load(Ordering::Acquire) {
                    break;
                }
                if let Ok(mut store) = maintenance.payloads.try_lock() {
                    store.expire(Instant::now());
                }
            }
        });
        let result = runtime
            .block_on(server.run_async_ready(
                self.context.stop.clone(),
                move |listener| {
                    let _gate = ready_context
                        .quit_gate
                        .try_lock()
                        .map_err(|_| crate::Error::Busy)?;
                    if ready_context.quitting.load(Ordering::Acquire) {
                        return Err(crate::Error::HandlerFailed);
                    }
                    if let Some(writer) = bootstrap {
                        let permit = ReadyPermit::from_ready(listener, &ready_context.actor)
                            .map_err(|_| crate::Error::HandlerFailed)?;
                        let mut auth = ready_context
                            .auth
                            .try_lock()
                            .map_err(|_| crate::Error::Busy)?;
                        writer
                            .issue_once(
                                &mut auth,
                                permit,
                                ready_context.native_policy,
                                Instant::now(),
                            )
                            .map_err(|_| crate::Error::HandlerFailed)?;
                    }
                    Ok(())
                },
                move |peer, input| {
                    let context = context.clone();
                    async move {
                        // Native credential I/O and any ledger waits are never performed
                        // on the listener event loop. Health never needs the auth mutex.
                        tokio::task::spawn_blocking(move || context.request(peer, input))
                            .await
                            .map_err(|_| crate::Error::HandlerFailed)
                    }
                },
            ))
            .map_err(|_| Error::OwnerUnavailable);
        self.context.quitting.store(true, Ordering::Release);
        let cleanup = self
            .actor
            .take()
            .ok_or(Error::ActorUnavailable)?
            .shutdown()
            .map_err(|_| Error::ActorUnavailable);
        expiry.abort();
        // Native credential reads are not cancellable. Do not indefinitely block
        // the event-loop owner on an abandoned OS prompt; no actor lease is held
        // by those tasks and their results are refused after quitting.
        runtime.shutdown_timeout(Duration::from_millis(100));
        self.context.finish_shutdown(result.and(cleanup))
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        // A retained OS-signal handle cannot mutate a later owner's lifecycle
        // record. Finish any already-authorized marker write before releasing
        // the legacy/manager exclusion and mark this composition inactive.
        if let Ok(_gate) = self.context.quit_gate.lock() {
            self.context.owner_active.store(false, Ordering::Release);
            self.context.quitting.store(true, Ordering::Release);
        }
        if let Ok(mut auth) = self.context.auth.try_lock() {
            auth.revoke_all();
        }
        self.context
            .payloads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        if let Some(actor) = self.actor.take() {
            let _ = actor.shutdown();
            // Dropping without a completed serve is not a successful explicit
            // shutdown, even if the best-effort service cleanup returned Ok.
            let _ = self.context.finish_shutdown(Err(Error::ShutdownFailed));
        }
        self.server.take();
    }
}

struct WireToken(Zeroizing<String>);
impl<'de> Deserialize<'de> for WireToken {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let value = Zeroizing::new(String::deserialize(deserializer)?);
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(serde::de::Error::custom("invalid credential"));
        }
        Ok(Self(value))
    }
}
#[derive(Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    ResolveSubject {
        session: WireToken,
        selector: crate::desktop::SubjectSelector,
    },
    OperationByKey {
        session: WireToken,
        project_id: Id,
        idempotency_key: Id,
    },
    DesktopRead {
        session: WireToken,
        idempotency_key: Id,
        request: Box<crate::desktop::ReadRequest>,
    },
    DesktopAccess {
        session: WireToken,
        idempotency_key: Id,
        request: Box<crate::desktop::AccessRequest>,
    },
    #[serde(rename = "discovery_request")]
    DiscoveryScan {
        session: WireToken,
    },
    Operations {
        session: WireToken,
        project_id: Option<Id>,
        cursor: Option<crate::actor::OperationCursor>,
        limit: u32,
    },
    Health {},
    Authenticate {
        credential: WireToken,
    },
    RedeemBootstrap {
        token: WireToken,
        root_fingerprint: String,
        manager_instance: String,
    },
    Renew {
        session: WireToken,
    },
    Snapshot {
        session: WireToken,
    },
    Submit {
        session: WireToken,
        project_id: Id,
        idempotency_key: Id,
        command: Box<Command>,
    },
    Status {
        session: WireToken,
        project_id: Id,
        operation_id: Id,
    },
    Result {
        session: WireToken,
        project_id: Id,
        operation_id: Id,
    },
    FailureDetail {
        session: WireToken,
        project_id: Id,
        operation_id: Id,
    },
    Cancel {
        session: WireToken,
        project_id: Id,
        operation_id: Id,
    },
    Quit {
        session: WireToken,
    },
    ExternalGrantIssue {
        session: WireToken,
        project_id: Id,
        scopes: Vec<devforge_control::Scope>,
        ttl_ms: u64,
    },
    ExternalGrantList {
        session: WireToken,
    },
    ExternalGrantRevoke {
        session: WireToken,
        grant_id: Id,
    },
    ApproveServicePlan {
        session: WireToken,
        digest: devforge_control::Digest,
    },
    ConfirmDomainChange {
        session: WireToken,
        digest: devforge_control::Digest,
    },
}
impl Context {
    fn finish_shutdown(&self, cleanup: Result<()>) -> Result<()> {
        self.services.clear();
        self.external.close();
        let mut lifecycle = read_inhibition(&self.endpoint)?;
        if lifecycle
            .shutdown
            .as_ref()
            .map(|value| value.owner.as_str())
            != Some(self.shutdown_id.as_str())
        {
            return Err(Error::ShutdownUnconfirmed);
        }
        lifecycle.shutdown = Some(ShutdownRecord {
            owner: self.shutdown_id.clone(),
            outcome: if cleanup.is_ok() {
                ShutdownOutcome::Succeeded
            } else {
                ShutdownOutcome::Failed
            },
        });
        persist_lifecycle_checked(&self.endpoint, &lifecycle, || Ok(()))?;
        cleanup
    }
    // Caller holds quit_gate. Failure changes no live admission/auth/stop state.
    fn commit_quit<F: FnOnce() -> Result<()>>(&self, authorize: F) -> Result<()> {
        if !self.owner_active.load(Ordering::Acquire) {
            return Err(Error::OwnerUnavailable);
        }
        if self.quitting.load(Ordering::Acquire) {
            return Ok(());
        }
        let mut lifecycle = read_inhibition(&self.endpoint)?;
        lifecycle.inhibited = true;
        lifecycle.shutdown = Some(ShutdownRecord {
            owner: self.shutdown_id.clone(),
            outcome: ShutdownOutcome::Pending,
        });
        persist_lifecycle_checked(&self.endpoint, &lifecycle, authorize)?;
        self.quitting.store(true, Ordering::Release);
        self.payloads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        if let Ok(mut auth) = self.auth.try_lock() {
            auth.revoke_all();
        }
        self.stop.request();
        Ok(())
    }
    fn request(&self, peer: PeerIdentity, input: Value) -> Value {
        if input
            .get("method")
            .and_then(Value::as_str)
            .is_some_and(crate::service_control::Services::handles)
        {
            let _gate = match self.quit_gate.try_lock() {
                Ok(gate) => gate,
                Err(_) => {
                    return self.services.request(
                        peer,
                        input,
                        &self.actor,
                        Some(devforge_control::ErrorCode::Busy),
                    );
                }
            };
            return self.services.request(
                peer,
                input,
                &self.actor,
                self.quitting
                    .load(Ordering::Acquire)
                    .then_some(devforge_control::ErrorCode::Interrupted),
            );
        }
        if input.get("method").and_then(Value::as_str) == Some("external") {
            let invalid = crate::external_control::invalid_reply(&input);
            return match serde_json::from_value(input) {
                Ok(wire) => serde_json::to_value(self.external.request(
                    peer,
                    wire,
                    &self.actor,
                    self.quitting.load(Ordering::Acquire),
                ))
                .unwrap_or_else(|_| fault("controller_output_limit")),
                Err(_) => serde_json::to_value(invalid)
                    .unwrap_or_else(|_| fault("controller_output_limit")),
            };
        }
        if input
            .get("method")
            .and_then(Value::as_str)
            .is_some_and(crate::payload::Request::handles)
        {
            return match serde_json::from_value(input) {
                Ok(request) => self.payload_request(peer, request),
                Err(_) => fault("controller_invalid_request"),
            };
        }
        let request = match serde_json::from_value::<Request>(input) {
            Ok(value) => value,
            Err(_) => return fault("controller_invalid_request"),
        };
        if matches!(request, Request::Health { .. }) {
            let state = if self.quitting.load(Ordering::Acquire) {
                "quitting"
            } else if self.actor.bootstrap_ready_root().is_ok() {
                "ready"
            } else {
                "unavailable"
            };
            return success(json!({"api_version":1,"owner_pid":std::process::id(),"state":state}));
        }
        if self.quitting.load(Ordering::Acquire) {
            return fault("controller_quitting");
        }
        match request {
            Request::Authenticate { credential } => {
                let credential = match DesktopCredential::from_wire(&credential.0) {
                    Ok(value) => value,
                    Err(error) => return fault(&error.to_string()),
                };
                let mut auth = match self.auth.try_lock() {
                    Ok(value) => value,
                    Err(_) => return fault("controller_busy"),
                };
                let result = auth.authenticate_native_gui(
                    &peer,
                    &credential,
                    self.auth_store.as_ref(),
                    self.native_policy,
                    Instant::now(),
                );
                if self.quitting.load(Ordering::Acquire) {
                    auth.revoke_all();
                    return fault("controller_quitting");
                }
                match result {
                    Ok(session) => session_reply(&mut auth, &peer, &session),
                    Err(error) => fault(&error.to_string()),
                }
            }
            Request::RedeemBootstrap {
                token,
                root_fingerprint,
                manager_instance,
            } => {
                let (Some(root), Some(instance)) =
                    (decode_hex(&root_fingerprint), decode_hex(&manager_instance))
                else {
                    return fault("controller_invalid_request");
                };
                let credential = match BootstrapCredential::from_wire(&token.0, root, instance) {
                    Ok(value) => value,
                    Err(error) => return fault(&error.to_string()),
                };
                let mut auth = match self.auth.try_lock() {
                    Ok(value) => value,
                    Err(_) => return fault("controller_busy"),
                };
                let result = auth.redeem_private_bootstrap(&peer, &credential, Instant::now());
                if self.quitting.load(Ordering::Acquire) {
                    auth.revoke_all();
                    return fault("controller_quitting");
                }
                match result {
                    Ok(session) => session_reply(&mut auth, &peer, &session),
                    Err(error) => fault(&error.to_string()),
                }
            }
            Request::Renew { session } => {
                let credential = match SessionCredential::from_wire(&session.0) {
                    Ok(value) => value,
                    Err(error) => return fault(&error.to_string()),
                };
                let mut auth = match self.auth.try_lock() {
                    Ok(value) => value,
                    Err(_) => return fault("controller_busy"),
                };
                let result = auth.renew_native_gui(&peer, &credential, Instant::now());
                if self.quitting.load(Ordering::Acquire) {
                    auth.revoke_all();
                    return fault("controller_quitting");
                }
                match result {
                    Ok(session) => session_reply(&mut auth, &peer, &session),
                    Err(error) => fault(&error.to_string()),
                }
            }
            request => self.privileged(peer, request),
        }
    }
    fn payload_request(&self, peer: PeerIdentity, request: crate::payload::Request) -> Value {
        let _gate = match self.quit_gate.try_lock() {
            Ok(value) => value,
            Err(_) => return fault("controller_busy"),
        };
        if self.quitting.load(Ordering::Acquire) {
            return fault("controller_quitting");
        }
        let mut auth = match self.auth.try_lock() {
            Ok(value) => value,
            Err(_) => return fault("controller_busy"),
        };
        let credential = match request.credential() {
            Ok(value) => value,
            Err(error) => return fault(error.code()),
        };
        let authority = match auth.authorize(&peer, &credential, Instant::now()) {
            Ok(value) => value,
            Err(error) => return fault(&error.to_string()),
        };
        let mut store = match self.payloads.try_lock() {
            Ok(value) => value,
            Err(_) => return fault("controller_busy"),
        };
        match request.dispatch(&authority, &mut store, &self.actor) {
            Ok(value) => bounded_reply(&value),
            Err(error) => fault(error.code()),
        }
    }
    fn privileged(&self, peer: PeerIdentity, request: Request) -> Value {
        // One gate serializes admission against durable Quit. No caller can
        // enqueue after inhibition is persisted but before quitting is visible.
        let _gate = match self.quit_gate.try_lock() {
            Ok(value) => value,
            Err(_) => return fault("controller_busy"),
        };
        if self.quitting.load(Ordering::Acquire) {
            return fault("controller_quitting");
        }
        let session = match &request {
            Request::Snapshot { session }
            | Request::ResolveSubject { session, .. }
            | Request::OperationByKey { session, .. }
            | Request::DesktopRead { session, .. }
            | Request::DesktopAccess { session, .. }
            | Request::DiscoveryScan { session }
            | Request::Operations { session, .. }
            | Request::Submit { session, .. }
            | Request::Status { session, .. }
            | Request::Result { session, .. }
            | Request::FailureDetail { session, .. }
            | Request::Cancel { session, .. }
            | Request::ExternalGrantIssue { session, .. }
            | Request::ExternalGrantList { session }
            | Request::ExternalGrantRevoke { session, .. }
            | Request::ApproveServicePlan { session, .. }
            | Request::ConfirmDomainChange { session, .. }
            | Request::Quit { session } => session,
            _ => return fault("controller_invalid_request"),
        };
        let credential = match SessionCredential::from_wire(&session.0) {
            Ok(value) => value,
            Err(error) => return fault(&error.to_string()),
        };
        let mut auth = match self.auth.try_lock() {
            Ok(value) => value,
            Err(_) => return fault("controller_busy"),
        };
        let authority = match auth.authorize(&peer, &credential, Instant::now()) {
            Ok(value) => value,
            Err(error) => return fault(&error.to_string()),
        };
        match request {
            Request::ConfirmDomainChange { digest, .. } => {
                match self
                    .services
                    .confirm_domain(&authority, &digest, &self.actor)
                {
                    Ok(()) => success(json!({"confirmed":true})),
                    Err(error) => fault(control_code(error)),
                }
            }
            Request::ApproveServicePlan { digest, .. } => {
                match self.services.approve(&authority, &digest, &self.actor) {
                    Ok(receipt) => bounded_reply(&receipt),
                    Err(error) => fault(control_code(error)),
                }
            }
            Request::ExternalGrantIssue {
                project_id,
                scopes,
                ttl_ms,
                ..
            } => {
                match self.external.issue(
                    &authority,
                    &self.actor,
                    &project_id,
                    &scopes,
                    Duration::from_millis(ttl_ms),
                ) {
                    Ok(issued) => success(
                        json!({"record":issued.record,"credential":issued.credential.expose_secret()}),
                    ),
                    Err(error) => fault(control_code(error)),
                }
            }
            Request::ExternalGrantList { .. } => match self.external.list(&authority) {
                Ok(records) => bounded_reply(&records),
                Err(error) => fault(control_code(error)),
            },
            Request::ExternalGrantRevoke { grant_id, .. } => {
                match self.external.revoke(&authority, &grant_id) {
                    Ok(()) => {
                        self.services.revoked(&grant_id);
                        success(json!({"revoked":true}))
                    }
                    Err(error) => fault(control_code(error)),
                }
            }
            Request::ResolveSubject { selector, .. } => {
                match self.actor.resolve_subject(&authority, &selector) {
                    Ok(project_id) => success(json!({"project_id":project_id,"advisory":true})),
                    Err(error) => fault(error.code()),
                }
            }
            Request::OperationByKey {
                project_id,
                idempotency_key,
                ..
            } => match self
                .actor
                .operation_by_key(&authority, project_id, idempotency_key)
            {
                Ok(receipt) => bounded_reply(&receipt),
                Err(error) => fault(error.code()),
            },
            Request::DesktopRead {
                idempotency_key,
                request,
                ..
            } => match self
                .actor
                .desktop_read(&authority, idempotency_key, *request)
            {
                Ok(receipt) => bounded_reply(&receipt),
                Err(error) => fault(error.code()),
            },
            Request::DesktopAccess {
                idempotency_key,
                request,
                ..
            } => match self
                .actor
                .desktop_access(&authority, idempotency_key, *request)
            {
                Ok(receipt) => bounded_reply(&receipt),
                Err(error) => fault(error.code()),
            },
            Request::DiscoveryScan { .. } => match self.actor.discovery_request(&authority) {
                Ok(()) => success(json!({"requested":true})),
                Err(error) => fault(error.code()),
            },
            Request::Operations {
                project_id,
                cursor,
                limit,
                ..
            } => match self.actor.operations(&authority, project_id, cursor, limit) {
                Ok(page) => bounded_reply(&page),
                Err(error) => fault(error.code()),
            },
            Request::Snapshot { .. } => match self.actor.cached_snapshot(&authority) {
                Ok(view) => {
                    #[derive(Serialize)]
                    struct View<'a> {
                        snapshot: &'a devforge_workspace::Snapshot,
                        observed_at: u64,
                        busy: bool,
                        stopped: bool,
                        error: Option<&'static str>,
                        progress: Option<devforge_workspace::setup::Progress>,
                    }
                    bounded_reply(&View {
                        snapshot: &view.snapshot,
                        observed_at: view.observed_at,
                        busy: view.busy,
                        stopped: view.stopped,
                        error: view.error.map(|error| error.code()),
                        progress: view.progress,
                    })
                }
                Err(error) => fault(error.code()),
            },
            Request::Submit {
                project_id,
                idempotency_key,
                command,
                ..
            } => match self.actor.submit_desktop(
                &authority,
                OperationIdentity {
                    project_id,
                    idempotency_key,
                },
                *command,
            ) {
                Ok(value) => bounded_reply(&value),
                Err(error) => fault(error.code()),
            },
            Request::Status {
                project_id,
                operation_id,
                ..
            } => match self.actor.status(&authority, &project_id, &operation_id) {
                Ok(value) => bounded_reply(&value),
                Err(error) => fault(error.code()),
            },
            Request::Result {
                project_id,
                operation_id,
                ..
            } => match self.actor.result(&authority, &project_id, &operation_id) {
                Ok(value) => bounded_reply(&value),
                Err(error) => fault(error.code()),
            },
            Request::FailureDetail {
                project_id,
                operation_id,
                ..
            } => match self
                .actor
                .failure_detail(&authority, &project_id, &operation_id)
            {
                Ok(value) => bounded_reply(&value),
                Err(error) => fault(error.code()),
            },
            Request::Cancel {
                project_id,
                operation_id,
                ..
            } => match self.actor.cancel(&authority, &project_id, &operation_id) {
                Ok(value) => success(
                    json!({"cancellation":match value{crate::actor::CancelOutcome::Requested=>"requested",crate::actor::CancelOutcome::AlreadyTerminal=>"already_terminal"}}),
                ),
                Err(error) => fault(error.code()),
            },
            Request::Quit { .. } => {
                if let Err(error) = self.commit_quit(|| {
                    authority
                        .check_current(Instant::now())
                        .map_err(|_| Error::Unauthorized)
                }) {
                    return fault(error.code());
                }
                auth.revoke_all();
                success(json!({"state":"quitting","shutdown_id":self.shutdown_id}))
            }
            _ => fault("controller_invalid_request"),
        }
    }
}
fn success(data: Value) -> Value {
    json!({"ok":true,"data":data})
}
fn control_code(error: devforge_control::ErrorCode) -> &'static str {
    use devforge_control::ErrorCode as E;
    match error {
        E::Unauthorized => "controller_external_grant_unauthorized",
        E::InvalidInput => "controller_external_grant_invalid_input",
        E::Busy => "controller_busy",
        E::ProjectMismatch => "controller_external_grant_project_mismatch",
        _ => "controller_external_grant_unavailable",
    }
}
fn session_reply(
    auth: &mut DesktopAuth,
    peer: &PeerIdentity,
    session: &SessionCredential,
) -> Value {
    match auth.session_info(peer, session, Instant::now()) {
        Ok(info) => success(
            json!({"session":session.expose_secret(),"expires_in_ms":info.expires_in_ms,"context_remaining_ms":info.context_remaining_ms,"renewable":info.renewable}),
        ),
        Err(error) => fault(&error.to_string()),
    }
}
fn fault(code: &str) -> Value {
    json!({"ok":false,"error":{"code":code}})
}
fn bounded_reply<T: Serialize>(data: &T) -> Value {
    struct Count(usize);
    impl Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > (crate::MAX_RESPONSE_BYTES - 128).saturating_sub(self.0) {
                return Err(std::io::Error::other("reply limit"));
            }
            self.0 += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    if serde_json::to_writer(&mut Count(0), data).is_err() {
        return fault("controller_output_limit");
    }
    match serde_json::to_value(data) {
        Ok(value) => success(value),
        Err(_) => fault("controller_output_limit"),
    }
}
fn decode_hex(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut output = [0u8; 32];
    for (index, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let digit = |byte: u8| match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            _ => None,
        };
        output[index] = (digit(pair[0])? << 4) | digit(pair[1])?;
    }
    Some(output)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Inhibition {
    schema: u32,
    inhibited: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shutdown: Option<ShutdownRecord>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ShutdownRecord {
    owner: String,
    outcome: ShutdownOutcome,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ShutdownOutcome {
    Pending,
    Succeeded,
    Failed,
}
impl Default for Inhibition {
    fn default() -> Self {
        Self {
            schema: 1,
            inhibited: false,
            shutdown: None,
        }
    }
}
const INHIBITION: &str = "quit-inhibited.json";
fn inhibition(endpoint: &Endpoint) -> Result<bool> {
    Ok(read_inhibition(endpoint)?.inhibited)
}
fn read_inhibition(endpoint: &Endpoint) -> Result<Inhibition> {
    endpoint
        .validate()
        .map_err(|_| Error::PersistenceUnavailable)?;
    let path = endpoint.directory.join(INHIBITION);
    match fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Inhibition::default());
        }
        _ => return Err(Error::PersistenceUnavailable),
    }
    let mut options = OpenOptions::new();
    options.read(true);
    crate::transport::os::secure_lock_options(&mut options);
    let file = match options.open(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Inhibition::default());
        }
        Err(_) => return Err(Error::PersistenceUnavailable),
    };
    crate::transport::os::validate_lock(&file).map_err(|_| Error::PersistenceUnavailable)?;
    let mut bytes = Vec::new();
    file.take(1025)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::PersistenceUnavailable)?;
    if bytes.len() > 1024 {
        return Err(Error::PersistenceUnavailable);
    }
    let state: Inhibition =
        serde_json::from_slice(&bytes).map_err(|_| Error::PersistenceUnavailable)?;
    if state.schema != 1
        || state
            .shutdown
            .as_ref()
            .is_some_and(|value| decode_hex(&value.owner).is_none())
    {
        return Err(Error::PersistenceUnavailable);
    }
    Ok(state)
}
/// The lease is acquired here and retained throughout the read, excluding a
/// replacement owner until the bounded owner-specific outcome is confirmed.
pub(crate) fn confirm_shutdown(endpoint: &Endpoint, owner: &str) -> Result<()> {
    if decode_hex(owner).is_none() {
        return Err(Error::ShutdownUnconfirmed);
    }
    let _lease = AppLease::acquire(endpoint.root()).map_err(|error| {
        if error.kind() == std::io::ErrorKind::WouldBlock {
            Error::Busy
        } else {
            Error::PersistenceUnavailable
        }
    })?;
    let lifecycle = read_inhibition(endpoint)?;
    let record = lifecycle.shutdown.ok_or(Error::ShutdownUnconfirmed)?;
    if record.owner != owner {
        return Err(Error::ShutdownUnconfirmed);
    }
    match record.outcome {
        ShutdownOutcome::Succeeded => Ok(()),
        ShutdownOutcome::Failed => Err(Error::ShutdownFailed),
        ShutdownOutcome::Pending => Err(Error::ShutdownUnconfirmed),
    }
}
fn persist_inhibition(endpoint: &Endpoint, value: bool) -> Result<()> {
    persist_inhibition_checked(endpoint, value, || Ok(()))
}
fn persist_inhibition_checked<F: FnOnce() -> Result<()>>(
    endpoint: &Endpoint,
    value: bool,
    authorize: F,
) -> Result<()> {
    let mut state = read_inhibition(endpoint)?;
    state.inhibited = value;
    persist_lifecycle_checked(endpoint, &state, authorize)
}
fn persist_lifecycle_checked<F: FnOnce() -> Result<()>>(
    endpoint: &Endpoint,
    state: &Inhibition,
    authorize: F,
) -> Result<()> {
    endpoint
        .validate()
        .map_err(|_| Error::PersistenceUnavailable)?;
    let target = endpoint.directory.join(INHIBITION);
    if fs::symlink_metadata(&target).is_ok() {
        let _ = inhibition(endpoint)?;
    }
    let mut nonce = [0u8; 16];
    getrandom::fill(&mut nonce).map_err(|_| Error::PersistenceUnavailable)?;
    let name = nonce
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let temporary = endpoint.directory.join(format!("quit-{name}.tmp"));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).read(true).create_new(true);
        crate::transport::os::secure_lock_options(&mut options);
        let mut file = options
            .open(&temporary)
            .map_err(|_| Error::PersistenceUnavailable)?;
        crate::transport::os::validate_lock(&file).map_err(|_| Error::PersistenceUnavailable)?;
        serde_json::to_writer(&mut file, state).map_err(|_| Error::PersistenceUnavailable)?;
        file.sync_all().map_err(|_| Error::PersistenceUnavailable)?;
        drop(file);
        authorize()?;
        replace_marker(&temporary, &target)?;
        #[cfg(unix)]
        std::fs::File::open(&endpoint.directory)
            .and_then(|file| file.sync_all())
            .map_err(|_| Error::PersistenceUnavailable)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
#[cfg(not(windows))]
fn replace_marker(source: &Path, target: &Path) -> Result<()> {
    fs::rename(source, target).map_err(|_| Error::PersistenceUnavailable)
}

#[cfg(windows)]
fn replace_marker(source: &Path, target: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let target = target
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(Error::PersistenceUnavailable);
    }
    Ok(())
}

#[cfg(test)]
use crate::external_auth::test_support as quit_test_support;
#[cfg(test)]
mod service_quit_tests {
    use super::*;
    struct NoSecrets;
    impl SecretStore for NoSecrets {
        fn get(&self, _: &str) -> std::result::Result<Option<String>, devforge_core::ErrorCode> {
            panic!("no native credential probe")
        }
        fn set(&self, _: &str, _: &str) -> std::result::Result<(), devforge_core::ErrorCode> {
            panic!("no native credential write")
        }
        fn delete(&self, _: &str) -> std::result::Result<(), devforge_core::ErrorCode> {
            panic!("no native credential delete")
        }
    }
    #[test]
    fn service_admission_obeys_existing_quit_gate() {
        let root = quit_test_support::root("df-service-quit-");
        let controller = Controller::start(
            root.path(),
            StartIntent::AutoStart,
            Stores {
                workspace: Arc::new(NoSecrets),
                desktop: Arc::new(NoSecrets),
            },
        )
        .unwrap();
        let peer = PeerIdentity {
            user: local_owner_user().unwrap(),
            process_id: Some(std::process::id()),
        };
        let wire = json!({"method":"service_apply","credential":"0".repeat(64),"review":"0".repeat(64),"request":{"api_version":1,"request_id":"quit-race","project_id":"fixture","expected_revision":1,"idempotency_key":"quit-key","input":{"tool":"services.stop","arguments":{}}}});
        let gate = controller.context.quit_gate.lock().unwrap();
        let busy = controller.context.request(peer.clone(), wire.clone());
        assert_eq!(busy["error"]["code"], "busy");
        drop(gate);
        controller.quitter().request_explicit().unwrap();
        let quitting = controller.context.request(peer, wire);
        assert_eq!(quitting["error"]["code"], "interrupted");
        controller.serve(None).unwrap();
    }
}
