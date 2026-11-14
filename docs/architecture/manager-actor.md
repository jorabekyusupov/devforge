# M13.1.3: one Workspace actor and durable operation receipts

Scope: an in-process composition boundary. No Tauri/CLI/MCP wire dispatcher,
external grant issuer, launch service or new native credential flow is added.
The preceding [manager infrastructure](manager-lifecycle.md) and desktop
capability policy remain separate boundaries. This document is implementation
description, not native GUI or Windows acceptance evidence.

## Exactly one owner

`Owner::claim_workspace()` atomically issues at most one non-cloneable
`WorkspacePermit`. The permit shares an `Arc<OwnerLease>` containing the manager
singleton and installer operation leases. `WorkspaceActor::start(permit, secrets)`
constructs the real `Workspace::open` on one dedicated worker. No second Workspace
is constructed for status reads or client calls. Dropping the Server/Owner cannot
release the leases while this worker still holds its permit.

The permit is one-shot, including failed startup: its share drops on failure but
the original Owner still holds the singleton. Drop that Owner and acquire a fresh
one for a new startup attempt. This avoids reusing an owner whose earlier worker
or startup might still be in an uncertain state.

`WorkspaceActor` owns the worker lifecycle. Its cloneable `ActorHandle` does not:
client/handle drop does not stop services. Explicit shutdown rejects new jobs,
requests active/queued cancellation, joins Workspace cleanup and only then
releases the worker's lease share. Shutdown failure is reported; cancellation
does not promise preemption or rollback of existing synchronous Workspace work.
Workspace/process destructors run before the permit is released, including worker
unwind. Public product Quit inhibition and relaunch policy remain future wiring.

## Desktop authority and identity

Admission, snapshot, status, result and cancellation all require a current
`DesktopAuthority`. The actor checks `check_current(Instant::now())` and the
canonical owner root. It never probes or provisions native credentials.

Logical private desktop client ID is a server-derived SHA-256 of native root
identity and kernel-observed user. It excludes PID so GUI/session relaunch can
find earlier receipts. No incoming client ID is accepted. Full private desktop
sessions represent one root/user owner's intent; external per-client/project
grants require their own future authority and are not obtained through this API.

Accepted jobs continue after the admitting session expires or is later revoked.
Every new operation/read/cancel needs fresh authority. This slice does not claim
execution-time revalidation of an external grant, or immediate cross-process
revocation notification. The trusted composition root must resolve project and
root-level receipt namespaces; an OperationIdentity alone is not authorization.
An exhaustive Command classifier verifies the subject before any receipt write:
project-specific command IDs must equal the supplied project ID, and genuinely
global commands require the reserved `desktop-root` namespace (not a project).
The initial actor slice rejected worktree/cache resource commands without trusted
owner resolution. The native facade now resolves their persisted source/project
owners and revalidates them at execution; a caller-provided label is insufficient.
Existing UI routes are not switched by that backend extension. This remains an
audit/cancellation correctness boundary even
for a full desktop owner, independently of later external-grant isolation.

Admission rechecks authority after waiting for the admission mutex, after an
IMMEDIATE SQLite transaction obtains its write lock, and immediately before
COMMIT. Expiry before that authorization point rolls back the uncommitted row.
Accepted jobs may complete after subsequent expiry; an expired proof cannot read
their status, but a fresh valid desktop session can. The exact authorization
point is immediately before commit, not a promise that disk fsync finishes before
the session deadline.

## Queue and responsiveness

There are at most32 waiting jobs in addition to one active Workspace operation.
Admission account-checks capacity while serializing the operation receipt insert.
A capacity rejection writes no Queued row. A disconnected/full channel after a
receipt write is terminalized as Interrupted instead of leaving a Queued
operation with no delivery. An owner/process failure is reconciled on the next
startup; persisted commands are never replayed.
The native [desktop facade](manager-desktop-facade.md) extends this same worker
with typed read/access work. Pure polling reads instead use bounded ephemeral
RAM receipts (no operations SQL row), with explicit ReadExpired after eviction/
expiry/restart. Secret binding and known raw HTTP/path/YAML command bodies use
random durable markers and bounded volatile comparison, not an offline secret
verifier. Fixed-metadata commands keep their durable digest semantics.

Commands are bounded at8MiB serialized input and exist only in memory. The
worker runs ordinary `Workspace::execute`, preserving its domain validations and
side effects. No fake runtime or alternate metadata implementation is used.
Waiting command encodings additionally share a16MiB byte budget, beside the
32-item bound and one active command. The established project/command tuple hash
is streamed through a capped writer; adding the larger limit does not change
earlier idempotency fingerprints or allocate another command-sized hash buffer.
Channel entries retain only receipt/cancellation metadata. Actual queued Commands
live in a guarded map so cancellation drops the body and releases its byte
reservation; if that mutex is busy, the next admission purges canceled bodies
before checking capacity. The remaining channel tombstone still counts toward
32 waiting items until consumed, preventing unbounded canceled metadata growth.
Dequeue/send failure/shutdown also release actual payloads and byte reservations.

M13.1.5's authenticated private [payload codec](manager-payload.md) preserves
M7 DebugHttp's existing1MiB UTF-8 body even under worst-case JSON escaping, while
public IPC requests remain64KiB. The actor's former256KiB internal limit is
superseded by the explicit8MiB bound. This is backend/codec compatibility evidence,
not a completed GUI migration or actual native HTTP acceptance. Core request,
header, YAML and other domain validations are unchanged.

