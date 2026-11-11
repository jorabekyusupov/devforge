# DevForge

This repository contains a desktop development-environment manager for macOS and
Windows. Its location under `Herd` does not make it a Laravel web application.

For DevForge product, implementation, diagnosis, review, and verification work,
read [.agents/skills/devforge/SKILL.md](.agents/skills/devforge/SKILL.md) and only the
supporting references relevant to the task.

Start project work with [docs/knowledge/index.md](docs/knowledge/index.md), then read
only the knowledge relevant to the task. The skill defines how to use and maintain
this OKF bundle. Distinguish confirmed requirements, working directions, proposals,
and implemented behavior; a knowledge record is not runtime verification.
Follow the skill's knowledge completion gate for significant tasks and its
code/evidence reconciliation at the end of each development phase.

The M1 foundation implements Rust + Tauri + React/TypeScript + SQLite.
Read the knowledge index for current evidence and unverified platform gates.
Preserve recorded decisions and explicit user changes to scope or technology.
Follow the skill's version policy; exact dependency
and toolchain versions belong in project configuration rather than these instructions.

For UI/UX work, follow the skill's UI/UX routing rules. Use the installed
`ui-ux-pro-max`, `ui-taste`, `uizze-ui`, and `web-design-guidelines` skills at their
applicable stages; choose the workflow by task rather than running all stages for
every change. Apply them to DevForge's desktop/WebView context.

DevForge must initially support Russian, Uzbek, and English and allow additional
locales through a shared registry and translation catalogs. Follow the skill's
localization policy; do not embed user-facing strings or language branches in
product/domain logic.

Both macOS and Windows are product targets. Give separate evidence for
platform-sensitive behavior, and identify any target that could not be tested.
Do not alter other Herd projects or system-wide development configuration merely
because they are adjacent to this repository.
