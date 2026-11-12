---
type: Implementation Report
title: M7 Laravel debugging va profiling
description: Opt-in collector companion package va native Inspector birinchi ishlaydigan qatlami.
status: draft
decision_status: confirmed
tags: [m7, inspector, profiling, laravel, evidence]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: M7 ni boshlash topshirig'i
  - id: core
    resource: ../../crates/inspector/src/lib.rs
    title: Authenticated loopback bounded capture store
  - id: companion
    resource: ../../packages/laravel-inspector/src/InspectorServiceProvider.php
    title: Laravel events va dump handler integration
  - id: coordinator
    resource: ../../crates/workspace/src/lib.rs
    title: Project setup capture lifecycle va capability probe
  - id: ui
    resource: ../../apps/desktop/src/pages/InspectorPage.tsx
    title: Real Inspector event/context UI
  - id: desktop-local
    resource: ../evidence/m7/local-desktop.json
    title: Native local Inspector UI and seven axe cases
  - id: core-ci
    resource: ../evidence/m7/ci-core.json
    title: Immutable source macOS Windows core check job conclusions
  - id: counters
    resource: ../evidence/m7/test-metrics.json
    title: Actual job log counters
  - id: audit
    resource: ../evidence/m7/ui-audit.md
    title: New Inspector visual and source audit
  - id: arm-native
    resource: ../evidence/m7/hosted-macos-arm64.json
    title: Native hosted ARM64 Laravel and WKWebView acceptance
  - id: windows-first
    resource: ../evidence/m7/hosted-windows-attempt1.json
    title: Windows backend pass and actual first accessibility finding
  - id: intel-first
    resource: ../evidence/m7/hosted-macos-x64-attempt1.json
    title: Intel backend pass and actual missing request summary finding
  - id: late-ack
    resource: ../evidence/m7/late-ack-regression.json
    title: Actual PHP baseline and fixed delayed-acknowledgement reproduction
  - id: cli-local
    resource: ../evidence/m7/local-cli.json
    title: Actual local native CLI reads history without changing its database
  - id: source-map
    resource: ../evidence/m7/source-reconciliation.json
    title: Immutable source differences revalidation and actual Windows fixture correction
  - id: windows-native
    resource: ../evidence/m7/hosted-windows-x64.json
    title: Final native Windows Laravel and WebView2 acceptance
  - id: intel-native
    resource: ../evidence/m7/hosted-macos-x64.json
    title: Native Intel Laravel and WKWebView acceptance
  - id: native-matrix
    resource: ../evidence/m7/ci-native.json
    title: Actual required native jobs and preserved failed attempts
  - id: artifacts
    resource: ../evidence/m7/artifact-verification.json
    title: Three final archives SHA-256 and report equality receipts
  - id: setup
    resource: ../../packages/laravel-inspector/README.md
    title: Actual companion install session and privacy behavior
  - id: xdebug-install
    resource: https://xdebug.org/docs/install
    title: Official native extension installation and matching requirements
  - id: local
    resource: ../evidence/m7/local-native.json
    title: Actual local Laravel PG request SQL dump dd job exception acceptance
---

# Scope va hozirgi holat

Foydalanuvchi qayta tartiblangan roadmap bo'yicha M7'ni boshlashni topshirdi.
M7.1 opt-in event capture, Laravel companion, request/query profiling va desktop
Inspector yaratildi va macOS ARM64/Intel hamda Windows x64 native qabuldan o'tdi.
M7 to'liq yopildi deb belgilanmaydi: Xdebug
capability aniqlanadi, native extension/debugger/function profiler boshqaruvi
uchun mos provider va alohida tekshirish keyingi qismda qoladi. M15 yakuniy beta
bosqichi oxirida qoladi; doimiy test va data preservation to'xtatilmaydi.[^request]

# Kod kontrakti

`crates/inspector` Tauri'dan mustaqil. Collector loopback ephemeral port, loyiha
bo'yicha random credential va project-id tekshiruvi bilan ishlaydi. Origin'li
browser so'rovlari rad etiladi; request body128KiB, batch200 event, concurrency8.
Session JSON private app-data'da; UI/CLI DTO token bermaydi. Kernel owner lease va
instance nonce stale config'ni boshqa collector session'ida qo'llashni oldini oladi.
Event data bounded/sanitized, SQLite ring4000 events va16MiB payload budget,
visible24h/200 events/256KiB. History saqlanadi, capture app sessiyasiga bog'liq.[^core]

