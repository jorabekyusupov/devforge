# M13.4 reviewed service lifecycle

This first M13.4 slice exposes only existing project start, stop and restart.
Runtime installation/selection, environment/YAML, database and HTTPS mutations
remain later slices. The default MCP surface remains read-only.

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
Windows execution of this new service slice remains unverified locally.
