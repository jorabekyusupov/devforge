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
uses Laravel13. The native Xdebug surface below adds optional extension/runtime
management, interactive debugging and function-level profiling.
`devforge inspect <project-id>` reads captured history and context summaries as JSON
without starting a collector or printing its credentials.

## M7.2 native debugger and function profiles

Open **Debugger**, select a stopped project and choose **Prepare debug runtime**.
Review the native-source/download consent. macOS builds a matching CLI/FPM and
Xdebug in a private temporary prefix using Apple Command Line Tools; the first
build can take tens of minutes. Windows installs the matching official NTS DLL.
Completed SDKs are reused across projects with the same PHP version.

The normal runtime catalog keeps older installed package identities and their
checksums. If a provider replaces an archive, a reviewed build receives a separate
ID; **Install updated build** installs it without changing project selections.
A withdrawn build can still be restored from a verified cached archive through
the CLI. Portable manifests retain explicit old pins and choose the active build
only when no exact pin was provided.

Choose **Configure Xdebug**, select Debug or Profile mode and a matching installed
runtime, then start the project. In Debug mode, add a project-relative PHP file and
line, enter a local GET path and choose **Run request**. **Continue** reaches a
breakpoint; Step into/over/out, the call stack and masked read-only locals/source
are available. **Stop request** ends a paused request while keeping the web runtime.

In Profile mode, run a request and open **Function profiles**. Select a retained
profile, search functions, inspect self/inclusive time, memory and call counts, or
export Cachegrind to a new filename. Delete requires confirmation. Raw exports
retain source paths and function names. History targets20 files/128MiB, with a
32MiB file limit; the supervisor stops the project's owned processes if its sampled
profile budget is exceeded. Stop the failed project before retrying.

Stop the project and select Off to restore its normal PHP runtime. The project
PHP pin, `.env`, other projects and Inspector capture remain independent. M7.2
introduces project metadata schema6 after M12's schema5; older builds refuse the
new schema. Current source, native target evidence and release boundaries are in
[M7.2 results](docs/knowledge/m7-xdebug-results.md).

The public CLI reads retained results and configures the desired mode. Close the
desktop app for install/mode mutations; starting the desktop project activates the
configured session. Built-in request triggering is GET-only; IDE integrations are
part of the later roadmap.

```sh
devforge debug status <project-id>
devforge debug install <project-id> --trust-source
devforge debug mode <project-id> debug <runtime-id>
devforge debug source <project-id> app/Http/Controllers/Controller.php
devforge debug profile <project-id> <profile-id>
devforge debug mode <project-id> off none
```

## M8 project setup

**New project** creates a Laravel project or clones a Git repository into a new
folder. Choose installed PHP/Node runtimes, review the domain/HTTPS selection
and approve preparation actions. **Add project** keeps the existing read-only
import flow. The Laravel starter constraint comes from the runtime catalog.

**Prepare project** installs Composer/npm dependencies and can create a missing
`.env` and empty `APP_KEY`. Existing keys are preserved and changed environment
files get private backups. Scripts are off by default; allowing them still
respects the project's npm policy. Composer plugins stay disabled. New Laravel
projects can initialize their own SQLite database; existing Git-project databases
are not silently migrated. Operations show actual stages and can be cancelled;
registered partial projects retry preparation without cloning again.

**PHP settings** changes only the selected project's INI and restarts it when
running. Diagnostics show the actual PHP/extension/Composer platform state.
Favorites and recently used projects survive app restarts.

**HTTPS setup** reviews owned domain entries and the exact local CA fingerprint
before invoking the packaged fixed-purpose helper with OS authorization. Preview
does not install trust. Real system authorization dialogs remain a manual M15
acceptance gate. See [M8 evidence and limitations](docs/knowledge/m8-project-setup-results.md).
Publication can leave a preserved partial new directory on disk/crash failure;
setup never replaces an existing destination. SSH/private Git authentication and
recursive submodule workflows require their own client setup/coverage.

## M9 databases and cache

Services now manages owned PostgreSQL/MySQL instances and dedicated per-project
Garnet processes. Install the chosen server package in Runtimes, create an instance
in Services, then select the project connection. Garnet creation downloads its
managed .NET dependency; Redis-compatible Laravel drivers require the explicit
Predis installation action. Project cache/queue settings apply to DevForge contexts.
Database-backed Laravel drivers still require the project's migrations.

