# Verification and evidence

Read when implementing, reviewing, measuring performance, or preparing releases.
Scale verification to changed behavior; do not run the entire release matrix for
a wording change or add tests that only mirror implementation details.

## Code checks

Use the repository's actual scripts, workspace layout, toolchain, and lockfiles.
Once a Cargo workspace exists, typical Rust checks are `cargo fmt --all -- --check`,
focused `cargo test`, and `cargo clippy --workspace --all-targets`. Respect existing
warning policy; do not introduce stricter global gates as an incidental change.
Run relevant frontend type checks and builds from the package scripts that exist.

For dependency or toolchain updates, check manifest/lockfile consistency and any
minimum compiler, plugin, OS, and architecture requirements affected by the change.
Exercise the changed integration on both platforms where applicable; include
configuration/schema migration and recovery checks when the update changes them.
A resolved lockfile or successful compilation alone does not prove upgrade safety.

For UI changes, follow the task-specific skill sequence and finishing checks in
[ui-ux-workflow.md](ui-ux-workflow.md). Keep source-guideline review, visual target
comparison, and actual desktop interaction checks distinct; no one check replaces
the others. Read-only audits report findings without changing the product.

For runtime behavior, useful tests cover desired/observed state transitions,
executable selection, argument preservation, package/architecture selection,
configuration recovery, and persistent-data ownership. Use small fixture programs
or isolated temporary directories for failure and process-lifecycle tests.

## Platform evidence

- Run platform-sensitive checks on both macOS and Windows, locally or through
  authorized CI. A cross-compile proves compilation for that target, not runtime
  behavior. A browser preview does not prove Tauri IPC or OS integration.
- Derive the supported OS/architecture matrix from actual project decisions and
  manifests. Do not claim Intel Mac or Windows ARM support without its build,
  runtime-package, and validation evidence.
- For a desktop integration slice, exercise the packaged app when packaging or
  system behavior is affected: launch, command/event flow, tray/window lifecycle,
  service readiness, child cleanup, and re-launch as applicable.
- For a managed PHP slice, verify HTTP behavior and CLI version selection, then
  the requested HTTPS/restart behavior. Keep fixture projects and databases separate
  from the user's real projects and data.
- For installers and updates, use disposable test environments where possible.
  Verify relevant first-install, upgrade, denied-permission, interrupted-operation,
  and uninstall behavior; preserve project/database data throughout.
- If a target machine, runner, signing identity, or dependency is unavailable,
  complete independent work and report exactly what remains unverified. Do not
  replace missing Windows runtime evidence with a macOS success claim.

## Localization checks

Once application catalogs exist, check required-locale key coverage, placeholder
contracts, plural/message parsing, registry consistency, and missing-key behavior.
Exercise Russian, Uzbek, and English across the affected UI and owned desktop
menus; check language switching, persistence after re-launch, unsupported-locale
recovery, text expansion, focus/accessibility labels, and locale-sensitive formatting
on each affected target. A representative extra-locale fixture should prove that
adding catalogs/registry data does not require editing domain/UI language branches.
The OKF validator does not validate application translation catalogs or prove
runtime language support. See the
[localization contract](../../../../docs/knowledge/localization.md).

## Performance

Measure the built artifact and distinguish debug from release builds. Record OS,
hardware, app/package versions, workload, and measurement method when reporting
numbers. Relevant measurements include idle CPU/RAM, launch time, service readiness,
UI response during log bursts, and watcher/event-buffer growth with many projects.

Separate DevForge overhead from PHP/database/Node resource usage. Rust alone does
not guarantee a faster managed Laravel application or lower total system memory.
Use profiling and explicit targets to guide optimization; label proposed budgets
and architectural expectations as such until measured.

## Knowledge completion

Before closing a significant change task, assess and update relevant project
knowledge, then run the strict validator using the environment documented in
[validation-tooling.md](../../../../docs/knowledge/validation-tooling.md).
At phase completion, compare the affected documented claims against code and
verification evidence. Follow the task-close and phase-close procedures in
[knowledge-policy.md](../../../../docs/knowledge/knowledge-policy.md), preserving
read-only task scope and explicitly recording unavailable platform evidence.

## Completion evidence

Report what changed and the checks actually performed. For platform-sensitive work,
state macOS and Windows status separately, including skipped checks and their
reason. Keep build success, runtime success, signing, publishing, and installation
success distinct. Commit, push, and external release actions stay within the user's
requested scope.
