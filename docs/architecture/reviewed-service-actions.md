# M13.4 reviewed project actions

M13.4 exposes the reviewed service lifecycle, runtime install/select and
environment/YAML actions described below. Database and HTTPS mutations remain
later slices. The default MCP surface remains read-only.

## Authority and execution

The existing control Request<MutationInput>, PlanReceipt, ReviewedPlan and
MutationReceipt are shared by the native owner, JSON CLI and optional MCP tools.
An external grant needs ProjectManage; start/restart additionally need
ExecutionTest. Native approval of the exact plan supplies project trust. A caller
cannot approve itself or provide a trusted flag.

Review binds the grant/client, project root and filesystem identity, revision,
action, key, payload fingerprint and owned-service state. Plans expire after five
minutes and are bounded in memory (128 entries / 1 MiB). Admission and actual
dequeue recheck authority and state. Restart rechecks cancellation, deadline,
grant and directory identity before its start step.
Service admission also holds the existing Quit gate, returning Busy while that
gate is held and Interrupted once explicit Quit has inhibited new work.

The single existing actor performs the action. Start/restart require matching
cached packages and existing configuration; they do not install/download or
silently change project pins. Stop targets the selected owned project session.
Configured shared database/cache dependencies may remain running after failure
or cancellation. Restart may already have stopped the selected session when its
later start is refused. Existing bounded database/cache startup calls are checked
before and after; readiness loops cooperate with cancellation. Cleanup can outlast
the deadline, and a completed success is not relabeled by late cancellation.

Receipts use the existing SQLite journal. Additive schema2 stores the original
admitted revision in service_operations in the same transaction as admission;
no bearer or raw command is journaled. Uncertain admission returns
admission_unconfirmed: preserve the original key and look it up, without hidden
retry or generating a replacement key. Replays return the existing receipt;
changed payloads using that key conflict. Lookup remains grant/client/project
scoped. Successful missing lookup retains an explicit JSON data:null.

## Adapters

```
devforge api service plan ACTION PROJECT_ID --revision N --idempotency-key KEY --credential-stdin
devforge api service ACTION PROJECT_ID --revision N --idempotency-key KEY --review DIGEST --credential-stdin
devforge api operation lookup KEY --project PROJECT_ID --credential-stdin
devforge mcp serve --credential-file ABSOLUTE_PATH --allow-service-actions
```

ACTION is start, stop or restart. The MCP flag adds services.plan/start/stop/restart
to the five default tools. It selects the surface and grants no authority. Native
exact-plan approval and current grants remain mandatory. Neither adapter starts
a manager or obtains native desktop credentials.

## Evidence boundary

Source: manager service_control/actor, workspace service_control, serving manager,
CLI api/mcp, and manager external_client/mcp_contract. Focused integration cases
live in manager/tests/service_control.rs, cli/tests/service_actions.rs,
workspace/tests/service_control.rs and control/tests/service_envelope.rs.
Actual test outcomes and platform limitations are recorded in the
[execution status](../knowledge/execution-status.md).

Client verification: one exact-intent CLI metadata/parser case, one actual CLI
integration covering reviewed inactive Stop and denied scope/cache/revision/
cross-project/revocation cases, one MCP integration (nine opt-in tools versus
five default tools, approval, lookup and EOF), and one real-owner fault relay
passed. The relay discarded an accepted admission reply: the SDK returned
admission_unconfirmed with no automatic retry, and original-key lookup/replay
recovered the same operation; changed-body reuse conflicted. Scoped CLI
bin/service_actions/json_api clippy passed. A subsequent help metadata literal
correction was checked by its existing targeted help case; clippy was not repeated.
These client tests use inactive Stop; they do not prove native HTTP startup.

Independent QA inspected only these service boundaries and found no demonstrated
acceptance blocker. That was read-only source review, not additional runtime proof.

Actual macOS ARM64 cached-runtime acceptance passed (one case, 32.12 seconds):
HTTP from two isolated projects, same-key replay with stable PID, restart with
changed selected PID, and stopping A while B retains its PID and HTTP response;
then B stopped and the owned fixture joined cleanly. No runtime download or
system credential/permission change was used. Earlier setup failures (uncached
legacy variant and store marker order) preceded startup. A later transient-busy
polling error exposed an active cancellation state-transition bug during cleanup;
both were corrected before the successful final run. The original runtime cache
was read-only. This is backend/native-process proof, not GUI/native-dialog proof.
Windows local execution remains unavailable. Subsequent service baseline8ae765c
passed macOS/Windows CI; actual Windows policy/migration3 and workspace refusal/
preservation3 tests passed. This is not Windows PHP HTTP/native GUI proof.

## Runtime install/select slice

Install/select reuse the canonical PackageInput and ConfigureInput, the existing
review store, native approval, actor receipts, original-key reconciliation and
Quit gate. Separate runtime_plan/apply/lookup routes keep service-only clients
finite. The independent --allow-runtime-actions MCP surface is opt-in; neither
runtime nor service flag grants authority.

