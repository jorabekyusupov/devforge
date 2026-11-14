# M13.1.4: private owner controller and readiness composition

This slice composes the real manager Owner, one WorkspaceActor, authenticated
local IPC and desktop capability policy. It does not migrate the existing GUI,
expose external-agent grants/tools, install a daemon or certify native keychain
behavior. The current mode is explicitly `LegacyStandalone`.

## Entry points and exclusion

`Controller::start(root, StartIntent, Stores)` derives the owner user from the OS,
acquires ManagerLease and the legacy desktop AppLease, then creates one actor and
an auth policy. Both injected SecretStores are constructed without probing them.
Legacy AppLease exclusion lasts through actor cleanup, preventing the old
in-process desktop and this new Workspace owner from running against one root.
The GUI remains unchanged until its real client migration is accepted.

`LegacyExclusion::acquire(root)` is the transitional raw-CLI guard. It holds only
the same manager.lock and cannot create a WorkspacePermit. Legacy command modules
retain their original OperationLease policy. In particular, config setup/secret
can acquire their required exclusive lease without a self-held shared lease.
Both guard and owner reuse the same root/control/regular-file/ownership checks.
Socket path-length checks apply to actual manager endpoints, not an offline
legacy guard that never creates a listener.

`Controller::serve(Some(BootstrapWriter))` is the private cold-launch path. The
writer is captured only from a validated inherited pipe. The OS listener first
binds and mints an unforgeable crate-private ListenerReady; ReadyPermit combines
it with the actual initialized, nonstopping actor and matching root. Only then
can the writer issue its one-use root/instance credential. A supplied boolean,
client role or persisted PID cannot establish readiness. Ordinary serve has no
writer and never emits a token to console stdout. Busy existing-owner acquisition
never gets a new readiness capability.

The listener readiness hook and trusted Quit share a gate: a completed Quit
commit cannot be followed by bootstrap issuance. Explicit Quit before serve
returns through cleanup without binding/advertising a ready endpoint.

## Private protocol and responsiveness

The private protocol uses strict tagged DTOs with unknown fields rejected. Health
uses an empty struct variant so serde does not silently ignore extra unit-variant
fields. It is the only unauthenticated method and returns only API version,
actual owner PID and ready/quitting/unavailable state. It does not return paths,
projects, logs, environment, credentials or capability inventory.

The remaining methods are authenticate, redeem_bootstrap, renew, snapshot,
submit, status, result, failure_detail, cancel and quit. No role, provisioning,
explicit-start, arbitrary shell or external-grant issuer exists on the wire.
Admission routes to the existing actor, including its validated project/global
subjects and idempotency policy. An auth token alone cannot bypass those rules.

Request work runs on blocking workers outside the listener event loop. Native
credential verification uses an explicit existing-key request only. Cache checks
use try_lock and return Busy while another credential request owns the auth
policy; health remains responsive. Cached native sessions and explicit renewal
do not probe the SecretStore. NativeGuiPolicy fixes the absolute context deadline
and session renewal/rotation rules; it is not indefinite sliding access.
Successful authenticate/redeem/renew replies now include cache-derived
expires_in_ms, nullable context_remaining_ms and renewable alongside the session.
A client need not hardcode a ten-minute lease or extend the absolute native
context deadline. These metadata reads perform no credential-service access.

M13.1.5 delegates upload_begin/chunk/finalize/abort and snapshot_begin/page/release
to the private payload module. They require fresh native session authority and
bind retained data to peer/root/store epoch. No new external tool or grant is
issued. A maintenance task expires buffers, and successful Quit/Drop clear them.

Request/response bounds remain64KiB/256KiB. Owned encoded/decoded frame byte
buffers are zeroized after use. This is not a promise to erase every generic
serde_json copy or arbitrary caller-owned memory. Tokens are not logged or added
to actor receipts/lifecycle files. Auth responses deliberately carry a session
token over the authenticated private IPC channel.

