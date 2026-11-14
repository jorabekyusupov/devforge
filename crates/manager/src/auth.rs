//! Desktop-only capability policy, independent of external project grants.
//!
//! `PeerIdentity` MUST come from the transport's OS authentication, and the owner
//! user passed to `new` MUST come from trusted OS context, never request JSON.
//! Same-user IPC is not desktop authority. Only an explicit trusted desktop
//! action may rotate/revoke the desktop key; neither operation is an RPC tool.
//!
//! All methods accepting `SecretStore` are BLOCKING: call them on a dedicated
//! authentication worker, never inside `Server::run`'s synchronous handler/current
//! thread event loop. Construction and session authorization never probe the
//! credential service. No production store, GUI route or tool handler is wired
//! by this module. This is not a sandbox against another same-user process with
//! equivalent credential/process access.
//!
//! A key changed by another trusted process is detected at the next permitted
//! store-backed authentication, invalidating old sessions. Cache-only checks
//! cannot observe that change earlier; sessions may survive until that check or
//! their fixed expiry. Explicit renewable native GUI contexts have a separate
//! absolute deadline (default12h, maximum24h); renewal can extend the unseen-change
//! window up to that deadline. Immediate cross-process revocation needs an owner
//! notification and is not established by cache-only verification.
use crate::{Endpoint, PeerIdentity};
use devforge_core::SecretStore;
use sha2::{Digest, Sha256};
use std::{
    fmt,
    path::Path,
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthError {
    InvalidPolicy,
    UnsafeEndpoint,
    Unauthorized,
    NotProvisioned,
    SecretUnavailable,
    EntropyUnavailable,
    RateLimited,
    SessionCapacity,
    BootstrapUnavailable,
}
impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidPolicy => "manager_auth_invalid_policy",
            Self::UnsafeEndpoint => "manager_auth_unsafe_endpoint",
            Self::Unauthorized => "manager_auth_unauthorized",
            Self::NotProvisioned => "manager_auth_not_provisioned",
            Self::SecretUnavailable => "manager_auth_secret_unavailable",
            Self::EntropyUnavailable => "manager_auth_entropy_unavailable",
            Self::RateLimited => "manager_auth_rate_limited",
            Self::SessionCapacity => "manager_auth_session_capacity",
            Self::BootstrapUnavailable => "manager_auth_bootstrap_unavailable",
        })
    }
}
impl std::error::Error for AuthError {}
pub type Result<T> = std::result::Result<T, AuthError>;

#[derive(Clone, Copy, Debug)]
pub struct Policy {
    pub max_sessions: usize,
    pub session_lifetime: Duration,
    /// Minimum interval between ALL store-backed attempts, including success.
    pub store_interval: Duration,
    /// Failed/denied/unavailable store access gets a longer cooldown.
    pub failure_cooldown: Duration,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            max_sessions: 8,
            session_lifetime: Duration::from_secs(600),
            store_interval: Duration::from_secs(5),
            failure_cooldown: Duration::from_secs(30),
        }
    }
}
impl Policy {
    fn validate(self) -> Result<Self> {
        if !(1..=32).contains(&self.max_sessions)
            || self.session_lifetime.is_zero()
            || self.session_lifetime > Duration::from_secs(3600)
            || self.store_interval < Duration::from_secs(1)
            || self.store_interval > Duration::from_secs(300)
            || self.failure_cooldown < Duration::from_secs(5)
            || self.failure_cooldown < self.store_interval
            || self.failure_cooldown > Duration::from_secs(300)
        {
            return Err(AuthError::InvalidPolicy);
        }
        Ok(self)
    }
}

/// Server-owned policy for explicit native GUI contexts, never request JSON.
/// Ordinary `authenticate` sessions remain fixed and nonrenewable.
#[derive(Clone, Copy, Debug)]
pub struct NativeGuiPolicy {
    pub bootstrap_lifetime: Duration,
    pub context_lifetime: Duration,
}

/// Cache-only lifetime metadata; contains no credential or authority proof.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionInfo {
    pub expires_in_ms: u64,
    pub context_remaining_ms: Option<u64>,
    pub renewable: bool,
}
impl Default for NativeGuiPolicy {
    fn default() -> Self {
        Self {
            bootstrap_lifetime: Duration::from_secs(30),
            context_lifetime: Duration::from_secs(12 * 3600),
        }
    }
}
impl NativeGuiPolicy {
    fn validate(self) -> Result<Self> {
        if self.bootstrap_lifetime.is_zero()
            || self.bootstrap_lifetime > Duration::from_secs(120)
            || self.context_lifetime.is_zero()
            || self.context_lifetime > Duration::from_secs(24 * 3600)
        {
            return Err(AuthError::InvalidPolicy);
        }
        Ok(self)
    }
}