Stop the project before switching connections, changing drivers or restoring data.
Backups use `.dfbackup` archives (managed PostgreSQL/MySQL only); a restore creates
a new limited-role database and retains the prior connection. Use Retained databases
to reactivate it. Imported SQL must be trusted. Export uses a new filename, without
overwriting an existing file. Exact engine/version and MySQL InnoDB table-only limits
are described in [M9 results](docs/knowledge/m9-services-results.md).

MySQL's current portable package is available for macOS ARM64 and Windows x64.
Intel Mac uses PostgreSQL and Garnet in this phase. Native CI and release limitations
are tracked separately; this source change does not publish a new installer.

## M10 reproducible environments

`devforge.yaml` is a versioned team contract for a checked-out Laravel repository.
It contains exact PHP/Node/Composer/service versions, platform-specific publisher
package SHA256 pins, site/PHP settings, fixed dependency actions and logical secret
names. It contains no credential values, machine paths, ports, instance IDs or
arbitrary hooks. See [the example](docs/examples/devforge.yaml).

The M10 workflow uses the existing CLI; the desktop configuration editor is not
part of this phase. Quit DevForge through its Quit action (closing the window keeps
services alive) before `config setup` or `config secret`. A process lease rejects
concurrent GUI/CLI activity. Run an installed CLI, or build from source with
`./scripts/with_rust.sh cargo build -p devforge-cli`.

```text
devforge config export <registered-project-id>
devforge config init <project-folder> 8.5.12 24.22.2 2.11.2
devforge config plan <project-folder> --write-env
devforge config secret <project-folder> api_token
devforge config setup <project-folder> --review <plan-digest> --write-env --trust
devforge config status <project-folder>
```

Use either export for a registered project or init for a new manifest; neither
replaces an existing file. Export accepts an optional destination as its third
argument. Review the manifest before committing it to the team's repository.
On another device, clone the repository, supply missing secrets and review its
local plan before setup. `config secret` reads at most4096 bytes from stdin, never
from an argument. For example, pipe a password-manager/CI secret into stdin;
Unix `printf '%s' "$LOCAL_TOKEN" | devforge config secret "$project" api_token`, or
PowerShell `$env:LOCAL_TOKEN | devforge config secret $project api_token`.

Plans return schema1 JSON, redacted key actions, template/key-generation flags,
settings, exact packages, downloads, local secret availability and a review digest.
Config/project/lockfile/`.env` changes and secret rotations invalidate prior plans.
Pin mismatches fail before execution; URLs or package scripts cannot be supplied
through YAML. `setup` requires an explicit trusted-repository flag, always disables
Composer plugins/scripts and npm scripts, and never runs database migrations.
Predis setup is opt-in and adds the client only when absent; the repository's
Composer/npm locks still control its application libraries.

Without `--write-env`, every `.env` file remains untouched, including key generation
and template copying. With it, only reviewed manifest variables and managed DB/cache
keys are patched; existing APP_KEY is preserved, unrelated lines stay, changes get
private backups, and stale/duplicate/symlink files are refused. Multiline local
secrets can be used in managed process environments but cannot be written to `.env`.
Existing local variables not declared by the manifest are preserved; removing an
entry does not delete local credentials or rewrite unrelated `.env` keys.

Setup registers the project, prepares packages/dependencies, provisions isolated
local services, and then exits; it does not leave unowned background processes.
Open the desktop to start the prepared project. Database changes retain old data
and connections, without copying or migrating SQL. A stage journal supports review
and retry after failure/cancellation; this is not an all-or-nothing SQL/filesystem
transaction. Pending interrupted service initialization needs recovery, not forced
reinitialization. Host DNS/trust/elevation is handled by the existing explicit HTTPS
flow. The MySQL Intel-Mac capability gap remains visible during planning.

Project metadata upgrades to schema3 for local credential links; older records
remain readable by the new app. Older app builds refuse the new metadata schema;
this is not a tested downgrade/rollback strategy. See the M15 release gates.

## M11 mail and logs

