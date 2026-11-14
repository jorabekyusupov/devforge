# M13.2 restricted external control boundary

M13.4 adds a separate [reviewed service boundary](reviewed-service-actions.md).
Its native-reviewed mutation authority does not expand ExternalInput or the
default read-only MCP client described here. Native grant issuance additionally
permits ExecutionTest for that slice; information-content rights remain absent.

The JSON CLI uses `ExternalClient` and the existing authenticated local manager
transport. It never constructs a Workspace, launches a manager, reads native
credentials or obtains a desktop session. Public owner health remains the
existing sanitized version/PID/state observation, with no project/root details.

Six live operations are allowed: `health.health`, `services.status`,
`projects.list`, `projects.get`, `operations.status`, `operations.cancel`.
The remaining planning inventory is unavailable. Version1 requests and responses
carry their request ID; unknown tools return `unsupported_capability`, versions
return `unsupported_version`, and unknown authority fields fail validation.

External requests carry a high-entropy bearer through the private socket/pipe.
The client pins the kernel peer and PID before writing it. The registry derives
the project/client identity; request fields cannot supply a root, client ID,
role or native authority. All routes require Observer, with ProjectManage also
required for own-operation cancellation. A grant's generated client ID is unique
and its rights are immutable: validating that same current grant plus the
receipt client/project namespace revalidates the original grant. No external
operation-admission or execution route is introduced to create receipts here.

The facade resolves the project from the manager cache and checks its stored
filesystem identity against the current directory before issuance/authorization.
Project lists contain at most the one granted project. Outputs use explicit
bounded metadata projections, excluding paths, environment references/values,
headers, command bodies, log text, SQL, mail and source content. Project/service
metadata labels its cached origin/time/revision. Operation status labels the
persisted receipt observation. Cancellation rechecks authorization after the
cancellation-lock wait and before setting the exact owned operation's bit.

Native Rust owner administration can issue/list/revoke grants through its
current desktop session. Issuance requires Observer and permits only this
slice's Observer/ProjectManage rights, with no information-content rights. The
credential is returned once to trusted Rust over the private transport; listing
returns metadata only. There is no renderer/automation grant-issuer route or
`role`/`confirm` substitute for that authority. Registry storage retains its owner
lease; controller cleanup explicitly closes it before the shutdown-completion
record and legacy exclusion release, including when an OS quit handle survives.

Focused macOS ARM64 fixtures exercise actual controller/client IPC, native-issued
external grants, two-project isolation, revoke/expiry, revision mismatch, replaced
directory refusal, unsupported/version/authority-field rejection and denial of
desktop receipt access. An own completed-operation test explicitly relabels a
real terminal receipt in an isolated journal because external admission remains
unavailable; it is namespace/projection evidence, not external execution proof.
All secret stores are in-memory fixtures. Windows native behavior remains a
separate CI gate; no native credential/window/permission actions are performed.