/// Private inherited-channel credential, distinct from the native-store key.
/// No serde/Clone and no token-bearing Debug. Its instance/root fields bind the
/// one-use capability to this particular newly ready manager, not another owner.
pub struct BootstrapCredential {
    pub(crate) token: Zeroizing<String>,
    pub(crate) root: [u8; 32],
    pub(crate) instance: [u8; 32],
}
impl BootstrapCredential {
    /// Bounded private authentication adapter only; parsing is not authority.
    pub fn from_wire(token: &str, root: [u8; 32], instance: [u8; 32]) -> Result<Self> {
        if !valid_token(token) {
            return Err(AuthError::Unauthorized);
        }
        Ok(Self {
            token: Zeroizing::new(token.to_owned()),
            root,
            instance,
        })
    }
    /// Secret transport boundary only; never normal operation output or logs.
    pub fn expose_secret(&self) -> &str {
        &self.token
    }
    pub fn manager_instance(&self) -> &[u8; 32] {
        &self.instance
    }
    pub fn root_fingerprint(&self) -> &[u8; 32] {
        &self.root
    }
}
impl fmt::Debug for BootstrapCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("BootstrapCredential([REDACTED])")
    }
}
struct PendingBootstrap {
    credential: BootstrapCredential,
    generation: u64,
    expires: Instant,
    context_lifetime: Duration,
}

// The API permits trusted virtual time in fixtures. Add real monotonic elapsed
// blocking time rather than anchoring deadlines to the pre-prompt timestamp.
fn completed_at(now: Instant, started: Instant) -> Result<Instant> {
    now.checked_add(started.elapsed())
        .ok_or(AuthError::InvalidPolicy)
}

fn valid_token(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn random_token() -> Result<Zeroizing<String>> {
    let mut bytes = Zeroizing::new([0u8; 32]);
    getrandom::fill(bytes.as_mut()).map_err(|_| AuthError::EntropyUnavailable)?;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut token = Zeroizing::new(String::with_capacity(64));
    for byte in bytes.iter() {
        token.push(HEX[(byte >> 4) as usize] as char);
        token.push(HEX[(byte & 15) as usize] as char);
    }
    Ok(token)
}
macro_rules! credential {
    ($name:ident) => {
        /// Opaque 256-bit credential. No serialization, cloning or secret Debug.
        pub struct $name(Zeroizing<String>);
        impl $name {
            /// A transport adapter may parse a bounded secret, but parsing grants
            /// no authority. The server must authenticate/authorize it separately.
            pub fn from_wire(value: &str) -> Result<Self> {
                if !valid_token(value) {
                    return Err(AuthError::Unauthorized);
                }
                Ok(Self(Zeroizing::new(value.to_owned())))
            }
            /// Use only at the trusted credential/transport boundary. Never log.
            pub fn expose_secret(&self) -> &str {
                &self.0
            }
        }
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($name), "([REDACTED])"))
            }
        }
    };
}
credential!(DesktopCredential);
credential!(SessionCredential);

/// In-process marker supplied only by the trusted desktop composition boundary.
/// It has no wire decoder: an incoming `role`/`confirm` field cannot create it.
pub struct TrustedDesktopAction {
    _private: (),
}
impl TrustedDesktopAction {
    /// Call only after an explicit native desktop provisioning/revocation action,
    /// never from startup, a generic IPC handler, or an external-agent request.
    pub fn from_explicit_desktop_action() -> Self {
        Self { _private: () }
    }
}

struct Session {
    token: Zeroizing<String>,
    root: [u8; 32],
    peer: PeerIdentity,
    generation: u64,
    expires: Instant,
    native_deadline: Option<Instant>,
}

