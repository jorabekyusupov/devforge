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
//! their fixed expiry. Immediate cross-process revocation needs owner notification
//! in the future composition layer and is NOT established here.
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
        })
    }
    fn cooldown(&mut self, now: Instant, duration: Duration) -> Result<()> {
        self.next_store_attempt = Some(now.checked_add(duration).ok_or(AuthError::InvalidPolicy)?);
        Ok(())
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

    /// Nonblocking cache-only check. Peer identity binds PID when the platform
    /// supplies it; Darwin getpeereid currently binds UID only, not process identity.
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
        self.generation = self.generation.wrapping_add(1);
    }
}
