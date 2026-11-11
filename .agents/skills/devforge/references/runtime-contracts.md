# Runtime and operating-system contracts

Read for managed packages, PHP/web serving, service lifecycle, network setup,
privileged operations, installation, updates, and data handling.

## Package and data ownership

- Keep application binaries, downloaded packages, generated configuration, caches,
  logs, user projects, and database data distinguishable. Updating the application
  or removing a runtime version must preserve user projects and database data.
- Keep DevForge-owned resources separate from pre-existing Herd, Homebrew, system,
  and user-managed resources. Detect ownership and conflicts before modifying
  services, PATH, ports, DNS, or certificate trust. Inspection is not permission
  to migrate or stop an unrelated development environment.
- Model package identity with version, OS, architecture, source, integrity
  metadata, layout, and compatibility requirements. Verify downloads against a
  trusted manifest before activation and validate archive paths during extraction.
- Keep installed runtime versions, available catalog releases, and each project's
  selected version distinct. Refreshing the catalog or updating DevForge must not
  silently switch existing projects to a newer runtime or migrate database data.
- Stage installation and configuration changes before activation; retain a usable
  prior state when activation fails. Do not label a database major-version change
  as a routine binary replacement: data compatibility needs an explicit strategy.
- Establish licensing and redistribution suitability before shipping third-party
  binaries. Do not copy proprietary Herd application assets or bundled artifacts.

## Processes and managed services

- Specify desired state, observed state, readiness, failure, and stop behavior.
  A successful spawn or stored PID alone does not establish a healthy service.
- Track process identity and ownership; prevent duplicate instances and bound
  restart attempts. Stop the owned process tree with a grace period and a defined
  fallback; account for Windows process trees and macOS child-process behavior.
- Launch verified executable paths with explicit argument lists and environment.
  Do not interpolate project names, paths, or user input into shell strings.
- Define per-project executable/version resolution so web requests, CLI commands,
  queues, and Composer use the intended PHP configuration. Platform PHP launch
  mechanisms may differ; do not assume PHP-FPM is available on Windows.
- Keep logs and event queues bounded and rotate persisted logs. Watchers must be
  cancellable and removable when a project or service is removed.

## Local domains, HTTPS, and privileges

- Treat DNS resolution, HTTP routing, certificate creation, and certificate trust
  as separate capabilities. Verify each changed part on its target OS.
- Account for occupied ports, VPN/proxy settings, existing resolvers, unavailable
  trust stores, and denied elevation through actionable errors and recoverable state.
- Run ordinary UI and service supervision without administrator privileges where
  possible. Put necessary elevated operations behind a narrow, authenticated local
  interface; Tauri capabilities do not themselves grant or secure OS elevation.
- Validate IPC commands and ownership on the receiving side. Do not expose an
  unrestricted shell or unauthenticated administrative HTTP endpoint to the UI.
- Protect certificate private keys and service secrets using suitable OS storage
  and access controls. Avoid exposing credentials in logs, diagnostics, or exports.

## Distribution and updates

- Build the appropriate artifacts for each supported OS/architecture. Track the
  desktop installer and managed runtime packages separately; their update and
  compatibility lifecycles are different.
- Plan macOS signing/notarization and Windows signing/installer behavior according
  to distribution requirements. An unsigned local build is not a signed release.
- Tauri updater signatures, OS code signing, and downloaded-runtime integrity are
  separate checks. Application rollback must account for local schema compatibility;
  an updater plugin alone does not implement a complete recovery strategy.
- Make uninstall cleanup ownership-aware. Destructive removal of projects,
  databases, or unrelated resources requires explicit task authorization.

## Official starting points

Verify these against the versions in use rather than copying examples blindly.
The versioned URLs identify documentation editions, not permanent stack pins;
use the corresponding official edition when the selected project version differs.

- Tauri architecture: https://v2.tauri.app/concept/architecture/
- Tauri capabilities: https://v2.tauri.app/security/capabilities/
- External binaries: https://v2.tauri.app/develop/sidecar/
- Distribution: https://v2.tauri.app/distribute/
- Updater: https://v2.tauri.app/plugin/updater/
- Rust platform support: https://doc.rust-lang.org/rustc/platform-support.html
