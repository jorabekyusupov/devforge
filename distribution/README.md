# Internal desktop packages

Run `python scripts/package_desktop.py` inside the repository with its Rust/Node
versions available (on the local Mac use `./scripts/with_rust.sh python3 ...`).
This builds release CLI/helper sidecars, collects dependency license texts and a
CycloneDX inventory, then packages the desktop. Output is under
`target/release/bundle` and `output/distribution`. No publishing occurs. DMG builds skip Finder cosmetic automation (headless CI mode).

Targets: native aarch64-apple-darwin and x86_64-apple-darwin DMG/app;
x86_64-pc-windows-msvc per-user NSIS. Runtime archives are fetched from pinned
upstream URLs on demand; they are not embedded in installers. Applications do
not require a global PHP/Node/PostgreSQL installation. Windows needs the x64
Microsoft Visual C++ v14 runtime; its absence blocks install with a recovery URL.
Tauri detects WebView2 and downloads its Evergreen bootstrapper when absent;
first install then needs network access. Shared prerequisites are never removed.

Packages are explicitly internal/unsigned. Mac builds use ad-hoc signing only.
OS signing, notarization, runtime integrity and any future updater signatures are
independent. The generated manifest denies public release. No updater is enabled.
Signing credentials are absent in this environment. Public distribution requires
verified Developer ID/notarization, Windows Authenticode, minimum OS acceptance
and completed dependency/provider licensing review; an inventory is not that review.

## Removal and ownership

Quit through the tray first. A second launch restores the existing desktop window; it does not execute the forwarded arguments or start another supervisor. Windows uninstall blocks while the desktop lease is
held, or a DevForge CLI/terminal session is active. Projects, database clusters, selections, credentials and preferences are
retained. The NSIS template has **no data-deletion checkbox or recursive app-data
removal**. App replacement likewise preserves data and runtime versions.

Before uninstall, run the bundled `devforge distribution cleanup-plan <new-dir>`.
It inventories only this installation's hosts marker and public root CA, emits
reviewable digest-bound plans, and never reads private keys or applies elevation.
For each plan, explicitly run the bundled `devforge-helper apply <plan> <digest>`
with OS administrator authorization. Re-run `devforge distribution check-uninstall`.
Malformed/changed records fail closed. Windows uninstall refuses to continue while
owned hosts/trust entries remain. A Mac app is removed by dragging it to Trash;
that OS action cannot execute an application cleanup hook: run this preflight
before removing the app. Retaining data permits reinstall and deferred cleanup.
The Mac trust API may retain inactive, nontrusting admin metadata; physical
metadata deletion remains an interactive OS-authorization acceptance gate.

CLI location: macOS `DevForge.app/Contents/MacOS/devforge`; Windows installation
folder `devforge.exe`. The helper is adjacent. No global PATH changes are made.
`devforge distribution prerequisites` returns machine-readable prerequisite state.
`--startup-report <new-file>` on the desktop opt-in writes a single local report
after preferences/localization and two frontend animation frames. It opens no
HTTP endpoint; the caller chooses the output path, which must not already exist.

## Template provenance

`windows/installer.nsi` is derived from Tauri `tauri-cli-v2.12.4`, commit
`993038c49185307dfc895ab7657d77ca0d5ed7c7`, under MIT OR Apache-2.0:
https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.4/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi
The uninstall data checkbox/callbacks and associated deletion block are
removed; silent target validation runs before file installation, and .onInstFailed preserves a nonzero failure status. Preserve these differences when synchronizing the template. Upstream
license texts are retained alongside it. Uzbek Tauri-specific installer messages
are maintained locally; English/Russian and NSIS standard translations come from
the bundled installer toolchain. The language selector covers all three locales.
