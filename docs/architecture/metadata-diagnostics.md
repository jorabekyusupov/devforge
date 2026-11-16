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
mail subject/recipient filters, including an empty string, remain denied before
registration/source IO. MCP omits and rejects those
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
mail subject/recipient filters remain refused.
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
project, client and grant, selecting Inspector or logs and optional context. A
missing collector store fails noncreating precondition. `activity.wait` observes
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
finish during a 500 ms grace subject to scheduling/transport; this is not a hard
real-time deadline. Native quit interrupts waits. One MCP stdio session serializes
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
