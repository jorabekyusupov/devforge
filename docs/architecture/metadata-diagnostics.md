# M13.5 metadata diagnostics

This first slice uses existing ToolInput/ToolOutput and prepare_read for Health,
Log, Inspector and Mail. The new diagnostics_read RPC is finite; other canonical
handlers remain unavailable here. Public health.doctor aliases the existing
Health projection without changing legacy health.health or CLI api health.
The four safe metadata tools join the default read-only MCP surface (default9,
all existing action opt-ins29); no new surface flag grants authority.

## Authority and output

In the original metadata slice, Observer grant, project registration/root/filesystem
identity and revision were checked; content issuance and all log query/mail content
filters were denied. A02 and A03 change issuance and log query as recorded below;
mail content filters remained denied before source IO until the A06 follow-up below. MCP omits and rejects those
keys, including explicit null. Level/context metadata filters remain scoped.

Outputs allow only health codes/retry/next-action IDs; Inspector context/method/
status/query count/duration; log ID/level/stable code/time; and mail ID/received
time/attachment count. Raw native Values, event name/data, exception/stack, SQL,
URL/query/header/body, mail addresses/subject/body/links and artifacts are not
metadata output. Doctor uses cached safe facts rather than global settings or
native credential/DB/process probes. Existing stores are read without creating
mailboxes, schema, collectors or credential state.

## Frozen windows and cursors

The existing Cursor gains optional context with client/grant, filter digest,
source epoch, high-water and an opaque per-page handle. Live continuation
requires context and the exact bounded server-held handle. Project/tool/revision/
root/filter/context/client/grant and source identity are revalidated. The sequence
boundary uses Record.sequence rather than SQL OFFSET or timestamp ordering.

Only already-safe metadata is retained: at most16 frozen windows/8MiB,
128 handles and120seconds. ProjectionFreshness.window optionally reports epoch,
high/low water, row/byte truncation and bounded=true. Missing optional fields
preserve older serialized output. File replacement or manager restart expires
continuation. Clearing rows in the same native store does not invent a writer
epoch: an already observed frozen window can remain Cached with its original
time until expiry. It is never described as current live data or complete history.

Existing retention limits still apply: Inspector200 events/256KiB, logs4000
events/24hours, mail200 messages. Window pagination does not restore discarded
history. Later M13.5 content/correlation/wait/artifact work remains a separate
slice with explicit authorization and acceptance.

## Verification

Five initial metadata reader tests and the exact 51-attachment compatibility
case passed. QA found that the UI's 50-part index bound had incorrectly limited
stored attachment count; only the read guard now accepts a nonnegative JS-safe
count, with writer/schema/part access unchanged. QA source review closed that
blocker and found no other demonstrated gap in the requested critical paths.
Manager policy1 passed for noncreating/missing sources, scalar canary allowlists,
actual sequences/frozen Cached pages, cursor tampering/filter/grant/epoch fencing,
mail metadata and revoke. Three consumer/parser cases passed, including rejection
before read transport; no duplicate producer startup. Scoped clippy, typed tool
contracts10 plus generated-export exact rerun1 and default9 contract1 passed.

One isolated actual macOS ARM64 flow used copied Laravel13.42.0 vendor/manifests
and receipt/hash-verified PHP8.4.27: actual Foundation Kernel→existing managed
Collector and SMTP, four public Observer SDK reads, four stdio CLI aliases and
the cached official MCP Inspector client. Private log/mail canaries were verified
in storage and absent from every public output. MCP EOF preserved the manager;
owned cleanup completed. No source project process, downloads, HOME/global config,
native credential service or application HTTP server was used.

The first two harness attempts stopped before provider boot/capture: long system
temp socket path, then copied autoloader's source-local companion reference.
Only the owned fixture path/classmap changed; product guards were preserved.
The successful flow was not rerun for routine final checks. Windows/native UI,
minimum OS and richer content/correlation/wait/artifacts remain separate evidence
and subsequent M13.5/M13.9/M15 work. Publication and source references are in
[execution status](../knowledge/execution-status.md).


## A02 authorization follow-up

