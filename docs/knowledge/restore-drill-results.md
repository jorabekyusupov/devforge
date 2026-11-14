---
type: Implementation Result
title: E06.1 non-activating restore drill backend
description: Owned fresh PostgreSQL and MySQL restore verification with durable sanitized receipts and explicit retained cleanup state.
status: draft
decision_status: working-direction
tags: [backup, restore, e06, ownership, recovery]
sources:
  - id: plan
    resource: /integrated-execution-plan.md
    title: E06 disposable restore drill before retention and scheduling
  - id: drill
    resource: ../../crates/projects/src/backup/drill.rs
    title: Restore drill and focused metadata and native fixtures
  - id: restore
    resource: ../../crates/projects/src/backup.rs
    title: Shared bounded restore process and existing activation boundary
  - id: evidence
    resource: ../evidence/restore-drill/2026-11-14-macos-arm64.json
    title: Sanitized native outcomes and relevant source hashes
  - id: owner
    resource: ../../crates/projects/src/database.rs
    title: Frozen running instance identity and prepared staged provisioning
---

# Implemented boundary

`ProjectStore::restore_drill` takes an existing owned `RunningDatabase`, a project,
backup ID and expected project revision. It never starts/stops a server or project,
changes the active binding, calls connection activation, or deletes a backup.
Backup checksum, archive bounds, engine/exact database version, current revision,
active binding and the frozen running server identity are checked before work.
The server identity includes package, port, admin/secret reference, ready state and
canonical data root; prepared provisioning rechecks persisted identity under the
existing operation lock. A different server or changed instance is refused.

The existing restore primitive is reused for a newly generated database and role.
The archive is copied into an owned temporary directory and its copied payload is
checked again before provisioning. Existing bounds remain: 1 GiB logical archive,
300-second tool readiness/310-second supervision deadline, cancellation and bounded
connection checks. This is the existing owned logical `.dfbackup` format, not an
arbitrary SQL import API.

A passed result means restore tool success and a connection check of the new
role/database identity. It does not establish arbitrary application invariants,
full table/row equivalence, engine upgrade compatibility or recovery on another OS.
The native fixture additionally verifies a known table value in the new database.

# Receipt and recovery semantics

A pending candidate binding is retained before physical provisioning. A sanitized
`DrillReceipt` is durably written before that step and updated to passed, failed or
cancelled afterward. A failed candidate may represent no database, a partially
created role/database, or an interrupted restore; absence is not inferred. Errors
are stable `ProjectError` codes. Receipts contain no password, secret reference,
role, SQL body, process output or user filesystem path.

Receipt JSON is capped at 16 KiB. The new `restore-drills/<project>` namespace has
bounded exact ownership markers at both directory levels. Existing unmarked,
symlinked or reparse directories are rejected; only newly created directories get
permissions set. Receipt reads check those ancestors and bounded ordinary files.
These checks are not a hostile concurrent filesystem-rename sandbox or an atomic
snapshot of all registry and filesystem state.

Even a successfully verified candidate remains `ready=false` in retained history.
It cannot be silently selected by the existing generic retained-connection path.
The outcome lives in the receipt, separately from connection activation eligibility.
A crash can leave a provisioning receipt and pending generation; there is no hidden
replay, automatic activation or claim that physical cleanup completed.

Cleanup is explicitly `retained_requires_review`. Automatic physical database,
role/credential removal, retention preview/scheduling, GUI/CLI/control dispatch and
E05 inventory presentation of the new receipt namespace remain later slices.
Receipts are not deletion authorization. Unknown references are never deleted.

# Validation

Focused `cargo test -p devforge-projects --lib backup::` passed six tests; the two
explicit native fixtures are ignored by default. Tests cover stale/cancelled/
corrupt/incompatible preflight, bounded sanitized receipt roundtrip, wrong receipt
activation flag, directory symlinks/unmarked ownership and outside canary/permission
preservation, plus the existing backup checksum/path and external-option contracts.
Scoped projects library/test clippy and owned-file formatting/diff checks passed.

The explicit native command is:

```sh
DEVFORGE_DRILL_RUNTIME_STORE=/path/to/reviewed/runtime-store \
  scripts/with_rust.sh cargo test -p devforge-projects --lib \
  drill_preserves_live_binding_and_records_all_outcomes -- \
  --ignored --nocapture --test-threads=1
```

The fixture copies only current catalog-matching cached packages into disposable
roots and verifies local receipts/executable hashes before use. It never downloads,
opens native credentials, reuses an existing server/data directory, or changes user
projects, global development configuration, hosts or trust.

[Sanitized source/outcome evidence](../evidence/restore-drill/2026-11-14-macos-arm64.json)
records a dirty build and relevant source hashes, not a clean whole-product artifact
attestation. macOS ARM64 native PostgreSQL 18.6.0 and MySQL 8.4.11 passed success, invalid logical
dump failure and cancellation receipts. The backup contained canary 701 while the
live source was changed to 902. The drill restored 701 into its fresh target while
preserving the source binding, live 902, server PID and original backup hash.
Wrong server and drifted instance metadata are refused before physical staging.
The same fixture confirms ordinary `restore_backup` still explicitly activates a
new generation. Target-specific execution beyond macOS ARM64, native credential
services, actual GUI behavior and automatic cleanup remain unverified.
