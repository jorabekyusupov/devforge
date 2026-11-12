# DevForge

A macOS and Windows desktop foundation for managing local development environments.
The desktop provides local preferences, diagnostics, native menus/tray, and
Russian, Uzbek Latin, and English interfaces. M2 adds a Rust runtime package store
and process supervisor, exposed through the library and CLI. Project management
and database bindings are now available through M3's library/CLI. Their desktop
controls belong to the later UI phase.

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
- `crates/projects`: Laravel inspection/registry, scoped runtime context, native
  secret references, owned PostgreSQL clusters/roles and explicit `.env` patches.
- `crates/cli`: doctor, runtime and project commands using the shared services.
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

The current development catalog is `runtime/catalog.json`, derived from M0
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
hard-killed macOS supervisor recovery or arbitrary renderer IPC. Project bindings
are handled by `crates/projects`.
Future desktop integration must use a worker and narrow typed commands.

Meaningful M2 tests and an optional cached-candidate smoke are available:

```sh
cargo test -p devforge-runtime
cargo run -p devforge-runtime --example catalog_smoke -- /path/to/m0/artifacts /path/to/report.json
```

The example uses temporary runtime/log directories and starts two Node loopback
HTTP fixtures on OS-assigned ports. It does not change hosts, trust, Herd or real
project databases. The fixture executable is a test tool, not a desktop sidecar.
With an empty cache, pass `--download-missing` explicitly to fetch the pinned
publisher artifacts. The manually dispatched `runtime-candidates.yml` workflow
runs this native smoke on macOS and Windows; it is separate from routine CI so
ordinary source pushes do not repeatedly download all third-party candidates.

## M3 project environments

Import checks Laravel's `composer.json`/`public/index.php` and optional
`package.json` without running scripts, installing dependencies, migrating data or
editing `.env`. Composer-style PHP constraints and npm-style Node constraints use
their respective parsers. This is declared version validation, not a full Composer
dependency/extension solver. Exact package IDs are persisted for each project.

The following commands assume `devforge` is built from `crates/cli` and on PATH:

```sh
devforge project inspect /path/to/laravel
devforge project import /path/to/laravel <php-package-id> <node-package-id> "My project"
devforge project list
devforge project configure <project-id> <php-package-id> <node-package-id> "My project"
devforge project doctor <project-id>
devforge project exec <project-id> php -- -v
devforge project exec <project-id> composer -- --version
devforge project exec <project-id> artisan -- --version
devforge project exec <project-id> npm -- --version
devforge project exec <project-id> queue -- --once --stop-when-empty
devforge project terminal <project-id>
```

Install the selected PHP/Node packages first. Composer uses the pinned resource
package for this host. On macOS, `php-web` resolves the matching PHP-FPM package;
on Windows it uses the selected package's PHP-CGI. Both use the same project INI
and environment. Serving/proxy readiness is M4, not a version probe.

Use a PostgreSQL catalog ID ending in `-server`, which adds verified `pg_ctl`
without changing earlier M2 package identities or receipts:

```sh
devforge runtime install <postgres-server-package-id>
devforge project db-init <postgres-server-package-id>
devforge project db-serve <instance-id>
```

`db-serve` is an explicit foreground owner. Keep it running in one console; enter
`quit` or send Ctrl+C for verified `pg_ctl` fast shutdown. In another console:

```sh
devforge project db-provision <project-id> <instance-id>
devforge project db-check <project-id>
devforge project env-plan <project-id>
devforge project env-apply <project-id>
```

On Windows, run the application/CLI without enabled administrator privileges;
PostgreSQL refuses an Admin/Power User token. The CI-only fixture reduces its
own process token; production never changes UAC, accounts or machine permissions.

Each project receives a distinct database and nonprivileged login role. SCRAM
passwords live in the OS credential store; SQLite holds references only. Loopback
HBA and database ACLs reject cross-project connections. Data is stored separately
from binaries. `forget` unregisters metadata while preserving the folder, bindings,
credentials and database data. It does not drop a database.

`env-plan` reports keys with hidden values. Only `env-apply` writes selected DB
keys, keeps a private backup and rejects changed, duplicate or symlink `.env`
files. Existing DB URLs are explicitly cleared so they cannot override the new
binding. Other content is preserved. Custom environment values can be piped to
`project env-set <project-id> <NAME>`; they also use the secret store. Runtime/loader
variables are reserved. Known secret values are redacted before memory/disk logs,
including across read boundaries; this is not a sandbox for hostile application
code or a detector for unknown/encoded secrets printed by a project.

`project shell` launches a clean project shell in the current console;
`project terminal` opens a new native console that invokes it. PHP/Composer/Artisan
shims include the selected INI, Node/npm resolve the selected package, and leases
last until the shell exits. Managed runtime directories precede existing absolute OS tool paths. Global PATH
and shell profiles are not changed.

M3 checks use temporary projects/data and test-only secret adapters:

```sh
cargo test -p devforge-projects -p devforge-runtime
cargo run -p devforge-projects --example project_smoke -- /path/to/m0/artifacts report.json
```