## Durable explicit Quit

Authenticated Quit and the trusted native `OsQuitHandle::request_explicit()` use
the same gate and persist-before-stop operation. The latter has no wire decoder
and is intended for a dedicated OS-signal worker, not a UI/event-loop callback.

The fixed-schema quit-inhibited.json is staged privately, file-synced and
replaced atomically. Unix also syncs the parent directory; Windows uses explicit
replace/write-through flags. Existing symlinks, hard links, nonregular files,
weak/foreign ownership and unknown lifecycle schema are rejected. Authenticated
Quit rechecks session validity immediately before the replace commit. Trusted
OS intent does not require a credential-store read or a session.

Only a successful persistence step freezes admission, revokes cached sessions and
requests IPC stop. Failure leaves the controller usable and returns a stable
error. Quit acknowledges quitting without waiting for all IPC drain/service
cleanup; the serve composition then stops acceptance, drains bounded exchanges
and joins the real actor before releasing legacy/manager exclusion. Stale OS
handles are marked inactive and cannot modify a later owner's lifecycle record.

AutoStart refuses a persisted inhibition. Only the trusted Rust ExplicitUserStart
entry clears it; a role/confirmation/clear-inhibition JSON field cannot do so.
Restart creates no automatic project/service starts. Crash recovery, Interrupted/
Unknown operation observations and result loss follow the actor contract.

Native credential I/O cannot be forcibly canceled by a request timeout. Event-loop
runtime shutdown uses a bounded wait for those workers; late authentication
results are refused/revoked after quitting and hold no actor/owner permit. This
does not prove native OS dialog cancellation, power-loss recovery or a clean
minimum-OS acceptance.

## Platform identity and compatibility gates

macOS now verifies both effective UID via getpeereid and a positive kernel peer
PID via getsockopt(SOL_LOCAL, LOCAL_PEERPID), on both client and server. A missing
or malformed PID fails closed. This strengthens session binding and lets a cold
launcher compare the actual endpoint owner against its own startup child without
trusting a health payload. Logical desktop idempotency identity still excludes
PID to survive GUI relaunch. M13.1.1's older UID-only evidence is not retroactively
described as PID verification. Windows retains pipe ACL/SID and kernel peer PID
checks. See [Apple's socket constants](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/un.h)
and [Windows move flags](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw).

M13.1.5 adds an authenticated private chunk channel for M7's existing1MiB HTTP
body under JSON escaping: encoded commands are bounded at8MiB, upload chunks at
16KiB raw, and public requests remain64KiB. Snapshot exports use the same bounded
chunk approach and pin a complete immutable byte image. This does not change GUI
routing or establish actual core/native HTTP success. Worktree/cache
instance subject resolution and partial-create failure mapping are also required
before their GUI routes can move to the actor. Existing UI behavior is unchanged.

Controller tests inject memory/deny SecretStores and isolated owned data roots.
They exercise real controller/actor IPC, strict and denied requests, metadata
outcomes, renewal, health during blocked credential I/O, persistence failure,
inhibited restart, explicit trusted clear, legacy exclusion and trusted OS Quit.
They do not open native credentials, launch a real GUI or execute runtimes.
Native platform outcomes must be reported separately from source/build evidence.

Local macOS ARM64 focused results: controller7/7 passed; the actual two-client
IPC case passed with kernel UID/PID assertions on both ends; LegacyExclusion's
config-exclusive-lease regression passed. The earlier actor storage-contention
test observation deadline was corrected from2s to6s to cover an existing3s idle
SQLite refresh plus scheduling margin; its targeted rerun passed without changing
product timeouts, queue bounds or cache/cancel latency assertions. Native Windows
actor job43567704236's first deadline failure is retained as failure history,
not treated as a platform pass. Whole-workspace/GUI/minimum-OS checks were not run
by these focused cases. The separate private-bootstrap/auth tests retain their
own source/evidence boundary.
