# DevForge

A macOS and Windows desktop foundation for managing local development environments.
The current M1 build provides local preferences, diagnostics, native menus/tray, and
Russian, Uzbek Latin, and English interfaces. Runtime/project management belongs to
later phases; the Projects screen states that limitation explicitly.

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
