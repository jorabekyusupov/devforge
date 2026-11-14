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

The initial ManagedTest guard accepts only verified data-free contexts. Managed
DB/cache/mail/env, .env/test overrides, PHPUnit/config/database directories or
other unverified data context are refused rather than falling back to development
data. Full Laravel/worktree/job-specific SQLite isolation is still a necessary
following M13.6a slice. Ordinary project code is explicitly trusted code, not OS
sandboxed. The initial PHP fixture cannot establish real Laravel data isolation.

Existing immutable MCP config remains readable unchanged; new exports add the
execution surface only for a qualifying grant. Known legacy flag subsets are
accepted with exact fixed CLI/credential/environment paths; unknown/duplicate
flags or changed targets fail validation. No config is repaired or overwritten.

## Verification state

Focused contracts, policy, client and review compatibility checks have passed;
core and one actual owned execution flow are being completed. Actual outcomes,
remaining isolation/platform limits and publication will be recorded in
execution status before this first slice ships.
