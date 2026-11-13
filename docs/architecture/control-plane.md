# M13.0.1: control-plane contract foundation

Decision date: 2026-11-13. Status: working implementation direction; the contract
library is implemented, a manager process and external agent integration are not.
The [external-agent plan](../knowledge/m13-external-agents-plan.md) and
[integrated execution plan](../knowledge/integrated-execution-plan.md) define scope.

## Context

Desktop currently owns a Workspace and serializes mutations. Several CLI commands
open their own stores/foreground sessions. OperationLease protects install/remove
boundaries, but does not route live reads and commands to a single supervisor.
A stdio MCP wrapper that opens another Workspace per call would not establish
coherent process lifetime. Modularity of the Rust core must be preserved.

## Decision

Add `devforge-control` as a framework-independent contract/policy boundary. CLI,
Tauri and later MCP adapt to a common manager; business logic stays in the existing
workspace/projects/runtime/serving/inspector/debugger layers. Versioned JSON CLI
and MCP JSON-RPC envelopes remain distinct. This crate adds no network endpoint,
Tauri command, secret provider or process owner.

The initial boundary defines explicit project IDs, revisions, scopes, review and
idempotency checks, outcome envelopes and operation states. IDs are opaque bounded
ASCII strings, not paths. Registered paths are resolved by trusted server code;
a request cannot supply a root or a `confirm:true` authorization shortcut. A scope
is an independent capability: ProjectManage does not imply ExecutionTest or
SensitiveOperation. The server must verify authenticated peers and load/revoke
its own grants. Constructing a contract type is not peer authentication.

A review receipt must be tied to the same project revision and a canonical digest
of all side effects and current inputs. The producer computes the SHA-256 digest;
the contract validates its representation and compares it exactly. A future
manager must include method/input/secret revision and canonical target identity
in this digest. Idempotency records are keyed by authenticated client, project
and key, and additionally bind the request fingerprint. Atomic persistence and
race handling belong to M13.1, not this in-memory contract guard.

This is the first validated M13.0 slice, not full M13.0 completion. Per-tool typed
payload schemas, concrete adapter dispatch/error mapping, cursor/limit enforcement
and cross-language contract generation remain M13.0.2 work. `Request<T>` validates
the envelope; each adapter must supply a strict typed `T`, not dispatch arbitrary
JSON based on this generic wrapper.

The current embedded inventory maps all sixteen plan families to existing code,
new adapters, new operations and unsupported behavior. Each listed operation has
required grant scopes, side effects, review requirement and policy metadata.
Implicit execution is included in scope metadata: project start/restart, runtime
selection that restarts a running project, environment setup/apply, Inspector
capture restart and workspace preparation/merge require ExecutionTest in addition
to management capabilities. Environment setup/apply also carry sensitive review
because they may change credentials and DB bindings. Inspector companion install
runs Composer/Artisan and is distinct from debugger provider installation, which
builds/downloads verified SDKs without running project code.

Operation-owner policy also requires the original operation's grant, not generic
Observer access to another caller's job. This is planning metadata, not a dispatcher. `available:false` is
intentional for every family: a core feature is not an implemented agent tool.
Do not expose this planning inventory as a live list of invokable tools. Live
capabilities will be generated from installed adapters and caller grants.

## Lifecycle for M13.1

One manager per user/data root owns all live supervisor sessions. macOS user-scoped
socket and Windows named-pipe/ACL adapters are candidates, pending native spike.
Each caller verifies the manager/peer identity. A second stdio adapter never
starts an independent supervisor. Start with conservative serialization and
operation IDs; resource-specific concurrency is introduced only when required.

Persisted read projections/schema fixtures can precede the manager. Live status
must come from its actual owner, with origin/time/revision on persisted/cached
reads. Headless start while the GUI is absent is an explicit M13.1 acceptance
requirement; an always-on launchd/Windows Service is not predetermined.

Window close continues tray behavior. Explicit Quit stops owned jobs/services and
records the reason, preventing client retries from immediately relaunching the
manager. Client EOF/kill is distinct from explicit Quit: durable services survive
an adapter disconnect; temporary jobs follow their declared cancellation policy.
After crash/restart, reconcile process identity and receipts; never infer running
solely from a stored PID. The exact launch/recovery mechanism remains a spike.

Cancellation is a request, not successful rollback. If completion wins the race,
return Succeeded. Interrupted/Unknown are terminal observations of the old attempt;
recovery creates a new attempt after reconciliation instead of rewriting history.

## Execution and sensitive features

PHP, Artisan and package hooks execute trusted project code even with an executable
allowlist. They require an execution grant, explicit args/cwd/env and bounded
output/process lifetime. SQL prefix checks do not establish read-only execution;
role-limited DB-specific adapters are required before query tools are available.
Debugger scope preserves existing primitive locals and revision checks. Arbitrary
expression evaluation is unsupported; adding it requires a separate execution
contract, side-effect assessment and grant. No unrestricted shell/file resource,
raw credential export or automatic public network endpoint is introduced.

## Consequences and validation

The contract crate is usable without Tauri/OS adapters and introduces no new third
party dependency beyond existing serde/serde_json. Tests cover unknown/versioned
wire inputs, project/revision guards, grant/revoke/expiry, review digest/expiry,
client/project idempotency isolation, terminal transitions, v1 response fixtures
and complete non-live inventory. They do not prove authenticated IPC, durable
storage, single-owner concurrency, MCP client compatibility or native lifecycle.
M13.1 and later phases must establish those with scoped tests and actual macOS /
Windows evidence. No platform pass is implied by compiling these contracts.