Companion local/testing va DevForge endpoint/token ENV bo'lgandagina ulanadi.
Request route template, query duration, job lifecycle, log, dump va exception
konteksti olinadi. Request headers/body, SQL bindings va job payload olinmaydi;
SQL literal/comments normalized, known secrets va sensitive keys mask qilinadi.
Bu hostile code sandbox yoki barcha encoded/private qiymatlarni topish kafolati
emas; capture opt-in va lokal test ma'lumotiga mo'ljallangan. Collector unavailable
bo'lsa bounded transport va per-context circuit breaker application'ni davom
ettiradi. dd exit semantikasi saqlanadi, no-active-scope dump default handler'ga
qaytariladi. QueryException raw bindingsli message uzatilmaydi.[^companion]

Install amali user confirmation bilan .devforge/laravel-inspector, composer.json/
lock/vendor'ga ta'sir qiladi, .env'ni o'zgartirmaydi. Existing conflicting files
refuse qilinadi; package atomic staging bilan yoziladi. Composer require/reinstall
scripts/plugins'siz, keyin explicit package:discover ishlaydi. Running project avval
Stop qilinadi. Capture enable/pause faqat selected project'ni restart qiladi;
clear faqat uning Inspector history'sini o'chiradi.[^coordinator]

SQL pattern repetitions potential N+1 hint; actual source/query execution yoki
N+1 proof emas. Profiler request/job duration, captured SQL total/count va peak PHP
memory'ni ko'rsatadi. Xdebug probe loaded/version/mode haqiqiy selected PHP'dan;
local PHP8.5.12'dagi loaded=false extension bor degan claim'ga aylantirilmaydi.[^local]

# Tekshirish dalillari

Local native Laravel/PostgreSQL HTTP, normalized/slow/repeated SQL, dump/dd,
queue-job va exception, sentinel masking, project isolation/scoped clear,
enableB preservingA PIDs, pause va history persistence o'tdi. Companion Laravel13
fixture'da tekshirildi; declared11/12 constraints hali runtime dalili emas.[^local]

Local native Inspector UI ham o'tdi: real companion install B davomida A master
PID'lari saqlandi; enable/filter/context/dump/clear/cancel, selected PHP probe,
ru/uz/en minimum viewport, dark mode va history/capture-off reopen tekshirildi.
Seven axe holati zero violations. Frontend17, Rust62 passed/1 native credential
fixture ignored;3 locale regression va3 catalog coverage o'tdi. Core CI source
f7601dc'da macOS va Windows'da shu counters, fmt/clippy/typecheck/default native
build bilan o'tdi. Bu counters bitta OS uchun; matrix soni bilan ko'paytirib
alohida testlar deb ko'rsatilmaydi.[^desktop-local][^core-ci][^counters]

Hosted macOS ARM64'da55b9d2f source bilan real backend va WKWebView workflow o'tdi,
7 axe holatida0 finding. SHA-256 tekshirilgan artifact ichidagi JSON'lar job log
report'lari bilan tenglashtirildi. Hosted katta viewport1511x656 bo'lib cheklangan,
minimum820x592; local1511x1041 target bilan teng deb ko'rsatilmaydi.[^arm-native]

Windows birinchi native run'ida backend to'liq o'tdi, GUI'dagi dump holatida esa
`scrollable-region-focusable` topildi. fb93c01 named detail va nested JSON scroll
region'lariga keyboard focus qo'shdi; accessibility gate yumshatilmadi. Typecheck,
17 frontend va local real WKWebView7 axe holati shu fix bilan qayta o'tdi.
Default production-feature native binary ham build qilindi.[^windows-first][^desktop-local]

Intel'da ham backend o'tdi; GUI esa request-filterda yakuniy context topmadi.
Kechikkan ACK sabab companion circuit ochilgach final summary ham tashlab
yuborilishi alohida actual PHP sinovida takrorlandi:120ms delayed ACK bilan
oldingi source faqat dump, tuzatilgani dump va `truncated=true` request yubordi.
Bu hosted failure timing'ining o'lchangan isboti emas, unga mos keladigan
reproduced transport xatosidir.55b9d2f circuit ochilgandan keyin bitta bounded
final summary urinishini saqlaydi, mavjud deadline'larni uzaytirmaydi va incomplete
capture'ni ko'rsatadi. Native fixture'ga delayed ACK regression qo'shildi;
frontend accessibility yoki real SQL/job assertions yumshatilmadi. Yangi manba
uch targetli native matritsada tekshirildi.[^intel-first][^late-ack]

Local55b9d2f native backend delayed ACK qo'shilgan holda o'tdi; Inspector Rust6
testi va native example clippy o'tdi. Local WKWebView ham7 axe holatida0 finding
bilan yakunlandi. Native source freeze'dan keyin uch katalogdagi faqat mavjud
incomplete-capture matni aniqroq qilindi: holat event yoki transport limitida
ham kelishi mumkin. Key/placeholder o'zgarmadi;3 catalog va3 locale regression
shu copy bilan o'tdi. Native functional evidence55b9d2f'ga tegishli; undan keyingi
copy-only tahrir alohida qayd etilgan.[^desktop-local][^late-ack]

