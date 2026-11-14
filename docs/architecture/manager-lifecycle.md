# M13.1.1: owner and authenticated local IPC infrastructure

Status: infrastructure vertical slice. This does not yet compose a Workspace,
replace desktop/CLI owners, advertise live external tools or establish complete
M13.1 headless acceptance. The existing [control-plane contract](control-plane.md)
and [integrated plan](../knowledge/integrated-execution-plan.md) retain authority
over the later owner/operations/grants integration.

## Implemented boundary

`devforge-manager` adds three deliberately small APIs:

- `Owner::acquire(root)` holds an OS file lock on `control/manager.lock` and the
  existing shared `OperationLease` for its whole lifetime. `desktop.lock` remains
  the separate GUI singleton. An idle headless owner therefore also blocks the
  installer's exclusive operation lease. A PID file is not a singleton proof.
- `Server::new(owner).run(stop, handler)` serves bounded JSON frames over a private
  local endpoint. `Client::new(Endpoint::existing(root))` checks the endpoint and
  authenticates the OS peer before sending any request. The handler receives a
  kernel-observed `PeerIdentity`; incoming JSON cannot supply that identity.
- `launch::spawn_detached(LaunchSpec)` launches an explicitly resolved absolute
  executable with argument arrays, null standard handles and an explicit trusted
  environment. It does not use the managed-runtime kill-on-parent process adapter.
  Spawn is not readiness; callers must perform an authenticated owner handshake.

Root paths must be absolute, owned by the current user and non-symlink final
directories. The manager creates only its private `control` child. It does not
chmod or repair arbitrary existing directories, overwrite unrelated endpoint
files or unlink a stale socket before obtaining ManagerLease. Existing locks
must be regular non-reparse files with one hard link.

On macOS, `control` has mode0700, sockets use the owned directory, and both peers
use `getpeereid` to compare the kernel-observed effective UID. Existing weak
permissions and symlink endpoint paths are rejected. Socket paths over Darwin's
103-byte payload limit return an explicit endpoint error. Linux supports the same
infrastructure tests with SO_PEERCRED; this does not add Linux product support.

On Windows, the manager creates `control` with a protected DACL containing only
the current user's SID, verifies its owner and exact private DACL, rejects reparse
points, and creates a named pipe with an explicit same-user DACL and
`PIPE_REJECT_REMOTE_CLIENTS`. The first instance flag and ManagerLease protect
initial pipe ownership. Both ends obtain the peer process ID from the kernel,
open its process token and compare TokenUser against the current user. The client
requests identification-level security QoS. No administrator rights, service
installation, UAC change or security-permission request is used.

Same-user peer authentication is **not** a desktop-role credential or a project
grant. This transport is not a sandbox against another program already running
with the same user's filesystem/process privileges. The composition root must
establish server-issued client credentials/grants and distinguish trusted desktop
commands from scoped external operations. A `role: desktop` or `client_id` field
must never grant authority on its own. No public tool handler exists in this slice.

## Framing, responsiveness and shutdown

One connection carries one request and one response: a big-endian u32 byte length
followed by one JSON value. Request bytes are capped at64KiB and response bytes
at256KiB. The request length is checked before allocation; a counting response
writer stops serialization at the bound. There are at most16 active exchanges,
each with a5-second connect/read/write budget. Invalid, oversized or foreign-user
connections close without reaching the supplied handler. Transport errors expose
stable codes, not request contents, filesystem paths or raw OS diagnostics.
The Windows pipe reserves two additional instances: one just-connected excess
peer and the replacement pending listener can coexist transiently with16 active
exchanges. The excess peer is immediately dropped; saturation does not shut down
the owner. The common regression holds all16 peers at a partial header, exceeds
capacity, then proves listener recovery and timeout-driven capacity recovery.

Handlers must enqueue or read bounded owner work. A synchronous handler that
blocks the event loop cannot be preempted by an async timeout. Long Workspace
operations therefore belong on the single owner worker in the next slice, with
operation ID/status/cancel handled independently of that worker's busy state.

