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
