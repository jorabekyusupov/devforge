# E07.1: metadata-only offline readiness

Status: implemented backend slice; no GUI, remote catalog refresh or bundle distribution.

## Context

The integrated E07 plan separates application updates, runtime catalog lifecycle,
exact project selections and offline preparation. A network check or a package
folder alone cannot establish that a Laravel project will run offline. Composer
now has a persisted exact selection; a later catalog default must not replace it.

## Decision

`Workspace::offline_readiness(project_id, OfflineLimits)` returns selected managed
artifact requirements and limited project metadata prerequisites. It uses the
bounded project registry projection and reviewed in-memory catalog. PHP, Node and
Composer come from persisted project pins; DB/cache/runtime packages come from
instance records. PHP web pairing and the effective Caddy default are explicitly
identified as derived/default selections, not falsely called project pins.

A bounded streaming catalog fingerprint identifies the current catalog snapshot;
it does not establish remote signature, publisher provenance or revocation status.
Existing superseded metadata is exposed without automatic version replacement.

Each requirement has available/missing/unsupported/unverified state, stable reason,
exact package ID/version/archive digest, selection basis and verification scope.
Available means matching catalog receipt and expected regular files are present.
It does NOT mean executable hashes, PHP startup, service readiness or offline
Composer/npm cache completeness were verified. Unknown selected IDs, corruption,
link/reparse, truncation and budget failures remain explicitly unverified.

No download, subprocess, lease acquisition, secret-store call, DB initialization,
config mutation, global PATH change or catalog fallback occurs. Existing ownership
marker and package receipt metadata reads use bounded final-component nofollow
opens and per-record/aggregate caps. Every path ancestor is checked for known
links/reparse points; this remains a non-atomic observation with ancestor rename
races, not a hostile filesystem sandbox.

Only if the registered root identity matches, vendor/autoload.php, .env and optional
node_modules presence is observed through metadata. Their contents are not read;
presence does not establish completeness. No raw paths, URLs, credentials or secret
references are returned. Debugger provider state outside managed catalog artifacts
is unverified. A missing/unsupported required artifact makes offline_ready false;
otherwise offline_ready remains unknown, never true from metadata alone.

## Limits and validation

Registry rows and bytes are bounded by the shared read-only metadata API.
Requirements, descriptor entries/paths, catalog fingerprint stream, each receipt
and aggregate receipt bytes are capped. Truncation cannot yield managed artifacts
available. Wrong-host selections are unsupported rather than silently substituted.

Focused temp-fixture tests cover receipt presence/corruption/budgets, missing files,
foreign target and unknown pin, exact Composer despite default/superseded changes,
partial registry/requirement limits, known symlinks, project metadata presence,
secret-store prohibition, lease-directory stability and selected DB/cache metadata.
The format-header fixtures are inert; no native PHP, DB, cache or executable launch
is claimed. Windows/Intel native execution is unverified by this backend test run.

## Remaining E07 work

Signed/reviewed dynamic catalog refresh, revocation policy, rollback, offline cache
bundle export/import, UI and actual offline/native startup acceptance remain later
slices. This report does not claim E07 as a whole complete. See the integrated plan
and E00 execution ledger for dependency and publication status.

## Implementation evidence for E07.1

Base checkout at handoff: `5ff658d`, plus the reviewed working-tree files below.
The six catalog fixture cases are green across focused runs. The initial lease
fixture incorrectly expected an empty lease directory after installing packages;
it was corrected to compare before/after entry counts. This was a harness
assertion correction, not a runtime mutation defect.

Commands executed (with the repository Rust wrapper):

- `cargo test -p devforge-workspace --test catalog`: initial five cases; four passed,
  lease assertion failed before the fixture correction.
- Exact default-change fixture rerun passed after correction.
- Exact DB/cache registry-pin fixture passed after its addition.
- After enforcing a strict aggregate metadata read cap, only the default-change
  and receipt-budget fixtures were rerun; both passed.
- The default-change case was rerun after adding its explicit provider/minimum-OS
  unverified assertion; passed.
- `cargo clippy -p devforge-workspace --lib --test catalog -- -D warnings`: passed.
- Scoped `git diff --check`: passed. Full products, native UI and remote CI were
  not rerun; unaffected prior fixture results were reused.

Minimum-OS/provider vendor support remains explicitly unevaluated. Metadata
availability is not support certification. Runtime payload integrity, actual
process startup, external DB/cache, secrets, npm/Composer caches and true offline
end-to-end acceptance remain unverified. No product files or saved selections
were changed by the report calls.

Source fingerprints at handoff:

- `crates/workspace/src/catalog.rs`: SHA-256 `0b0805573eae40f0c42007a4d2af5f354862c932540b6a50f8a63e93540f1b14`.
- `crates/workspace/tests/catalog.rs`: SHA-256 `d36e80f8936b49ffd48386d1802d97192ea9e18106e728992aeba857192552e1`.

Scope/dependencies: [integrated execution plan](../knowledge/integrated-execution-plan.md).
