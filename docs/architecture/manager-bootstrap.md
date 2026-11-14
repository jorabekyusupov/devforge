# M13.1.4: private cold-GUI bootstrap and explicit native renewal

Status: private launch/channel and credential-policy slice. Controller and CLI
composition have separate evidence. This document does not establish a desktop
GUI migration, public full-Command dispatcher, external-agent grant or MCP tool.
The [M13.1.2 policy](manager-auth.md) remains the ordinary fixed-TTL native-store
authentication contract; the explicit native-GUI context below is an amendment,
not an implicit extension of every session.

## Private authority issuance

A newly launched manager may issue one cryptographically random256-bit bootstrap
credential only through its inherited private stdout pipe, after all three facts
hold: the OS owner lease is held, the actual Workspace actor initialized, and the
OS listener bound. A private `ListenerReady` proof is minted by the transport only
after successful bind. `ReadyPermit` combines it with actor health and canonical
root equality; it is not constructed from an incoming `ready` flag. The consumed
permit and one-issuance-per-auth-instance policy prevent arbitrary repeated
bootstrap issuance. Ordinary IPC exposes no capability-issuing method.

The pending credential binds a random manager-instance nonce, root fingerprint,
current generation and short expiry (default30 seconds, maximum120 seconds).
Wrong root/instance/token or foreign OS peer cannot redeem it. Only one successful
redemption consumes it; capacity failure does not issue a session. Expiry or
whole-cache/key-change revocation removes the pending credential. It never lives
in argv, environment, a temporary file, logs, diagnostics or ordinary operation
results. `BootstrapCredential` is an opaque zeroizing type with redacted Debug
and no serde implementation, distinct from the native credential-store key.

This is a same-user private launch channel, not a sandbox against another program
with equivalent process/credential access. The trusted installed manager launch
entry and composition root still define authority. A bootstrap flag exposed by a
generic renderer or external-agent dispatcher would violate this boundary.

## Actual pipe and standard-handle lifetime

`BootstrapWriter::capture_inherited_stdout` is called at the explicit private
manager launch entry before stdout logging or child processes. Unix `fstat`
requires FIFO/pipe type; Windows requires `GetFileType == FILE_TYPE_PIPE`. A TTY,
regular file or NUL handle is rejected. Merely checking `!is_terminal` would not
be sufficient.

The writer duplicates the pipe into a private noninheritable descriptor/handle,
then immediately redirects standard stdout to NUL. Unix uses `F_DUPFD_CLOEXEC`
and `dup2`; fd1 remains occupied. On Windows/MSVC, `_open_osfhandle` and `_dup2`
install NUL into CRT fd1 and close the original pipe, then `SetStdHandle` points
Win32 stdout to that live CRT-owned NUL handle. The temporary NUL descriptor is
closed; fd1 remains alive for process lifetime. The old pipe handle is not closed
again after `_dup2`, since its numeric value could already have been reused.
Diverged/uninitialized CRT and Win32 stdout are rejected at the private entry.
Non-MSVC Windows bootstrap is not supported by this implementation. Closing
stdout without a live replacement could let a later database/file reuse its
descriptor/handle. Runtime children inherit NUL rather than the private pipe.

Readiness then generates and writes exactly one138-byte binary frame: eight-byte
magic, two-byte version,32-byte root fingerprint,32-byte instance and64 ASCII hex
token bytes. The frame buffer and credential storage are zeroizing; no JSON/debug
representation carries the token. Consuming the writer closes its private pipe
after the frame. Failure to deliver does not convert the pending secret into a
public result; its short expiry/revocation still applies.

The parent reads a bounded frame followed by EOF using native poll/pipe-peek and
bounded reads. There is no abandoned blocking reader thread on timeout. An extra
byte, missing/bad magic/version, invalid token, root mismatch, early EOF or timeout
is rejected. The owned reader and zeroizing frame buffer are dropped on all exits.
Timeout must be positive and no longer than30 seconds. This channel is independent
of native credential prompts and never writes or probes SecretStore.

## Verified launch and conservative cleanup

`spawn_with_private_bootstrap` accepts the existing trusted absolute `LaunchSpec`
plus a mandatory expected SHA-256 digest. It validates the executable/root and
verifies the bounded executable file before spawning with explicit argument arrays,
environment and a private stdout pipe. It does not invoke a shell. A failed digest
never spawns a child. This is a local pinned-file check, not OS code signing or a
sandbox against concurrent modification by an equivalently privileged program.
The older `spawn_detached` API preserves its null-standard-handle behavior.

