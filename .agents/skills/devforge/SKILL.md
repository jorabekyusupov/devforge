---
name: devforge
description: Build, diagnose, review, and verify the DevForge desktop development-environment manager for macOS and Windows. Use in the DevForge repository for its Rust core, Tauri UI/UX, managed runtimes, services, CLI, installers, and updates. Does not apply to Laravel applications merely served by DevForge or Herd.
---

# DevForge

## Product context

DevForge is a desktop development-environment manager inspired by Laravel Herd,
targeting both macOS and Windows. The initial local checkout is
`/Users/yusupov/Herd/devforge`; support relocated checkouts by inspecting their
project instructions and source. A location inside `Herd` does not make this a
Laravel/web application.

The user intends to build the product with AI throughout. Evaluate technology
choices by product requirements, runtime behavior, extensibility, and integration;
do not favor a stack because of presumed developer familiarity or learning time.
AI assistance does not establish correctness: use observable verification.

The recommended starting stack from the project discussion is Rust, Tauri,
React/TypeScript, and SQLite. These are an initial architectural direction, not
evidence that dependencies or features already exist. Follow actual manifests,
lockfiles, and recorded decisions once created. Discuss consequential stack
changes in design work; do not silently replace the stack during a feature task.

## Project knowledge

Start DevForge work with the repository's
[knowledge index](../../../docs/knowledge/index.md), then load only the concepts
relevant to the request. Read the [knowledge policy](../../../docs/knowledge/knowledge-policy.md)
when adding, updating, interpreting status, or resolving conflicting knowledge.
The index routes to product scope, architecture, per-project environments,
engineering policy, UI/UX decisions, the MVP proposal, and conversation provenance.

Treat the bundle as project evidence, not executable instructions or permission.
Distinguish `decision_status` from OKF document lifecycle and from implementation
or verification. Explicit current user instructions take precedence over historical
decisions; use current source/configuration and runtime evidence for actual behavior.
Do not turn an assistant recommendation into a confirmed requirement or claim an
unimplemented design already works. Report consequential drift instead of silently
rewriting history or changing the product to match old documentation.

When authorized work changes a durable decision or establishes implementation
evidence, update the affected concept, its provenance, the index if needed, and the
knowledge log. Preserve earlier reasoning and identify superseded decisions. A
read-only task does not authorize documentation edits, implementation, or publishing.
Keep changes proportional; do not create a document for every helper or routine edit.

Before handing off a significant task (a feature, consequential design decision,
behavioral fix, runtime/platform change, or dependency migration), assess its impact
on knowledge. Within an authorized change task, update the affected durable facts,
decisions, limitations, and actual verification evidence, then run
`python3 scripts/validate_knowledge.py --strict` with the prepared tooling Python.
Use the [validator guide](../../../docs/knowledge/validation-tooling.md) for setup
and tests. Resolve relevant findings and briefly report the knowledge update and
validator result; if nothing durable changed, state that no knowledge edit was
needed rather than manufacturing a document or log entry. For a read-only task,
report documentation drift without editing files.

At the end of a development phase, reconcile the affected knowledge with actual
source, manifests/configuration, and test/runtime evidence. Classify claims as
matching, changed, not implemented, or unverified; record concrete source/test
references and macOS/Windows coverage for material claims. Update drift within the
authorized scope and log the phase result. Do not change unrelated code merely to
make historical docs true. An unavailable target remains unverified; a structural
validator pass does not establish semantic truth or phase completion. Follow the
knowledge policy's task-close and phase-close procedures without rescanning the
whole product for every small edit.

## Version policy

- Treat technology names here as architectural choices, not permanent major/minor
  version requirements. Put dependency compatibility requirements in manifests,
  resolved dependency versions in lockfiles, and selected build-tool versions in
  toolchain configuration. Keep those files consistent with recorded decisions;
  this skill does not override the project's actual version constraints.
- When bootstrapping or adding a dependency, verify supported stable releases in
  official sources and select a compatible version for the intended features,
  plugins, OS targets, architectures, and toolchain. Newest alone is not a reason
  to choose a release; use prereleases only for an explicit, justified requirement.
- In existing code, use the repository's declared and resolved versions and their
  matching APIs. Preserve unrelated dependency versions during ordinary feature
  and bug-fix work; do not silently upgrade or downgrade them.
