# M13.2 JSON CLI

The additive `api` family uses control API version1. Legacy execution routes stay
unchanged; wrapper help for legacy families is intercepted before their stores/
leases. Interception stops at `--` and, for `project exec PROJECT TOOL`, after the
selected tool, so `--help`/`-h` forwarded to that tool retain their old meaning.
No MCP, project execution, arbitrary RPC or content-read tool is added here.

## Syntax and metadata

```
devforge api [--api-version 1] [--request-id ID] COMMAND [--json]
devforge api health
devforge api status --credential-stdin
devforge api project list --credential-stdin
devforge api project inspect PROJECT_ID --credential-stdin
devforge api operation status OPERATION_ID --project PROJECT_ID --credential-stdin
devforge api operation cancel OPERATION_ID --project PROJECT_ID --credential-stdin
devforge api schema
devforge api capabilities
```

`api` output is JSON even when `--json` is omitted. Global/subcommand `--help`
returns usage/examples and does not read credentials, resolve an endpoint, open a
store or create an app-data root. `schema` describes the implemented logical CLI
inputs and versioned response envelope. `capabilities` lists this narrow live
surface separately from the broader M13.0 planning inventory. It makes no claim
that a manager is currently reachable or a grant is currently valid. Help, schema
and capabilities use the same command metadata.

Each stdout response is one JSON object: `api_version`, `request_id`, `ok`,
`warnings`, optional `operation_id`, and exactly one of `data` or typed `error`.
The existing control Response/Fault/ErrorCode contract is reused. With no supplied
request ID, the CLI generates a bounded invocation correlation ID; it is not an
idempotency key. Parse failures use a generated ID when the full invocation cannot
be accepted. Errors contain stable codes and retryability without raw paths,
credentials, parser details or request bodies. No progress text is mixed into
stdout; diagnostics belong to stderr.

| Exit | Meaning |
| --- | --- |
|0|Success|
|2|Invalid input or unsupported API version|
|3|Unauthorized, expired/revoked grant or project mismatch|
|4|Manager/endpoint/storage unavailable|
|5|Conflict, stale review/revision, invalid transition or busy|
|6|Unsupported capability or output/cursor bound|
|7|Timeout or interrupted owner|

## Restricted live boundary

The CLI constructs the restricted `ExternalClient`, never `NativeClient` or a
Workspace. It observes and pins the actual kernel peer/PID before sending a
credential, then uses only typed ExternalInput variants. It never launches a
manager or silently probes/provisions a native credential store. An absent owner
returns an error without creating a data root. The configured app-data endpoint
is native configuration; there is no JSON root/role/client-ID authority field.

Public `health` exposes sanitized owner/version/state only. Other commands require
explicit `--credential-stdin`: the caller supplies exactly64 lowercase hex bytes,
optional LF/CRLF, then EOF through its private input pipe. The CLI rejects a TTY,
invalid/oversized input and credential arguments; it has no credential environment
variable, token file lookup or native-store fallback. Temporary owned token
buffers are zeroizing; credentials are never returned by these commands.

The manager derives the grant's project and server-assigned client identity.
List/status show only that project; inspect must match it. Observer permits safe
metadata, not logs/mail/source/env values. Operation status/cancel require the
same server-assigned operation owner; cancellation additionally requires the
original ProjectManage right and revalidation at the actual cancellation boundary.
Grant issuance/revoke remain authenticated trusted native-administration calls,
not automation commands. M13.2 exposes no operation-admission tool to obtain new
execution rights. No auto-retry or alternate key/body is introduced.

## Focused evidence

Local macOS ARM64 checks cover no-write metadata/help, strict version/flag parsing,
redacted errors/exit codes, bounded stdin and missing-owner behavior. A real
Controller plus separate CLI subprocesses runs with two registered projects and
persisted grants: only the permitted project is listed/inspected, cross-project,
revoked and foreign-operation requests deny. Fixtures use Memory/Deny stores and
the existing compile-gated disposable root helper; no GUI, native credential
provider, runtime download or OS permission dialog is involved. This does not
establish native Windows execution or execution-tool behavior.
