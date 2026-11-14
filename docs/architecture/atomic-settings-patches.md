# Atomic preferences field patches

Status: implemented platform API and GUI writer migration. The manager discovery
root writer must use this same API when its desktop-controller slice integrates.

## Problem

The GUI's process-local mutex cannot protect a separate manager process. A
load/merge/save of the whole preferences JSON may overwrite a newer folder,
language or theme written by another SQLite connection. Default-folder
initialization had the same stale-record exposure.

## Decision

`SqliteSettings::patch(SettingsPatch, &LocaleRegistry)` obtains a SQLite
`BEGIN IMMEDIATE` write reservation before reading the latest record, merges only
supplied fields, writes and commits, then returns the merged typed settings.
Unknown JSON fields are preserved. Concurrent edits to different fields merge;
explicit edits to the same field retain ordinary commit-order semantics.

Explicit language changes must be in the locale registry. An unsupported saved
language is preserved for fallback when a caller changes only theme/folder.
Explicit folders must be nonempty absolute paths without control characters and
within the lexical byte bound. GUI/manager callers still resolve home shorthand,
canonical identity, existence and accessibility before supplying the folder.

The GUI snapshot writer normalizes only the supplied folder and calls patch; it
never saves a whole settings object derived from an earlier read. A manager root
change must send only project_folder in SettingsPatch, not stale theme/language.
`SettingsRepository::save` remains a compatibility whole-record replacement API;
it is not the concurrent GUI/manager field-update route.

`initialize_project_folder` uses the same immediate transaction and rereads the
latest record. It initializes only None, preserving any manual saved choice and
all other fields. A missing default directory after initialization is not silently
recreated. The first directory-creation attempt remains best effort; filesystem
creation and SQLite commit are not a cross-resource atomic transaction.

No preferences schema bump, locale branch, forced appearance, keychain call or
production/system preference change is introduced.

## Focused evidence

- Four atomic-settings tests: stale GUI read plus independent manager write,
  simultaneous disjoint patches on independent connections, default initialization
  racing a manual folder/language/theme patch, and unknown locale/future JSON fields.
- Existing discovery initialization/resolution: two tests passed.
- Existing corrupt/future-schema preservation: one test passed.
- Existing reopen/idempotent storage and unsupported-language fallback: two tests passed.
- Scoped platform lib/tests clippy with -D warnings passed.
- `cargo check -p devforge-desktop --lib`: passed on macOS ARM64; direct GUI
  Rust consumer compiles against the new patch API.

Tests use disposable SQLite connections and temporary home directories only. This
is storage-concurrency evidence, not native UI/Windows/keychain acceptance. Manager
controller integration and native cross-process/UI acceptance are recorded by the
integrator when their dependent source slice is ready; desktop compilation is
not a native interaction pass. No full-product suite was rerun for this settings
API change.
