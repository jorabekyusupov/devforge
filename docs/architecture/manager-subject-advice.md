# Authenticated native subject advice

This narrow prerequisite adds `resolve_subject` for the private native SDK and
future thin bridge. It is not an execution permission, external-grant adapter,
new Workspace, store or GUI/default-mode change.

`desktop::SubjectSelector::from_command` is the single exhaustive mapping shared
by SDK and actor command admission. It emits only one of:

- `{kind:root}` for the existing global desktop namespace;
- `{kind:project,project_id}` for direct project commands, including worktree
  plan/create's source project;
- `{kind:worktree,worktree_id}` for persisted worktree record commands;
- `{kind:cache,instance_id}` for dedicated cache lifecycle commands.

It never serializes a raw Command, HTTP body/header/query, YAML, native secret,
repository path or other command content into this protocol. Unknown selector
fields are rejected, including fields on the empty root struct variant.

The wire method is `resolve_subject{session,selector}`. A fresh authenticated
native desktop session is mandatory. The response is
`{ok:true,data:{project_id:<id>,advisory:true}}`. Project advice requires a unique
active row in the manager-origin cached registry snapshot. Worktree/cache advice
uses persisted resource ownership, including retained resources whose source
project is no longer active. Ambiguous, missing or malformed metadata fails
explicitly. No renderer-supplied root, project label or role proves ownership.

The lookup deliberately uses the existing manager's cached metadata and does not
block on a new Workspace or filesystem scan. Advice can become stale. The SDK/
bridge holds the resolved namespace for the intended operation and surfaces a
mismatch rather than silently relabeling/replaying a command. Actual admission
still checks the typed command subject, and resource ownership is resolved again
against worker-current metadata before execution. Direct project lifecycle
validation remains in the existing domain method; advice does not promise that
a project will still exist when the queued command runs.

Focused local verification covers selector serialization of sensitive HTTP,
query and YAML commands (IDs only), strict unknown-field denial, retained
worktree source/cache ownership, missing/duplicate resource metadata, and a real
authenticated controller lookup. The controller case verifies unauthorized
lookup denial, no startup credential probe, root/project advice, rejected command
relabeling and a project forgotten after advice: stale advice cannot manufacture
a successful domain operation. The existing authoritative subject regression is
also retained. These are backend/SDK prerequisites, not GUI cutover or native
window/platform release evidence.

Local macOS ARM64 checks: the authenticated controller advice/staleness case
passed1/1, selector privacy/resource policy cases passed2/2, and the existing
authoritative actor subject regression passed1/1. Manager all-target clippy with
warnings denied passed. No broad suite or new native GUI/platform run was used
as a substitute for these changed-boundary checks.
