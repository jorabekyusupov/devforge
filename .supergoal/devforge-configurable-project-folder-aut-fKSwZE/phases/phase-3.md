# Phase 3 — Registrni xavfsiz birlashtirish

SUPERGOAL_PHASE_START
Phase: 3 of 7 — Registrni xavfsiz birlashtirish
Task: Topilgan loyihalarni mavjud ma’lumotlar bilan dublikatsiyasiz va tiklanadigan tarzda birlashtirish.
Depends on phases: 1, 2
Status: PLANNED — implementation has not started.

## Work

ProjectStore’da additive discovery ledger/candidates/suppression metadata va source association; registration Project invariants saqlangan holda bajariladi. Compatible verified installed runtime topilsa register + HTTP site configure; topilmasa candidate snapshot’da qoladi. Registry va sites.json yozuvlari cross-file transaction emas: idempotent pending/recovery holati kerak. Root generation, optimistic revisions va operation/session guard ishlatiladi.

## Deliverables

- crates/projects/src/project.rs
- crates/projects/src/discovery.rs
- crates/workspace/src/discovery.rs (new)
- crates/workspace/src/lib.rs
- crates/serving/src/site.rs (only if scoped reuse is necessary)

## Acceptance criteria

- P3.1: Bir xil canonical/fizik loyiha manual import, create wizard va discovery orqali uchrasa bitta active Project ID bilan qoladi.
- P3.2: Compatible installed runtime mavjud bo‘lganda har qanday qayta skan registrda aynan bitta project/site yaratadi; yarim yozuv keyingi reconcile’da tuzatiladi.
- P3.3: Runtime yetishmasa candidate UI contract’da qoladi; register qilish uchun soxta package ID ishlatilmaydi va xizmat ishga tushmaydi.
- P3.4: Mavjud PHP/Node selections, custom domain, favorite, DB/cache/mail bindings, secrets, worktree ownership va project revisions skaner tomonidan almashtirilmaydi.
- P3.5: Forget discovery suppression yozadi; qayta skan/restart loyihani qaytarmaydi; Restore/Import suppression’ni explicit olib tashlaydi.
- P3.6: Complete skan yo‘qolgan papkani missing deb belgilaydi; failed/partial scan hech qanday ommaviy missing holatini qo‘llamaydi va DB/project fayllari o‘chmaydi.
- P3.7: Same-filesystem stable identity bilan ishonchli rename project ID va bog‘lanishlarni saqlaydi; running session bo‘lsa relocation pending bo‘ladi, guarded stop/apply yo‘li talab etiladi.
- P3.8: Windows File ID/volume identity mavjud bo‘lmasa yoki identity noaniq bo‘lsa avtomatik birlashtirish qilinmaydi: relocation review candidate, old record saqlanadi.
- P3.9: Root A dan B ga o‘tganda old association detached bo‘ladi, manual entries qoladi va A generation natijalari B ni o‘zgartira olmaydi.
- P3.10: Yangi domen shop.devforge.test formatida; Unicode/invalid nomga stable valid slug taklif qilinadi; domain conflict mavjud saytni egallamaydi va rename custom domenni o‘zgartirmaydi.

## Mandatory commands

- `cargo test -p devforge-projects -p devforge-workspace -p devforge-serving`
- `cargo fmt --all -- --check`

## Evidence required

- Legacy project DB additive migration/reopen, dual-source dedup, suppression/restart, rename/ambiguous move, pending-site recovery, stale generation va binding preservation assertions.
- Print each criterion as pass/fail/not-verified with source/test evidence; counts and exit codes only summarize real runs.
- Before close, reconcile affected knowledge with actual code and current evidence. Do not mark planned behavior implemented.
- Run strict knowledge validator when affected durable knowledge is updated, using prepared tooling.
- During implementation print SUPERGOAL_PHASE_VERIFY, then SUPERGOAL_PHASE_DONE only when all required criteria pass. Planning creates no such completion markers.

## Scope and failure rules

- User requested planning first. This document is a work specification, not permission to implement or publish.
- No global memory edits, unrequested dependency upgrades, unrelated project/system changes, or copying Herd proprietary code/assets.
- Future execution: investigate failure, focused retry, then focused fix; preserve original criteria. Missing target/access remains an honest open gate.
