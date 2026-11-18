---
type: Feature Contract
title: Runtime branch rows and manual patch updates
description: Release-family display, reviewed latest builds, explicit stopped-user updates and disconnected catalog visibility.
status: draft
decision_status: confirmed
tags: [runtime, updates, desktop, catalog]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: Manual branch update request
  - id: backend
    resource: ../../crates/workspace/src/runtime_updates.rs
    title: Native runtime updates
  - id: registry
    resource: ../../crates/projects/src/runtime_branch.rs
    title: Catalog policy and transactional reference updates
  - id: frontend
    resource: ../../apps/desktop/src/runtime-branches.ts
    title: Branch rows and selector preferences
  - id: public-catalog
    resource: ../../apps/desktop/src-tauri/src/workspace.rs
    title: Public bundled catalog command
  - id: postgres-policy
    resource: https://www.postgresql.org/support/versioning/
    title: PostgreSQL release families
  - id: mysql-policy
    resource: https://dev.mysql.com/doc/refman/8.4/en/mysql-releases.html
    title: MySQL LTS series
---

# Confirmed behavior

User approved a single PHP 8.5 row instead of patch rows, newest available patch
installation, manual Update button, and updating users of the same release family.
Cross-family upgrades remain explicit. User subsequently reported empty PHP and
other catalog sections; catalog visibility must not depend on authentication.[^request]

PHP CLI/FPM companions share one visible row. PHP/MySQL and other semantic tools
use major.minor families; Node.js and PostgreSQL/postgresql-tools use major
families. PostgreSQL minor numbers denote patch releases; MySQL LTS stays within
its named series.[^postgres-policy][^mysql-policy] Backend validates kind, platform,
variant and reviewed latest package identity. Versions with v prefix, numeric
publisher build suffix and staticphp numeric rebuild are recognized; other version
schemes/prereleases do not become rolling targets.[^registry]

# Implemented update contract

- Refresh reads installation metadata from the reviewed bundled catalog. It does
  not discover upstream internet releases or download binaries. A newer reviewed
  catalog still arrives through application/catalog maintenance.
- Install uses exact reviewed identity and installs PHP's matching server companion.
  Update first verifies and downloads the family, then changes stopped registry
  users. Existing project constraints/debugger compatibility remain guards.
- Running affected project, DB or cache services refuse update. No automatic stop
  or restart occurs. Initialized DB/cache instances hold their lifecycle locks
  through one SQLite reference transaction; data paths, names, credentials and
  other settings are preserved. Old packages remain available to existing terminal
  leases and recovery. Existing terminals keep their original binaries.
- Explicit updates replace active project PHP/Node/Composer references and stopped
  DB/cache package/.NET references in the same family. Forgotten records and exact
  YAML pins remain unchanged. New selectors prefer latest healthy installed builds
  while preserving an existing exact selection until explicit update.
- Explicit Caddy/Composer defaults are persisted separately in workspace preferences.
  Registry changes and preferences publication are separate durability boundaries;
  an error after registry commit must not be represented as a complete rollback.
- Snapshot branch/latest/update metadata keeps old retained cache packages from
  advertising an update forever after registry users have moved.[^backend][^registry][^frontend]

# Disconnected catalog

Native `runtime_catalog` exposes only bundled package metadata for the current
platform. It does not read credentials, inspect installed files, connect a manager
or authorize mutations. If the workspace snapshot is unavailable, the UI displays
these available branches with installation status explicitly unknown. Install,
update, remove and installed-only filtering remain disabled until status is known.
The existing explicit Connect flow remains authoritative.[^public-catalog]

# Evidence and limits — 2026-11-17

macOS host: 28 targeted frontend tests, 11 workspace runtime-control tests,
7 project branch/registry tests and 1 native public-catalog unit test passed.
`npm run build`, `npm run check:locales`, manager Cargo check and scoped diff
checks passed. The build retains its existing large-bundle warning. Browser fixture evidence is
under `output/playwright/runtime-branches-*`; fixture versions are illustrative,
not actual downloaded runtime installations. Source review covers shared Windows
paths and Windows PHP packaging; Windows runtime was not executed.

This work does not establish real DB-engine upgrade/rollback acceptance, Windows
native acceptance, live upstream release discovery or package publishing. An
already-running installed manager must be restarted with the newly built CLI
before it understands `update_runtime`; launching only a new GUI does not replace
that process. Real user services were not stopped to force that replacement.

[^request]: User confirmed manual update and reported empty sections in the active chat.
[^backend]: Workspace update planning, installation and explicit application.
[^registry]: Shared family policy and locked registry reference transaction.
[^frontend]: One-row families and exact-selection preservation.
[^public-catalog]: Native immutable current-target metadata, separate from authenticated snapshot.
[^postgres-policy]: PostgreSQL versioning and minor-release policy.
[^mysql-policy]: MySQL LTS series upgrade policy.

The idle previous manager was confirmed to have no child runtime processes and
was gracefully stopped through its registered SIGTERM/explicit-quit handler before
launching the newly prepared paired debug manager and GUI. No user package was
updated during these checks. Strict knowledge validation remains blocked solely
by the pre-existing unindexed `report-views-results.md` concept.


## Family display correction — 2026-11-17

PostgreSQL `default` and `server` catalog entries now share one major-family row;
the supported server variant controls latest/complete status and new selectors.
The legacy physical package remains visible in installed-build removal details.
This changes display/selection only and does not migrate cross-variant instances.

A PHP family with a healthy CLI but missing matching server companion is now
labelled Incomplete installation, with Complete installation instead of Install.
A complete family retains Installed with no install action; an older family with
a newer build retains the explicit Update action. These fix the user's reports
of duplicate PostgreSQL and contradictory Installed/Install states.

`runtime-family-status.test.tsx` reproduces both cases; the scoped frontend suite
passed 30 tests on macOS. TypeScript/build and locale checks passed. The running
dev GUI receives these frontend changes through Vite; native Windows remains
unverified. No package installation or database migration was performed.