Install requires ProjectManage and reviews the exact catalog package/target/
digest plus its shared-cache effect. It does not select the package for a project.
Select requires ProjectManage and ExecutionTest and changes only PHP/Node.
Name/domain/HTTPS/Vite/queue must equal the current server configuration;
Composer has no input field and stays unchanged. A scoped runtime_selection
projection supplies these preserved fields and revision without native
credential-store reads.
Clients retain the original expected revision. If unrelated preserved fields
later change, a client cannot reconstruct the old canonical selection body from
the current projection; it must use explicit original-key lookup rather than
claim body equivalence or generate a replacement key.
Candidate compatibility/cache/debugger checks precede stopping an active session.
Persistence/restart failures remain explicit; there is no blanket rollback claim.

Remove/probe and later environment/DB/HTTPS families remain outside this slice.

Focused independent QA inspected the runtime additions and found no demonstrated
acceptance blocker. It verified descriptor/target binding, pre-stop candidate
checks, preserved configuration and the existing authority/replay/cancellation
boundaries in source. QA ran no duplicate tests or native launches.

Verification: five workspace tests and scoped clippy passed. Client checks passed
for scoped projection/CLI mapping/authority/no-install effect (one integration),
independent default/service/runtime/both MCP surfaces (5/9/8/12 tools; one
integration), and runtime metadata/parser (one case). Scoped CLI clippy passed.
The manager policy case passed, covering exact preserved fields, required scopes,
cross-project/family denials, missing cache and unapproved install without network.

One actual macOS ARM64 cached native flow passed in 36.08 seconds: verified
receipt-matching PHP8.4.27/8.5.12 CLI/FPM copies, actual HTTP PHP-version change,
invalid-candidate refusal preserving the live PID, original-key replay preserving
the selected PID, PM-only cached install without pin change, and the other
project's unchanged PID/HTTP. The fixture stopped and joined cleanly. No cache
source writes, downloads, native credential-store access or global config change.
This proves cached install reuse and actual PHP selection; fresh network install,
native GUI and Windows runtime execution are not established by this flow.
Subsequent4132560 CI passed both platforms, including actual Windows runtime
fixture5 and manager policy4. This adds backend/fixture execution evidence;
Windows cached PHP HTTP/native GUI remain unverified.

## Environment/YAML slice

The agreed slice reuses EnvironmentPlan, EnvironmentApply and YamlSave plus the
existing manifest planner/setup/save implementation. Observer preview exposes
allowlisted metadata and stable blocker codes. Apply requires ProjectManage,
SensitiveOperation and ExecutionTest; YAML save requires ProjectManage and
SensitiveOperation. Both write paths require native exact-plan approval.
Apply retains the existing manifest setup effects, including declared downloads,
dependencies, project-code execution and owned database/credential changes.
Completed stages can remain after a later failure/cancellation; the guarded
boundary does not promise a full rollback.

The scoped document-state helper returns only project/revision/existence and an
expiring random version token. Its bounded private mapping binds current document
SHA to grant/client/root/revision; the canonical expected field carries the opaque
token, and None means create-only. Invalid existing YAML can still yield this
metadata so a reviewed replacement can repair it. Core hashes and raw YAML/env
content are not external preview fields.

The external inline limit is 16 KiB UTF-8, enforced as bytes and rejected on
overflow; the generic encoded-frame limit also applies. Accepted YamlText excludes
control characters other than CR/LF/TAB, bounding JSON expansion. Native core's
existing 64 KiB limit and project-owned configuration history retain their native
semantics. External save discards the core raw-document return before response/
cache projection. YAML idempotency uses an opaque durable marker and volatile
body comparison, with truthful unverifiable status and original-key lookup after
eviction/restart. CLI credentials use a protected file and canonical payload uses
bounded zeroizing stdin; there is no payload/token argument or new upload codec.

Independent QA source review found no demonstrated acceptance blocker in the
opaque-version, private-plan, journal/replay, projection, framing or callback
boundaries. QA performed no tests, edits or native launches.

Five guarded environment tests and one real owned child-process polling test
passed. A manager policy integration passed in 0.27 seconds, including invalid
YAML repair metadata, version/authority checks and sensitive journal privacy.
Two client integrations plus one parser/auth-channel case passed: protected
credential file/stdin, UTF-8 limit, masked output and independent MCP surfaces
(default5/environment8/all15) without duplicate writes/downloads/startup.

One actual isolated macOS ARM64 cached save/apply flow passed in 24.25 seconds:
native-approved YAML save and masked preview, manager-injected .env secret and
generated APP_KEY, successful existing setup with cached packages, original-key
lookup/replay, and manager reopen returning IdempotencyUnverifiable when the RAM
verifier was gone while preserving receipt lookup. Owned shutdown completed.
MemoryStore was used; no OS credential service, permission change, runtime
download or native DB/tool startup is established by this test.
