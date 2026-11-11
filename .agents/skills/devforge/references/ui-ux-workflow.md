# UI/UX skill routing for DevForge

Read for new screens, UI implementation, visual changes, interaction defects,
design-system work, polish, and UI audits. This document routes to the user's
installed skills; it does not replace their instructions or copy their manuals.
Backend-only tasks and edits to these instructions do not require generating UI
images, design systems, or conducting a product UI audit.

## Locate and load the source skills

The current installation paths are:

| Skill | Entrypoint | Primary responsibility |
| --- | --- | --- |
| ui-ux-pro-max | [SKILL.md](/Users/yusupov/.codex/skills/ui-ux-pro-max/SKILL.md) | UX decisions, design-system search, accessibility, and stack guidance |
| ui-taste | [SKILL.md](/Users/yusupov/.agents/skills/ui-taste/SKILL.md) | Product-specific visual direction, task playbooks, image target, and craft |
| uizze-ui | [SKILL.md](/Users/yusupov/.agents/skills/uizze-ui/SKILL.md) | Implementation/verification workflow and relevant real references/materials |
| web-design-guidelines | [SKILL.md](/Users/yusupov/.agents/skills/web-design-guidelines/SKILL.md) | Source-level Web Interface Guidelines review |

Load the actual entrypoint before applying a selected skill. Resolve its supporting
files relative to that skill's directory, not this repository. On another machine,
find the same named installed skills through the host catalog or local skill
directories rather than assuming these user-specific paths exist. If a skill is
missing, search for its installed location; report any remaining gap and continue
independent work with the available guidance without pretending it was applied.

## Begin with product evidence

Read the relevant request, UI source, assets, components, tokens, data contracts,
and existing PRODUCT.md, DESIGN.md, or design-system documents when present.
Identify the primary user task and affected states. Preserve an established visual
system unless redesign is in scope. Confirm the implementation stack from actual
project files and explicit project decisions; a recommended stack is not proof
that its framework or styling library has already been installed.

The task determines the starting skill and subsequent stages below. Use all
applicable stages, not all four full workflows indiscriminately. Read only the
relevant supporting playbooks and searches. Reuse results and visual evidence
across skills instead of generating competing systems or duplicate targets.

## Which skill comes first

| Task | Required sequence within the relevant scope |
| --- | --- |
| First visual system, new screen, or substantial redesign | `ui-ux-pro-max` design-system search -> `ui-taste` direction with `new-work` and `operate` -> `uizze-ui` implementation -> `web-design-guidelines` code review -> visual/interaction verification |
| Implement a supplied or already settled visual target | Inspect the target and use `ui-taste` fidelity/craft guidance -> `ui-ux-pro-max` focused UX/stack guidance -> `uizze-ui` implementation -> `web-design-guidelines` review -> target comparison |
| Polish or simplify an existing surface | `ui-taste` `polish` or `distill`, with `operate` as needed -> `ui-ux-pro-max` searches for the actual concern -> `uizze-ui` scoped changes -> `web-design-guidelines` review -> reinspect affected states |
| Specific keyboard, accessibility, overflow, form, or interaction defect | `ui-ux-pro-max` semantic outcome search first, then relevant stack guidance -> `ui-taste` scoped-fix/craft guidance -> `uizze-ui` implementation -> `web-design-guidelines` affected-file review -> reproduce and verify the fix |
| Explicit UI code/accessibility compliance review | `web-design-guidelines` first with explicit file scope -> `ui-ux-pro-max` targeted interpretation where needed -> `ui-taste` runtime audit for visible/interaction evidence; keep read-only unless fixes were requested |
| Visual/UX critique of an existing flow | `ui-taste` `audit` and rendered evidence first -> `ui-ux-pro-max` relevant UX guidance -> `web-design-guidelines` for the inspected UI source; do not implement changes without a fix request |

`uizze-ui` resource discovery may happen before direction/target selection when
references or assets would materially inform it. Its local implementation workflow
applies even without paid access. An explicit request naming a skill makes that
skill applicable; preserve the user's requested order or constraints if they differ.

## Design decisions: ui-ux-pro-max

- For a new screen/project or system-wide direction, use its `--design-system`
  mode. For a settled target or local defect, use a focused `--domain` query and
  detected `--stack` guidance instead of inventing a replacement visual system.
- Use its search script by the actual full installation path. Keep queries short,
  inspect returned results for product/platform fit, and follow its bounded retry
  policy. Results are recommendations, not authority over project requirements.
- Read an existing `design-system/devforge/MASTER.md` and applicable page override
  when present. Persist only verified output with `--output-dir` set to the actual
  project root. Preserve existing decisions and the source skill's explicit
  authorization requirement for `--force`; do not regenerate the master per task.
