# Native desktop facade ownership and bounded read lifecycle

Status: implemented private backend prerequisite. Existing Tauri routes and
LegacyStandalone mode are not switched by this slice. There is one real
Workspace and one discovery service, both owned by the manager worker; no facade
call constructs another Workspace or adopts a renderer-supplied ready/role flag.

## Read and access protocol

All new methods require a fresh native desktop session and the existing kernel
peer/root binding. They are not external-agent tools or grant issuers.

| Method | Input beside session | Result |
| --- | --- | --- |
| desktop_read | idempotency_key, typed request | Ephemeral read Receipt |
| desktop_read_result | project_id, operation_id | Pinned image descriptor |
| desktop_access | idempotency_key, typed request | Durable effect Receipt |
| discovery_request | none | requested:true, coalesced scan wake |
| operations | optional project_id/cursor, limit1–100 | Durable items, next_cursor, active_reads |
| operation_by_key | project_id, idempotency_key | Existing durable Receipt or null |
| result_begin | project_id, operation_id | Pinned large successful mutation result |

Read requests cover Inspector context, inbox/message/attachment, structured logs,
debugger view/profile/source and configuration document/plan. Access requests
cover secret binding, project open target and discovery-root changes. Project
subjects are derived from the typed request. Inputs have individual bounds,
including512-byte mail/log queries, bounded pages/attachment indexes,4096-byte
source/root paths and4096-byte zeroizing secret values.

The worker invokes the existing Workspace methods. It preserves domain scope and
code, including the finite Debugger scope used by actual M7 errors. Unknown scope,
malformed code or unvalidated foreign project metadata is still rejected. Large
data is never replaced by an invented empty value. Successful read images encode
exactly `{ok:true,data:<existing domain value>}`; failures retain bounded typed
failure metadata through the established failure_detail API.

## Ephemeral polling receipts

Pure reads use read-prefixed operation IDs and RAM-only receipts. They do not add
a SQLite operations row on every Inspector/mail/debug polling interval. Metadata
is bounded at128 entries with a120-second lifetime, and at most2 uncanceled read
jobs can wait/run. Duplicate keys within retained metadata return that observation;
fresh polling uses a new key. Expiry, eviction or manager restart yields
actor_read_expired and an explicit safe read/refetch decision, not a fabricated
durable effect or automatic replay of a mutation.

Read/access requests share the existing32 waiting-item and16MiB queued-byte
budget. Waiting and active read status remains separate from mutation/access
busy state: normal background reads do not set global command spinners or disable
controls. A queued user effect still reports busy even when it waits behind a
read. Cancellation uses the separate exact-operation fast path. A canceled read's
late data is discarded; this does not promise to preempt a blocking native store
or undo existing domain housekeeping.

The durable operations list is owner-derived, project-filtered and bounded. Its
cursor pins a maximum row ID and excludes later inserts without silently changing
the selected project/client namespace. Current uncanceled reads appear separately
as active_reads (at most2), allowing a reopened GUI to discover exact cancel IDs
without flooding the durable journal with polling rows. Terminal states can
advance while paging; the cursor pins membership rather than inventing a frozen
service state.
The indexed operation_by_key lookup allows lost-access-reply reconciliation even
when a secret comparison is no longer available. It projects no digest/key/body.
A null result is point-in-time, not proof that another accepted handler cannot
still commit; callers retain the original key and do not automatically invent a
new mutation key or execute again.

## Large images and successful mutation compatibility

Reads and successful mutation results that exceed the ordinary64KiB result cache
share at most2 retained images, each bounded at64MiB and120 seconds. The encoded
buffer budget is128MiB across actor results, in-progress encoding and pinned page
exports. A quota claim travels with the same Arc image and is released only after
the last pin drops; eviction cannot falsely release bytes that an export still
retains. Writers reserve quota before bounded allocation growth. Owned encoded
buffers are zeroized/freed before releasing their claim.

This preserves a valid near64KiB configuration save whose returned document plus
JSON metadata exceeds64KiB: the write remains Succeeded and the complete result
is available through result_begin. A missing/expired mutation image stays explicit
ResultUnavailable; it does not turn a successful file write into a false failure
or automatically rerun it. Read-image expiry is distinguished as ReadExpired.
If encoding exceeds the per-image/aggregate bound, an explicit output-limit code
is reported, with no fake empty success.

Image descriptors reuse snapshot_id/epoch/size/digest/observed_at/cursor, adding
expires_in_ms. The existing snapshot_page/release codec is reused. Clients can
yield between pages and validate complete bytes before JSON decoding. Snapshot
exports retain their separate8MiB image/16MiB aggregate/30s policy. Existing
mutation small-result cache remains128 entries/1MiB total.