A pending launch owns only the handle of its newly created child. On a failed
channel read, the caller supplies a bounded authenticated root/process-identity
readiness probe. Only `Ok(false)` authorizes kill-and-wait of that known new,
unready child. `Ok(true)` preserves the ready owner; an unknown/error result also
preserves it. Existing owners discovered separately are never cleanup targets.
Dropping a pending launch closes its reader but does not blindly kill a possibly
ready owner. A successful ready child is stopped through authenticated owner IPC;
an optional wait method only reaps its orderly exit. Spawn/frame success is not
perpetual liveness, so redemption must still use the authenticated owner endpoint.

## Native GUI contexts and the revocation-window amendment

The ordinary M13.1.2 `authenticate` method still produces a fixed, nonrenewable
session. For an explicit native-GUI fallback to an existing manager,
`authenticate_native_gui` first performs that same throttled worker-side native
store verification, then marks the session with a server-owned absolute native
context deadline. Cold private bootstrap redemption creates the same context
without touching SecretStore.

Native contexts default to12 hours and are bounded by a24-hour policy maximum.
Each individual token still has its fixed ordinary TTL (default10 minutes),
clamped to the absolute context deadline. Only an explicit `renew_native_gui`
call can atomically replace a still-valid token for the same OS peer/root/current
generation. Renewal does not access SecretStore, does not slide ordinary use,
does not add cache entries, and never moves the absolute deadline. Expired,
revoked, foreign-peer, wrong-root or ordinary nonrenewable sessions cannot renew.
After successful rotation, the preceding token is immediately invalid.

Whole-cache revocation, local key changes and an observed stored-key fingerprint
change invalidate renewable contexts as well as fixed sessions. A key changed
in another process is not observed by cache-only checks or renewal. Therefore a
renewable context can retain authority until its absolute deadline (up to the
configured12-hour default/24-hour maximum), unless the owner is notified or the
key change is observed sooner. **The10-minute fixed-session revocation bound must
not be claimed for a renewable native GUI context.** Immediate cross-process
revoke needs an authenticated owner notification in the subsequent composition.

Renewal and bootstrap are credential/lifecycle primitives only. No GUI refresh
timer, native first-action UI, arbitrary IPC issuance, queued-execution grant
revalidation or provider/LLM integration is established by these APIs.

## Verification boundary

On local macOS ARM64, scoped checks passed:

- `cargo test -p devforge-manager --test bootstrap`: five custom-harness cases.
  A real newly launched Controller/Workspace actor/listener issued the private
  frame; wrong root/instance did not consume it, successful redemption was
  single-use, and authenticated snapshot/quit completed. A separate exec child
  could not retain the private writer; frame+EOF returned before its lifetime
  ended. A post-bootstrap stdout/data-file guard stayed unchanged. Regular file
  and NUL stdout were rejected. Wrong executable digest never launched. Malformed
  frame and timeout cleaned/reaped only the known unready child, with unchanged
  parent descriptor counts. An authenticated kernel-PID readiness probe preserved
  a ready owner after timeout; its fixture self-exited afterward.
- Three new auth fixtures were run individually: ordinary fixed auth cannot
  renew; explicit native renewal rotates without store access and cannot exceed
  its absolute deadline; expired/revoked contexts and excessive policy are denied.
- `cargo test -p devforge-manager --lib bootstrap::tests::`: two fixtures combined
  actual actor initialization and an actual post-bind listener proof, then checked
  short bootstrap expiry, foreign-peer rejection and pending-context revocation.
- The affected old null-stdio detached-launch survival test was run individually
  and passed. This shared helper refactor did not change its existing behavior.
- Scoped manager all-targets clippy with denied warnings passed. All-targets here
  means this host's Cargo targets, not another OS or architecture.

These checks used memory/deny stores and temporary owned roots. Prior auth tests
outside the changed slice were reused, not rerun. No native credential service,
GUI, system permission, host resolver/trust store or adjacent project was changed.
Native credential dialogs, real GUI cold/warm start, packaged fixed-executable
paths, Windows MSVC CRT/handle/Job behavior and minimum OS acceptance require
separate actual-target evidence. Memory/deny-store tests cannot close those gates.
OS signing, native credential UX, installer/update and release gates remain in M15.

Relevant Windows ownership references:
[`_dup2`](https://learn.microsoft.com/en-us/cpp/c-runtime-library/reference/dup-dup2),
[`_open_osfhandle`](https://learn.microsoft.com/en-us/cpp/c-runtime-library/reference/open-osfhandle),
[`SetStdHandle`](https://learn.microsoft.com/en-us/windows/console/setstdhandle).