- Prioritize accessibility and task completion before decoration. Use its desktop
  guidance in `references/quick-reference.md`. Its mobile-only `pro-rules.md`
  checklist is not a requirement to add phone layouts or native mobile features.

## Direction and visual target: ui-taste

Select its action playbook rather than loading the entire archive: `new-work` for
new direction, `operate` for DevForge's working screens, `polish` for refinement,
`distill` for simplification, and `audit` for critique. Use `craft` after settling
direction and during implementation/inspection. Overdrive is opt-in.

Follow `references/uizze-image-to-ui.md`. For new visual work or an authorized
redesign, use an available authorized image-generation tool before UI coding.
Inspect the generated screen and save one stable target with its actual dimensions.
A supplied/approved target takes precedence; do not regenerate it or ask for a
second approval of a settled direction. Use that same target for `uizze-ui`.

For scoped refinement, an actual incumbent screenshot plus the requested changes
can define the baseline. Generate a constrained proposal only when needed. Small
technical fixes and read-only audits require no new concept image. If generation
is unavailable, disclose the limitation and use existing evidence/conventions;
do not install another image provider or claim an image was generated.

## Implementation and resources: uizze-ui

Before UI implementation, read its `references/workflow.md`. Implement real
semantic controls, project tokens, and the reachable loading/empty/error/success
states while preserving actual data and Rust/Tauri contracts. An image or reference
must not introduce fictional services, actions, metrics, or backend behavior.

Use its references/materials path when actual examples, fonts, icons, or a selected
design pack would help. Before retrieval, also follow ui-taste's reference policy.
Paid MCP access is optional: do not require signup for local UI work, invent tools,
or claim authenticated access from configuration presence. Respect user selection
before retrieving a chosen pack and the source skill's rule before replacing an
existing DESIGN.md. Preserve asset licensing and inspect actual returned images.

## Code audit: web-design-guidelines

Use after meaningful UI implementation/refinement and as the first stage for an
explicit compliance review. Pass the actual affected UI files/pattern as scope;
do not ask the user to enumerate files already identified by the task or change.
For an ambiguous standalone review, establish the file/screen scope first.

Fetch the current guidelines before each review from the source in its SKILL.md:
`https://raw.githubusercontent.com/vercel-labs/web-interface-guidelines/main/command.md`.
Check the scoped source against applicable rules and use the skill's current
`file:line` findings format. Do not cache a copied ruleset in this project. If the
source cannot be fetched, report the limitation and distinguish available local
checks from a completed review against the latest guidelines.

This review covers the HTML/CSS/React WebView layer. It does not validate Rust
permissions, native dialogs, tray integration, service behavior, or the entire
desktop accessibility experience. Check those through actual desktop evidence.

## DevForge desktop context and completion

- Design for operational work: project selection, version assignment, service
  status, useful logs, and recovery. Favor clear hierarchy and appropriate density
  over decorative cards, fake charts, and landing-page composition.
- Use semantic tokens and coherent components/icons. Preserve established theme
  preferences; avoid screen-specific visual drift or forced appearance changes.
- Follow the [localization contract](../../../../docs/knowledge/localization.md).
  Design for Russian, Uzbek, and English text expansion, localized errors and
  accessible names, and live language changes; use registry-backed catalogs rather
  than local per-screen translation branches. No UI image or library reference
  authorizes translating real project names, paths, or raw service output.
- Verify keyboard navigation, visible focus, dialog focus/escape behavior,
  meaningful accessible names, and status feedback beyond color alone. Distinguish
  pending commands from confirmed running/ready state and expose actionable errors.
- Check actual desktop window resizing, minimum supported window size, long paths,
  localization, scrolling/overflow, text scaling, and relevant supported themes.
  Apply macOS/Windows shortcut and window conventions. Do not apply iOS safe-area,
  mobile navigation, or touch-only layout rules as desktop requirements; size
  controls appropriately for supported inputs and accessibility requirements.
- Run and inspect the real Tauri screen on each target when available. Compare at
  the stable target's dimensions after assets load, exercise important controls,
  correct material issues in a bounded pass, and recapture after visual changes.
- Source review, browser preview, target image, and native desktop checks are
  different evidence. A successful build or one screenshot cannot establish visual
  fidelity, all interaction states, or both platforms. Report what was actually
  checked and remaining limitations without claiming an unverified pass.

Keep the handoff concise: changed behavior, source skills actually used, relevant
targets/references, checks, and material gaps. Routing does not authorize paid
resources, purchases, account changes, publishing, or unrelated redesigns.
