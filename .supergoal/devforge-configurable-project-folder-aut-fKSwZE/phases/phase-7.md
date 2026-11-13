# Phase 7 — Polish & Harden

SUPERGOAL_PHASE_START
Phase: 7 of 7 — Polish & Harden
Task: Oxirgi source barcha kontrakt, foydalanish va platforma mezonlariga mosligini yakuniy tekshirish.
Depends on phases: 1, 2, 3, 4, 5, 6
Status: PLANNED — implementation has not started.

## Work

Bounded UX/copy/a11y pass, input/IPC/diff review, isolated rename/permission/storm regression, release-mode performance measurement va adjacent import/create/prepare/worktree/service smoke. Documentation/code/evidence reconciliation; faqat affected knowledge. Old unrelated tests failures baseline bilan alohida ko‘rsatiladi, yangi regression yopiladi. Audit source native tekshirilgan hashga teng bo‘lishini tekshiradi; keyingi code o‘zgarsa tegishli native gate qayta bajariladi.

## Deliverables

- affected implementation/tests from phases 1-6
- docs/knowledge/project-discovery.md
- docs/knowledge/references/conversation-decisions.md
- docs/knowledge/index.md
- docs/knowledge/log.md
- docs/evidence/project-discovery/

## Acceptance criteria

- P7.1: Final workspace fmt/clippy/tests va frontend typecheck/test/build/locale checks exit 0; relevant baseline failures bo‘lsa ular aniq dalil bilan ochiq va complete claim berilmaydi.
- P7.2: 100 local sibling candidate fixture cold scan 3s ichida, UI confirmation native event’dan 5s ichida, fallback 35s ichida; release build/hardware/workload bilan o‘lchanadi yoki asosli revised budget review’da qayd etiladi.
- P7.3: Normal local idle’da periodic root reconciliation 30s dan tez emas, unchanged candidates reimport qilinmaydi, child dependency subtree o‘qilmaydi; resource trend 50 root switch/10k event test’da bound ichida.
- P7.4: Minimum supported desktop size va 3 locale x light/dark (system representative) states screenshot/interaction bilan tekshiriladi; scoped axe’da yangi finding 0, native picker focus alohida tekshiriladi.
- P7.5: Existing manual import/Create/Prepare, custom runtime/domain, worktree cleanup, service binding va persistent DB ma’lumotlari uchun tegishli regression stsenariylari o‘tadi.
- P7.6: Discovery avtomatik process/download/DB/.env/system DNS/CA mutation boshlamasligi sentinel/owned-state checks bilan tasdiqlanadi; Start/Prepare mavjud explicit oqimida qoladi.
- P7.7: Yakuniy knowledge’da confirmed requirement, proposed/changed design, actual implementation va har platformaning verified/unverified chegarasi ajratiladi; strict validator 0 error/0 warning.
- P7.8: Final diff’da task debug prints, session TODO/FIXME, dead imports yoki aloqasiz source o‘zgarishi yo‘q; publish/deploy/commit/push userning yangi ko‘rsatmasisiz bajarilmaydi.
- P7.9: Original ROADMAP criteria yakuniy source/evidence bilan qayta audit qilinadi; native yoki system-dialog gate ochiq bo‘lsa feature fully verified deb yopilmaydi.

## Mandatory commands

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace`
- `npm run typecheck`
- `npm test`
- `npm run check:locales`
- `node --test scripts/check_locales.test.mjs`
- `npm run build`
- `npm run tauri -- build --debug --no-bundle`
- `python scripts/validate_knowledge.py --strict (prepared tooling interpreter)`

## Evidence required

- Final check summaries/exit codes, native matrix final-source table, measured budgets, scoped diff/a11y review, knowledge validation va open gates; product completion faqat actual evidence bilan.
- Print each criterion as pass/fail/not-verified with source/test evidence; counts and exit codes only summarize real runs.
- Before close, reconcile affected knowledge with actual code and current evidence. Do not mark planned behavior implemented.
- Run strict knowledge validator when affected durable knowledge is updated, using prepared tooling.
- During implementation print SUPERGOAL_PHASE_VERIFY, then SUPERGOAL_PHASE_DONE only when all required criteria pass. Planning creates no such completion markers.

## Scope and failure rules

- User requested planning first. This document is a work specification, not permission to implement or publish.
- No global memory edits, unrequested dependency upgrades, unrelated project/system changes, or copying Herd proprietary code/assets.
- Future execution: investigate failure, focused retry, then focused fix; preserve original criteria. Missing target/access remains an honest open gate.