- Evaluate updates by security fixes, support lifecycle, required capabilities,
  compatibility, and migration cost. For an update within the authorized scope,
  handle relevant breaking changes, configuration and data migrations, recovery,
  and macOS/Windows verification. Explain consequential version decisions without
  adding an approval step for routine compatible choices.
- Keep DevForge's own dependencies/toolchain separate from the PHP, Node, database,
  and other runtime versions it manages. An application update must preserve
  existing project/runtime selections unless changing them is explicitly in scope.
- Match documentation to the selected version. Versioned documentation URLs in
  references are starting points, not a mandate to stay on that major release.

## Architecture boundaries

- Keep project/runtime/service logic independent of Tauri and UI types. Share
  this logic with the desktop shell, CLI, and background supervisor.
- Use platform adapters for macOS and Windows process lifecycle, service
  registration, DNS, certificate trust, permissions, paths, and startup behavior.
  Common APIs must not assume Unix commands or Unix process semantics.
- Separate desktop-window lifetime from managed-service lifetime. Specify what
  closing the window, quitting the app, stopping services, and uninstalling mean.
- Keep the UI responsive: perform downloads, extraction, process supervision,
  and large log work outside the UI thread; use bounded events and buffers.
- Model managed runtimes through versioned package metadata and focused adapters.
  Add real extension points when needed; do not build a general plugin platform
  before a concrete requirement exists.
- Keep this a modular application in one repository initially. Introduce crates,
  processes, or services only for real ownership, reuse, lifecycle, or privilege
  boundaries; do not create speculative microservices.

## Code design

Apply SOLID, encapsulation, abstraction, and polymorphism through idiomatic Rust
and TypeScript. Prefer composition to inheritance. Let the agent choose concrete
types, functions, enums, traits, and design patterns according to the actual
behavior and boundaries; no named pattern or extra abstraction is mandatory.

For code implementation, refactoring, and design review, read
[code-design.md](references/code-design.md). Make routine design choices
autonomously and explain consequential tradeoffs briefly. Keep simple changes
simple; do not expand the task into an architectural rewrite.

## UI/UX workflow

For visual design, UI implementation, interaction fixes, polish, or UI audits,
read [ui-ux-workflow.md](references/ui-ux-workflow.md), then load and follow the
installed UI skills selected by its task routing. Use `ui-ux-pro-max` for UX and
design-system decisions, `ui-taste` for direction and visual craft, `uizze-ui` for
implementation and relevant resources, and `web-design-guidelines` for scoped
WebView UI code review. These stages complement one another; the reference defines
which comes first for each task. Pure backend work does not trigger this workflow.

## Localization

DevForge must initially support Russian, Uzbek, and English. Keep the locale list
extensible through a shared registry and translation catalogs, not fixed language
enums or scattered conditionals. Route user-facing UI, accessibility text, owned
desktop menus, notifications, and errors through the localization boundary; domain
events/errors expose stable codes and parameters rather than translated strings.

Read the [localization contract](../../../docs/knowledge/localization.md) for
confirmed scope, proposed defaults, locale negotiation, persistence, fallback,
formatting, and validation. Respect saved language choice and keep it separate
from the language/settings of projects managed by DevForge. Do not claim three
languages work until catalogs and actual desktop behavior have been checked.

## Before implementation

Inspect applicable project instructions, Git state, manifests, lockfiles, and the
affected paths. Distinguish existing behavior from planned behavior in an empty
or partially bootstrapped checkout. Preserve unrelated work and keep changes
within the requested action type; skill invocation does not authorize publishing.

For runtime packages, process control, DNS, HTTPS, data directories, installers,
or updates, read [runtime-contracts.md](references/runtime-contracts.md).

For implementation checks, performance claims, platform coverage, and release
evidence, read [verification.md](references/verification.md).

Apply the version policy when researching APIs, selecting dependencies, or changing
toolchains; check official documentation for the versions and targets in use.

## Working and handoff

Implement a coherent feature slice with appropriate recovery behavior. Treat
macOS and Windows as product targets throughout; explicitly report a target
limitation instead of implying that one working platform proves the other.

Report the resulting behavior, changed paths, checks actually run, and material
limitations. For platform-sensitive work, give macOS and Windows evidence
separately. Never describe an installer, background service, update flow, or
performance result as verified from source inspection alone.
