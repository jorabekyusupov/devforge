# M7 Inspector scoped UI audit

Scope: new InspectorPage, App routing, bridge/workspace types and Inspector CSS.
The existing DevForge visual system is retained. ui-ux-pro-max design-system
queries twice returned FAQ/landing composition; those were rejected. Ui Taste
operate/craft and Uizze local workflow were applied to the operational surface.
No paid reference connection/assets were used.

Generated target `design-system/devforge/reference/inspector-target.png`, actual
1511x1041 pixels, was inspected before UI code. Values in it are illustrative.
Actual product uses real backend records, semantic tables/buttons, project/type/
search controls, capture/pause, install/clear confirmation and context details.
The incumbent operational shell has a205-250px responsive sidebar. Its colors,
font and controls were reused; only Inspector keeps its sidebar sticky during
long-detail scrolling. Native platform select rendering differs from the image; compact system-select painting was kept.
No OS chrome is painted and no target is used as a background.

Fresh Web Interface Guidelines were reviewed. Fixed findings: native fallback
button background gave active-capture3:1 contrast, so M5 token controls were
reused; search padding specificity and duplicate native search decoration fixed;
metrics override the inherited dd width to prevent numeric wrapping; dialog has
an accessible name and returns focus; event lists page at50 rows rather than
rendering200 controls together. Search/type/project inputs have visible or
screen-reader labels, dump values render as text and timestamps/numbers use Intl.

The first Windows WebView2 run found `scrollable-region-focusable` on the detail
panel in the dump state. Source fb93c01 makes both the named detail region and
the nested JSON payload keyboard-focusable. The axe gate is unchanged; this was
a product accessibility fix, not a suppressed finding. The screenshot helper
also resets native window origin/main/sidebar scroll and waits for repaint.

Local real WKWebView suite passed: actual companion install B with A master PIDs
preserved, capture/filter/context/clear and cancel, Xdebug probe, ru/uz/en minimum
window, dark mode, history retained/capture paused after reopen. Seven axe cases
have zero WCAG2 A/AA and2.1 AA findings: integration confirmation, light Inspector,
dump view, three minimum-locale states and dark Inspector. This is not native
screen-reader/system-dialog or function-level debugger certification.

Raw large captures after a resize did not completely repaint the header/sidebar.
For an actual visual comparison, the same1511x1041 content viewport was captured
at a safe native window origin after resetting main/sidebar scroll and waiting
for paint. Inspected `captures/macos-inspector-light.png`: real request/query/log/
dump rows and slow/repeated hints, visible sidebar, readable context values and
bounded panels. Also inspected the minimum Uzbek and dark native captures. There
is no fresh pixel-perfect certification claim. Actual viewport/scale is in
local-desktop.json; hosted captures/results are recorded separately when complete.

Source: https://raw.githubusercontent.com/vercel-labs/web-interface-guidelines/main/command.md

Follow-up source review retained keyboard-native semantic controls and the OS
focus treatment for the new scroll regions. A focused ui-ux-pro-max UX query
covered visible/unobscured keyboard focus; current Web Interface Guidelines were
fetched again for this review. Project choice is deep-linkable; event filters,
search and pagination remain local view state rather than a sharable URL contract.
The audit is scoped WebView evidence, not a claim of universal guidelines or
native assistive-technology certification. After the functional source freeze,
the existing incomplete-capture message was clarified in three catalogs; keys and
placeholders did not change and catalog/regression checks passed.

Final native Windows x64 e4f4b97 passed backend/WebView2 controls and all seven
axe cases with zero findings. The checksum-verified archive reports match job
logs. Its light1511x1041, minimum Uzbek and dark captures were inspected; the
minimum outer820x620 window has a smaller actual content area recorded in the
JSON. Native selectors/scrollbars and stacked detail layout are retained.
`captures/windows-inspector-light.png` preserves the actual Windows surface.

Final native Intel Mac55b9d2f also passed backend, delayed-ACK regression, real
WKWebView flow and all seven axe cases. Its light1511x942, minimum Russian and
dark captures were inspected after verifying the archive digest and report
equality. The host capped the requested1041px height at942px; local target-height
evidence remains separate. `captures/intel-inspector-light.png` preserves it.
