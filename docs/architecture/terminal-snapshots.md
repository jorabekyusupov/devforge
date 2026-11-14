# Terminal snapshots during manager coexistence

Status: implemented source prerequisite for M13.1. This does not establish complete
GUI/CLI/MCP owner integration, actual terminal-window acceptance or public release.

## Problem and decision

The terminal's interactive shell does not construct a Workspace, but the previous
Context preparation rewrote `config/<project-id>/php.ini` and `bin` launchers.
Opening a second terminal or changing manager runtime/settings could change files
used by the first terminal. Allowing generic legacy CLI commands beside a manager
would also permit independent lifecycle ownership. Merely exempting the old
`project shell` route from manager exclusion was insufficient.

An explicitly opened terminal now uses `terminal::Session`. It chooses a unique
private `.terminal-*` directory before Context preparation. That directory owns
only the terminal INI, scan/session files and PHP/Composer/Artisan launchers.
Verified PHP, Node, Composer and applicable debugger runtime resources remain
leased until the shell exits. Drop removes only the invocation's own directory;
a sibling session, manager configuration, project source and persistent tooling
homes remain intact.

The shell uses the exact captured PHP/Node/Composer selection and PHP settings.
Later manager selection changes do not rewrite its INI or launchers. Persistent
`COMPOSER_HOME`, Composer/npm caches and npm user configuration remain project
scoped. Authentication files are not copied. Initial npmrc creation uses
`create_new`, preserving an existing file even when construction races.

## Metadata and credential boundary

`ProjectStore::open_existing_current` accepts only an existing marked schema8
store through a read-only SQLite connection. It does not create the store, repair
permissions or migrate metadata. Older/future schemas fail without modification.
The root and metadata files reject symlink/reparse entries.

`FrozenTerminalFacts` captures only the selected Project, optional DB Binding and
Instance, and selected CacheInstance in a brief read transaction. Each selected
record is bounded to 262144 bytes. The transaction ends before native credential
access, provider lookup or child execution. DB credentials and cache environment
are resolved from those captured identities, avoiding a second mutable binding or
instance lookup after a credential prompt. This is not a clone of every project's
database or a long-lived SQL lock.

Mail, Inspector and debugger services can subsequently stop or rotate. Capturing
their environment is not a promise to keep those services alive. No manager or
bootstrap credential is handed off through terminal argv or a new credential file.
Existing Xdebug trigger-value/idekey behavior remains unchanged in the terminal's
private INI; on Unix the session directory is0700 and INI0600. That existing debug
material is deleted with the owned session, not replaced by an unverified weaker
environment-based trigger policy.

The debugger `resolve_existing` path reads and verifies existing attested entries
without creating or chmod-repairing the manager's provider registry.

## Entry points and paths

Only exact CLI argv `project shell <id>` enters the new snapshot path before the
legacy command router. Extra arguments, other commands and prefixes do not gain a
generic execution or mutation exemption. The operation lease still protects the
shell against installer replacement.

The desktop binary's terminal handler likewise requires its exact argument shape
and opens current metadata read-only. `Workspace::open_target_inner` no longer
performs shared Context/shim preparation before launching the terminal process.

Before the writable PackageStore constructor is called, `existing_packages`
checks the existing runtime root and parent, marker and fixed layout directories.
Symlinks, reparse/junction entries and canonical escapes are rejected. The marker
must be a regular file with the expected exact content. Missing stores/layouts
are not silently created by this entrypoint. These are ordinary private-path
checks, not a claim of race-free hostile-filesystem/openat containment.

## Verification actually performed

Focused local macOS checks:

- Seven `devforge-projects --lib terminal::tests` cases passed: current-schema
  read-only/no migration; metadata symlink refusal; two independent sessions with
  exact runtime/Composer/settings selection and lease/cleanup checks; concurrent
  DB/cache replacement during SecretStore access; failed-session cleanup; existing
  provider lookup without registry repair; outside-path canary/link guards.
- The concurrent metadata test commits a writer with SQLite busy_timeout0 during
  SecretStore access. The shell retains the captured DB port/binding/password
  reference and cache port, demonstrating the source SQL lock was already released.
- The immutable-session case executes native argv-reporting programs via the real
  shell and verified runtime package path. Shared manager INI/shims and existing
  npmrc auth sentinel remain unchanged; no auth-file content is copied to launchers.
- That case was rerun after adding PHP-settings immutability checks and Windows
  fixture `.exe`/simplified-path handling; it passed on macOS.
- The CLI exact-route filter test passed once locally. A later integration attempt
  was blocked by concurrent, incomplete manager actor source, not by a terminal
  test failure. The root integrator owns the final isolated staged-tree CLI and
  desktop consumer gate; this document does not predeclare that result.
- Targeted projects/debugger lib/tests clippy with warnings denied passed. Scoped
  diff/format checks passed. A desktop-library cargo check passed locally.

The initial native reporter fixture copied Apple's `/bin/echo` into a verified
package. The relocated platform binary was killed with exit137 and empty output.
The fixture was corrected to compile a tiny test-owned native Rust argv reporter
before hashing/installing it. No production retry, bypass or sleep was added.
This fixture failure is not evidence of a terminal runtime defect.

## Remaining acceptance

Windows test source includes a disposable junction fixture and native reporter
with `.exe` paths, but this task did not execute those tests on Windows. macOS
source/native shell probes do not establish Windows execution. No actual Terminal
or Windows console window was opened. Real PHP/Node/Composer/Xdebug packages and
production keychain dialogs were not reaccepted in this slice. Those native
integration checks remain distinct from the synthetic verified reporter tests.

No full product suite was rerun for this isolated change. Public runtime, UI,
installer and release claims retain their existing per-target evidence limits.


## Integrator staged-tree gate

The integrator archived the exact staged tree to isolate this slice from concurrent
unfinished manager-facade source. `cargo test -p devforge-cli --bin devforge
terminal_route_tests::` passed one case; `cargo check -p devforge-desktop --lib`
passed. The staged knowledge bundle passed strict40 documents/0errors/0warnings.
These checks do not establish native terminal/GUI behavior on either platform.
