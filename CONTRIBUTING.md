# Contributing to DevForge

DevForge is a native desktop development-environment manager with a shared Rust
core and React/Tauri presentation. Start with [AGENTS.md](AGENTS.md), the
[DevForge skill](.agents/skills/devforge/SKILL.md) and the
[knowledge index](docs/knowledge/index.md). Use the versions in manifests,
lockfiles and `rust-toolchain.toml`; do not update unrelated dependencies.

## First change

1. Inspect Git status, the affected source and its applicable instructions. Keep
   unrelated local work. A milestone report or plan is not proof of current runtime
   behavior; identify the exact source and evidence relevant to your task.
2. Write the observable outcome, affected contracts, file scope and acceptance
   cases. Look for an existing fixture before creating a second test harness.
3. Use a focused branch/worktree when concurrent work warrants it. Agree on file
   ownership and shared API revisions before parallel edits. One writer owns a
   shared DTO, locale catalog, lockfile or integration file at a time.
4. Reproduce the fault with the smallest meaningful fixture, or define a behavior
   test for the new capability. Implement only the coherent task slice.
5. Run the checks selected for that change, review the diff, update the affected
   knowledge and hand off evidence with remaining limitations. Commit/push only
   within the task's authorized publication scope.

The [integrated execution plan](docs/knowledge/integrated-execution-plan.md)
contains the current work IDs and dependencies. Read the affected subsystem's
source before deciding that a planned API or example already exists.

## Safe fixtures and data ownership

Use disposable project folders, data roots and database instances. Tests that
support `DEVFORGE_DATA_DIR` must point it at a dedicated temporary directory,
not the user's app data. Use test SecretStore adapters for automated fixtures;
never export credential values into snapshots, logs or evidence.

Keep project/database data separate from package binaries and test caches. Do not
stop, migrate or remove Herd, Homebrew, system or user-owned services to make a
test pass. Use free loopback ports and owned process trees. Do not share a mutable
runtime store, database, port or trust configuration between concurrent tests.

Native credentials and hosts/trust changes need the relevant explicit fixture or
interactive acceptance procedure. Ordinary unit tests must not silently invoke
ignored credential tests or change system trust. Unavailable permissions, hosts
or signing identities are reported as gaps, not replaced with a mock success.

## Select checks by impact

Do not run all tests after every edit. Record the changed behavior and its direct
consumers, then choose the smallest coverage that verifies them. These are
examples, not a checklist to run in full:

| Change | Focused starting point |
| --- | --- |
| Manifest parsing/pin resolution | `cargo test -p devforge-projects --test manifest` |
| Project storage/binding migration | `cargo test -p devforge-projects --test projects` |
| Reproducible setup/export | `cargo test -p devforge-workspace --test repro` |
| Workspace lifecycle | `cargo test -p devforge-workspace --test worktrees` |
| Configuration view | `npm run test -w @devforge/desktop -- src/configuration.test.tsx` plus `npm run typecheck` |
| Translation catalog | `npm run check:locales`; parser/registry changes also need the relevant locale regression tests |
| Distribution helper | `python3 -m unittest scripts/test_distribution.py` and the affected package scenario |
| Knowledge/documentation | Strict validator with the prepared interpreter; scoped whitespace/link review |

Use `scripts/with_rust.sh` before Cargo-dependent commands on the prepared macOS
machine when Cargo is not on PATH. For knowledge tooling, follow the existing
[interpreter setup](docs/knowledge/validation-tooling.md); do not reinstall a venv
or change global Python just to validate a document.

Expand to direct consumer crates, adapter contracts or native slices when shared
types, process lifecycle, schemas or behavior change. A workspace-wide test/build
or full native matrix needs a concrete cross-cutting reason or release acceptance
requirement. Required CI checks remain required; focused local testing does not
bypass them. CI optimization is its own reviewed change.

After a failure, fix and rerun the failed test and affected regressions. Repeated
runs do not resolve an unexplained flaky failure. Distinguish runner/network/tool
failures from product failures and preserve the first failure when recording a
successful retry.

## Reuse valid verification

Record the work ID, source revision or relevant file hashes, test command,
OS/build/architecture, fixture/tool versions, result and artifact references.
A result can be reused while its code, contract, fixture, toolchain and target
remain unchanged. Explain reuse rather than saying the test ran again.

A docs-only commit does not invalidate a native artifact when the tested product
sources are unchanged and that correspondence is recorded. A changed public
contract or schema invalidates its affected consumer evidence. At integration,
run the shared regression once under the integrator rather than once per agent.

## Desktop and platform evidence

Use the [UI skill routing](.agents/skills/devforge/references/ui-ux-workflow.md)
for the actual task. Preserve the existing design system; do not regenerate it
for a small fix. UI changes include relevant loading/error/recovery states,
keyboard/focus behavior, RU/UZ/EN text and accessible names.

Source review, browser preview, compiled binary, native interaction and installer
acceptance are different evidence. Report macOS ARM64, macOS Intel and Windows
results separately. A Windows Server runner is not actual Windows 10/11 desktop
acceptance. Cross-compilation is not execution. Keep minimum-OS, native credential,
system dialog, signing and release gates explicit.

## Handoff and pull requests

Lead with the trigger/problem and resulting behavior. Include the exact changed
scope, focused checks actually run, valid results reused and material gaps.
Tests should verify observable contracts and important failures, not mirror each
implementation step. Do not add redundant tests for a reversible wording edit.

For a significant change, reconcile source/configuration and evidence with the
affected knowledge record, update its provenance and the knowledge log through
the designated integrator, then run the strict validator. Structural validation
does not establish semantic correctness or native behavior. Do not label a
proposal as implemented or a platform as verified without its actual evidence.

Preserve project/runtime selections and user data across migration and cleanup.
Application signing, updater signatures and runtime artifact integrity are
separate checks. Never put credentials, private keys or real `.env` content in a
commit, screenshot, support artifact or PR description.
