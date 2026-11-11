# Contextual code design

Use for implementation, refactoring, and code review. SOLID and OOP principles
guide decisions; patterns are optional tools selected by the agent. Apply them
where they clarify ownership, behavior, contracts, or actual variation.

## Choosing the appropriate structure

Inspect the affected behavior and existing design first. Identify invariants,
side effects, ownership, and what actually varies. Choose the smallest structure
that preserves those contracts and accommodates requirements already in scope.

A function, concrete struct, or enum is often sufficient. Introduce an abstraction
when it protects a real domain/OS/storage boundary, expresses meaningful alternate
behavior, or materially enables testing important side effects. A boundary can be
worth protecting with one current implementation; a trait is not required for
every type. Reuse an existing abstraction only when its contract fits.

Make routine design choices autonomously without a pattern-selection approval
step. For consequential choices, briefly explain the concrete problem, chosen
approach, and material cost. Record a durable architectural decision only when
its impact warrants it; do not create an ADR for every helper or pattern.

## SOLID in DevForge

| Principle | Practical application and limit |
| --- | --- |
| Single responsibility | Group behavior that changes for the same reason. Separate service-state policy from OS process calls and UI presentation when these are distinct concerns. Do not split every operation into its own class or crate. |
| Open/closed | Put real variation, such as platform integration or runtime providers, behind focused extension points. Keep a closed set of variants as an enum when exhaustive handling is useful; new variants may appropriately require edits. |
| Liskov substitution | Implementations of a shared contract preserve its promised errors, readiness, ownership, cancellation, and stop semantics. Do not report success through a no-op implementation; model unsupported capabilities explicitly. |
| Interface segregation | Expose only the capabilities consumers need. A status reader should not require package installation or administrator methods. Avoid one trait that every service must partially implement. |
| Dependency inversion | Keep domain policy independent of Tauri, OS APIs, and SQLite details. Supply dependencies at the composition boundary using constructors, parameters, or focused traits; a DI container or service locator is not required. |

## OOP expressed idiomatically

- Encapsulation: hide mutable state that enforces an invariant; expose meaningful
  operations rather than public mutation or mechanical getters/setters everywhere.
- Abstraction: give domain concepts and external boundaries explicit contracts.
  Keep process handles, SQL records, and Tauri payloads out of unrelated domain APIs.
- Polymorphism: choose concrete types, generics, enums, or `dyn Trait` according to
  whether variants are closed, selected at compile time, or selected at runtime.
  Account for actual ownership and performance needs; do not default to boxing.
- Reuse and inheritance: prefer composition and delegation. Rust has no classical
  struct inheritance; do not emulate Java/PHP base-class hierarchies in Rust.
- State and errors: use types and enums to express valid states and outcomes when
  useful. Keep failures meaningful across boundaries; avoid scattered flags that
  admit contradictory service states or errors converted to success.
- Frontend: use React component and hook composition plus TypeScript types. Apply
  the same responsibility and dependency boundaries without forcing class-based
  OOP or backend repository/service layers into presentation code.

## Pattern candidates, selected by need

This is a non-exhaustive decision aid, not a required implementation checklist.

| Situation | Candidate | Simpler option or caution |
| --- | --- | --- |
| macOS and Windows expose different system APIs | Adapter / ports and adapters | Protect the specific OS boundary; do not wrap every library call. |
| The same operation has interchangeable runtime policies | Strategy | A function or enum may suffice for a small, closed set. |
| A service has lifecycle transitions and state-dependent operations | State machine / State / typestate | Prefer an enum and explicit transitions when adequate; typestate is useful only when compile-time constraints fit the lifecycle. |
| Operations need explicit dispatch, scheduling, or audit identity | Command | Direct calls suffice otherwise. Cancellation or undo requires real semantics; a command type does not make a destructive operation reversible. |
| Status or logs must reach independent consumers | Observer / publish-subscribe | Bound queues, define delivery needs, and clean up subscribers; do not hide important control flow in a global event bus. |
| Construction varies by provider or requires validated combinations | Factory / Builder | Prefer constructors for simple creation; avoid a hierarchy of factories for a single fixed case. |
| Persistence must be isolated from domain behavior | Repository | Design around domain operations; avoid a generic CRUD abstraction for every table. |
| Behavior such as metrics must wrap the same contract | Decorator / middleware | Ordinary function composition may suffice. Keep retries explicit where repetition could change data or service state. |

Patterns may be combined when each solves a distinct problem. Do not introduce
Singletons, registries, managers, or global mutable state as a default shortcut
for dependency wiring. Do not replace a clear local branch with a general plugin
system for hypothetical future requirements.

## Checking the decision

Check observable contracts rather than pattern names or class counts. For changed
boundaries, verify relevant implementations agree on behavior. For changed state
logic, cover valid transitions and important rejected transitions. Keep tests
focused on actual regression risks using the project's verification guidance.

During review, remove indirection that adds no useful contract. Preserve deliberate
platform differences and avoid unrelated rewrites solely to achieve abstract
SOLID purity or to demonstrate a named design pattern.

## Rust references

- OOP characteristics: https://doc.rust-lang.org/book/ch18-01-what-is-oo.html
- Pattern tradeoffs in Rust: https://doc.rust-lang.org/book/ch18-03-oo-design-patterns.html
