---
type: Feature Implementation
title: Sozlanadigan loyiha papkasi va avtomatik aniqlash
description: Sozlanadigan home/Devforge, avtomatik Laravel discovery, safe registry va native platforma qabul dalillari.
status: draft
decision_status: confirmed
tags: [projects, discovery, settings, filesystem, macos, windows, implementation]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: Avtomatik loyiha aniqlash uchun avval reja topshirigi
  - id: plan
    resource: ../../.supergoal/devforge-configurable-project-folder-aut-fKSwZE/ROADMAP.md
    title: Yetti bosqich va oltmish qabul mezoni
  - id: settings
    resource: ../../crates/core/src/lib.rs
    title: Backward compatible Settings va partial patch kontrakti
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
    resource: ../../.github/workflows/discovery.yml
    title: Ushbu feature uchun uch native target acceptance workflow
  - id: watcher
    resource: https://docs.rs/notify/8.2.2/notify/
    title: Native filesystem events va polling cheklovlari
---

# Talab va bajarish vakolati

Foydalanuvchi Settings’da default `~/Devforge`, o‘zgartiriladigan papka va avtomatik
loyiha topishni so‘radi. Avval yetti bosqich/60 mezonli reja tuzildi. Keyingi aniq
topshiriq barcha bosqichlarni bajarish, macOS/Windows’da tekshirish va xatolarni
tuzatishni vakolatladi. Ushbu feature M7/M13 yoki signed release scope’ini
kengaytirmaydi. Original qarorlar va mezonlar reja faylida saqlangan.[^request][^plan]

# Amalga oshirilgan xatti-harakat

- Default OS home ichidagi `Devforge`: macOS’da `~/Devforge`, Windows’da
  `<user profile>\Devforge`. Eski language/theme sozlamalari saqlanadi. Fresh default
  bir marta yaratiladi; keyin o‘chirilgan root jim qayta yaratilmaydi.
- Settings’da text input, haqiqiy native picker, Save, Create, default draft va
  Scan now bor. Custom folder yo‘qligida Save yaratmaydi. Invalid/persist xatosida
  oldingi root qoladi. Language/theme patch unavailable root’ni qayta validatsiya
  qilmaydi. Draft navigation guard va picker focus qaytishi mavjud.[^settings]
- Bir root’ning faqat bevosita Laravel child directory’lari read-only tekshiriladi.
  Dot/dependency/owned staging/data directory, non-Unicode child va symlink/junction
  children skip qilinadi.10,000 entry cap yoki permission xatosi partial/unavailable,
  ommaviy missing emas. Scanner script, `.env`, install yoki secret ishlatmaydi.
- Runtime mustaqil candidate saqlanadi. Mos installed PHP CLI+web va Node bo‘lsa
  project/site yaratiladi; bo‘lmasa needs-runtime holati. Prepare/Start explicit.
- Project schema6 additive discovery ledger, fizik identity, transactional Forget
  suppression va Restore bor. Restore original project ID, runtime va binding’larni
  qaytaradi. Birinchi scan’dan oldingi CLI Forget ham suppression qiladi. Stale
  revision suppression’ni o‘zgartirmaydi; explicit Import faqat muvaffaqiyatdan keyin
  suppression’ni yechadi.
- Aniq stopped same-volume rename ID/domain/bindings’ni saqlaydi; live session,
  managed worktree yoki noaniq identity relocation-pending qoladi. O‘sha path’ga boshqa
  physical directory qo‘yilsa eski profil bilan jim birlashtirilmaydi. Root switch
  old associations’ni detached qiladi, project/DB fayllari saqlanadi.[^projects][^workspace]
- New Project parent saqlangan root’dan olinadi. Uch til mavjud registry/catalog
  orqali kengaytiriladi. `.devforge.test`/`.localhost` serving kontrakti saqlandi;
  domain collision boshqa saytni egallamaydi.[^serving]

Owning source: [scanner va ledger](../../crates/projects/src/discovery.rs),
[reconciler/watcher](../../crates/workspace/src/discovery.rs),
[Settings UI](../../apps/desktop/src/components/ProjectDiscoverySettings.tsx),
[native backend fixture](../../crates/workspace/examples/support/discovery.rs),
[native desktop acceptance](../../scripts/discovery_desktop_smoke.py).

# Watcher va platforma farqi

Bitta worker, bitta bounded wake slot va bitta result slot ishlaydi. Native event
scan’ni invalidatsiya qiladi;500ms quiet debounce,2s maximum invalidation window,
startup/manual/focus va30s periodic fallback bor. Heavy filesystem walk UI/Workspace
lock tashqarisida; generation/cancellation eski root natijasini rad qiladi. Tray’da
monitor davom etadi; Quit worker’ni cancel/join qiladi.[^watcher]

**O‘zgargan dizayn:** macOS’da root/parent va shallow child/public watch’lar qoladi.
Windows real testida per-project descendant handle stopped Laravel katalogining
rename’ini AccessDenied bilan to‘sdi. Windows adapter endi faqat collection root’ni
recursive, parent’ni nonrecursive watch qiladi. Callback direct child va project
marker’larini lexical filter qiladi; vendor/node_modules/storage deep eventlari
canonicalization/queue’dan oldin chiqariladi. Scanner baribir shallow; kernel root
watch bilan application deep-directory scan bir xil emas.

# Yakuniy tekshirish — 2026-11-13

Feature scope’idagi7 bosqich va60 mezon kod/test/dalil bilan solishtirildi.
Actual native ARM64/Windows source0fb0eca va Intel sourcec33eadc: app/core/catalog
hamda runner fayllarining SHA256 tree’i bir xil. Oxirgi commit faqat cache entry
va CI target selector’ni o‘zgartirgan. Har target o‘z checkout/archive/ZIP hashiga
bog‘langan; public ZIP SHA256 GitHub metadata bilan mos keldi.[^native]