55b9d2f Windows run'i yangi regression fixture'ning absolute verbatim PHP include
yo'lida to'xtadi; bu run'da native acceptance tugamadi. e4f4b97 fixture'ni mavjud
project cwd'ga nisbatan require qilishga o'tkazdi.55b9d2f→e4f4b97 farqi faqat
shu test helper va yuqoridagi3 catalog qiymatidan iborat; Rust/PHP backend yoki
UI action logic o'zgarmadi. Mac functional source55b9d2f, final Windows source
e4f4b97 alohida qayd etiladi; oldingi failed run success'ga aylantirilmaydi.[^source-map]

Windows x64 e4f4b97 workflow43273196588/job131374774744 to'liq o'tdi: actual
Laravel/PHP/PG delayed ACK regression, capture isolation/privacy/clear/pause/history,
genuine companion install va WebView2 controls, ru/uz/en minimum window, dark mode,
7 axe holatida0 finding. Bu cross-compile yoki browser mock emas.[^windows-native]

Intel macOS55b9d2f job131369200652 ham backend, delayed ACK va real WKWebView
flow'ni tugatdi;7 axe holatida0 finding. First failed request-profile holati shu
manbada qayta o'tdi. Final uch archive publisher SHA-256'ga mos, ichidagi report
JSON'lari job log JSON'lariga teng. Hosted Intel katta content viewport1511x942,
ARM64'da1511x656 bo'lib cheklangan; local1511x1041 va Windows1511x1041 alohida
dalil. Source55b9d2f→e4f4b97 farqi yuqoridagi allowlist bilan solishtirildi.[^intel-native][^artifacts][^source-map]

