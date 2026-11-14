# Isolated native bridge prerequisite

Status: the desktop now uses this thin native bridge. `workspace::State` adapts
all20 original handlers to the manager and owns no Workspace/Supervisor/watcher.
Two additive commands provide explicit Connect and read-only operation
reconciliation. Menus, dialogs, appearance and hide-to-tray remain local.

## Ownership and connection boundaries

`NativeBridge::from_native_configuration` obtains the root from native app data
configuration and the CLI binding from the compiled `manager_artifact` resolver.
There is no renderer-provided endpoint/root/role constructor, Workspace,
Supervisor or discovery owner in this module. It holds `NativeClientLease` for
its whole native lifetime: the thin-client singleton/installer lease is separate
from the manager's legacy `desktop.lock` exclusion. A failed connection never
creates a local Workspace or kills/adopts an existing owner.

Connection views contain only typed states/errors, no credentials or paths.
SDK/server request codes remain available instead of being flattened into a
generic successful/default response. `connect_cold_user_launch` is a distinct
explicit cold-launch action. An unprepared build has no artifact authority and
fails rather than searching PATH/cwd/runtime configuration. An existing owner
requires explicit authentication; quitting/unavailable owners do not trigger a
credential probe. The manager path stays fixed and SHA-256 verified. The inherited
native environment is allowlisted; absolute PATH entries and SSH_AUTH_SOCK serve
project system-tool discovery, never selection of the manager executable.

## Explicit provider and native lifecycle renewal

The provider trait has one entry, `credential_after_user_action`. Construction,
connection views, snapshot polling and missing/expired sessions do not invoke it.
`StoreCredentialProvider` uses the root-scoped explicit `read_if_existing` helper.
Missing remains None until that explicit action chooses key creation; malformed
or denied storage returns an error and is never silently repaired. Production
NativeSecretStore access belongs to a worker at that action boundary. A blocked
provider does not hold the SDK mutex.

The published private wire `Authenticate` already verifies the full desktop key
then creates the server-bounded renewable native GUI context. This differs from
the unchanged core `DesktopAuth::authenticate`, which still creates fixed,
nonrenewable sessions. No additional role/TTL flag is accepted from a caller.

`renew_connected_context` is separate native lifecycle maintenance. The native
coordinator calls `maintain_connected_context` before expiry; normal SDK polling
does not silently renew. Maintenance never calls the provider, never resurrects an
expired/revoked/nonrenewable token, and never extends the hard GUI-context
deadline. An expired context requires explicit user authentication. The timer never obtains or provisions a credential; expired authority requires
explicit localized Connect.

## Incremental data and GUI intent state

The SDK now exposes opaque incremental `SnapshotRead` in addition to read/result
images. Begin/step/finish/release retain the8MiB,30-second, owner/root, digest,
cursor and observed-at checks; the older `snapshot()` convenience method delegates
to the same codec. Each bridge page method holds the SDK only for that RPC, so a
native adapter can keep its buffer outside the SDK mutex and yield between pages.
It must not lock the SDK around an entire job/polling/image lifetime.

GUI command intents use the shared finite selector factory and authenticated
`resolve_subject` advice without sending raw headers/YAML for the lookup. Advice
does not bypass authoritative admission/dequeue checks. The tracker retains the
subject, original key, cancellation token and accepted receipt. Its explicit
Prepared/Sending/Admitted/Rejected/Unconfirmed phases prevent replacing an active
or uncertain intent with a hidden new key. A definite rejection can be discarded
explicitly; a stale subject is not silently relabeled.

Cancellation sets the native flag before SDK locking. Before finalization it
aborts admission; after acceptance it targets only the exact GUI receipt. A late
cancel-delivery failure never discards that accepted receipt or pretends the
effect was undone. Durable accesses use the same tracker and keep their original
key on an uncertain reply. Pure `operation_by_key` reconciliation does not retry
execution, and None is not proof an older handler cannot still commit.

The compatibility adapter now returns actual operation results and domain errors,
uses trusted recovery metadata and preserves GUI host_pid. Snapshot/read pages
yield the SDK mutex between RPCs. Unconfirmed durable intents retain their key
across reconnect and expose Check operation without replay. The migration record
separates measured acceptance from remaining platform/dialog/terminal gates.

## Verification and remaining gates

Local macOS ARM64 evidence:

- The focused explicit-provider auth helper case passed: existing load, missing,
  malformed/denied storage, no creation/repair and distinct root identities.
- The targeted1,000-project snapshot producer passed through incremental pages
  with health calls between pages; the old convenience wrapper returned the same
  complete row count. No credential clone or fake snapshot was introduced.
- Four focused native-bridge library fixtures passed with actual Controllers and
  Memory/Deny providers: disconnected polling/artifact absence without probing or
  fallback; singleton/install leases; explicit existing-owner authentication and
  no repeated prompt; separate maintenance renewal without store access; tracked
  cancellation/definite rejection/access and exact accepted IDs; denied provider
  leaving the owner healthy; expired polling with no probe until explicit reconnect.
- Focused manager SDK/auth and desktop library/test clippy checks passed. These
  are host checks, not Windows/Intel native execution or complete GUI acceptance.

No fixture opened a window, Terminal.app, browser, editor, native credential store
or OS permission dialog. The common test helper normalizes only its newly created
disposable Windows fixture root. No production ACL policy was weakened.

The native GUI default is thin-client. Prepared builds require a compiled fixed
CLI target/digest; unprepared builds show the connection notice without PATH
fallback. Debug E2E uses explicit guarded fixture roots and shared cross-process
FixtureSecrets. Production has no environment-selected fixture store.

## Actual cutover acceptance (macOS ARM64)

`output/native/m13-thin-cutover/result.json` records13 passed checks from a real
embedded WebView and actual Tauri IPC without invoke mocks. A fresh e2e CLI and
GUI used only copied cached packages with receipts matching the current catalog.
An actual managed PHP HTTP response passed; GUI plus two independent SDK processes
observed identical manager, Caddy and PHP PIDs. Same-key retry returned the same
operation ID. GUI crash/relaunch and Close preserved those PIDs; a real Connect
button authenticated the warm GUI, with no control-store read before that action.
Quit waited for matching successful cleanup and persisted inhibition. Both cold
and warm screenshots were inspected. No native store, download or permission
dialog was involved. This proves PHP serving, not Laravel vendor/queue/DB behavior.

Shutdown ACK alone is not completion: the SDK captures an opaque shutdown ID and
confirms its owner-bound outcome while holding the legacy AppLease after cleanup.
Failed/stale/missing outcomes keep the GUI open. Every in-flight cold/manual
connection settles before no-owner success; quitting rejects late connections.
Three focused consumer checks cover pending-discovery restoration, cold Connect/
Quit ordering and actual result/large-read/error/host-PID/cleanup compatibility.
The affected frontend workspace tests passed8/8. Windows native cutover,
production credential prompts, native terminal/dialog/focus and signed-package
post-signing CLI digest acceptance remain unverified by this macOS fixture.