| Native target | 100 cold scan | 100-project event → UI | Actual fallback | UI / axe / violations |
| --- | --- | --- | --- | --- |
| macOS ARM64 | 11ms | 1397ms | 30663ms | 12 / 10 / 0 |
| macOS Intel | 9ms | 2022ms | 30588ms | 12 / 10 / 0 |
| Windows x64 | 26ms | 1199ms | 30518ms | 12 / 10 / 0 |

Har targetda PHP8.5.12/Laravel13.42.0 real HTTP, no-runtime, incomplete copy,
rename/domain/ID, original-profile Restore, manual metadata, permission-denied,
root delete/recreate, symlink/junction, long Unicode/case va10k-file workload o‘tdi.
50 tugallangan root generations’dan keyin ARM64/Intel handles13→13/threads3→3;
Windows handles178→178/threads2→2. Actual system picker select/cancel, native Tab,
focus return, save/relaunch, tray va New Project parent tekshirildi. Uch locale,
light/dark va820x620 content’da horizontal overflow0. Workload natijalari tezlikning
universal kafolati emas.

Core CI:macOS110/Windows108 Rust pass; har birida1 opt-in credential fixture
ignored.37 frontend,6 packaging,3 locale testi,3 semantic catalog,
typecheck/fmt/clippy va default-feature desktop build o‘tdi. Native example ham
feature-enabled clippy bilan tekshirildi. Native GUI alohida e2e featureli debug
acceptance build; normal default build’da control endpoint bo‘lmaydi.

Qo‘shimcha real Apple M4/macOS27.2.1 dalili, source/binary/report/runner hashlari va
har bir platformaning actual natijalari [yakuniy hisobotda](../evidence/project-discovery/README.md).
[60 mezon](../../.supergoal/devforge-configurable-project-folder-aut-fKSwZE/acceptance-audit.json)
va [phase reconciliation](../../.supergoal/devforge-configurable-project-folder-aut-fKSwZE/PHASE_RECONCILIATION.md)
qaror, o‘zgargan dizayn va actual qamrovni ajratadi.

# Upstream va release chegarasi

PHP tarqatuvchisi macOS arxivlarini versiya nomini o‘zgartirmasdan qayta chiqardi;
[current checksum farqi](../evidence/project-discovery/upstream-runtime-checksums.json)
qayd etildi. Katalog pinlari va user runtime tanlovlari saqlandi. Native CI uchun
original reviewed public arxivlar hash-bound vaqtinchalik Git blob cache’dan olinib,
original SHA256 va PackageStore orqali qayta tekshirildi; branch tree yoki release’ga
runtime binary qo‘shilmadi. Bu production live-URL/catalog maintenance gate’ini
yopmaydi va o‘zgargan upstream arxivlar ishga tushirilmadi.

OS security permissions so‘ralmadi/o‘zgartirilmadi. Native AX mavjud ruxsatdan
foydalandi; denied-root testi faqat owned disposable fixture ruxsatini vaqtincha
almashtirib, tikladi. User project yoki system-wide Herd konfiguratsiyasi o‘zgarmadi.

Multiple roots/generic drivers, signed release, fresh installer/minimum OS,
Windows ARM va network/removable filesystem certification shu feature scope’iga
kirmaydi. Existing M15 gate’lari saqlanadi; bu task aloqasiz M7/M13 kodini o‘zgartirmadi.
Remote main parallel M7 task’da65948e0 ga o‘tdi; feature qabuli ajratilgan branch/source
hashlariga tegishli, birlashtirilgan main integratsiyasi tekshirilmagan.

[^request]: Aynan shu suhbatdagi plan-first va keyingi to‘liq bajarish topshiriqlari.
[^plan]: Original7 bosqich/60 mezon, risklar va yakuniy individual audit.
[^settings]: Backward-compatible Settings/SettingsPatch; persistence/resolve platform qatlamida.
[^projects]: Laravel import, additive registry migration, physical identity va suppression ownership.
[^workspace]: Runtime/site lifecycle va safe discovery reconciliation owning layer.
[^serving]: Existing domain/alias validator, collision guard va explicit Start kontrakti.
[^native]: Ushbu feature uchun native workflow va public artifacts.
[^watcher]: notify backend events authoritative inventory emas; shallow recovery scanner bilan birga ishlatiladi.


# Main/M7 va upstream integration — davom etmoqda

Foydalanuvchi upstream katalog masalasini yakunlash, discovery’ni main’dagi M7
bilan birlashtirish, ikki OS’da tekshirish va main’ga push qilishni aniq topshirdi.
Integration baseline main65948e0 va discoverye5c5f64. M7’dagi immutable legacy pinlar,
superseded mapping va alohida reviewed build family modeli saqlanmoqda. Yetishmagan
Intel PHP8.5 CLI/FPM current build’lari shu modelga qo‘shildi; eski descriptor va
user tanlovlari o‘zgarmaydi. Discovery fixtures endi current, non-superseded SHA
bo‘yicha tanlaydi; vaqtinchalik original-cache workaround ishlatilmaydi.

Ikkala branch schema6 ishlatgani uchun additive schema7 migratsiyasi ikkala
history’ni qo‘llaydi. Native combined acceptance actual auto-discovered Laravel’da
M7 debug/profile va Forget/Restore debugger configuration’ni tekshiradi. Bu section
hali final native success yoki main push dalili emas; yakuniy job/source/hashlar
integration evidence’ga yoziladi.