`StopSignal` closes acceptance, drains bounded existing exchanges and releases the
listener/owner/installer leases. It is infrastructure shutdown, not an authorized
product Quit command. Product integration must persist a Quit inhibition before
stopping services, retain it after exit, reject adapter auto-relaunch, and clear
it only after an explicit authorized user launch. No persistent inhibition,
operation journal or crash-reconciliation logic is claimed here.

## Dedicated launch and remaining native gate

Unix launch uses `setsid` before exec and no parent-pipe guard. Windows launch uses
`DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP | CREATE_BREAKAWAY_FROM_JOB`; it never
adds the manager to a client-owned kill-on-close Job Object. A restricted ancestor
Job can prohibit breakaway. That is returned as `manager_restricted_parent_job`,
not silently retried with flags that would leave the manager client-owned. The
caller can report the required independent user launch; a brokered alternative
requires a separately justified native slice. No always-on OS daemon is installed.

The isolated process tests distinguish actual detached survival from this explicit
Windows capability failure. A capability-failure test branch is not successful
Windows headless acceptance. Native Windows ordinary-user pipe ACL/token checks,
restrictive parent-Job behavior and real manager/Workspace lifetime still require
their own actual target evidence. Minimum-OS, native credential/UI, signing and
release gates are unchanged.

## Next composition slice

1. The existing CLI sidecar gains an internal `manager serve` entry before its
   ordinary router/foreground-session composition; no new installer binary is
   required. A manager library owns one Workspace worker and discovery service.
2. Desktop State becomes a client/cache. Snapshot, workspace command, inspector,
   activity, debugger, configuration, secret and discovery calls become explicit
   trusted requests. File dialogs, menus and tray remain in Tauri. Close still
   hides to tray; explicit Quit invokes owner shutdown before the UI exits.
3. GUI and two external clients receive live snapshots from that same Workspace.
   External tool availability is derived from installed handlers and grants;
   the planning inventory is never exposed as executable capability discovery.
4. Durable operation/idempotency records, target cancellation, grant/revocation
   policy, owner epoch, truthful freshness, crash reconciliation and Quit
   inhibition precede broad external mutation exposure. Busy serialization is
   retained until actual concurrency requires resource-specific scheduling.
5. Legacy CLI paths that currently create independent Workspace/foreground owners
   must forward through the live manager or return a clear conflict. They must
   not silently start another supervisor. Existing config setup's exclusive
   OperationLease also requires this integration; it cannot coexist unchanged
   with a long-lived manager's shared installer lease.

## Verification scope

`cargo test -p devforge-manager` exercises an actual isolated owner process, two
independent client processes, same-user peer identity, singleton/install leases,
graceful listener shutdown, detached-launcher exit, framed bounds and endpoint
path/permission rejection. The subprocess fixture is intentionally ignored when
run directly and is invoked explicitly by its parent tests; it is not a shipped
tool or a fake Workspace. Test-only process probes do not establish GUI+2-client
or public external-agent acceptance.

Initial local macOS ARM64 execution: manager package tests passed9/9, with the
subprocess-only fixture intentionally ignored as a direct test and invoked by
the parent process tests. Package clippy with all targets and denied warnings
passed. This includes actual detached survival after the launcher exits and
abrupt owner-process replacement through the released OS lease. Windows code
has not been compiled or executed in this local run; its native target gate is
open. No desktop GUI or Workspace was started by these tests.
After shared platform lease hardening, a new manager-level dangling
`operations.lock` regression passed: acquisition neither creates the outside
target nor replaces the symlink, and the already-acquired ManagerLease is
released on failure. The existing owner/installer exclusion and unsafe endpoint
tests were rerun individually and passed, together with one scoped package
clippy pass. The other earlier package results were reused, not rerun.

Platform references checked while implementing:
[Apple getpeereid](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man3/getpeereid.3.html),
[Microsoft named pipe security](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights),
[CreateNamedPipe flags](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createnamedpipea),
[Windows process creation flags](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags).
