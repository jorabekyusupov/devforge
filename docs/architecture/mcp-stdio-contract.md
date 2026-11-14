# M13.3 bounded read-only stdio contract

M13.4 optionally adds [reviewed project actions](reviewed-service-actions.md)
behind independent --allow-service-actions, --allow-runtime-actions,
--allow-environment-actions, --allow-database-actions and --allow-domain-actions
flags. The five-tool read-only surface below remains
the default; these flags themselves grant no mutation authority.

The adapter supports modern `2026-07-28` and legacy `2025-11-25`, based on the
official primary specifications read on 2026-11-14. Modern requests independently
require `params._meta` protocol version and client capabilities; `server/discover`
advertises both supported revisions. Modern complete results carry `resultType`
and server identity. Required caching hints use TTL 0; discovery/tool definitions
are public, authorized resource listings/contents are private. No prior modern
request supplies missing metadata to another request. Legacy clients instead
use initialize/initialized; responses use the legacy result shape.

The pure `mcp_contract::Protocol` returns a local reply, ignored notification,
validated request-cancellation ID or typed read-only call. The CLI owns framed
stdin/stdout, bounded work, reply suppression for MCP request cancellation and
EOF. Such cancellation never calls the manager's operation-cancel route. Client
identity/capabilities are protocol data, never project authority. No server-to-
client requests, HTTP, tasks, subscriptions, content exports or code execution
are implemented or advertised.

Exactly five tools map to the existing external-grant facade: `health.health`,
`services.status`, `projects.list`, `projects.get`, `operations.status`. A single
fixed `devforge://project/ID/summary` resource uses the same authorized project
metadata projection. Resource listing also calls the manager before exposing
the URI; completion verifies the returned project's ID. Foreign URIs, extra
project/root/role arguments and cursors are rejected. Metadata/protocol fixtures
are separate from actual external authorization evidence.

Credentials come only from an explicit absolute credential-file path, never
token argv, environment or protocol stdin. The read-only loader accepts at most
4096 bytes of complete strict JSON: `schema:1`, `project_id`, `credential`. The
file must be regular, unlinked elsewhere and private to the current OS user;
Unix uses no-follow/nonblocking opens plus UID/mode/link checks, Windows uses
no-reparse file checks and owner/effective single-user DACL validation on the
opened handle. Nothing is created, chmodded or repaired. Owned bytes/tokens are
zeroized; this does not claim all serde/OS copies are erased. The shared test
helper creates only fresh disposable private files, with explicit Windows
TokenUser ownership to avoid elevated-runner default-owner differences.

The file's project is an additional constraint, not authorization.
`BoundReadOnlyClient::connect` verifies it against the manager's actual grant
before data delivery; every read still passes through registry authorization.
It rejects operation cancel even when the underlying grant has ProjectManage,
and rejects foreign projects. Peer/PID pinning precedes bearer delivery. No
Workspace, owner autostart, native session/store or implicit reconnect is used.
Static discovery/tool metadata may work without a manager; protected resource
listing/reads and tool calls cannot.

Bounds remain 64 KiB input and 256 KiB output with an explicit output-limit failure,
never truncation. Focused macOS ARM64 evidence: credential loader 2 tests, pure
protocol 4 tests, actual manager-bound read-only client 1 test and scoped clippy
passed. The Windows handle/DACL implementation requires its native CI gate.

Actual macOS client acceptance also completed against the isolated, granted
fixture manager:

- MCP Inspector 2.15.0, using its memory backend and the official SDK 1.29.0,
  completed tool discovery/call and resource discovery/read. Discovery exposed
  the five read-only tools and the single bound summary resource.
- Codex CLI 0.167.0-alpha.4.1 completed an actual `projects.list` tool call.
- VS Code 1.107.1 completed built-in discovery of the five tools and an actual
  `projects.list` call. Its recorded `initialize`/`notifications/initialized`
  exchange negotiated the supported legacy `2025-11-25` revision.

The first Inspector attempt failed while locating its backend; the first VS
Code attempt did not reach a handshake. Those were client bootstrap attempts,
followed by the successful runs above, rather than product protocol failures.
The owned fixture was subsequently shut down and its manager released cleanly.
These results cover the recorded calls and legacy compatibility, not every tool
in every client, a third-party modern `2026-07-28` client, Windows client runtime,
HTTP transport, execution or production credential-store behavior. Modern
stateless behavior remains supported by the focused protocol tests.

Primary references:

- [Modern stdio](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/basic/transports/stdio.mdx)
- [Modern versioning](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/basic/versioning.mdx)
- [Required request metadata](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/basic/index.mdx)
- [Tools](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/server/tools.mdx), [resources](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/server/resources.mdx), [caching](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/server/utilities/caching.mdx)
- [Legacy lifecycle](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2025-11-25/basic/lifecycle.mdx)

## CLI transport and acceptance evidence

`devforge mcp serve --credential-file ABSOLUTE_PATH` reserves stdin/stdout for
newline-delimited JSON-RPC. `mcp --help` and `mcp serve --help` resolve before
credential/endpoint access and do not create state. Invalid startup arguments or
an unsafe credential file fail with a stable stderr diagnostic and no stdout
payload. No bearer value is accepted as a command argument or environment value.

The reader bounds each frame, drains oversized lines and preserves the next
frame. Malformed JSON, oversized input and a partial final frame produce bounded
protocol errors without echoing the input. Output is serialized and size-checked
before writing one line; a larger response becomes an explicit output-limit
error. Empty/invalid frames never become manager calls. The fixed five-tool and
one-resource inventories fit one page: no cursor is issued, and a supplied
cursor is rejected so the client must restart listing without it.

An input queue of8 and a work queue of8 bound retained requests. One worker owns
the restricted client; at most9 protected calls can be queued/in flight. Protocol
metadata and cancellation remain responsive while a read waits for IPC.
Cancellation marks only an already pending MCP request: queued work is skipped
and an in-flight read's reply is suppressed. It never cancels a manager job.
EOF marks pending reads cancelled, drops the work queue and joins bounded IPC
work; it sends neither Quit nor an owner-stop signal. The first protected read
lazily verifies the fixed credential/project binding; failure does not silently
load another credential, repin a replacement owner or launch a Workspace.

Focused CLI evidence on macOS ARM64:

- 2 transport unit cases passed: framing/output bounds and cancellation suppressing
  a pending read reply while protocol ping remains responsive.
- 3 subprocess integration cases passed: no-write help/startup rejection; modern/
  legacy lifecycle, malformed/oversized frames, cursor rejection, missing owner
  and EOF; actual scoped tools/summary, foreign/revoked binding denial and EOF
  preserving the real Controller owner.
- 1 separately invoked manual fixture passed after Inspector, Codex and VS Code
  used the shared real manager. Explicit release joined the manager worker,
  removed its disposable root and left `api health` reporting unavailable/exit4.
- Scoped CLI bin/MCP integration clippy passed. Existing unrelated suites were
  not rerun. Native Windows behavior still requires its own CI/native evidence.

Sanitized client and cleanup artifacts are under `output/native/m13-stdio/`:
`inspector-result.json`, `legacy-sdk-result.json`, `codex-result.json`,
`vscode-result.json`, `vscode-handshake.json`, `vscode-verification.json` and
`fixture-cleanup.json`. These contain permitted metadata and outcomes, not bearer
contents. The initial Inspector default-backend lookup failure remains explicitly
separate from the successful memory-backed acceptance run. No DevForge fixture
used a native credential provider or requested OS permission changes.
