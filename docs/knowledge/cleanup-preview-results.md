---
type: Implementation Result
title: E05.2 bounded runtime cleanup review preview
description: Read-only exact runtime reference and lease observations plus E06 restore-drill receipt inventory integration.
status: draft
decision_status: working-direction
tags: [storage, cleanup, e05, e06, leases, ownership]
sources:
  - id: plan
    resource: /integrated-execution-plan.md
    title: E05 inventory then reviewed preview without expanding purge semantics
  - id: preview
    resource: ../../crates/workspace/src/cleanup_preview.rs
    title: Runtime removal review projection
  - id: lease
    resource: ../../crates/runtime/src/package.rs
    title: Existing-file read-only removal lease observation
  - id: metadata
    resource: ../../crates/projects/src/storage_metadata.rs
    title: Bounded opt-in active and inactive registry metadata
  - id: inventory
    resource: ../../crates/workspace/src/storage.rs
    title: Owned restore-drill receipt and pending generation inventory
  - id: drill
    resource: /restore-drill-results.md
    title: Retained non-activating drill candidates
---

# Implemented review boundary

`Workspace::runtime_cleanup_preview(package_id, PreviewLimits)` returns blocked,
review_candidate or unknown. It includes a bounded exact descriptor SHA-256,
nonsecret managed references, an instantaneous kernel lease observation, stable
problem codes and logical apparent bytes when measured. This is not authorization,
a review/apply token, a promise that removal will succeed, or an estimate of
allocated/freeable bytes. Existing runtime removal and other apply paths are not
changed or wired to the preview.

Known project PHP/Node/Composer and matching web runtime selections, including
inactive/forgotten registry rows, block the matching physical package key
(kind/version/variant/target). Catalog aliases do not hide references. Different
URL, SHA or executable/resource maps under that same physical key also block;
the preview reports physical_package_descriptor_collision_unverified. Full
installed receipt matching remains exact and is a separate ownership check. Registered database and
cache instances, active/retained/pending database generations and pinned retained
worktree configurations also contribute references. Current project proxy defaults
and PostgreSQL restore-tool dependencies are conservatively retained. Debug-provider
reference coverage remains explicitly unverified for php-debug packages.

The default active-only `storage_metadata` API is unchanged. New opt-in
`storage_metadata_all_registered` includes all registered projects and binding
history with the same row, per-record UTF-8 byte and total byte limits. Unresolved
records, references, worktree sources or exact pins, unknown ownership, malformed
receipt hash metadata and truncated scans prevent a review candidate. Worktree
source files and SQL payloads are never read. Only bounded owned receipt metadata
and the current in-memory catalog are used; secrets are not resolved.

A candidate requires a catalog-matching installed receipt with well-formed stored
hash metadata, complete relevant metadata observations, no known managed references
and an existing lease file observed unheld. It does not establish payload integrity
or a census of processes that bypass DevForge's leases. It is deliberately a
bounded, non-atomic managed-context observation. Future apply must recheck the
catalog/receipt/reference state and acquire its own exclusive lease; the descriptor
digest alone grants no action.

# Read-only lease observation

`PackageStore::observe_removal_lease` opens an existing lease file with read access,
final-component no-follow/reparse protection and handle metadata checks. Unix
checks additionally require current-user ownership, one file link and matching
opened device/inode. Runtime root/lease directories and the bounded store marker
are checked first. No lease contents are read, and no file is created, chmodded,
truncated or deleted. Unknown/missing/unsafe/inaccessible files remain unknown.

An exclusive nonblocking kernel probe detects existing managed shared leases. A
successful probe is explicitly unlocked before returning unheld_at_observation;
that state is not retained as authority. Windows owner/ACL proof is not added by
this helper. The Windows read-handle design follows the official
[LockFileEx access contract](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-lockfileex),
which allows a handle opened for read access; actual new Windows execution remains
a separate CI/native gate.

# E06 inventory integration

Storage inventory now recognizes `projects/restore-drills/<project>` only through
both exact ownership markers and bounded typed receipts. Per-receipt reads are
capped at 16 KiB, the receipt scan at 4 MiB and the existing entry/resource limits.
Linked/reparse paths, unknown markers and malformed/oversized receipts remain
unknown and are not traversed as owned payloads.

A matching non-active ready=false binding is reported as a restore-drill candidate
retained for review. Without that registry association the receipt's generation
reference remains unverified. Ordinary pending retained generations are now labeled
as pending/unverified rather than implying a verified retained connection.
Shared physical database generation size remains unattributed; receipt file lengths
are logical metadata size only. No database, role, credential, backup, branch,
archive, mail or log deletion is performed.

# Evidence and remaining scope

Focused local macOS ARM64 checks passed:

- Runtime existing-file lease probe: one test covering missing/no-create, held
  shared lease, released probe and linked-file canary preservation.
- Bounded registry metadata: four tests, including active-only default preservation
  and inactive oversized records without payload allocation.
- Cleanup preview baseline: five tests covering candidate/held/missing states,
  inactive exact pin aliases, pending generation references, retained worktree pins,
  incomplete coverage/limits and malformed stored hash metadata.
- Collision follow-up: two new regressions reproduced false review_candidate
  results before fixes. URL/SHA/map changes with identical physical keys cover
  inactive records and pinned retained worktrees; a separate inert web-header
  fixture covers the derived web dependency prefilter. Both new tests and the two
  affected existing reference tests passed afterward. Unchanged baseline tests
  were reused rather than rerunning the other suites.
- Storage inventory: nine tests, including owned drill receipt association,
  oversized/unknown ownership and retained secret-reference redaction.
- Scoped runtime/projects/workspace library and test clippy, owned-file formatting
  and diff whitespace checks passed.

No real runtime process, native credential store, network, PostgreSQL/MySQL service connection,
GUI or user data scan was invoked by this slice. Inert resource packages and
isolated SQLite metadata fixtures were used; kernel lock behavior was exercised.
Windows/Intel execution, user-facing review UI, operation grants, reviewed apply,
physical drill cleanup, retention and scheduling remain separate work.
