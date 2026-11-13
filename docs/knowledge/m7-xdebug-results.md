---
type: Development Milestone
title: M7.2 Xdebug debugger va function profiler
description: Project-scoped native Xdebug runtime, DBGp boshqaruvi va bounded Cachegrind profiling dalillari.
status: draft
decision_status: confirmed
tags: [m7, xdebug, debugger, profiler, runtime, desktop]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: M12 keyin qolgan M7 ishlarini tugatish topshirigi
  - id: core
    resource: ../../crates/debugger/src/lib.rs
    title: Native debugger crate va settings kontrakti
  - id: provider
    resource: ../../crates/debugger/src/provider/mod.rs
    title: Verified Windows DLL va private macOS source build
  - id: recipe
    resource: ../../crates/debugger/src/provider/recipe.json
    title: Reviewed PHP Xdebug SPC va private tools inputs
  - id: workspace
    resource: ../../crates/workspace/src/debugging.rs
    title: Project ownership commands va lifecycle
  - id: native
    resource: ../../crates/workspace/examples/support/debugger.rs
    title: Actual PHP Laravel HTTP CLI va recovery acceptance
  - id: ui
    resource: ../../apps/desktop/src/pages/DebuggerPage.tsx
    title: Uch tilli debugger va function profile sahifasi
  - id: workflow
    resource: ../../.github/workflows/xdebug.yml
    title: Uch native target uchun acceptance workflow
  - id: previous
    resource: /m7-inspector-results.md
    title: Bajarilgan M7.1 Inspector dalillari
---

# Joriy holat

M12 yakunidan keyin qolgan M7.2 bajarilmoqda. M7.1 capture/Inspector companion va
uning avvalgi qabul dalillari saqlanadi. Ushbu yozuv native matritsa tugaguncha
M7 to'liq yopilganini bildirmaydi.[^request][^previous]

# Runtime va boshqaruv

Normal project PHP tanlovi o'zgarmaydi. Xdebug uchun stopped project'da reviewed
install va Off/Debug/Profile konfiguratsiyasi mavjud. macOS normal StaticPHP
common CLI/FPM native Xdebug module'ga kerakli exported PHP API'larni bermaydi;
shu sabab optional matching CLI/FPM va Xdebug private prefix'da source'dan
build qilinadi. PDO mysql/pgsql/sqlite saqlanadi. Studio paired binary spike'da
PDO pgsql yo'qligi sabab universal provider sifatida olinmadi.[^provider][^recipe]

Windows official matching NTS VS17 x64 DLL SHA256 bilan olinadi. macOS PHP source,
Xdebug3.5.3, SPC va build tools recipe'da pinned; transitive library inputs SPC
orqali resolve qilinib actual SHA256 provenance'da yoziladi. Bu dependency'larning
barchasi oldindan pinned yoki bit-reproducible build degan claim emas. Natijaviy
archive digest va descriptor OS secret store'dagi key bilan attestation qilinadi;
activation'da paket/resource integrity tekshiriladi. Inventory poll faqat receipt
va file presence'ni o'qiydi, katta executable'larni har poll'da hash qilmaydi.
Build private prefix'dan tashqarida package manager ishlatmaydi; Apple CLT va
network talab qilinadi. Apple CLT Python HTTPS probe real TLS protocol xatosi
berdi; shu source system curl bilan kutilgan SHA256'ga mos yuklandi. Builder
secure HTTPS-only system curl va SHA256 yo'liga o'tdi; TLS verification o'chirilmaydi. Upstream GitHub API rate limit build'ni fail qilishi
mumkin; prior runtime o'zgarmaydi, bounded log bilan retry mavjud.[^provider][^recipe]

DBGp listener ephemeral loopback port va per-session private key bilan ishlaydi.
Breakpoint, Continue, Step into/over/out, stack frame va bounded read-only locals
mavjud. Control queue16 bilan bounded; resume/step engine ACK'idan oldin Running
holatini beradi, duplicate step Busy bo'ladi. Key/sensitive variable nomlari va known project secrets mask qilinadi;
source viewer faqat project ichidagi PHP fayllarni ochadi. Arbitrary eval yoki
variable mutation API yo'q. CLI debug status/source/profile va reviewed install/
mode commands shu Rust boundary'dan foydalanadi.[^core][^workspace]

