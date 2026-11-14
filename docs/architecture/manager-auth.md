# M13.1.2: desktop capability policy

Status: credential-policy module and fixture acceptance only. This extends the
[manager infrastructure](manager-lifecycle.md), not the desktop composition or
public tool dispatcher. It does not expose Workspace commands, project grants,
MCP tools, a GUI authentication flow, or a production credential-service adapter.
The [control-plane contract](control-plane.md) continues to govern external
project-scoped permissions.

## Decision and trusted boundary

OS peer authentication establishes a user identity, not a desktop role. The
`auth` module adds a separate 256-bit desktop credential and server-generated
256-bit session credentials. A successful check produces an opaque in-process
`DesktopAuthority`; request fields such as `role: desktop`, `client_id`, or
`confirm: true` cannot construct that authority.

`DesktopAuth::new(endpoint, owner_user, policy)` validates the already resolved
private endpoint and binds the policy to its canonical data root and trusted OS
owner identity. `owner_user` must be supplied by the composition root from OS
context, never copied from request JSON. The endpoint/root and owner are hashed
using SHA-256 into the SecretStore key identity; no raw project path is used as an
authority claim. Construction never reads or creates a credential.

Only an explicit trusted native desktop action may construct the
`TrustedDesktopAction` marker and call `rotate_desktop_key` or
`revoke_desktop_key`. The marker has no wire decoder. It marks the composition
boundary; it does not itself implement a GUI consent dialog or establish that an
arbitrary caller is trustworthy. These APIs must never be put in a generic
client-selectable dispatcher. In particular, a missing key during server
authentication is `manager_auth_not_provisioned`, not a provisioning request.
Provisioning never adopts a token supplied by a client: cryptographic randomness
is generated internally with `getrandom`.

The existing `devforge_core::SecretStore` contract is injected at the explicit
trusted boundary. This slice does not instantiate `NativeSecretStore`. A later
desktop flow must use the matching native credential namespace and perform
first-use provisioning or rotation only after its explicit user action.

## Session and secret handling

- Credentials are opaque, non-serializable types with redacted `Debug`. Explicit
  secret access is restricted by convention to the credential/transport adapter.
  That future adapter must also handle and redact its original wire buffers.
- Tokens, random source bytes, cached session secrets and key fingerprints use
  zeroizing storage. Fixed-length valid credentials use `subtle` constant-time
  comparison. Invalid shape is rejected before any credential-store lookup.
- A desktop key and session token are distinct types and independent random
  values. Possessing the persistent key is not itself a valid session token.
- The session cache defaults to eight entries and ten minutes of fixed lifetime.
  Policy validates a hard maximum of32 entries and one hour; expiry is not
  extended by use. Expired entries are dropped before authorization/authentication.
- Sessions bind the root, current in-memory generation and complete
  kernel-observed `PeerIdentity`. Windows/Linux can bind the observed process ID;
  current Darwin `getpeereid` provides UID only. This slice does not claim Darwin
  process identity or defense against another same-user process with equivalent
  credential/process access.
- Individual and whole-cache revocation do not alter external grants. Restart
  drops session authority; a persistent desktop key must be authenticated again.

## Store access, throttling and responsiveness

Every method accepting `SecretStore` is blocking and belongs on a dedicated
authentication worker. It must **not** be invoked in `Server::run`'s synchronous
handler on the current-thread async event loop. The current transport timeout
cannot preempt a synchronous OS credential prompt, and dropping a caller future
does not cancel the underlying credential API. A later worker/composition layer
must keep bounded enqueue/status/cancellation separate and report an unavailable
or still-pending credential operation truthfully.

Authentication never writes or creates a missing secret. All store-backed
authentication attempts have a root-owner cooldown: five seconds after success,
thirty seconds after mismatch, unavailable/denied store access, missing or invalid
stored key, or other incomplete authentication. The cooldown is set before
calling the store. Different self-reported client names therefore cannot bypass
it. Foreign OS identities are rejected before store access. Policy intervals are
bounded and cannot be disabled. Explicit trusted rotate/revoke APIs are not
untrusted authentication attempts and are not wire-exposed.

The pre-access guard is only an initial reservation. Every success/error path
re-arms its final interval after the blocking call, adding measured monotonic
elapsed time to the trusted invocation time. Session expiry is likewise anchored
to issuance after the store returns, and capacity drops sessions that expired
during a prompt. A slow credential prompt cannot consume the later cooldown or
the newly issued session's lifetime. Rotation and deletion also finalize their
failure cooldown after blocking work; their failed policy instance stays disabled.

Ordinary session authorization and cache revocation never touch SecretStore.
This reduces repeated native prompts; it does not prove native prompt behavior
until the production composition is exercised on each target.

## Rotation and revocation semantics

Local trusted rotation/revocation clears sessions and advances the generation
before the store operation. A failed write/delete leaves this policy instance
disabled, so its previous sessions and persistent key cannot continue through
it. An error means persistent completion has **not** been established: if a
delete failed, a fresh manager can still observe the old key in the store.

A separate trusted desktop process can rotate the same root's key. On the next
permitted store-backed authentication the manager compares its fingerprint and
invalidates old sessions before accepting the new credential, even if the new
attempt itself supplied an old/incorrect key. Missing or malformed stored keys
also invalidate the cache when observed. No invalid stored token is adopted or
automatically repaired.

Cache-only checks cannot see a change made by another process. Existing sessions
can remain valid until the next permitted store-backed check or their fixed
expiry. Immediate cross-process revoke requires an authenticated owner
notification in the future composition; it is not implemented or claimed here.

## Evidence and remaining gates

On local macOS ARM64, the focused `cargo test -p devforge-manager --test auth`
run passed13/13 fixture tests. These cover startup without credential probing,
missing/malformed keys without creation/adoption, root and peer binding,
key/session separation, invalid/denied-store throttling, bounded capacity and
fixed expiry, local and separately issued rotation, individual/persistent revoke,
failed durable deletion and restart semantics, redacted diagnostics, and unsafe
endpoint/policy rejection. Tests use only in-memory and deny-store fixtures plus
isolated temporary owner roots. They do not open Keychain/Credential Manager,
change system permissions, or start a Workspace/GUI.
The scoped `cargo clippy -p devforge-manager --all-targets -- -D warnings` check
also passed. Its all-targets flag covers this host's Cargo targets, not native
execution or cross-compilation for another operating system.

Native credential prompts and their first-action UX remain unverified on macOS
and Windows. Windows compilation/native peer/session behavior and minimum-OS
acceptance require separate target evidence. Production worker scheduling,
desktop/manager cross-process provisioning, owner notification for immediate
revoke, external grants, and full desktop command routing remain later slices.
M15 credential/system-dialog, signing and distribution gates are unchanged.

QA found and corrected pre-prompt deadline anchoring. Two new delayed-memory-store
regressions use only20ms fixture sleeps plus trusted virtual time: denied/mismatch
paths retain their complete post-return failure cooldown, while successful auth
retains its complete interval and fixed session lifetime. Earlier immediate-store
fixtures now leave margins around completion-anchored deadlines. This remains
fixture evidence, not an actual native credential-dialog timing test.
