---
type: Feature Plan
title: Sozlanadigan loyiha papkasi va avtomatik aniqlash
description: Sozlanadigan home/Devforge va avtomatik discovery implementatsiyasi, mahalliy dalil va ochiq native platforma qabul chegaralari.
status: draft
decision_status: confirmed
tags: [projects, discovery, settings, filesystem, macos, windows, planning]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: Avtomatik loyiha aniqlash uchun avval reja topshirigi
  - id: plan
    resource: ../../.supergoal/devforge-configurable-project-folder-aut-fKSwZE/ROADMAP.md
    title: Yetti bosqich va oltmish qabul mezoni
  - id: settings
    resource: ../../crates/core/src/lib.rs
    title: Amaldagi language va theme settings kontrakti
  - id: projects
    resource: ../../crates/projects/src/project.rs
    title: Amaldagi runtime tanlovli Laravel import va registry
  - id: workspace
    resource: ../../crates/workspace/src/lib.rs
    title: Amaldagi explicit import va registered projects snapshot
  - id: serving
    resource: ../../crates/serving/src/site.rs
    title: Amaldagi devforge.test domen va localhost alias kontrakti
  - id: native
    resource: ../../.github/workflows/worktrees.yml
    title: Uch native target va desktop harness namunasi
  - id: watcher
    resource: https://docs.rs/notify/8.2.2/notify/
    title: Native filesystem events va polling cheklovlari
---

# Talab va scope

2026-11-13 foydalanuvchi avval to‘liq reja tuzishni so‘radi: Settings’da default
`~/Devforge`, foydalanuvchi yo‘lni o‘zgartira olishi, loyihalarni avtomatik topish,
macOS va Windows’da tekshirish. Bu talab tasdiqlangan; quyidagi texnik tafsilotlar
ishchi taklif. Hozir implementatsiya yoki publish boshlash topshirilmagan.[^request]

# Reja paytidagi boshlang‘ich kod

- Settings faqat language/theme saqlaydi; discovery root hali yo‘q.[^settings]
- Laravel import exact PHP/Node IDs talab qiladi; no-runtime candidate modeli
  amaldagi Project bilan bir xil emas.[^projects]
- Workspace snapshot registered loyihalarni o‘qiydi; avtomatik folder scan yoki
  watcher hozir implementatsiya qilinmagan.[^workspace]
- Domenlar `.devforge.test` va `.localhost` alias modelida ishlaydi. Herd’dagi
  bare `.test` misoli shu task’da mavjud serving kontraktini almashtirmaydi.[^serving]

# Taklif qilinayotgan xatti-harakat

Default OS home ichidagi `Devforge`; macOS’da `~/Devforge`, Windows’da odatda
`C:\Users\name\Devforge`. Settings’da input, native picker, explicit Save, Reset,
Scan now va actionable status. Yangi default papka bir marta yaratiladi; custom
yo‘l mavjud bo‘lmasa explicit Create amali kerak.[^plan]

Bir root’ning direct child Laravel kataloglari aniqlanadi; dependency subtrees,
dot/staging kataloglari va symlink/junction children avtomatik follow qilinmaydi.
Tashqaridagi loyiha existing Import orqali qo‘shiladi. Installed compatible runtime
bo‘lsa registration, bo‘lmasa needs-runtime candidate; Prepare/Start explicit qoladi.
Scan script, install, migration, `.env`, DB, hosts yoki CA amallarini bajarmaydi.[^plan]

Discovery ledger, source associations, suppression va idempotent reconciliation
taklif qilinmoqda. Root switch eski metadata’ni o‘chirmaydi; Forget avtomatik qayta
qo‘shishni to‘xtatadi. Stable identity rename’da ID/bindings’ni saqlashga xizmat
qiladi; running session yoki noaniq move review/pending holatga o‘tadi.[^plan]

Native watcher invalidation, startup/focus/manual scan va 30s shallow recovery
taklifi bor. Heavy scan UI thread yoki workspace lock ichida bajarilmaydi;
single-flight, bounded queue, cancellation va generation stale-result rejection
kerak. Event yo‘qolishi, editor atomic replacement va root deletion sabab authoritative
rescan zarur; bu planned architecture, runtime dalili emas.[^plan][^watcher]