`--download-missing` explicitly downloads pinned artifacts; `--laravel` additionally
creates a disposable real Laravel skeleton with Composer scripts/plugins disabled,
then explicitly runs migrations, an empty queue worker and a PHP version switch.
The manually dispatched `project-environments.yml` workflow exercises this on
macOS and Windows. The shell probe checks native command resolution; opening the
actual Terminal/console window is a separate GUI acceptance check.

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

### M4 local serving (CLI)

`devforge site configure <project-id> <name>.devforge.test [--vite] [--http-only]`
registers a project route. Install its selected PHP CLI/web package, Node and the
catalog's default Caddy first. `--vite` requires the project's installed Vite;
DevForge does not install project dependencies when starting a site.
`devforge site list` shows saved routes; `devforge site serve` starts all configured
sites in the foreground. Send `quit` on stdin or Ctrl+C to stop owned processes.
The returned status contains the actual ports/URLs, PHP/Vite readiness, DNS
resolution, certificate activation and system trust separately. Ports 80/443
fall back to 8080/8443 or another free loopback port without stopping other tools.

Domains are scoped to `.devforge.test`. DNS or trust setup is optional: the
reported HTTP `*.localhost` fallback can be used without changing the system.
HTTPS uses a DevForge CA whose signing key stays in the native credential store.
The public certificate and fingerprint are shown by `devforge site authority`.

System integration is an explicit two-step operation:

1. `devforge site system-plan hosts <new-plan.json>` or
   `devforge site system-plan trust <new-plan.json>` writes a reviewable plan and
   prints its SHA-256 digest. Hosts plans preserve unrelated entries and reject
   conflicts. Review the domains/CA and the digest before applying.
2. Run **only** `devforge-helper apply <plan.json> <digest>` with OS elevation
   (`sudo` on macOS or an administrator terminal on Windows). The main supervisor
   must stay unprivileged. The helper accepts fixed hosts/CA operations, never an
   arbitrary target path or project command. There is no automatic elevation UI.

`hosts-remove` prepares removal of only this installation's marker block;
`trust-remove` prepares removal of the exact CA. Removing a site does not silently
change system configuration or another application's trust. A stale hosts plan
must be regenerated. Trust removal is explicit; an already present CA is never
adopted for automatic cleanup. The disposable CI workflow tests and restores only
its own hosts/CA entries. On macOS, removal resets the exact CA to system-default
trust and removes its keychain certificate; a nontrusting settings record may
remain, reported by `may_retain_nontrusting_record`. Physical removal of that
metadata belongs to the later GUI/uninstall flow. See
[M4 evidence](docs/knowledge/m4-serving-results.md).


### M5 daily desktop workspace

The desktop now provides Projects/details, Runtimes, Services/logs and the existing
Settings/Diagnostics. Import an existing Laravel folder, select its catalog PHP/
Node versions, and configure its owned domain. Install missing runtimes through
Runtimes. Add a PostgreSQL server through Services, assign each project its own
database, and test the connection. Project import does not run scripts or change
`.env`; database assignment is explicit.

Start/Stop/Restart is scoped to the selected project. Its optional Vite and queue
worker use the same project runtime/environment. Saving runtime/development
settings restarts that project when needed, while other project sessions continue.
A project Stop leaves shared PostgreSQL running; active users prevent a database
service Stop. Closing the window hides to tray; Quit stops app-owned services.
Reopening preserves configuration without auto-starting services.

Open-browser, VS Code, folder and terminal actions resolve the registered project
on the Rust side. VS Code needs its OS URL handler. The desktop executable also
supports the scoped terminal launcher mode; project paths are not inserted into
arbitrary shell commands. Raw paths/names/logs are kept as data; owned UI is
catalog-backed in Russian, Uzbek and English.

Native acceptance uses an optional debug-only embedded WebDriver and an isolated
filesystem SecretStore fixture. Production builds have no test HTTP endpoint or
fixture credential adapter. See [M5 results](docs/knowledge/m5-daily-workspace-results.md).

## M6 internal desktop distribution

See [packaging and removal](distribution/README.md) for the release build command,
DMG/NSIS artifacts, required OS components, data-preserving uninstall and explicit
owned hosts/CA cleanup plans. Packages are internal unsigned/ad-hoc builds; public
release and automatic application updates are not enabled. Actual target evidence
and remaining gates are tracked in [M6 results](docs/knowledge/m6-distribution-results.md).

## M7 Laravel Inspector (initial capture and application profiling)

Open **Inspector**, select a trusted local project, stop it and confirm **Install
companion**. Enable capture, then open a route or dispatch a queued job. The new
surface shows actual request/query/job/log/dump/exception events, context durations,
SQL repetition/slow-query hints and peak PHP memory. Pause/clear are project scoped;
history survives reopening while capture defaults to off. The local authenticated
telemetry listener is separate from the debug-only WebDriver acceptance endpoint.

See [companion setup and privacy](packages/laravel-inspector/README.md) and
[M7 actual evidence/open work](docs/knowledge/m7-inspector-results.md). The declared
Laravel11/12 compatibility requires separate runtime coverage; the current fixture
uses Laravel13. Xdebug capability detection is implemented; extension installation,
interactive debugging and function-level profiling remain later M7 work.
`devforge inspect <project-id>` reads captured history and context summaries as JSON
without starting a collector or printing its credentials.
