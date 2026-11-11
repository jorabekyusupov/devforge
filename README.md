# DevForge

A macOS and Windows desktop foundation for managing local development environments.
The desktop provides local preferences, diagnostics, native menus/tray, and
Russian, Uzbek Latin, and English interfaces. M2 adds a Rust runtime package store
and process supervisor, exposed through the library and CLI. Project management
and runtime controls in the desktop belong to later phases.

## Development

Use Rust from `rust-toolchain.toml` and a compatible Node installation (CI uses
24.22.2). macOS needs Xcode Command Line Tools; Windows needs MSVC Build Tools and
WebView2. Python is only for repository knowledge tooling, not an app runtime.

```sh
npm ci
npm run check:locales
node --test scripts/check_locales.test.mjs
npm run typecheck
npm test
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
npm run tauri -- dev
```

The M1 task installed an isolated macOS toolchain without changing shell profiles.
On that machine, prefix Rust-dependent commands with `scripts/with_rust.sh` when
Cargo is not already on PATH, for example:

```sh
scripts/with_rust.sh cargo test --workspace
scripts/with_rust.sh npm run tauri -- build --debug --bundles app
```

With a standard toolchain, run `npm run tauri -- build --debug --bundles app` on
macOS, or use `--no-bundle` for a native binary build on either platform. Debug
artifacts are development builds, not signed/public releases.

## Boundaries

- `crates/core`: settings, locale negotiation, storage/secret contracts; no Tauri.
- `crates/platform`: SQLite migrations, OS paths, native credential adapter.
- `crates/runtime`: pinned packages, safe staged installation, cancellation,
  executable leases, owned process groups/jobs, readiness/restarts, bounded logs.
- `crates/cli`: `devforge doctor`, sharing the real storage/diagnostics layer.
- `apps/desktop`: React presentation and a narrow Rust/Tauri IPC shell.
- `shared/locales`: canonical registry and catalogs used by both WebView and native
  menus. Add a locale/catalog and rebuild; no per-language Rust/UI branch is needed.

Preferences use the OS data directory under `com.devforge.desktop`. Tests can set
`DEVFORGE_DATA_DIR` to a disposable directory; it is not accepted through renderer
IPC. Secrets use the OS credential service and are not part of the SQLite schema.
Startup does not probe credentials. The explicitly ignored native credential test
uses and removes a unique, non-production fixture when deliberately invoked:

```sh
cargo test -p devforge-platform native_secret_roundtrip_removes_its_ephemeral_item -- --ignored
```

The app uses system language/appearance initially, preserves explicit choices,
and falls back through the locale registry. Uzbek Latin and English fallback are
initial implementation defaults. Browser-only previews explicitly disable editing
when no native connection is present; they do not fake desktop persistence.

## M2 runtime core

The reviewed development catalog is `runtime/m2-catalog.json`, derived from M0
publisher digests/layouts. These are feasibility candidates; licensing, signing,
minimum OS and native runtime release gates remain open. The CLI uses exact IDs:

```sh
scripts/with_rust.sh cargo run -p devforge-cli -- runtime catalog
scripts/with_rust.sh cargo run -p devforge-cli -- runtime install caddy-v2.11.9-macos-arm64
scripts/with_rust.sh cargo run -p devforge-cli -- runtime probe caddy-v2.11.9-macos-arm64 caddy
scripts/with_rust.sh cargo run -p devforge-cli -- runtime remove caddy-v2.11.9-macos-arm64
```

Use the catalog ID matching the host. An optional archive path after `install`
imports a cached file and still verifies its digest. `DEVFORGE_DATA_DIR` isolates
CLI experiments. Package removal touches the owned binary directory only and
refuses while an executable lease is in use. No project version is auto-selected.

The synchronous store API belongs on a worker thread. `Supervisor` owns background
workers and must outlive windows; `request_stop` is nonblocking, `stop`/Drop join
cleanup. Services must run in foreground. macOS uses an owned process group;
Windows assigns a suspended child to a kill-on-close Job Object before resuming.
Readiness is an explicit stdout/stderr marker or successful exit for a one-shot
probe. Commands use argument arrays and explicit environments, never shell text.

Logs are raw child output: do not print credentials. Memory and persisted rotation
are bounded. This core does not implement a privileged sandbox, standalone daemon,
hard-killed macOS supervisor recovery, project bindings or arbitrary renderer IPC.
Future desktop integration must use a worker and narrow typed commands.

Meaningful M2 tests and an optional cached-candidate smoke are available:

```sh
cargo test -p devforge-runtime
cargo run -p devforge-runtime --example catalog_smoke -- /path/to/m0/artifacts /path/to/report.json
```

The example uses temporary runtime/log directories and starts two Node loopback
HTTP fixtures on OS-assigned ports. It does not change hosts, trust, Herd or real
project databases. The fixture executable is a test tool, not a desktop sidecar.

## Evidence and project knowledge

Start with [the knowledge index](docs/knowledge/index.md). Milestone evidence and
unverified targets are recorded there. A prepared macOS/Windows CI workflow is not
proof that remote jobs ran; Windows native/UI checks require a real runner.

Use [the validator setup](docs/knowledge/validation-tooling.md) and then run:

```sh
python scripts/validate_knowledge.py --strict
```

The [design reference](design-system/devforge/MASTER.md) records the visual target
and applicable UI workflows. Generated imagery is a reference, not the app UI.