# Yetti bosqich

1. Settings kontrakti va backward compatibility.
2. Chegaralangan read-only scanner.
3. Registry reconciliation va ma’lumotni saqlash.
4. Watcher lifecycle va recovery.
5. Settings/Projects UI va uch til.
6. Native macOS ARM64/Intel va Windows x64 acceptance.
7. Polish, regression, performance va final evidence audit.[^plan]

# Tekshirish chegarasi

Planning’da source/manifests, native harness va primary watcher docs o‘qildi.
Yangi funksiya uchun product test, build, actual watcher yoki native GUI acceptance
bajarilmadi. Mavjud uch-target workflow faqat kelajak sinov yo‘lining namunasidir;
yangi funksiya shu platformalarda tekshirilgan deb hisoblanmaydi.[^native][^plan]

Har targetda real watcher/path/identity, Settings save/relaunch, actual system picker
select/cancel, Laravel HTTP, uch locale va keyboard tekshiruvi rejalangan. Mock picker
yoki cross-compile native behavior o‘rniga o‘tmaydi. Target/picker unavailable bo‘lsa
ochiq gate saqlanadi; signed release va Windows ARM bu task qamroviga qo‘shilmagan.[^plan]

Plan’da 60 individual acceptance criterion bor. 100 candidate cold scan 3s,
event-to-UI 5s va fallback 35s budget’lari taklif, hali o‘lchangan natija emas.
To‘liq mezonlar, risklar, recovery va source yo‘llari roadmap’da.[^plan]

[^request]: Foydalanuvchining aynan shu suhbatdagi plan-first topshirigi.
[^plan]: Hali bajarilmagan phase specs va qabul mezonlari.
[^settings]: Current source, discovery setting yo‘q.
[^projects]: Current import/inspect, runtime compatibility va duplicate identity.
[^workspace]: Current explicit command va registry-based snapshot.
[^serving]: Current domain validator va alias derivation.
[^native]: Existing workflow yangi feature uchun bajarilgan sinov emas.
[^watcher]: notify primary docs, research snapshot; dependency tanlovi implementatsiyada qayta tekshiriladi.

# Implementatsiya progressi — 2026-11-13

Foydalanuvchi barcha bosqichlarni bajarish, ikkala OS’da tekshirish va xatolarni
tuzatishni topshirdi. Root Settings/SQLite compatibility, read-only shallow scanner,
registry association/suppression/identity, bounded native watcher + 30s recovery,
Settings/Projects UI va New Project default parent yozildi. Bu butun platforma
matritsasi yakunlangani degani emas. Tafsilotlar [STATE](../../.supergoal/devforge-configurable-project-folder-aut-fKSwZE/STATE.md)
va [mahalliy native backend dalilida](../evidence/project-discovery/local-macos-backend.json).

Joriy owning source: [scanner](../../crates/projects/src/discovery.rs),
[reconciliation va watcher](../../crates/workspace/src/discovery.rs),
[Settings UI](../../apps/desktop/src/components/ProjectDiscoverySettings.tsx),
[native fixture](../../crates/workspace/examples/support/discovery.rs).

Mahalliy macOS ARM64: 104 Rust/36 frontend/3 locale; intermediate fmt/clippy o‘tdi.
Release backend real Laravel 13.42.0/PHP 8.4.27 HTTP, rename/Forget/Restore, manual
metadata preservation, 50 root switch, long Unicode path/case-only rename va actual
polling fallback o‘tdi. 100 candidate 4ms, discovery 882ms, fallback 30504ms; bu
fixture natijalari universal tezlik kafolati emas.

Native Settings persistence, actual discovery va 9 axe/0 UI case o‘tdi; dialog focus
xatosi topilib tuzatildi. Remote dialog automation fixture hali tekshirilmoqda.
Windows x64 va Intel Mac yangi source’ni hali bajarmadi. To‘liq acceptance, final
source hash va audit gate’lari ochiq. Public signing/deployment bajarilmadi.