| Target | Source va actual job | Natija |
| --- | --- | --- |
| macOS ARM64 |55b9d2f, [job131369200216](https://github.com/jorabekyusupov/devforge/actions/runs/43271574916/job/131369200216) | Backend + WKWebView passed,7 axe/0 finding |
| macOS Intel |55b9d2f, [job131369200652](https://github.com/jorabekyusupov/devforge/actions/runs/43271574916/job/131369200652) | Backend + WKWebView passed,7 axe/0 finding |
| Windows x64 |e4f4b97, [job131374774744](https://github.com/jorabekyusupov/devforge/actions/runs/43273196588/job/131374774744) | Backend + WebView2 passed,7 axe/0 finding |

Bu assembled qabul matritsasi.43271574916 workflow'ning umumiy failure holati
eski Windows fixture path urinishidan qolgan; Windows uchun keyingi successful
run ishlatiladi. Original failure'lar o'chirilmagan yoki success deb yozilmagan.
M7.1 qabul tugadi; M7.2 amalga oshirilmagan scope quyida ochiq.[^native-matrix]

Actual native CLI `devforge inspect <project-id>`22 retained events/2 contexts'ni
JSON sifatida o'qidi; event DB hash oldin/keyin bir xil, capture session yaralmadi,
sentinel yo'q. Bu faqat fixture history read dalili, remote SQL tool emas.[^cli-local]

Native resize'dan keyingi raw katta capture header/sidebarni to'liq repaint
qilmagan. Shu1511x1041 viewport xavfsiz window origin va scroll reset/paint wait
bilan qo'shimcha capture qilinib ko'rildi; actual controls/data tekshirildi.
Illustrative target bilan bir xil hierarchy saqlangan; native select rendering
va incumbent shell farqi ochiq, pixel-perfect certification da'vosi yo'q.[^audit]

# Keyingi M7 qismi

M7.1 request/query/job/log/dump/exception inspection va application profiling'dir.
Function-level Xdebug profiler yoki interactive debugger boshqaruvi hali yo'q.
Native extension install OS, PHP ABI va build turiga mos artifact/source build
hamda project-specific INI/recovery'ni talab qiladi. Hozirgi selected native PHP
probe loaded=false; hech qanday global Homebrew/PECL/PIE install bajarilmadi.
Keyingi ish: compatible runtime/extension provider spike, verified artifact va
managed INI modes/trigger, bounded profile output hamda har targetdagi haqiqiy
Xdebug/debugger/function-profile sinovi. Declared Xdebug ecosystem support native
DevForge coverage o'rniga o'tmaydi.[^xdebug-install][^setup]

Public release/installer qayta paketlanmadi. M6'dagi minimum OS, native
credential/system-dialog, interrupted-install, signing va licensing gate'lari
M15'da ochiq qoladi. M7 fixture private file-backed test credential adapter'dan
foydalanadi; OS credential UI shu sinov bilan tekshirildi deb hisoblanmaydi.

# Kod bilan solishtirish

| Da'vo | Holat | Kod va dalil |
| --- | --- | --- |
| Project-scoped authenticated capture, bounded history, stale session rejection | Mos | `crates/inspector`, own Rust6 tests va native project isolation/clear/reopen |
| Real Laravel request/query/job/log/dump/dd/exception | Mos | Companion provider/Capture va real Laravel13/PG native fixture |
| App-level elapsed/SQL/memory profiling va repeated/slow hints | Mos | `crates/inspector/src/profile.rs`, Inspector context panel va native assertions |
| Uch tilli Inspector, install/clear confirmation va keyboard scrolling | O'zgargan | New UI/locale keys; Windows finding sabab fb93c01 focus correction |
| Timeout'dan keyin context summary | O'zgargan |55b9d2f one bounded final attempt + incomplete flag; old-source failure reproduced |
| Function-level Xdebug profiler/interactive debugger | Amalga oshirilmagan | Selected PHP capability probe only; M7.2 provider/runtime spike kerak |
| Laravel11/12 actual runtime, minimum OS va native credential UI | Tekshirilmagan | Compatibility declarations va file test adapter bu acceptance o'rniga o'tmaydi |

Yakuniy strict OKF22 hujjat,0 error/0 warning bilan o'tdi. U faqat hujjat
strukturasini tekshiradi; native test o'rniga o'tmaydi. Yakuniy docs-only commit
uchun o'tgan product suite'lari qayta ishlatilmaydi. Private local fixture data
va uning jarayonlari tekshirilib tozalandi; default-feature desktop build saqlandi.

# Takrorlash yo'li

`.github/workflows/inspection.yml` actual managed package/Laravel/PG fixture
yaratadi, `native_workspace inspect-native <fixture>` backend/delayed ACK'ni,
`inspect-ui-prepare <fixture>` genuine companion install holatini tayyorlaydi.
`npm run tauri -- build --debug --no-bundle --features e2e` so'ng
`python scripts/m7_desktop_smoke.py <fixture> <desktop> <native_workspace>` real
WKWebView/WebView2 controls'ni tekshiradi. Debug-only driver production build'ga
kiritilmaydi. Local so'nggi build `npm run tauri -- build --debug --no-bundle`
bilan default feature'larga qaytarildi. Fixture private credential/DB data'si
artifactga qo'shilmaydi; faqat bounded public report va native capture'lar saqlanadi.

[^request]: «m7dan boshla»; M7 debugging/profiling birinchi, M15 beta oxirgi.
[^core]: Token never Debug/DTO; no SQL execution API.
[^companion]: Local Composer path package, source binariesni o'zgartirmaydi; native PHP extension emas.
[^coordinator]: Managed runtime/INI/env va worker boundary reused.
[^ui]: Generated target design-system'da illustrative; product mock data bilan to'ldirilmagan.
[^local]: Actual isolated task fixture; user Herd/hosts/trust o'zgartirilmagan.

[^desktop-local]: No IPC mocking; private fixture adapter and debug-only driver.
[^core-ci]: Exact source workflow43264983195, both job conclusions success.
[^counters]: Per-OS own workspace tests, dependency testlari qo'shib hisoblanmagan.
[^audit]: Fresh guidelines, inspected generated/actual targets and bounded fixes.
[^arm-native]: Workflow43271574916 job131369200216,55b9d2f source; report va artifact verified.
[^windows-first]: Workflow43264989478 job131346889013; backend pass va actual GUI failure saqlandi.
[^intel-first]: Workflow43264989478 job131346888526; request summary missing, backend pass.
[^late-ack]: Native fixture120ms ACK delay; standalone before/fixed PHP8.5.12 reproduction ham saqlandi.
[^cli-local]: Local built CLI + isolated fixture; no collector/session created and history hash unchanged.
[^source-map]: Exact source diff allowlist, actual failed fixture and relevant follow-up checks.
[^windows-native]: Actual hosted Windows x64 source e4f4b97, backend and desktop reports both passed.
[^intel-native]: Actual native Intel source55b9d2f, backend and desktop reports both passed.
[^native-matrix]: Actual per-job conclusions and source reconciliation; original run-level failure retained.
[^artifacts]: All three final ZIP digests and report equality checked before accepting evidence.
[^setup]: Source behavior and meaningful project changes disclosed before install.
[^xdebug-install]: Official installation paths; current catalog'da actual loaded=false.