The added encoded read/result cap is128MiB; combined declared server budgets are
approximately189MiB for large images, snapshot exports, pending uploads, waiting
commands, one active command, cached snapshot and small results. This excludes
bounded metadata, JSON/domain producer allocations, caller copies and allocator
overhead; it is not an exact RSS/performance claim. Actual OS/device budgets remain
a measured acceptance gate.

## Secret binding retries

Secret access writes use the existing native SecretStore boundary only after
explicit authenticated admission. The same volatile policy covers known raw
free-text commands: DebugHttp headers/body, DebugRequest path/query and
ConfigurationSave YAML/comments. These can carry low-entropy passwords or tokens,
so an unkeyed persisted request digest would be an offline verifier. No raw value,
header/body or secret-derived offline verifier is journaled for these classes.
The durable fingerprint field contains a random
volatile-secret-v1 marker independent of the input. Same-manager duplicate/conflict
proof uses at most128 zeroizing RAM fingerprints and constant-time comparison.
After eviction/restart, the same durable key is IdempotencyUnverifiable with the
original receipt unchanged. The client can inspect status/list and make an
explicit new user-intent decision; it must not silently omit the value from
comparison, invent a match or automatically generate a new mutation key.
Fixed metadata commands retain their existing durable request digest. This finite
classification is not general DLP for arbitrary project data. A deliberate
configuration save can still write its chosen YAML to the project; the journal
policy does not redact or silently rewrite user content.

## Discovery and preferences

The discovery service is created once by the worker, loads only a saved root and
does not create/scan a user-home default in manager fixtures. The service performs
walking/watch collection outside Workspace locks. The worker takes immutable scan
results and reconciles registry state, then publishes the cached observation.
Shutdown takes/drops the service and joins it before releasing the owner permit.
Retained ActorHandle/OS-signal handles cannot keep another watcher alive.

Explicit root changes normalize and validate the folder with the existing OS path
adapter, then use transactional SqliteSettings.patch to change only project_folder.
Language, theme and unknown preference fields are not overwritten by a stale
read/modify/write. GUI settings use the matching atomic writer prerequisite.
Missing saved folders remain visible/degraded rather than being silently created.

## Resource subjects and recovery

Worktree commands resolve their persisted source project, including after child
registration/archive, matching the existing control-adapter binding. Dedicated
cache commands resolve their persisted project owner even after deselection.
Admission uses manager-origin cached metadata and the worker resolves current
metadata again before execution. Missing, ambiguous, malformed or mismatched
resource subjects fail without executing a differently labeled command.

A failed CreateProject can retain a separate recovery_project_id only after a
trusted before/after proof: same operation/root, absent destination before,
uniquely new registry row at the canonical expected destination, exact selected
metadata and actual filesystem identity. The receipt's root/source namespace is
never relabeled from an arbitrary Error.project_id. Unsupported/unproven partial
associations are not invented.
WorktreeCreate additionally binds the actual review, before/new receipt and
registry sets, unchanged source metadata/OS identity, exact input/failed stage/
error code, canonical manager-owned child tree and actual child identity. Its
claim-window child can have no worktree marker yet or the exact new record marker;
a different marker is rejected. Recovery remains a separate child association,
not a replacement for source ownership or a new grant.

## Cutover and evidence limits

Open target delegates the existing registered-project resolver. Terminal now uses
the separately accepted isolated CLI shell path; test fixtures do not launch
Terminal.app or a native terminal window. Actual native window behavior remains
an E2E gate. The manager retains legacy AppLease exclusion; a future thin GUI
needs its separate client singleton and must contain no Workspace fallback.

Local isolated tests exercise real registered-project reads, large source data,
bounded polling metadata/no read SQL growth, exact failure details, secret retry
and journal privacy, saved-root discovery, atomic preference integration,
background-read busy separation and stable owner listing. Quota/input policy tests
cover shared pin lifetime and allocation rejection. SDK tests independently cover
complete large read/result images and native cancellation access. These are not
native keychain dialogs, GUI cutover, minimum OS, signed release or all-platform
acceptance evidence. Those gates remain separate and source-mapped.

Focused local macOS ARM64 results:6 facade integration cases and2 quota/input
policy cases passed. The manager actor regression suite passed10 cases, including
the independently revised deterministic capacity/expiry fixtures. The finite
debugger adapter boundary regression passed; its unknown-scope denial remains.
System analysis supplied5 resource/Create policy tests plus2 new worktree recovery
proof cases (the earlier5 were reused for the extension). A final focused secret
lookup/eviction/restart case passed, and manager all-target clippy with warnings
denied passed. These are scoped verification results, not a whole-workspace or
native GUI/terminal-window/platform release pass.