The [grant boundary](external-grants.md#a02-explicit-content-rights-2026-11-16)
now supports explicit native-owner InformationScope selection. Metadata reads use
its central `authorize_read(None)` gate at admission, dequeue/source and output.
At A02, content grants did not install live readers and query/subject/recipient
filters remained refused before source IO. A03 changes log query as described below;
A06 changes mail content filtering as described below.
Log/Inspector/mail/source-artifact/DB content consumers remain later packets.
The initial statement about no issuance describes the original metadata slice;
A02 changes issuance only, preserving its metadata-only output/privacy contract.


## A03 scoped log content and actionable doctor (2026-11-16)

`logs.search` keeps its metadata-only default. Explicit `content=true` or any
`query` requires the grant's independent LogContent right, checked against the
registered project and revision before source IO, during queued work, and before
output. Wrong project, missing/expired/revoked right and foreign cursor fail closed.
The cache binds the original client/grant, project, revision, content mode and
filter; a separate source epoch distinguishes metadata and content windows. MCP
and the Rust SDK expose the content request; the existing JSON CLI log command
continues to expose metadata only.

Content uses the existing noncreating, read-only project-scoped SQLite reader,
with the existing 4,000-row/24-hour source bounds and a 4 MiB projected-window cap. The `since` timestamp
and level filter also work for Observer metadata; explicit content/query needs
LogContent. Only log/exception messages are projected; filtering occurs after bounded masking, never over raw body. The
masker handles recognizable credential assignments, Bearer values, URI userinfo
and long token patterns. It is best effort: arbitrary unlabeled secrets cannot be guaranteed
removed, and LogContent remains explicit sensitive access. No separate raw context, stack,
SQL, mail or bulk-file fields are returned. Empty and absent stores remain noncreating.

Health doctor still reports cached snapshot origin and now includes stable evidence
and impact codes next to the existing issue/retry/next-action codes. Missing runtime
in the snapshot means not confirmed there, not a live process diagnosis. Additive
optional fields preserve metadata clients. The scoped tests cover project/right/
revoke/cursor isolation, metadata privacy, redacted-query inference, legacy and
Bearer patterns, Unicode/control normalization and cached doctor evidence. Exact
platform CI and limitations are recorded in the PR; no native UI or OS credential
service acceptance is claimed by these tests.


## A04 scoped Inspector detail (2026-11-16)

`inspector.request` remains metadata-only by default. Explicit `detail=true`
requires InspectorQueries for the registered project. The existing read gate
checks the right and the frozen project revision before source IO, at queued
execution, and before returning output. Windows and cursors are bound to client,
grant, project, revision, detail mode and context; an unrelated grant or mode
cannot reuse a cursor. The reader is read-only and noncreating, with the existing
200-event/256 KiB retained-source boundary and a distinct detail epoch.

Each detail row represents one captured request, job, query, cache or dump event.
HTTP request data is the normalized method, status, captured duration and available
profile metrics; the request URL and headers are not projected from request events. Query names are normalized to
remove literal values and comments, including MySQL `#` tails; repeated_count is
only the count of identical normalized SQL in the same context and retained window,
not proof of N+1 or complete history. Dump returns a bounded best-effort masked projection; oversized or invalid
payloads are withheld. SQL-labeled dump fields and recognizable SQL-shaped dump
strings are withheld as well. Cache reports only hit/miss/write/forget; keys, values, tags and store names
are not captured. Masking is conservative and cannot guarantee identification of
all arbitrary secrets embedded in unstructured names.

Detail `duration_ms` is present only for measured request/job/query events;
cache/dump durations are unknown. The mandatory outer `elapsed_ms`/`queries` are
legacy request-summary fields and may contain placeholders for non-request events;
consumers of detail rows should use the optional nested metrics. `truncated` denotes
projection text truncation, while `capture_truncated` records the collector flag
only when present (absence is unknown). Window row/byte truncation remains separate.
Captured sql_ms and peak_memory_bytes are optional. All timestamps use Unix ms.

The companion now captures finite cache operations in an active context without
reading event keys or values. Exact earlier owned companion versions remain safely
upgradable; modified user files are refused. The focused offline Laravel 13 cache
repository/dispatcher fixture and affected Rust source, grant/cursor and SDK/MCP
checks are recorded in the A04 PR. The macOS actual producer fixture is not Windows
native producer evidence; JSON CLI Inspector remains metadata-only. No A05 work or
bulk source access is implied.


## A05 activity checkpoints and bounded wait (2026-11-16)

`activity.mark` records an opaque manager-held checkpoint for one registered
project, client and grant, selecting Inspector or logs and optional context. The
credential-bound project is resolved before any requested-project lookup, so
existing and nonexistent foreign project IDs cannot be distinguished. A missing
collector store fails noncreating precondition. `activity.wait` observes
only records whose project sequence is greater than that checkpoint, returns the
earliest matching observed event and an advanced bookmark, or a typed timeout.
The caller may narrow a context-free mark to an Inspector context after an action;
the returned bookmark remains bound to that context. Inspector detail requires
InspectorQueries; log detail requires LogContent. Metadata needs Observer. Every
source read and final response rechecks the original grant, project and revision.

A bookmark retains its original 120-second expiry, is held only in manager RAM and
may be evicted at the bounded capacity. A changed/replaced source, missing project
anchor, expired bookmark or detectable rewind requires resynchronization. The
source checks the project-specific anchor and first omitted row in one read-only
SQLite transaction; gaps from other projects' global sequence values do not imply
loss. If retained rows or bytes hide a post-mark range, the wait returns an explicit
limit error instead of reporting a clean timeout.

Each wait polls for at most 3000 ms with at most four simultaneous waits. No actor
or SQLite lock is held while sleeping. An in-flight read and final authorization may
finish during up to 1 second of validation grace; registry SQLite contention can
add up to 250 ms. Scheduling and the 5-second transport limit still apply, so this
is not a hard real-time deadline. Native quit interrupts waits. One MCP stdio session serializes
its calls while a wait is active; another manager client remains responsive.

Every result says `continuity=unverified`: polling cannot prove that an event was
not inserted and deleted or pruned between polls, especially from an initially
empty project. `timed_out` means no matching event was observed in the polling
window, not proof no event existed. If the deadline expires before any source
read completes, the call returns a retryable `TimedOut` error instead of a
successful observation. Sequence after a mark is temporal evidence,
not proof that an action caused it. Context narrows the association; HTTP-probe
correlation and mail waiting remain A08/A06, respectively. Exact platform/test
results are recorded by the A05 PR; no unrelated source or bulk data is exposed.


## A06 scoped mail detail and wait (2026-11-16)

`mail.list` remains metadata-only for Observer by default. `since` alone is
metadata-safe. Explicit detail, mail_id, sender, recipient or subject filters
require the project-bound MailContent right before mailbox IO and again before
output; empty filters are still explicit content requests. `mail.show` is a thin
MCP alias for one ID with detail, including an optional page byte budget. Exact ID
lookup is independent of newer message volume. Filter-only metadata does not parse
MIME or retain old verification links. Time and filters are bound to opaque cursors;
public cursor filter tokens are random and filter-equivalence digests remain private.

Authorized detail may contain subject/from/to, masked text, attachment name/MIME/
size metadata and HTTP(S) link candidates. Candidates retain query tokens so they
can be used, may contain private verification/reset credentials, and are never
fetched or validated by DevForge. Trailing punctuation can produce both trimmed and
original candidates; clients must not assume either was verified. Body text and
headers are masked on a best-effort basis for recognizable patterns; arbitrary
unlabeled secrets cannot be guaranteed absent. No raw MIME, attachment bytes,
automatic external send/fetch, export or delete is exposed by these tools.

The noncreating read-only source keeps the existing 200-message/16 MiB window;
detail parsing is bounded to 16 matching messages and 4 MiB raw/4 MiB projected per
list window, 2 MiB per message, 16 link candidates and 50 attachment entries.
Truncation is explicit; use metadata listing plus `mail.show` for an exact ID.
Mail activity marks bind sender/recipient/subject/since and the project/grant to the
existing A05 bookmark. A filtered metadata-only mark requires MailContent but does
not parse MIME. A detail wait checks the same mailbox anchor in its read-only
snapshot, scans retained summaries and parses only the earliest matching new MIME;
a burst of 17 messages does not create a false gap. A05 `continuity=unverified`,
3s polling plus bounded validation grace, expiry/revoke and stdio serialization
limitations remain in force. Mail context_id is invalid.

Sensitive source buffers and cached detail strings are zeroized on drop. Existing
one-second maintenance prunes expired windows/bookmarks when it can acquire the
locks; this is near-TTL cleanup, not a hard real-time guarantee. Grant revoke evicts
cached windows, and a failed in-flight sensitive read removes its own newly inserted
window to close revoke/insert races. A same-file native deletion does not change the
source epoch: a frozen cursor can still return rows included in its server snapshot
until TTL, even if a deleted row was not previously delivered to the client. Such
results are labeled Cached and retain the original observation time. Revoke evicts grant windows; deleting a local
message does not itself revoke a remote verification token. Responses to authorized
clients intentionally contain candidate link tokens. Existing JSON CLI mail command remains metadata-only.

Focused source and manager tests cover authorization/noncreating IO, project and
filter/cursor scope, opaque filter tokens, candidate URL extraction without fetch,
attachment metadata, old-ID lookup behind large newer messages, burst wait and revoke.
A disposable real SMTP→MCP mark/wait/show fixture passed locally on macOS. The PR
records exact CI for macOS/Windows; actual Windows SMTP/MCP producer evidence is not
claimed by the local fixture. Attachment export/delete remain separately reviewed.