Open **Mail & logs**, choose a stopped project, then **Start capture**. DevForge
starts an authenticated localhost SMTP sink and injects standard Laravel MAIL_*
settings into its managed PHP/web/queue contexts. It never relays outgoing mail.
Use the running desktop capture when sending through managed CLI PHP contexts;
raw SMTP credentials stay in the local credential store. Stop the project before
changing capture. Inbox data and the allocated port survive capture restart; Quit
closes the listener. Unrelated external development services are not changed.

The inbox supports subject/sender/recipient search, HTML/plain text and image/text
attachment preview, Save, and confirmed delete/clear. Remote HTML resources, scripts,
forms and links are blocked. Inline images can be viewed as attachments. Unsupported
formats (including PDF/SVG) use Save rather than active embedding. Limits:2MiB raw
message,50 attachments,200 retained messages and64MiB raw data per project; body
preview is truncated at65,536 characters. Captured mail may contain private data.

In **Logs**, install/update the companion and explicitly enable capture. Search and
filter level/type/time over retained M7 events, inspect sanitized structured context
and exception frames, or follow a context to Inspector. Companion updates back up
recognized old bundles and refuse edited/foreign files. No stack arguments, request
bodies or SQL bindings are captured. Redaction is heuristic; capture remains opt-in.

`devforge.yaml` can export an optional `mail: true|false`; absence preserves an
existing local choice. M10 `--write-env` can explicitly patch mail fields with its
review/backup contract. Standard Laravel MAIL_URL uses the null sentinel when capture
is selected. Custom hardcoded transports are not intercepted by a network firewall.
M11 introduced schema4; M7.2 above describes the current schema. See [M11 results](docs/knowledge/m11-mail-logs-results.md)
for current native evidence and release/rollback limitations.

## M12 branch workspaces

Open **Workspaces** and choose a registered Git project. Review a new or unused
existing local branch, the resolved commit, generated domain, and empty/copied
database before creation. Workspaces use committed history; source `.env`, ignored
uploads, `vendor` and `node_modules` are not copied. Each gets a new project ID,
application key, database credentials and selected Redis/mail services. M10 runtime
pins and PHP settings are reused; repository `devforge.yaml` is preserved.

Dependency setup disables Composer plugins/scripts and npm scripts. Redis profiles
require dependency installation for their PHP client. Custom secret copies are
opt-in, use independent local SecretStore entries and are not written to `.env`.
Managed connection settings are written only to the newly owned private `.env`.
An empty database is not automatically migrated; use Prepare/Artisan as appropriate.
SQLite copies require the conventional DevForge-owned `database/database.sqlite`
and matching SQLite profile; PostgreSQL/MySQL use the existing logical backup and
staged restore path. Stop the source before copying its database.

**Review cleanup** stops only the selected workspace, archives all its local files,
unregisters its Git worktree and retires its project/site. The branch, databases,
credentials and backups remain. Dirty, detached, locked, foreign and nested-project
worktrees are refused. Git submodule branches/metadata are outside this M12 flow.
Close external tools using the workspace before cleanup. Disk purge and automatic
archive reactivation are not implemented. An archived folder is a file recovery
copy; create a new worktree on its retained branch to resume Git work.

The CLI uses reviewed JSON and the same core. Close the desktop app for mutations;
list/plan are read-only. A request file contains:

```json
{
  "source_id": "<registered-project-id>",
  "name": "Search improvements",
  "branch": "feature/search",
  "base": "HEAD",
  "new_branch": true,
  "database": "empty",
  "dependencies": true,
  "copy_environment": false
}
```

```sh
devforge workspace list
devforge workspace plan workspace-input.json
devforge workspace create workspace-input.json --review <result.digest> --trust
devforge workspace cleanup-plan <workspace-id>
devforge workspace cleanup <workspace-id> --review <result.digest> --confirm
```

Output uses `schema: 1` and `result`. Plans contain metadata and secret names,
never secret values. Checkout does not implicitly fetch promisor objects. Host Git
uses the existing process-group/Windows Job adapter and does not need installed
PHP/Node packages for cleanup. M12 introduced schema5; M7.2 adds schema6. Older builds refuse newer metadata.
[M12 results](docs/knowledge/m12-workspaces-results.md) track native coverage,
archive semantics and the open distribution/recovery gates.
