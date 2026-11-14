# Minimal native agent connections UI

M13.8a moves ahead under the user-confirmed delivery order. It exposes existing
native owner APIs through the current Settings visual system: explicit connect,
project/four-scope/TTL grant creation, protected config delivery, list/revoke,
pending exact-plan review, separate machine confirmation and agent-operation
status/cancel/original-key reconciliation. Full audit/report/content-right UI and
HTTP transport remain later slices.

## Native authority and delivery

AgentApi exchanges safe typed metadata through the Tauri bridge. Blocking work
runs outside the UI thread, and the SDK lock is released between calls/pages.
Native session/bearer credentials never enter renderer DTOs, snippets or logs.
State uses the connected SDK/cache; cold credential-provider work requires the
explicit native connection action.

Issue writes only fresh private leaves using the native selected data root:
Unix0700/0600/current owner, Windows TokenUser/private DACL. Existing paths are
not adopted, repaired or overwritten. Client configuration references the
credential file and fixed verified CLI path, with no PATH fallback or global
client-settings overwrite. Export failure must confirm bounded grant revocation
or report compensation as unconfirmed; it cannot claim an inactive grant after
discarding Busy or another revoke error.

## Review and operations

Pending review comes from the current native session only. It includes project
name/root, tool/revision/expiry/state, required scopes, effects and concrete typed
targets. Native semantic details show candidate runtime/domain/driver/setup
changes and key-name counts without raw YAML/values/secret references/core hashes.
Exact machine details include actual fixed steps/targets, hosts deltas, public CA
fingerprint and expiry. The list is advisory; approval revalidates authoritative
state, including fields that do not increment project revision.

Generic plan approval is separate from one-shot domain machine confirmation.
Renderer consent/flags are not authority. Missing, stale, expired or incomplete
exact machine summaries fail closed. This UI does not itself apply the plan or
launch an OS helper.

Native operation lookup/status/cancel resolve client/project through persisted
grant ID, rather than accepting a renderer client namespace. Native owner control
remains available for a revoked external grant. Reconciliation uses the original
key and reports point-in-time absence, never a guessed replay or a new key.

## Interface and verification boundary

The approved1536×1024 Settings reference/MASTER supplies existing tokens, system
font, Lucide and system/light/dark behavior. The new panel uses semantic forms,
native dialogs, visible state text, keyboard/escape and focus restoration after
pending work/DOM refresh. en/ru/uz and minimum820×620 remain applicable. Deferred
report components/namespaces are not mounted by this slice.

Backend3, semantic-summary2, bridge2 and compensation1 focused cases passed;
frontend11 initial and3 affected/new plan/focus cases passed, with earlier6 App
cases reused. Scoped Rust/TypeScript/locales checks and fresh source guidelines
review passed. QA closed focus restoration and failed-export compensation gaps.
UI now waits for pending/DOM refresh before stable-heading focus; compensation
uses bounded Busy retry plus confirmed revoked metadata or explicit unconfirmed.

Actual macOS owned WebView/real IPC acceptance passed: DOM grant/config,
actual MCP A-only/B-denial, public Stop plan with DOM approval, completed Stop,
status/original-key reconciliation, terminal Cancel disabled, revoke invalidating
the real credential, and clean Quit. Passed mutations were not repeated after
harness fixes: the same fixture continued only its remaining steps.

The integrator inspected four actual captures: wide review and EN/light1536×1024,
RU/dark820×620, UZ/light820×620 panels, DPR2. Three scoped axe cases had zero
violations. Modal Tab/Escape/focus proof passed; synthetic matrix focus stayed
inside controls but did not activate focus-visible, so native OS focus-ring
behavior is unverified. No services/downloads/native Store/security permission
changes occurred. Dirty debug/e2e source and binary hashes are in the evidence;
this is not a clean signed-release or Windows UI attestation.

Harness-only failures were Python file_digest compatibility, debug devUrl without
custom-protocol, and hidden/reconciliation/keyboard starting-point adapters. No
invalid capture was counted pass. Verified owned roots/windows/managers alone
were cleaned. Existing Settings target was inspected as incumbent authority;
no new redesign/image/provider or optional polish was introduced. Actual
source/runtime outcomes and remaining gates are in execution status.