/// An in-process proof, created only after verifying a server-issued session.
/// This is not a serializable role, external project grant or client claim.
pub struct DesktopAuthority<'a> {
    root: &'a Path,
    peer: &'a PeerIdentity,
    expires: Instant,
}
impl DesktopAuthority<'_> {
    pub fn root(&self) -> &Path {
        self.root
    }
    pub fn peer(&self) -> &PeerIdentity {
        self.peer
    }
    /// Recheck at admission/consumption using trusted monotonic time. Holding a
    /// proof does not extend its lifetime or authorize later queued execution.
    pub fn check_current(&self, now: Instant) -> Result<()> {
        if now < self.expires {
            Ok(())
        } else {
            Err(AuthError::Unauthorized)
        }
    }
}
impl fmt::Debug for DesktopAuthority<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DesktopAuthority([REDACTED])")
    }
}

/// One manager owner/root's bounded desktop session cache. Move this policy to
/// the authentication worker; ordinary session checks never touch SecretStore.
pub struct DesktopAuth {
    endpoint: Endpoint,
    owner_user: String,
    root: [u8; 32],
    key: String,
    policy: Policy,
    sessions: Vec<Session>,
    next_store_attempt: Option<Instant>,
    generation: u64,
    disabled: bool,
    key_fingerprint: Option<Zeroizing<[u8; 32]>>,
    manager_instance: [u8; 32],
    pending_bootstrap: Option<PendingBootstrap>,
    bootstrap_issued: bool,
}
impl fmt::Debug for DesktopAuth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DesktopAuth")
            .field("active_sessions", &self.sessions.len())
            .field("disabled", &self.disabled)
            .finish_non_exhaustive()
    }
}
impl DesktopAuth {
    /// `owner_user` is kernel-derived composition context, NOT a wire field.
    /// Does filesystem validation only; never reads/creates native credentials.
    pub fn new(endpoint: Endpoint, owner_user: String, policy: Policy) -> Result<Self> {
        endpoint.validate().map_err(|_| AuthError::UnsafeEndpoint)?;
        if owner_user.is_empty() || owner_user.len() > 256 {
            return Err(AuthError::InvalidPolicy);
        }
        let mut hash = Sha256::new();
        hash.update(b"devforge.desktop.auth.v1\0");
        hash.update(owner_user.as_bytes());
        hash.update(b"\0");
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            hash.update(endpoint.root().as_os_str().as_bytes());
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            for unit in endpoint.root().as_os_str().encode_wide() {
                hash.update(unit.to_le_bytes());
            }
        }
        let root: [u8; 32] = hash.finalize().into();
        let suffix = root.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let mut manager_instance = [0u8; 32];
        getrandom::fill(&mut manager_instance).map_err(|_| AuthError::EntropyUnavailable)?;
        Ok(Self {
            endpoint,
            owner_user,
            root,
            key: format!("control/desktop/v1/{suffix}"),
            policy: policy.validate()?,
            sessions: Vec::new(),
            next_store_attempt: None,
            generation: 0,
            disabled: false,
            key_fingerprint: None,
            manager_instance,
            pending_bootstrap: None,
            bootstrap_issued: false,
        })
    }
    fn cooldown(&mut self, now: Instant, duration: Duration) -> Result<()> {
        self.next_store_attempt = Some(now.checked_add(duration).ok_or(AuthError::InvalidPolicy)?);
        Ok(())
    }
    /// Trusted native provider boundary only, after an explicit UI/user action.
    /// BLOCKING: credential worker, never startup/polling or a wire dispatcher.
    /// Missing stays None, malformed/denied stays an error: this never creates,
    /// adopts, rotates or repairs a key and does not grant a session by itself.
    pub fn read_if_existing(
        &self,
        _action: &TrustedDesktopAction,
        store: &dyn SecretStore,
    ) -> Result<Option<DesktopCredential>> {
        let Some(value) = store
            .get(&self.key)
            .map_err(|_| AuthError::SecretUnavailable)?
        else {
            return Ok(None);
        };
        let value = Zeroizing::new(value);
        if !valid_token(&value) {
            return Err(AuthError::SecretUnavailable);
        }
        Ok(Some(DesktopCredential(value)))
    }
    /// Explicit trusted desktop first-action/rotation only; NEVER expose as a
    /// client-selected role or provisioning RPC. BLOCKING: authentication worker.
    /// Generates the credential internally, invalidates all sessions before the
    /// write, and fails closed locally if the store refuses the change.
    pub fn rotate_desktop_key(
        &mut self,
        _action: &TrustedDesktopAction,
        store: &dyn SecretStore,
        now: Instant,
    ) -> Result<DesktopCredential> {
        self.revoke_all();
        self.disabled = true;
        self.key_fingerprint = None;
        self.cooldown(now, self.policy.failure_cooldown)?;
        let started = Instant::now();
        let result = (|| {
            let token = random_token()?;
            store
                .set(&self.key, &token)
                .map_err(|_| AuthError::SecretUnavailable)?;
            self.key_fingerprint = Some(Zeroizing::new(Sha256::digest(token.as_bytes()).into()));
            Ok(DesktopCredential(token))
        })();
        self.cooldown(completed_at(now, started)?, self.policy.failure_cooldown)?;
        if result.is_ok() {
            self.disabled = false;
            self.next_store_attempt = None;
        }
        result
    }
    /// Explicit trusted action. BLOCKING: authentication worker. Sessions are
    /// revoked even if deletion fails; an error means persistent revocation has
    /// NOT been established and must not be reported as successful across restart.
    pub fn revoke_desktop_key(
        &mut self,
        _action: &TrustedDesktopAction,
        store: &dyn SecretStore,
        now: Instant,
    ) -> Result<()> {
        self.revoke_all();
        self.disabled = true;
        self.key_fingerprint = None;
        self.cooldown(now, self.policy.failure_cooldown)?;
        let started = Instant::now();
        let result = store
            .delete(&self.key)
            .map_err(|_| AuthError::SecretUnavailable);
        self.cooldown(completed_at(now, started)?, self.policy.failure_cooldown)?;
        result
    }
    /// Verify an EXISTING desktop key; never creates a key or accepts a requested
    /// role. BLOCKING: authentication worker, not the synchronous Server handler.
    /// `now` is trusted monotonic time, never deserialized from a request.
    pub fn authenticate(
        &mut self,
        peer: &PeerIdentity,
        credential: &DesktopCredential,
        store: &dyn SecretStore,
        now: Instant,
    ) -> Result<SessionCredential> {
        if self.disabled || peer.user != self.owner_user {
            return Err(AuthError::Unauthorized);
        }
        if self.next_store_attempt.is_some_and(|until| now < until) {
            return Err(AuthError::RateLimited);
        }
        self.sessions.retain(|s| now < s.expires);
        // Set before touching the store, including denied/unavailable prompts.
        self.cooldown(now, self.policy.failure_cooldown)?;
        let started = Instant::now();
        // Keep finalization outside the fallible body: unavailable/denied store
        // errors and every other outcome re-arm cooldown AFTER blocking work.
        let result = (|| {
            let stored = store
                .get(&self.key)
                .map_err(|_| AuthError::SecretUnavailable)?;
            let Some(stored) = stored else {
                self.revoke_all();
                self.key_fingerprint = None;
                return Err(AuthError::NotProvisioned);
            };
            let stored = Zeroizing::new(stored);
            if !valid_token(&stored) {
                self.revoke_all();
                self.key_fingerprint = None;
                return Err(AuthError::SecretUnavailable);
            }
            let fingerprint = Zeroizing::new(<[u8; 32]>::from(Sha256::digest(stored.as_bytes())));
            if self.key_fingerprint.as_ref().is_none_or(|previous| {
                !bool::from(previous.as_slice().ct_eq(fingerprint.as_slice()))
            }) {
                self.revoke_all();
                self.key_fingerprint = Some(fingerprint);
            }
            if !bool::from(stored.as_bytes().ct_eq(credential.0.as_bytes())) {
                return Err(AuthError::Unauthorized);
            }
            // Drop entries that expired while a credential prompt was open.
            let issued_at = completed_at(now, started)?;
            self.sessions.retain(|s| issued_at < s.expires);
            if self.sessions.len() >= self.policy.max_sessions {
                return Err(AuthError::SessionCapacity);
            }
            let token = random_token()?;
            let expires = completed_at(now, started)?
                .checked_add(self.policy.session_lifetime)
                .ok_or(AuthError::InvalidPolicy)?;
            self.sessions.push(Session {
                token: Zeroizing::new(token.to_string()),
                root: self.root,
                peer: peer.clone(),
                generation: self.generation,
                expires,
                native_deadline: None,
            });
            Ok(SessionCredential(token))
        })();
        let interval = if result.is_ok() {
            self.policy.store_interval
        } else {
            self.policy.failure_cooldown
        };
        self.cooldown(completed_at(now, started)?, interval)?;
        result
    }

    /// Explicit native-GUI fallback for an existing manager. This invokes the
    /// normal throttled BLOCKING native-store verification first. Only this
    /// server-owned route grants a renewable context; ordinary auth stays fixed.
    pub fn authenticate_native_gui(
        &mut self,
        peer: &PeerIdentity,
        credential: &DesktopCredential,
        store: &dyn SecretStore,
        policy: NativeGuiPolicy,
        now: Instant,
    ) -> Result<SessionCredential> {
        let policy = policy.validate()?;
        let started = Instant::now();
        let token = self.authenticate(peer, credential, store, now)?;
        let deadline = completed_at(now, started)?
            .checked_add(policy.context_lifetime)
            .ok_or(AuthError::InvalidPolicy)?;
        let session = self
            .sessions
            .iter_mut()
            .find(|s| bool::from(s.token.as_bytes().ct_eq(token.0.as_bytes())))
            .ok_or(AuthError::Unauthorized)?;
        session.native_deadline = Some(deadline);
        session.expires = session.expires.min(deadline);
        Ok(token)
    }
    /// Only the private new-owner composition path can supply this consumed
    /// readiness permit. Never expose arbitrary IPC/bootstrap issuance.
    pub fn issue_private_bootstrap(
        &mut self,
        permit: crate::bootstrap::ReadyPermit,
        policy: NativeGuiPolicy,
        now: Instant,
    ) -> Result<BootstrapCredential> {
        let policy = policy.validate()?;
        if self.disabled || self.bootstrap_issued || permit.root() != self.endpoint.root() {
            return Err(AuthError::BootstrapUnavailable);
        }
        let started = Instant::now();
        let credential = BootstrapCredential {
            token: random_token()?,
            root: crate::bootstrap::root_fingerprint(self.endpoint.root()),
            instance: self.manager_instance,
        };
        let expires = completed_at(now, started)?
            .checked_add(policy.bootstrap_lifetime)
            .ok_or(AuthError::InvalidPolicy)?;
        self.pending_bootstrap = Some(PendingBootstrap {
            credential: BootstrapCredential {
                token: Zeroizing::new(credential.token.to_string()),
                root: credential.root,
                instance: credential.instance,
            },
            generation: self.generation,
            expires,
            context_lifetime: policy.context_lifetime,
        });
        self.bootstrap_issued = true;
        Ok(credential)
    }
    /// Redeem a private-channel credential once, after OS peer authentication.
    /// No native-store access. Invalid peer/root/instance/token never consumes a
    /// valid pending credential; expiry/revoke makes it permanently unavailable.
    pub fn redeem_private_bootstrap(
        &mut self,
        peer: &PeerIdentity,
        credential: &BootstrapCredential,
        now: Instant,
    ) -> Result<SessionCredential> {
        let started = Instant::now();
        if self
            .pending_bootstrap
            .as_ref()
            .is_some_and(|p| now >= p.expires)
        {
            self.pending_bootstrap = None;
        }
        if self.disabled || peer.user != self.owner_user {
            return Err(AuthError::Unauthorized);
        }
        let pending = self
            .pending_bootstrap
            .as_ref()
            .ok_or(AuthError::BootstrapUnavailable)?;
        if pending.generation != self.generation
            || credential.root != pending.credential.root
            || credential.instance != self.manager_instance
            || !bool::from(
                pending
                    .credential
                    .token
                    .as_bytes()
                    .ct_eq(credential.token.as_bytes()),
            )
        {
            return Err(AuthError::Unauthorized);
        }
        self.sessions.retain(|s| now < s.expires);
        if self.sessions.len() >= self.policy.max_sessions {
            return Err(AuthError::SessionCapacity);
        }
        let pending_expires = pending.expires;
        let context_lifetime = pending.context_lifetime;
        let token = random_token()?;
        let issued = completed_at(now, started)?;
        if issued >= pending_expires {
            self.pending_bootstrap = None;
            return Err(AuthError::BootstrapUnavailable);
        }
        let deadline = issued
            .checked_add(context_lifetime)
            .ok_or(AuthError::InvalidPolicy)?;
        let expires = issued
            .checked_add(self.policy.session_lifetime)
            .ok_or(AuthError::InvalidPolicy)?
            .min(deadline);
        self.sessions.push(Session {
            token: Zeroizing::new(token.to_string()),
            root: self.root,
            peer: peer.clone(),
            generation: self.generation,
            expires,
            native_deadline: Some(deadline),
        });
        self.pending_bootstrap = None;
        Ok(SessionCredential(token))
    }
    /// Explicit renewal only, not sliding ordinary usage. The token must still
    /// be valid for this peer/root/generation and a native context. Rotation is
    /// atomic after entropy succeeds and never extends the absolute deadline.
    /// No SecretStore access; external key changes may remain unseen until this
    /// context's hard deadline unless the owner is explicitly notified.
    pub fn renew_native_gui(
        &mut self,
        peer: &PeerIdentity,
        credential: &SessionCredential,
        now: Instant,
    ) -> Result<SessionCredential> {
        let started = Instant::now();
        self.sessions.retain(|s| now < s.expires);
        if self.disabled || peer.user != self.owner_user {
            return Err(AuthError::Unauthorized);
        }
        let session = self
            .sessions
            .iter_mut()
            .find(|s| {
                bool::from(s.token.as_bytes().ct_eq(credential.0.as_bytes()))
                    && s.root == self.root
                    && s.generation == self.generation
                    && s.peer == *peer
            })
            .ok_or(AuthError::Unauthorized)?;
        let deadline = session.native_deadline.ok_or(AuthError::Unauthorized)?;
        if now >= deadline {
            return Err(AuthError::Unauthorized);
        }
        let token = random_token()?;
        let issued = completed_at(now, started)?;
        if issued >= session.expires || issued >= deadline {
            return Err(AuthError::Unauthorized);
        }
        let expires = issued
            .checked_add(self.policy.session_lifetime)
            .ok_or(AuthError::InvalidPolicy)?
            .min(deadline);
        session.token = Zeroizing::new(token.to_string());
        session.expires = expires;
        Ok(SessionCredential(token))
    }

    /// Nonblocking cache-only check. Bind the complete kernel-observed identity;
    /// current Darwin transport combines getpeereid with LOCAL_PEERPID.
    pub fn authorize<'a>(
        &'a mut self,
        peer: &PeerIdentity,
        credential: &SessionCredential,
        now: Instant,
    ) -> Result<DesktopAuthority<'a>> {
        self.sessions.retain(|s| now < s.expires);
        if self.disabled || peer.user != self.owner_user {
            return Err(AuthError::Unauthorized);
        }
        let session = self
            .sessions
            .iter()
            .find(|s| {
                bool::from(s.token.as_bytes().ct_eq(credential.0.as_bytes()))
                    && s.root == self.root
                    && s.generation == self.generation
                    && s.peer == *peer
            })
            .ok_or(AuthError::Unauthorized)?;
        Ok(DesktopAuthority {
            root: self.endpoint.root(),
            peer: &session.peer,
            expires: session.expires,
        })
    }
    /// Verify the same root/peer/generation/expiry as authorization, then return
    /// conservative whole-millisecond remaining lifetimes. Never probes a store.
    pub fn session_info(
        &mut self,
        peer: &PeerIdentity,
        credential: &SessionCredential,
        now: Instant,
    ) -> Result<SessionInfo> {
        self.authorize(peer, credential, now)?;
        let session = self
            .sessions
            .iter()
            .find(|s| {
                bool::from(s.token.as_bytes().ct_eq(credential.0.as_bytes()))
                    && s.peer == *peer
                    && s.root == self.root
                    && s.generation == self.generation
            })
            .ok_or(AuthError::Unauthorized)?;
        Ok(SessionInfo {
            expires_in_ms: u64::try_from(
                session.expires.saturating_duration_since(now).as_millis(),
            )
            .map_err(|_| AuthError::InvalidPolicy)?,
            context_remaining_ms: session
                .native_deadline
                .map(|deadline| {
                    u64::try_from(deadline.saturating_duration_since(now).as_millis())
                        .map_err(|_| AuthError::InvalidPolicy)
                })
                .transpose()?,
            renewable: session.native_deadline.is_some(),
        })
    }
    /// Cache-only revocation. No credential-service access or global grant change.
    pub fn revoke_session(
        &mut self,
        peer: &PeerIdentity,
        credential: &SessionCredential,
    ) -> Result<()> {
        let before = self.sessions.len();
        self.sessions.retain(|s| {
            !(bool::from(s.token.as_bytes().ct_eq(credential.0.as_bytes())) && s.peer == *peer)
        });
        if self.sessions.len() == before {
            Err(AuthError::Unauthorized)
        } else {
            Ok(())
        }
    }
    pub fn revoke_all(&mut self) {
        self.sessions.clear();
        self.pending_bootstrap = None;
        self.generation = self.generation.wrapping_add(1);
    }
}
