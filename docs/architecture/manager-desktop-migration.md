# Thin native-client desktop migration: design and evidence gates

Status: thin-client cutover implemented. The GUI retains local window/dialog/
appearance responsibilities and adapts all20 original handlers to one manager
without a local Workspace fallback. Two additive commands provide explicit
Connect and read-only uncertain-operation reconciliation. The design below
records preserved contracts and platform gates; current macOS evidence is in
[the bridge record](manager-native-bridge.md#actual-cutover-acceptance-macos-arm64).

## Existing contract that must survive

The current desktop registers20 Tauri handlers:16 in
[`workspace.rs`](../../apps/desktop/src-tauri/src/workspace.rs) plus four local
handlers in [`lib.rs`](../../apps/desktop/src-tauri/src/lib.rs). The existing
React bridge expects these methods to return their actual old values/errors;
`workspace_command` is not an API returning an admission receipt.

| Current route | Thin-client destination | Preservation requirement |
| --- | --- | --- |
| `workspace_snapshot` | Native SDK complete snapshot | Preserve projects/runtimes/services/logs and progress; no fabricated empty state on disconnect. |
| `workspace_command` | Cancellable admission, operation status, actual result/failure | Await the real outcome; retain its operation/project/idempotency identity. |
| `workspace_cancel` | Separate GUI-owned cancellation state | Abort upload before finalization or cancel the exact admitted operation; never another client's job. |
| `inspector_snapshot` | Typed inspector owner read | Preserve optional trace context and exact profiling/capture value. |
| `activity_read` | Typed inbox/message/part/log reads | Preserve filters/paging, MIME/plain/HTML/attachment data and current error codes. |
| `debug_read` | Typed debugger view/profile/source reads | Preserve stack/session/profile/source values, including large source/profile payloads. |
| `configuration_read` | Typed document/plan reads | Preserve expected hash, journal, YAML text and reviewed environment plan. |
| `configuration_secret` | Sensitive native owner access | Return no secret; preserve references and errors; no raw secret or password-derived verifier in the operation DB. |
| `open_project` | Registered-project owner access | Preserve browser/editor/folder and a separately verified terminal handoff. |
| `discovery_scan`, settings-root/focus notifications | Owner discovery service | One watcher/coordinator; preserve safe detection, suppression, relocation and root change. |
| File dialogs and main-window actions | Stay in GUI Rust | Keep localized native chooser titles, path review, focus restoration and window/tray behavior. |
| Bootstrap/preferences/native menus/appearance | Stay in GUI Rust with an explicit settings writer contract | Keep locale registry, saved language/theme and project-folder choice. |
| Explicit Quit | Authenticated owner Quit then GUI exit | Persist inhibition, clean owned services and report shutdown failure; window Close still hides to tray. |

The old `View.host_pid` is the GUI process used by desktop/window acceptance.
Preserving that field means the native adapter supplies its own GUI PID, rather
than silently substituting the manager PID. The SDK separately pins the actual
manager PID. Map manager progress to old `operation`; retain truthful busy/error
state. A cached source error must not be converted to an apparently fresh empty
snapshot. Preserve structured `{scope, code, project_id}` errors and localize their
presentation through the shared catalogs.

## Native state and cancellation

The replacement GUI state holds native SDK/session state, cached nonsecret views,
and a small GUI operation tracker. It constructs no Workspace, Supervisor,
database/cache server or discovery service. Dialogs, menus and preferences remain
native GUI responsibilities. No auth key/bootstrap/session is returned to React.

Keep admission cancellation and the active receipt outside a mutex held for SDK
I/O. `admit_cancellable`/`admit_observed` already support cancellation before
upload, between chunks and before finalization. The accepted receipt always
survives a late cancellation race; cancel is then routed to its exact IDs.
Ambiguous finalization is unconfirmed, not a promise that nothing ran.

After admission, release the SDK mutex between status polls. Do not wrap an entire
long-running job in one locked SDK call: the cancel handler must remain able to
send its own bounded request. The same rule applies to large read-image downloads;
the SDK facade requires an opaque native reader with single-page steps so the bridge can
yield between pages. Background polling must not implicitly touch SecretStore or
renew authority after expiry.

The old64KiB operation-result cache is insufficient for debugger source, expanded
mail HTML/text/attachments and rich profiles. Native owner reads therefore need
their separate bounded, digest-checked read-image path; no truncated result may be
reported as success. Pure read tickets are ephemeral bounded metadata, not one
durable SQL row per Inspector/debugger poll. Polling must reuse an active ticket
and refetch a new read after explicit expiry; access/mutations remain durable.

The SDK facade now has typed read/access submission, paged operations and the
opaque per-page reader. It also supports explicit `result_begin` retrieval for a
large Succeeded mutation, without executing that command again. A valid near64KiB
YAML save was verified to stay Succeeded and return its complete document through
that image path. Unavailable/evicted results remain unavailable, not fabricated
success data or an automatic replay. These backend/SDK checks do not switch the
Tauri handlers or close the remaining native UI gates.

## Single-owner and singleton transition

The current manager intentionally holds the legacy GUI `AppLease`, and legacy
GUI setup acquires that same lease. Replacing only `State::with` cannot make these
processes coexist. Removing the manager's exclusion would allow an older GUI to
open a second Workspace.

The platform prerequisite `NativeClientLease::acquire` now provides the distinct
`desktop-client.lock` singleton and a shared installer operation lease; the manager
retains legacy `desktop.lock`. Actual GUI adoption remains proposed. Keep Tauri's
existing single-instance plugin and never construct a local Workspace on failure.
An old GUI can win the startup gap before the manager acquires its legacy lease;
the new path must fail Busy, not kill/adopt the old owner or fall back. The secure
helpers reject a final root symlink/Windows reparse before lock writes; no
ancestor-path security claim is added by this prerequisite.

Closing/reopening a window does not stop/recreate the owner. A crashed GUI may
reconnect to a still-running manager after explicit authentication. A manager
crash/restart invalidates its old session and interrupts old in-flight intent;
do not replay a mutation automatically. Durable receipts without retained results
remain unavailable/unknown as specified. Read-only expired observations can be
refetched safely; they must not be confused with a successful durable mutation.

Retain every durable intent's original key until its admission is reconciled.
After an unconfirmed reply, `operation_by_key` performs a read-only lookup; it
does not retry the business action. None is point-in-time absence, not proof that
an old handler cannot still commit. Do not generate a new key or re-execute on
that basis. Secret receipt lookup also does not establish body equivalence after
the volatile verifier expires; preserve the explicit unverifiable outcome.
That exception covers configuration-secret access and known Core free-text
`DebugHttp` headers/body, `DebugRequest` path/query and `ConfigurationSave`
YAML/comments. Ordinary fixed metadata retains durable fingerprints; this is not
universal secret detection or permission to replay unverifiable actions.

Global create/setup recovery also needs a trusted worker-created resource
association. A domain error's arbitrary `project_id` alone is not authority.
Keep receipt scope in `FailureDetail.project_id` and use a separately validated
optional recovery project ID so the old wizard can resume preparation rather
than blindly clone into an existing destination.

## Trusted packaged CLI and connection flow

Packaging already includes CLI/helper sidecars through
[`distribution/bundle.json`](../../distribution/bundle.json). Installed locations
are the desktop executable's sibling `devforge` in macOS `Contents/MacOS`, and
`devforge.exe` in the Windows install directory. Resolve the fixed sibling from
the actual native executable; never search PATH, current working directory, a
renderer path or a runtime environment override.

The build-gated
[`manager_artifact::bundled_launch`](../../apps/desktop/src-tauri/src/manager_artifact.rs)
now returns the fixed candidate, compiled target and expected32-byte SHA-256.
The SDK launcher verifies that expected digest before execution. Computing the
expected value from the same candidate file at runtime would not establish trust.
Ordinary unprepared development builds return None, even if stale generated
packaging files exist. Their development/acceptance launch path must be explicitly
prepared; there is no silent fallback executable.

Pre-bundler digest generation alone is not packaged acceptance. macOS sidecar
signing/notarization or Windows signing can change executable bytes. The packaging
sequence must establish the final CLI bytes before compiling the GUI binding and
verify the installed sidecar against that binding after bundling/signing. A
mismatch must fail; do not repair the expected digest from an untrusted runtime
file. Installer hashes and a compiled sidecar binding are separate evidence.

Connection order:

1. Read native nonsecret configuration and observe the root's authenticated owner
   health without probing a credential store.
2. On an explicit cold user launch with no owner, use the fixed trusted CLI and
   private inherited-pipe bootstrap. Verify root and actual child/kernel PID.
3. If another owner won the race, preserve it. Do not kill it or loop-spawn a
   replacement; use the existing-owner flow.
4. Existing owner/expired context needs an explicit native action for credential
   fallback. The SDK receives the credential; it does not read/provision it or
   silently switch roots/providers. Renew a still-valid native context explicitly.
5. Respect durable Quit inhibition. Background reconnect/refresh does not clear
   it or relaunch services. A new explicit user start is a separate intent.

If explicit Quit cannot authenticate or persist shutdown, retain the current
failure path instead of silently exiting and claiming all services stopped.
No native credential/dialog permission has been exercised by this design work.

## Terminal and preference ownership gates

Terminal exposed a concrete parity issue: `terminal::open` uses `current_exe()`,
so the CLI-owned manager starts `devforge project shell`, which the broad legacy
guard would reject. The published prerequisite now recognizes only the exact
three-argument interactive shell route before that guard and uses a current-schema
read-only project snapshot. Per-shell context/leases preserve registered-project
validation and do not create another Workspace/Supervisor or weaken the remaining
legacy routes. The facade delegates to this core terminal route; the SDK fixture
uses an unknown project and must fail before opening any native window. Real
Terminal.app/Windows console launch, scoped executable/INI resolution and lifetime
still need explicit native acceptance before claiming full GUI terminal parity.

Discovery-root updates exposed a second write-boundary issue: replacing the whole
settings JSON under process-local locks could lose a concurrent theme/language
or folder update. Both GUI preferences and the manager path now use the shared
transactional `SqliteSettings::patch` field merge rather than a stale load/save.
This preserves local/offline appearance and unknown saved fields. Actual
coexisting-GUI acceptance is still separate; merely reading current fields before
a replacement write would not have established the same guarantee.

## Evidence required before switching the GUI

- Complete typed read/access/command mapping, operation subject coverage and
  trusted partial-create recovery metadata; all existing M7–M12 paths remain usable.
- No second Workspace/watcher on cold start, existing owner, reconnect, denied
  authentication, two launchers or GUI crash. Exact singleton/install leases.
- Real oversized debugger/mail/profile reads through the read-image codec; no
  placeholder data or64KiB result-cache regression.
- GUI-specific cancellation before/after admission, busy progress, actual domain
  errors, expired results and interrupted/replayed intent.
- Verified compiled CLI target/digest after bundling/signing and no PATH/runtime
  executable fallback. Restricted Windows Job capability stays explicit.
- Debug E2E sidecar store factory must be compile-gated and require an isolated
  root, matching the old GUI's fixture-secret behavior. A production binary must
  never accept a test-store environment switch. No automated native Keychain or
  Credential Manager probe is substituted for this fixture boundary.
- Real macOS/Windows native UI checks for dialogs, focus, terminal, window/tray
  Close versus Quit, three locales and appearance. Source/build/browser checks
  alone cannot establish those results.

The adapter and default owner mode are now switched. Actual macOS WebView
acceptance covers cold/warm ownership, two native clients, exact retry identity,
managed PHP HTTP, large reads, Close and confirmed Quit. It does not substitute
for Windows native, production credential-store, terminal/dialog or signing gates.
