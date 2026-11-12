# M5 UI review — 2026-11-12

Scope: `App.tsx`, `WorkspacePage.tsx`, `workspace.ts`, `bridge.ts`, operational
CSS and the native acceptance harness. Current Web Interface Guidelines fetched
from https://raw.githubusercontent.com/vercel-labs/web-interface-guidelines/main/command.md.
Review used the installed ui-ux-pro-max, ui-taste, uizze-ui and web-design-guidelines
workflow. Existing system fonts/palette/theme preference were preserved; no paid
Uizze resources were retrieved.

## Findings resolved

- `apps/desktop/src/pages/WorkspacePage.tsx:45` — separate search state per page;
  Runtimes search no longer hides Projects after navigation. Regression test added.
- `apps/desktop/src/pages/WorkspacePage.tsx:845` — focus the actual input after
  `showModal`; Escape and focus restoration covered. Inline request errors stay
  inside the active modal and preserve form input.
- `apps/desktop/src/styles.css:762` — removed background-only color transitions
  that briefly mixed dark text tokens with a light selected-nav background during
  theme changes. Native axe caught 1.2:1 transient contrast before the fix.
- Import path label is separate from its Browse button, giving the input one
  unambiguous accessible name. Runtime/download pending and confirmed-ready states
  are separate; duplicate requests are guarded and install cancellation is real.

## Verified scope

Native macOS WKWebView, real IPC (no invoke mocks), private fixture SecretStore:
WCAG2 A/AA and WCAG2.1 AA axe rules returned zero violations in nine cases:
import error dialog; Runtimes light; Services light; Projects light; minimum
window Projects in Russian, Uzbek and English; Projects dark; Services dark.
This is scoped automated evidence, not a complete screen-reader certification.

UI tests cover error retry/input preservation, dialog Escape/focus restoration,
raw log filtering in Russian, install cancellation and independent page filters.
The native loop covers real project/runtime/database controls, version switching,
close-to-tray, normal Quit/reopen and app-crash recovery with Vite enabled.

The 820×620 native window has 820×588 content on this macOS host; vertical scrolling
keeps controls reachable and no document horizontal overflow was found. The
reference comparison uses an observed1487×1058 content viewport at2× backing scale.

## Visual comparison

Target: `design-system/devforge/reference/projects-target.png`,1487×1058, compared
with `macos-projects-light.png` at the same content size (backing bitmap2×).
The sidebar, master/detail grouping, typography and teal control hierarchy match
the incumbent system. Live fixture values, generated project IDs, runtime
versions and real logs replace illustrative target copy. Domain/HTTPS settings
use a disclosure; VS Code/terminal/folder actions implement the M5 brief.
No pixel-perfect claim is made. Russian/Uzbek minimum-size and dark captures were
also inspected; long paths truncate or wrap without hiding the actual actions.

## Deliberate boundaries

- Native OS chrome, file-picker buttons and external app UI follow OS conventions;
  this DOM audit does not certify those surfaces.
- Project selection/navigation use hash routes; transient desktop search filters
  are retained per view rather than exposed as browser bookmarks.
- No invented timestamps/severity: logs use actual process, stream and output.
- Optional editor/terminal/browser launch adapters depend on installed OS handlers;
  native process/backend evidence is distinguished from external-window inspection.
- Windows final-source native/accessibility workflow passed the same nine cases;
  captures are preserved separately. Hosted Mac large-viewport cap is recorded,
  while local Mac/Windows achieved the target content dimensions. Minimum OS/Intel/
  signing/native credential UI remain M6 gates.


## Windows capture inspection

Artifacts from native run43150152615 were downloaded and inspected. The large
Projects capture includes the actual transient Working/disabled state from a
snapshot refresh; it is retained as busy-state evidence, not a ready-state target
comparison. The Russian minimum-window and dark Services captures show stable,
legible controls and preserved raw output. Local Mac provides the inspected
1487×1058 ready-state reference comparison. No Windows large ready-state
pixel-fidelity claim is made. The full functional and nine-case axe assertions
passed independently of this screenshot-state limitation.
