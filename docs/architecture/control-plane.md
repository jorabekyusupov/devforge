# M13.0.1–M13.0.3: control-plane contract foundation

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

M13.0.1 establishes the guarded envelope/policy foundation. M13.0.2 adds concrete
strict initial read projection payload/result types for one operation in each of
the sixteen families. Every remaining inventory operation is explicitly Planned
or Unsupported with no fabricated input/output schema and no live handler.
Mutation payload schemas are introduced before their adapters, not represented as
ready by a generic JSON wrapper. Manager/MCP transport and backend projections
remain future stages.

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

## M13.0.2 projection contracts and generated artifacts

`tools.rs` defines sixteen concrete input/result pairs using contract-field macros.
These produce strict Rust serde structures, JSON Schema and TypeScript declarations
from the same field lists. `export.rs` supplies the bounded primitives/enums/envelope
schema. The checked JSON document includes all inventoried operation policies and schema
statuses. The exported TS has precise input/output maps and discriminated success /
failure shapes. Run `cargo run -p devforge-control --example export_contracts --
--write` after a contract change, then `--check` verifies byte-for-byte drift. This
exports files only inside the crate. It does not generate or replace existing
React/Tauri DTOs; integration into those adapters follows their phase work.

The public preparation boundary rejects request bytes above64KiB, wrong API version,
unknown envelope/payload fields, unsafe JS integer revisions, invalid text, excessive
page/byte/time limits, and foreign/stale/expired cursors. Text limits count Unicode scalar values in both Rust and JSON Schema maxLength;
total request/serialized-output byte limits are enforced separately. Runtime serde
validation remains authoritative. This is not a
claim that TypeScript type checking enforces numeric or resource bounds.

All observed_at/expires_at/now timestamps, including existing grant/review expiry,
use Unix milliseconds. Revisions/offsets/timestamps on the exposed projection wire
are safe integers at most2^53−1. `timeout_ms` is a monotonic duration, capped30s;
page size is at most500 and output payload at most256KiB. Defaults are100 rows,
64KiB and5s. Cursor is a typed pagination hint, not an authorization token; it binds
project/tool/revision/expiry and the normal grant is always checked independently.

ReadBudget uses a capped counting writer with serde_json::to_writer: oversized
serialization stops without allocating a payload-sized buffer. Deadline enforcement
is cooperative before/after serialization and at writer calls; this cannot preempt
arbitrary CPU work inside a producer/Serialize implementation. A future adapter must
also apply the deadline/cancellation to its own DB/network/event operations.

PreparedRead keeps the authorized request and budget private, exposing immutable
access/check methods. Returned rows are budget-checked; explicit project/resource
IDs cannot disagree with the bound project or requested operation/artifact.
Operation-backed reads (status, execution exit code, stored HTTP response) require
trusted manager ownership metadata and the original operation's grants. Merely
knowing an operation ID under another same-project client is insufficient.

HTTP readiness/response projection reads stored metadata only and therefore uses
Observer/read policy. It is distinct from managed_site_request, which actively
executes an HTTP request and requires ExecutionTest. Unknown TLS/readiness is null,
not false. ServiceState models stopped/starting/running/ready/backoff/stopping /
succeeded/failed/unknown instead of reusing operation queue states. Projections
intentionally omit env values, credential refs, raw log/mail bodies and filesystem
paths; future collectors still own redaction before populating richer DTOs.

Every tool contract remains live_available:false and prepare_read never executes a
backend. require_live_handler returns UnsupportedCapability. Typed preparation is
not an MCP server, authenticated IPC, a secret sandbox or a usable manager.

## M13.0.3 typed command preparation

The initial command surface adds twelve concrete payloads/receipt schemas matching
existing Workspace commands or its configuration_plan method: services.start,
services.stop, services.restart, runtimes.install, runtimes.remove, runtimes.select,
environment.plan, environment.apply, environment.yaml_save, workspaces.archive,
workspaces.restore and workspaces.purge. The last two use the existing archive
operation strings; archive itself maps to existing worktree_cleanup, retaining DB,
branch and credential data according to the existing core contract.

The inventory grows153→155 to name runtime removal and YAML save explicitly;
all old operations remain accounted for. There are16 typed read projections,
11 typed mutating commands and1 typed nonmutating plan; all other operations
remain Planned/Unsupported. Every live_available flag stays false. In particular,
workspaces.create/merge and generic execution/debug/eval are not enabled by this
slice. Generated JSON/TS includes both strict request unions and per-tool payload
and result types. Plan requests do not require an idempotency key; actual mutations
require non-null expected_revision and idempotency_key.

`prepare_mutation` accepts only a server-derived registry binding, authenticated
client's grant, and optional trusted ProjectTrust/ReviewApproval records. Clients
cannot send confirm/trusted/consent fields. Execution-bearing commands additionally
require recorded project trust tied to the canonical project root. A domain
configuration contains an owned lowercase .devforge.test domain and nonempty
bounded project name. YAML documents allow normal newline/tab formatting, reject
control characters, and remain under the64KiB total request-byte bound. The
existing backend still validates the manifest schema and expected file hash.

ReviewApproval binds two different digests: the current backend plan digest,
verified against ReviewedPlan's action/project/revision/expiry; and a fingerprint
persisted for the exact proposed typed arguments. `fingerprint_material` produces
deterministic JSON for the canonical registered root, project, current revision,
tool and all typed arguments including limits. It excludes request ID/idempotency
key so transport retry does not change operation meaning. A trusted caller hashes
these bytes using its existing SHA-256 provider. This crate deliberately adds no
new cryptographic implementation; the request supplies neither a hash nor a consent
boolean. Review store lookup itself must respect client authorization.

PreparedMutation keeps its request, registered binding, descriptor, computed
fingerprint, approved backend digest and job limits private with immutable getters.
`approved_review()` returns the verified BACKEND digest, not the request fingerprint;
an adapter must compare this with its actual backend review receipt before making
a reviewed core command. Workspace IDs additionally require trusted receipt/source
project lookup in the adapter; knowing an arbitrary workspace ID is not authority.

Typed receipts are checked against the same tool/project and a capped serializer
byte budget. Optional mutation limits bound requested job duration to30min and
output to256KiB, defaulting5min/64KiB. The manager must actually enforce those
limits during execution; preparing a descriptor does not start a timer-controlled
job. Atomic idempotency persistence, fresh grant/review/resource validation at the
commit boundary, cancellation and manager ownership remain M13.1/M13.4 work.
require_live_handler still returns UnsupportedCapability.

Focused tests cover all initial command mappings, independent execution grants,
project trust, client confirmation rejection, missing revision/key, incompatible
version/limits, changed arguments/backend state/action/revision/expiry, retry-stable
fingerprints, explicit unavailable operations and scoped/bounded receipts. There is
no claim of a running manager, authenticated IPC or native command execution.

ProjectName also follows the existing core's validate_name boundary: trim-empty
names and values over200 UTF-8 bytes are rejected, while valid Unicode is retained.
Its standard JSON Schema maxLength200 is a character bound; x-maxUtf8Bytes200 and
the schema description document the additional server byte constraint and trim
rule. Standard schema validation alone does not establish complete name validity.
Fingerprint preparation rejects a non-UTF8 registered root with InvalidInput before
JSON conversion; filesystem metadata cannot trigger a json! serialization panic.