Per-project generated INI mode/trigger/output'ni boshqaradi; project .env va
normal PHP pin yozilmaydi. Frontend Node process'dan Xdebug environment ajratiladi.
Stop/quit owned receipt/socket/request'ni yopadi; stale unlocked receipt faollashmaydi.
Schema6 debugger settings'ni default Off bilan qo'shadi; old records saqlanadi,
old build yangi schema'ni qabul qilmaydi. Worktree/import/portable setup yangi
project uchun local debug session'ni meros olmaydi.[^workspace][^core]

# Profiler

Triggered HTTP va explicit CLI invocation Xdebug Cachegrind yaratadi. Parser
self/inclusive vaqt, call count, memory va summary'ni ko'rsatadi; top500 function
qidirish, retained history, confirmed delete va no-clobber raw export mavjud.
UI/project read pathlari scoped va bounded. Fayl32MiB, history20/128MiB; supervisor
100ms intervalda own prefix uchun32MiB per file,128MiB/25 files budget'ni kuzatib,
limitda faqat tegishli owned process tree'ni restart qilmasdan to'xtatadi. Bu disk
write'ni har baytda cheklaydigan OS quota emas. Raw export real source paths va
function nomlarini saqlaydi; UI esa project-relative paths/known values'ni mask
qiladi. Profile argument yoki request body capture'ni yoqmaydi.[^core][^workspace]

# Dalillar va ochiq tekshiruv

Local macOS ARM64 PHP8.4.27/8.5.12 paired source-build spike va Xdebug3.5.3 load
probe o'tdi; PDO uch drayver bor. Actual Laravel HTTP breakpoint, into/over/out,
redacted locals/source, HTTP200, HTTP/CLI Cachegrind, export/delete/isolation,
other-project PID preservation va Off normal-PHP recovery o'tdi. Bu dastlabki
spike arxivlari bilan local integration dalili; production builder'ning yakuniy
native CI source/build matritsasi alohida tekshiriladi.[^native]

Local WKWebView'da genuine IPC/control flow, install review/cancel, breakpoint/
source/locals, stepping, profile/filter/delete-cancel, ru/uz/en katta va minimum
viewport, dark/relaunch o'tdi:11 axe holati0 violation. Native Save dialog
avtomatlashtirilmadi; export actual backend fixture'da tekshirildi. Scoped spacing/privacy va stale-profile-read fix'dan keyingi native UI retry
ham11 axe/0 bilan o'tdi. Oldingi retry aynan window resize'dagi ikki animatsiya
kadrini kutishda timeout berdi; debugger breakpoint/locals assertions ungacha
o'tgan edi. Harness paint wait endi bounded animation-frame-or-timer ACK bilan;
functional/accessibility assertions saqlanadi. Yakuniy CI hali kutilmoqda.
Local full workspace104 Rust passed/1 credential fixture ignored,34 frontend,
clippy va strict OKF28/0/0 o'tdi; target matritsa o'rniga o'tmaydi.[^ui][^workflow]

Windows x64 va Intel macOS native Xdebug acceptance hali tugamadi. M15 signing,
minimum OS, native credential/system-dialog, interrupted-install va licensing
review gate'lari saqlanadi; public runtime/installer release qilinmaydi.[^workflow]

# Kod bilan solishtirish

| Da'vo | Holat | Dalil/chegara |
| --- | --- | --- |
| Bajarilgan Inspector capture/app profiling | Mos | M7.1 source qayta yozilmadi; tarixiy report saqlanadi |
| Optional project debug runtime/modes/ownership | O'zgargan | New debugger crate, schema6 va Workspace commands |
| Actual breakpoints/function profiles ARM64 | Mos | Local PHP/Laravel va WKWebView fixture |
| Yakuniy production builder va uch native target | Tekshirilmagan | CI matrix yakunlanishi kutilmoqda |
| Signed distributive/minimum OS/native credential UI | Tekshirilmagan | M15 gate'lari |

[^request]: User: avval M12 to'liq, keyin qolgan M7; bajarilgan M7.1 saqlansin.
[^core]: Bounded DBGp XML, authenticated launch receipt va Cachegrind parsing/store.
[^provider]: OS-specific provider va existing managed package/process ownership adapters.
[^recipe]: Exact critical source/tools checksums; transitive inputs actual provenance bilan.
[^workspace]: Project revision/consent guards va common CLI/Tauri boundary.
[^native]: Disposable real Laravel13/PostgreSQL fixture; no user project changes.
[^ui]: Existing DevForge master va generated target; genuine native WebView screenshots.
[^workflow]: Native matrix uchun test-only credential adapter; release qualification emas.
[^previous]: Original M7.1 acceptance o'z source/job chegarasida saqlanadi.