Snapshot cache has its own short-lived mutex and retains one immutable Snapshot,
with the actual observation time. It refreshes after operations and approximately
every250ms while idle. Reads during a busy operation return the prior cache and
its old timestamp, never a newly stamped claim of live state. Cache retention is
bounded at4MiB serialized data; an oversized/failed refresh keeps the prior cache
and exposes an error. This bounds retention, not arbitrary producer allocation.

Target cancellation uses a separate in-memory cancellation registry. It sets the
specific operation's cancellation bit without waiting for the Workspace worker
or operation DB mutex. CancelRequested persistence is best-effort on the fast
path; the worker persists the eventual terminal observation. A successful
metadata operation remains Succeeded if it wins a late cancellation race. A
queued canceled job never calls Workspace::execute. Cancellation of A does not
set B's bit. Existing commands still own their cooperative cancellation behavior.
Background read work has separate active_reads status and does not set global
mutation busy/progress. Queued user effects still set busy while awaiting a read.

## Durable receipts and transient results

`control/operations.sqlite3` is pre-created through no-follow/non-reparse,
regular-file and single-hard-link checks, then opened with SQLITE_OPEN_NOFOLLOW.
Existing journal/WAL/SHM siblings undergo the same checks. The private owned
control directory remains mandatory. SQLite uses DELETE journaling and FULL
synchronous writes. No arbitrary existing root permissions are repaired.

The durable table contains operation/client/project/key identities, command
fingerprint, state, stable error code and timestamps. It contains no command
payload, env value, credential, log/mail body, source path or result data.
Fingerprint is a digest, not an encrypted command or a promised secret sandbox.
Persisted failure codes use a finite classification (project/runtime/serving/
workspace failure or cancellation), not arbitrary strings from backend errors.
Exact domain scope/code/subject/retryable metadata is validated and retained only
in RAM as typed `FailureDetail`. Validation matches the control adapter's finite
scope, bounded ASCII code and subject-consistency policy, using the already
validated receipt subject directly rather than inventing a ProjectBinding for
global operations or missing projects. No Error Display/message/path/payload is
included. `failure_detail()` requires the same fresh authority and receipt subject
as other privileged reads. Missing/invalid detail after eviction or restart is
explicit ResultUnavailable; coarse failure receipts remain queryable.

CreateProject and WorktreeCreate now attach a separately proven recovery child
association after trusted before/after, selected metadata and filesystem-identity
checks. Receipts retain the root/source namespace. Arbitrary mismatched failure
subject metadata is still rejected rather than relabeled.

Idempotency lookup is keyed by server-derived client, project and key, and binds
the request fingerprint. A duplicate returns the same operation receipt; changed
input under that key returns IdempotencyConflict. Terminal receipts are not
rewritten to rerun a command.

On startup, old Queued becomes Interrupted; old Running/CancelRequested becomes
Unknown. No old command is executed and no stored PID implies a running service.
Recovery or a fresh attempt requires an explicit caller decision and new key.

Small successful Values and validated failure details share one in-memory
outcome cache: at most64KiB per outcome,128 entries and1MiB total retained bytes.
Larger successful mutation results and native read results use the facade's
shared bounded64MiB image/128MiB aggregate cache and paged delivery. A successful
side effect whose result exceeds even that budget stays Succeeded with an explicit
`result_output_limit`;
it is not falsely labeled rolled back. Missing results after eviction/restart
return explicit ResultUnavailable. Response consumers retain their own memory;
these are actor retention bounds, not a global process memory limit.

## Focused verification design

Actor integration tests use real isolated Laravel-shaped source folders and
the current bundled catalog's exact target IDs, without installing or executing
those runtimes. Import, Favorite and Visit execute through the real Workspace;
fixture `.env` and package scripts remain unchanged. A credential adapter that
panics on access proves these metadata paths do not touch native credentials.
Desktop authentication uses a separate in-memory policy fixture.

For deterministic busy/queue/cancel coverage, a test holds BEGIN EXCLUSIVE on
the isolated real projects.sqlite3. The actual Favorite operation waits on that
SQLite lock while32 real metadata jobs are admitted. Cached reads and target
cancellation must still return quickly. Releasing the lock permits normal
Workspace execution; the canceled queued A operation is skipped while B's
operations complete. This is scheduling against real storage contention, not a
fake Workspace/backend or a fabricated service readiness pass.

Additional cases cover one-shot permits, lease retention after Owner drop,
duplicate/conflicting requests, retained-data limits, ledger content, graceful
reopen, startup receipt reconciliation, foreign-root authority, expired held
proofs and unsafe ledger symlinks. Native service lifecycle, actual GUI plus two
clients, native credential dialogs and Windows execution need separate evidence.

Local macOS ARM64 checks for the final slice: the targeted actor suite passed
9 integration cases, including a real invalid-name Import yielding exact
`project/invalid_project` metadata in privileged RAM while SQLite contains only
`project_failed`. Another128 real failing metadata jobs prove shared-LRU
eviction; reopening proves exact detail is unavailable after restart. Both
retention/validation policy unit cases passed. Package clippy with all targets
and warnings denied passed. The retention fixture uses short `/tmp` paths on
Darwin, preserving production socket-path rejection. These checks did not run
the whole-workspace suite, execute runtimes, open native credentials or change
GUI routing. Native GUI, Intel and Windows actor acceptance remain separate;
earlier transport/authentication evidence does not prove this actor slice.
