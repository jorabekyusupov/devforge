# M13.1.5: private memory payloads and complete snapshot exports

This is a private native-session backend and codec, not an external tool API or
GUI binding. Public IPC limits remain64KiB requests and256KiB responses. Existing
M7 DebugHttp still limits its actual UTF-8 body to1MiB and applies its existing
method/header/path rules. JSON encoding of a valid body containing NUL can exceed
6MiB, so the private command codec and actor allow an explicit8MiB encoding.
Increasing this transport capacity does not relax any core domain validation.

## Upload contract

All methods require a fresh desktop session. Retained objects bind the actual
authorized peer (UID/SID and PID), canonical root and random store epoch. Native
session renewal for the same peer/root can continue a transfer; another peer or
root cannot consume it. No client role, claimed PID or supplied timestamp grants
access. The controller uses trusted monotonic time and checks the native session
again before actor admission; the actor rechecks its own admission/commit point.

| Method | Input beside session | Result data |
| --- | --- | --- |
| upload_begin | project_id, idempotency_key, size, digest(SHA-256 hex) | upload_id, epoch, size, digest, next_index0 |
| upload_chunk | upload_id, epoch, index, data_hex | upload_id, epoch, received, next_index |
| upload_finalize | upload_id, epoch | Normal actor Receipt |
| upload_abort | upload_id, epoch | aborted:true |

There are at most2 pending uploads, each at most8MiB and together at most16MiB
declared bytes. Chunks contain at most16KiB raw bytes encoded as lower-case hex,
fitting the unchanged64KiB frame. Size, strict index order, epoch, peer, root,
fixed expiry and final digest are checked. The default fixed lifetime is30s;
trusted policy may select a bounded lifetime up to120s. An incomplete, expired,
tampered or replayed finalize yields no executable command.

Finalize consumes the upload before JSON parsing. The parser constructs a strict
existing Workspace Command and returns a non-cloneable PreparedCommand; its
owner/expiry is rechecked on consumption. The controller then calls the real
actor once. Actor namespace/subject and idempotency checks still apply. A body is
never treated as a shell string, public grant or arbitrary executable resource.
The module does not run HTTP itself or fabricate a successful core request.

Bodies/headers/encoded commands remain in memory. Upload bytes use zeroizing
buffers; expiry, abort, failure and consume drop those buffers. Controller expiry
maintenance runs periodically and Quit/Drop clear retained payloads. This does
not promise erasure of every generic JSON/caller copy or instant cleanup while a
process is suspended. No transfer data is persisted in actor receipts or a spool
file. Accepted actor commands follow their operation/cancellation policy rather
than the earlier transfer lifetime.

## Actor queue accounting

The actor retains at most32 waiting items and16MiB of encoded waiting commands,
plus one active command bounded at8MiB. The hash writer counts without a second
full encoded allocation and preserves the earlier tuple hash. Capacity rejection
occurs before a durable Queued row. The mailbox contains metadata, with Commands
held separately by operation ID. Dropping canceled/dequeued/failed-send payloads
releases real retained data and its byte count together. Canceled mailbox
tombstones keep their item slot until consumed, so repeated cancel/admit cannot
grow metadata beyond the item bound. Busy-state cancellation still sets its bit
without waiting for the Workspace/SQLite mutex; the next admission purges a
canceled body if immediate removal could not obtain that mutex.

These limits measure serialized payload bytes with bounded object/metadata
overhead; they are not a claim about exact process RSS. Actor SQLite stores only
identity/digest/status and approved codes, never the uploaded body or headers.

## Snapshot byte images

| Method | Input beside session | Result data |
| --- | --- | --- |
| snapshot_begin | none | snapshot_id, epoch, size, digest, observed_at, cursor |
| snapshot_page | cursor:{snapshot_id,epoch,offset} | snapshot_id, epoch, offset, data_hex, next_cursor |
| snapshot_release | snapshot_id, epoch | released:true |

The image is exactly the existing snapshot data wrapper: snapshot, observed_at,
busy, stopped, error and progress. Every typed row and retained log field is
serialized; no row/log truncation is introduced by paging. The actor's existing
4MiB cache retention/error boundary remains distinct from an8MiB export limit.
An oversized producer/cache has an explicit error rather than an invented empty
success. Exports are at most2 images and16MiB total, with fixed expiry.

Each image is immutable until release/expiry, even if the live actor publishes
another observation. Its cursor binds peer/root/epoch; wrong, missing, expired or
misaligned cursors require explicit resync. Aligned repeated pages are safe to
retry. Native clients concatenate raw decoded bytes, validate offset/total size/
digest, then decode UTF-8 and JSON once at the complete image boundary. A UTF-8
character may cross chunk boundaries. A changed live observation does not force
mixing images or silently dropping rows. Best-effort release frees the image.

## Session lease and transport peer pin

Native auth/redeem/renew responses carry session plus expires_in_ms,
context_remaining_ms and renewable from the authenticated cache. No SecretStore
probe is needed for that metadata or renewal. The SDK can bound its local lease
and preserve the absolute native-context deadline rather than assuming600s.

`Client::request_with_expected_peer` verifies the kernel peer identity/PID before
writing any secret-bearing frame. Initial health or the validated private
bootstrap establishes the pin; receiving a different owner requires explicit
reauthentication rather than silently sending an old key/session to it. Same-user
native access is not a sandbox against equivalent OS account/process privileges.

## Evidence and remaining gates

Local macOS ARM64 focused codec/backend checks passed5 payload cases,9 actor
regressions and7 controller cases, with manager all-target clippy warnings denied.
The codec roundtrips exact1MiB all-NUL and mixed Unicode/NUL DebugHttp bodies,
checks integrity/order/size/expiry/foreign identity/quota/replay, and reconstructs
an escaped log image larger than a single response without dropping fields.
The real isolated actor test fills the waiting-byte budget, proves capacity
rejection writes no receipt, cancels bodies to reclaim bytes, and observes those
jobs canceled before dequeue. It executes no HTTP request and is not an actual
core HTTP/native-runtime acceptance pass.

The separate native SDK tests exercise the real controller with1000 imported
project records and a complete paged image, plus a large Unicode configuration
command's real semantic failure. Those tests retain their own evidence scope.
GUI migration, actual core HTTP/body roundtrip, Windows/native platform matrices,
worktree/cache subject mappings and the recorded release gates remain separate.
