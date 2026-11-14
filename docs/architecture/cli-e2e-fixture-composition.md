# Debug CLI fixture-store composition

This slice supplies an explicit isolated CLI sidecar for actual desktop E2E work.
It does not enable the GUI client/default, open a native window, query native OS
credentials or change system permissions. Default production stores remain the
native project/control providers.

## Compile and activation boundary

The CLI e2e feature enables test-support only for debug acceptance builds. The
exact compiled e2e_guard module rejects feature=e2e with debug assertions disabled
using compile_error. The separate negative compile check runs this dependency-free
guard in release configuration; it is not a claim that an optimized release
application was built or signed.

Manager fixture activation requires all of:

- the CLI e2e compile feature in a debug build;
- exact internal flag `--internal-fixture-store` on an allowed route;
- explicit DEVFORGE_DATA_DIR;
- successful read-only FixtureRoot validation before data directories, leases,
  settings or credential stores are opened/written.

Production rejects the reserved fixture flag namespace before filesystem or
credential bootstrap, including malformed equal-sign spellings. Environment or
marker presence never selects FixtureSecrets. In an e2e binary, an unflagged
manager/shell aimed at a marker-bearing root fails rather than falling through
to native credentials. Invalid fixture roots have no native fallback.

The older developer test-support environment paths in other CLI commands are not
rewritten or advertised as this factory. Production feature selection/packaging
and those historical acceptance helpers retain their separate boundaries.

## Disposable root initialization

`FixtureRoot::create_new(path)` creates one NEW private directory leaf under an
existing inspected parent. It refuses an existing directory, including an empty
one. It never adopts, resets, chmods or repairs a user directory. Native path
canonicalization and parent identity are checked. Unix creates mode0700; Windows
creates only that new leaf with current TokenUser ownership and a protected
single-user inheritable DACL, then verifies it.

The bounded marker `.devforge-disposable-e2e.json` binds schema/kind, SHA-256 of
the native canonical root path and the root's actual filesystem identity. The
marker must remain a regular private single-link non-reparse file. Missing,
malformed, oversized, copied, identity-mismatched or linked markers fail closed.
Validation performs no repair or root creation.

An e2e-only subprocess entry makes harness setup concrete:

```text
DEVFORGE_DATA_DIR=<new-leaf> devforge manager __fixture-root-init --internal-fixture-store
```

Its only output is `{"initialized":true}`. The harness must choose a disposable
parent/root; this is not a sandbox or authority to create directories beneath
arbitrary user-selected locations. The marker is an accidental-misconfiguration
guard, not an unforgeable permission against equivalent same-user process access.

## Shared stores and shell path

After validation the manager uses only fixed locked fixture files:

- project credentials: e2e-secrets.json;
- desktop/control credentials: e2e-control-secrets.json.

The parent GUI and sidecar use the same path helpers and the existing transaction-
locked FixtureSecrets implementation. Every get/set/delete reloads under its
file lock; a manager constructed before a GUI write sees the latest control key,
and a GUI helper constructed before manager project writes sees merged values.
No cached map, filename from RPC or native-provider fallback is introduced.

The exact debug interactive route is `project shell <registered-id>
--internal-fixture-store`. It validates the marker before its operation lease,
existing registry/package snapshot or project credential provider is opened.
The original unflagged production route remains unchanged.

Trusted native composition propagates a fixed additional argument through
Controller options, the actor's initial Workspace configuration and the core
terminal launcher. Core open_with_arguments is agnostic to fixture flag meaning,
validates bounded/control-free arguments and uses the existing quoting/argument
array rules. It has no renderer/RPC setter. Production defaults are empty; fixture
composition supplies the exact flag, so an opened fixture terminal cannot silently
invoke a native credential-backed shell. Tests validate launch argument shape
without opening Terminal.app or a console window.

## Focused evidence

Local macOS ARM64 checks passed root-marker creation/validation/link tests,
production flag rejection before writes, a negative release-configuration guard
compile, explicit fixture factory/strict shell/launcher tests, and an actual debug
CLI private bootstrap plus warm authentication case. That case writes the control
key from a separate parent/GUI-side fixture object after manager construction;
it also merges parent and manager project-store writes and reads them through
the pre-existing parent helper. Invalid/missing/copied roots and an existing
canary-directory initialization are rejected without adoption or outside writes.

These checks use fixture files and no native credential provider. They are not
actual GUI flow/terminal-window evidence, an interactive shell/runtime acceptance,
a full release build or Windows native owner/DACL certification. Production
store behavior was not replaced or probed. Root shared knowledge/phase evidence
must retain those target limits when integrating this prerequisite.

Focused result counts for this slice: marker guard3/3, actual CLI fixture
composition3/3, production flag rejection1/1, release-configuration guard negative
compile1/1, fixture/native selection and fixed store factories2/2, strict shell
route1/1 and trusted launcher argument1/1 passed. CLI e2e all-target clippy and
workspace fixture-test clippy with warnings denied passed. No complete optimized
release/installer build, GUI/terminal window or Windows native run is claimed.
