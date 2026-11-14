# M13.1.5: Rust native-client SDK and large private payloads

Status: Rust SDK and isolated native-controller evidence. This is not a Tauri
bridge/GUI migration, complete M7 desktop compatibility, MCP server or public
external-agent command surface. The manager still retains its explicit legacy
desktop exclusion. The [private bootstrap](manager-bootstrap.md) and
[credential policy](manager-auth.md) remain its authority boundaries.

## Rust-only ownership and explicit authentication

`NativeClient` is a blocking Rust-only object. A later Tauri adapter must keep it
behind native state/worker coordination, not run it on the UI thread or serialize
it to JavaScript. There is no serde or Clone implementation for the client.
Debug exposes only the owner PID and whether a session is cached; credentials,
roots, payload bodies and user paths are not printed by its errors/Debug.

Two explicit construction routes exist:

- `from_bootstrapped(endpoint, child)` checks the pipe credential's canonical-root
  fingerprint and the actual kernel-observed owner PID against the freshly
  launched child before redemption. The ready child splits into a one-use
  credential and a credential-free monitor. The SDK erases the bootstrap
  credential after redemption and retains only the monitor for orderly reaping.
- `authenticate_explicit(endpoint, credential)` receives an already obtained
  native credential from an explicit trusted action, checks actual health and
  kernel identity, then performs the native authentication RPC. It does not
  instantiate, read or provision SecretStore. It never falls back to a provider
  or changes root/owner automatically.

Every subsequent private call pins the complete observed OS peer. The transport
checks that peer, including PID, **before** writing a session/key/body. A changed
owner/unsafe endpoint invalidates the local session and returns an explicit
owner-change error. A future adapter must supply its Endpoint from trusted native
configuration, not an arbitrary renderer-provided path or role flag. The SDK
exposes no raw-RPC method.

The SDK never kills a ready owner, performs a hidden start/restart or retries a
mutation automatically. An optional launched-child wait method only reaps the
owner after orderly authenticated Quit. A failed/expired session requires explicit
reauthentication. Existing owner replacement and update compatibility remain
separate from a successful initial handshake.

## Lifetime metadata and renewal

Auth/redeem/renew responses carry a private session plus cache-only metadata:
`expires_in_ms`, `context_remaining_ms` and `renewable`. The auth policy's
`session_info` accessor verifies peer/root/generation/expiry without touching a
store. Ordinary fixed sessions have no renewable context; explicit native GUI
contexts preserve the absolute deadline defined in the bootstrap contract.

The SDK removes/wraps the session string before fallible envelope/metadata
parsing, validates policy bounds and derives conservative local deadlines from
request-start time and server remaining duration, with a small expiry margin.
No clock synchronization is needed. A renewal can never move the previously
known absolute context deadline later, even when response timing differs.

`renew_explicit` is a distinct operation, not a side effect of snapshot/status or
ordinary usage. It rejects expired/nonrenewable contexts, rotates a still-valid
token and never reads native credentials. A lost or malformed renewal response
discards the old token, because server rotation may already have happened. There
is no expired-token resurrection or automatic credential-provider prompt.

The native context's12-hour default/24-hour maximum cross-process revoke window
still applies. A10-minute per-token TTL does not establish a10-minute revoke
bound when explicit renewal is enabled. Owner notification or an observed key
change can revoke sooner; this SDK does not claim to implement that notification.

## Fixed control frames, complete payloads

The SDK uses the shared backend command/snapshot/chunk bounds rather than raising
the64KiB public control-frame limit. It does not fabricate or silently truncate a
snapshot when an output cannot fit. Private native full-Command admission remains
separate from any future external-agent grant.

Command admission serializes the actual typed `Command` through a bounded writer
(8MiB), hashes the complete bytes and uses `upload_begin`, ordered16KiB chunks,
`upload_finalize` and best-effort `upload_abort`. Every response must echo the
expected upload/epoch, byte count and next index. The caller supplies its project
and idempotency IDs. Finalization is sent once. An ambiguous finalization response
is `native_client_admission_unconfirmed`; the SDK never invents a successful
receipt or silently sends a second admission. The original idempotency identity
is available to the caller for explicit reconciliation/retry.

Snapshot reads use `snapshot_begin`, immutable `snapshot_page` cursors and
best-effort `snapshot_release`. The complete export is bounded by8MiB and a fixed
TTL. The SDK checks size, SHA-256, snapshot/epoch identity, offsets and every next
cursor. UTF-8/JSON is decoded only after whole-byte reassembly, including when a
character crosses a page. The producer's `observed_at` must match the decoded
wrapper. Projects/runtimes/databases/logs must be actual arrays; no default or
placeholder snapshot is substituted. Expired/invalid cursors require explicit
resynchronization. Release failure preserves the original error or verified
snapshot; the server's fixed export TTL bounds retained memory.

Status, result, typed failure detail, cancellation and Quit wrappers preserve
their server outcomes. Receipt/project/operation identities are checked. Returned
failure codes are bounded stable codes, not raw exception messages or command
bodies. Known credential and encoded command fields are zeroized after requests;
private frame buffers and Rust credential types are zeroizing. This does not
promise that every generic serde JSON copy or unrelated project output has been
securely erased.

## Evidence and remaining gates

Local macOS ARM64 custom-harness SDK checks passed four cases:

1. Explicit memory-store authentication; real project import, idempotent replay,
   favorite update, receipt/status/result, terminal cancellation and failure
   detail. A Unicode command over1MiB crossed the complete upload/admission path,
   then failed the existing real manifest validation. This proves transport and
   typed failure behavior; it does not raise the domain's YAML size limit.
2. A real `ProjectStore` producer populated1,000 isolated Laravel metadata rows.
   The actual actor snapshot exceeded the old262KiB reply bound; the SDK returned
   all1,000 rows with Unicode names intact through the paged/digest codec.
3. Explicit renewal remained usable after the initial token lifetime, did not
   access the fixture store again, and failed after the absolute context deadline.
4. A real separately launched Controller issued a private pipe credential with
   deny-store adapters. Cold SDK redemption/renewal/snapshot worked; graceful owner
   replacement caused the old SDK's kernel pin to fail and discard its session.

The cache-only session metadata fixture also passed separately. Memory/deny stores
and disposable roots were used throughout; no OS credential service, GUI, system
permissions or adjacent application was changed. The SDK and bootstrap harnesses
report structured `passed`/`unavailable` case counts, so a restricted Windows
ancestor Job cannot be counted as a successful detached cold-launch test.

Native Windows/Intel execution of this new SDK slice requires its own pipeline
evidence. Hosted Windows may forbid detached breakaway; a separate ordinary-child
pipe/controller component can exercise CRT/pipe behavior without proving detached
survival. It is a test component, never a production fallback. Actual GUI cold/warm
start, renderer credential isolation, complete M7 control/read compatibility,
minimum OS, native credential dialogs, installers/signing and HTTP transport remain
separate gates. No GUI owner mode is enabled by this SDK.
