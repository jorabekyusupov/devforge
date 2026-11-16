# M13.6a managed execution

The first slice reuses Context/Invocation/Supervisor and the existing single
actor, grants, reviewed intents, receipts and cancellation. Six finite tools are
PHP, Artisan, Composer, Node, npm and Git; there is no unrestricted shell,
implicit runtime install or pin change. ManagedTest is the existing planned
test-job capability, not a sandbox.

## Authority and input

Every execution requires current ExecutionTest, exact native review and trusted
project intent. Existing native issuance requires ProjectManage with ExecutionTest;
actual credential/data context additionally requires SensitiveOperation before
SecretStore resolution. Frozen project/root/revision/package/files/DB/cache/debug
facts are checked again before credential resolution and before launch.

Canonical arguments use redacted Debug and zeroizing storage, at most64 entries
and16KiB total. CLI input is bounded protected JSON stdin with a protected
credential file, not secret arguments. All execution admissions use random
durable markers and volatile comparison; no raw arguments or unkeyed argument
hash are journaled. Loss of the verifier is explicitly unverifiable, with original
key lookup rather than guessed replay/new keys. Native UI displays safe concrete
invocation/context rather than count-only approval.

## Output and completion

Typed output is bounded/redacted (16KiB text/32KiB JSON), scoped to the original
client/project/current grant and execution authority. It uses existing actor RAM,
not a new persisted output store. Running output reports Busy; unavailable or
evicted/restarted output reports ResultUnavailable. Receipts and original-key
lookup remain available under their current authorization.

Execution.output includes process success/exit code. A completed process with
exit1 is distinct from admission/transport failure; consumers must inspect that
result rather than treating an accepted operation as a passing test. Deadline and
cancel cooperate with existing polling/stop grace; completed outcomes win over
late cancellation. MCP execution.cancel aliases the existing own-operation cancel
RPC; default read-only tools stay9, execution opt-in adds9 (all38).
Timeout/cancel/policy denial retains its terminal receipt/code but may have no
public output. No synthetic exit code or partial stdout is invented; private
bounded process logs do not imply a public streaming-output guarantee.

## Test-data boundary

The original data-free ManagedTest path remains unchanged. A01 additionally
accepts one existing, reviewed PHP file under `tests/` in a ready owned worktree.
The child project, worktree receipt, source relationship, Git identity/HEAD and
live filesystem identities are bound to the review fingerprint. A source grant
never becomes a child grant; existing project-bound admission still applies.

Each invocation allocates an exclusive job directory outside the project tree,
creates a new SQLite file and uses credential-free bootstrap context with the
child's exact runtime pins. APP_ENV is testing, DB_CONNECTION/database are fixed,
DB_URL/DATABASE_URL empty, cache/session array, queue sync and mail array. A
synthetic test APP_KEY replaces development keys in the process only. Laravel's
config/services/packages/events/routes caches and storage target the job directory.
Atomic private receipts distinguish completed exit status from unavailable outcome;
no raw argv or development credentials enter them. Retained job data has no new
public artifact route in this slice.

This conservative slice refuses child `.env`, alternate `.env.*` (except inert
`.env.example`), all PHPUnit XML configurations and cached bootstrap config before
launch; it never edits/removes them automatically. Inline PHP, extra arguments,
non-PHP tools and paths outside `tests/` cannot enter this Laravel context. Existing
PHPUnit/Artisan test orchestration is not claimed. Ordinary reviewed project code
and its dependencies remain trusted; this is operational data isolation, not an
OS/filesystem/network sandbox or a guarantee against hardcoded custom connections.
The source .env/APP_KEY and both development databases remain untouched.

Existing immutable MCP config remains readable unchanged; new exports add the
execution surface only for a qualifying grant. Known legacy flag subsets are
accepted with exact fixed CLI/credential/environment paths; unknown/duplicate
flags or changed targets fail validation. No config is repaired or overwritten.

## Verification state

Focused core5/context1/Git1, manager policy1/affected service1, argument/contract
and client5 checks passed before the saved limit checkpoint. The actual public
PHP8.4.27 flow passed: exit7, owned fixture fix/new review/key, exit0; replay did
not run twice, other/revoked grant output was denied and argument content/hash
was absent from the ledger. The existing diagnostics read reported missing
collector truthfully; no new Laravel capture or DB isolation is claimed.

On resume, the previously unconfirmed Git-follow-up gate passed with scoped
projects/workspace lib +execution_control clippy. Older unchanged passing tests
and native evidence were reused. Clean staged format/locale/knowledge checks
and publication are recorded in execution status. A01 single-file Laravel job isolation is recorded below; broader test profiles
and Windows actual Laravel execution remain separate gates.

## A01 verification (2026-11-16)

`crates/workspace/tests/execution_control.rs` adds owned-child/receipt and hostile
config/argv refusal. Its opt-in offline actual test uses PHP8.4.27 and Laravel13.42.0,
normal Foundation environment/configuration/facade/provider bootstrap, real migration,
insert and delete. Only the new job SQLite changes; two development canary files,
source env/APP_KEY, both Git HEADs/dirty status and the dirty source file remain
byte-identical. The actual fixture copies dependencies and a hash-registered PHP
binary into its disposable root; no downloads or native secret access. The synthetic
ready receipt composes the existing ownership boundary; it does not rerun or replace
M12 creation acceptance. See [actual evidence](../evidence/a01-isolation/actual-macos-arm64.json).
Windows portable refusal/runtime contracts are CI checks; the offline Laravel
fixture is explicitly ignored there, so Windows Laravel runtime proof remains open.
