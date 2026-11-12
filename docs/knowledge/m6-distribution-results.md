---
type: Implementation Report
title: M6 ichki distributiv va MVP qabul sinovi
description: Paketlash, prerequisite, ownership cleanup va native acceptance holati.
status: draft
decision_status: confirmed
tags: [m6, packaging, acceptance, macos, windows]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: M6 bosqichiga o'tish topshirig'i
  - id: packaging
    resource: ../../scripts/package_desktop.py
    title: Release sidecars DMG NSIS notices va inventory
  - id: acceptance
    resource: ../../scripts/m6_package_smoke.py
    title: Native installer lifecycle va release measurements
  - id: contract
    resource: ../../distribution/README.md
    title: Distribution ownership support va ochiq release gates
  - id: platform
    resource: ../../crates/platform/src/distribution.rs
    title: GUI CLI shared operation leases va OS prerequisite inventory
  - id: cleanup
    resource: ../../crates/cli/src/distribution.rs
    title: Bounded owned hosts va CA cleanup inventory
  - id: desktop
    resource: ../../apps/desktop/src-tauri/src/lib.rs
    title: Single-instance restoration va opt-in startup report
  - id: windows
    resource: ../../apps/desktop/src-tauri/tauri.windows.conf.json
    title: Per-user NSIS installer va uch tilli language selector
  - id: core-ci
    resource: ../evidence/m6/ci-core-checks.json
    title: macOS Windows core checks native builds
  - id: counters
    resource: ../evidence/m6/test-metrics.json
    title: Job logsdan olingan aniq test counters
  - id: system-ci
    resource: ../evidence/m6/ci-system-cleanup.json
    title: Disposable OS hosts trust cleanup job conclusions
  - id: system-reports
    resource: ../evidence/m6/system-cleanup-reports.json
    title: Uninstall block cleanup plans va after-cleanup inventory
  - id: local-release
    resource: ../evidence/m6/local-macos-acceptance.json
    title: Task-local release DMG install data preservation va measurements
  - id: local-gui
    resource: ../evidence/m6/local-native-desktop.json
    title: Actual WKWebView project workflow second launch va log burst
  - id: audit
    resource: ../evidence/m6/ui-audit.md
    title: Scoped source guidelines va inspected native captures
  - id: hosted-arm64
    resource: ../evidence/m6/hosted-macos-arm64.json
    title: Native macOS15 ARM64 DMG GUI va artifact checksum
  - id: hosted-intel
    resource: ../evidence/m6/hosted-macos-x64.json
    title: Native macOS15 Intel DMG GUI va artifact checksum
  - id: hosted-windows
    resource: ../evidence/m6/hosted-windows-x64.json
    title: Native Windows NSIS WebView2 va installer checksum
  - id: support
    resource: ../evidence/m6/support-and-acceptance.json
    title: Exact runtime versions support acceptance va measurements
  - id: attempts
    resource: ../evidence/m6/ci-attempts.json
    title: Meaningful CI failures fixes va pinned source provenance
  - id: production-build
    resource: ../evidence/m6/local-production-build.json
    title: Default-feature local desktop binary shipping state
  - id: single-instance
    resource: https://v2.tauri.app/plugin/single-instance/
    title: Official native single-instance lifecycle contract
---

# Scope va joriy holat

Foydalanuvchi M6 bosqichiga o'tishni topshirdi.[^request] Ichki installable
paketlar, prerequisite, data preservation, explicit system cleanup va qabul
sinovlari shu scope'da. Public release va automatic updater kiritilmadi.
Ichki distribution acceptance uch targetda o'tdi: macOS ARM64, native Intel va
Windows x64. Bu signed public release yoki minimum OS certification emas.
Natijalar, exact runtime versions, checksums va qolgan gate'lar birga beriladi.

# Paket va ownership kontrakti

`package_desktop.py` amaldagi lockfile bilan release CLI/helper sidecar'larini
build qiladi; target dependency'larini oldindan fetch qilib, offline metadata'dan
license matnlari va CycloneDX inventory oladi. Unicode subprocess metadata UTF-8
bilan o'qiladi. macOS uchun app/DMG, Windows uchun per-user NSIS yaratiladi.
DMG headless mode Finder cosmetic automation'ini o'tkazib yuboradi. macOS
ad-hoc signature integrity Developer ID yoki notarization o'rniga o'tmaydi.
Manifest `internal-unsigned` va `public_release_allowed:false` yozadi.[^packaging]

Runtime archive'lari installerga qo'shilmagan: pinned upstream catalog'dan
alohida yuklanadi va mavjud integrity kontrakti bilan tekshiriladi. Ilova update/
reinstall project tanlovlarini yangi runtime versiyasiga avtomatik almashtirmaydi.
Inventory Cargo lock graph va production npm dependencies'ni qamraydi; u binary
link-map yoki yakunlangan redistribution/license review emas. Provider notice,
SBOM va reproducibility gate'lari M0'dan ochiq qoladi.[^contract]

App binary, CLI/helper va OS user data alohida. NSIS template'dan app-data deletion
checkbox, callback va recursive deletion block chiqarilgan. Installer uninstall
loyihalar, DB cluster/data, preferences, runtime selections va OS credential'larini
saqlaydi. Mac app-bundle Finder/Trash orqali o'chirilganda app cleanup hook ishlata
olmaydi; oldin CLI preflight bajariladi.[^contract][^windows]

Desktop exclusive data-root lock va GUI/CLI shared operation lock uninstall oldidan
barcha own sessiyalar yopilishini talab qiladi. CLI/terminal GUI bilan birga
ishlay oladi. Lock kernel tomonidan release qilinadi; stale PIDga tayanilmaydi.
Native single-instance plugin birinchi ulanadi: ikkinchi GUI launch mavjud oynani
show/unminimize/focus qiladi, argumentlarini bajarmaydi va yana supervisor ochmaydi.
Setup'dagi data lock bu detection'dan keyin olinadi. E2E app identifier private data
root hash bilan ajratiladi; driver faqat debug acceptance build'da bor.[^platform][^desktop][^single-instance]

`devforge distribution cleanup-plan <new-dir>` faqat shu installation owner'ining
hosts marker'i va public CA sertifikatini ko'radi. Root private key o'qilmaydi.
Yaratilgan plan/digest tekshirilgach mavjud bounded `devforge-helper apply` explicit
OS elevation bilan ishlatiladi. Cleanup o'zi avtomatik elevation yoki arbitrary
shell ochmaydi. `check-uninstall` active own hosts/CA qolsa rad etadi; malformed
metadata, o'zgargan hosts va digest xatolari closed failure bo'ladi.[^cleanup]

Windows silent directory validation .onInit'da file copy'dan oldin unique temp
file bilan yoza olishni tekshiradi; unwritable target nonzero exit beradi. .onInstFailed
ham failure status'ni saqlaydi. NSIS /D= va _?= final assignment'lari path spaces
bo'lsa ham quote qilinmaydi; runner buni shell ochmasdan maxsus commandline bilan
bajaradi. Native denied-target exit5/no folder va normal retry dalili bor.

# Prerequisite va support chegarasi

macOS konfiguratsiyada minimum13.5; current OS versiyasi read-only tekshiriladi.
Windows native x64, OS build>=22000 va VC v14 x64 Installed flag installerda
tekshiriladi; yo'qligida localized recovery URL beriladi. WebView2 detection va
Evergreen bootstrapper Tauri installer oqimida. Umumiy VC/WebView2 komponentlari
uninstall'da o'chirilmaydi. Missing WebView2 bootstrapper uchun network kerak.
Win Server hosted test minimum Windows11 desktop testining o'rniga o'tmaydi.
Clean-machine prerequisite-absent va minimum OS holatlari alohida unverified.[^windows][^platform]

| Target | Ichki artifact | Native release/UI dalili |
| --- | --- | --- |
| macOS ARM64 | DMG/app | Local macOS27.2.1 Apple M4 va hosted macOS15.8.2 ARM64 release/WKWebView o'tdi. |
| macOS x64 | DMG/app | Native Intel macOS15.7.9 release DMG va WKWebView o'tdi. |
| Windows x64 | NSIS per-user | Windows Server2025 build26100 x64 ordinary-token NSIS va WebView2 acceptance o'tdi. |
| Windows ARM / Linux | MVP target emas | Support da'vosi yo'q. |

# Mavjud real tekshirish

Hosted macOS15 ARM64 va native Intel15.7.9: DMG readonly mount/copy, denied-target
retry, same-version replacement, uninstall/reinstall,2138 existing project/PG files
preservation, ru/uz/en release shell, same-process relaunch va private native GUI
workflow o'tdi. Har bir targetda9 axe holati zero violations;2000 actual queue stdout
lines bounded IPC orqali tekshirildi. ZIP digest GitHub metadata bilan, DMG hash
package manifest bilan mahalliy yuklashda solishtirildi. App source e03eb0c; job va
artifact ID raw report'da. Hosted screen cap/actual viewport alohida yozilgan;
minimum-OS yoki pixel-perfect certification deb talqin qilinmaydi.[^hosted-arm64][^hosted-intel]

Task-local release DMG: read-only install target rad etildi, keyingi normal install
ishladi. ru/uz-Latn/en shell va native menus/tray synchronization, second launch'da
original process saqlanishi, running app uchun preflight block, same-version
replacement, uninstall va undan keyingi reinstall o'tdi. Real isolated PostgreSQL
cluster va registry fayllari snapshot hash bilan taqqoslandi; saqlangan fayl soni
va aniq build/manifest JSON'da. User Herd, hosts/trust yoki boshqa loyihalar
mahalliy testda o'zgartirilmadi.[^local-release]

Native WKWebView real IPC: two-project runtimes/DB/queue/Vite, scoped version
switch, import/error/retry, runtime Remove/Install, close-to-tray, second launch
same PID, Quit/reopen va forced-crash recovery o'tdi. ru/uz/en minimum window,
dark persistence va9 axe holatida zero violations. New real Laravel job stdout'ga
2000 qator yozadi; actual UI/IPC log projection <=500 records va<=256KiB ekanligi
tekshirildi. Native installer dialoglari/screen reader to'liq audit qilindi degan
da'vo yo'q.[^local-gui][^audit]

Core CI va native cleanup'dagi exact source SHA, job counters/conclusions JSON'da.
Source e03eb0c core CI ikkala OS'da56 Rust,13 frontend,3 locale regression va5
distribution-contract test, fmt/clippy/typecheck/native build bilan o'tdi. Keyingi
NSIS argument kontrakti bilan Python suite6 ta testga kengaydi va final Windows
distribution run'da ham o'tdi.
Native credential fixture ignored holati alohida: u bajarildi deb hisoblanmaydi.
Local debug binary yakunda default production features bilan qayta build qilindi;
E2E server/fixture adapter bilan qoldirilmadi.[^production-build][^core-ci][^counters]

System-only cleanup CI ikkala OS'da o'tdi: active entries uninstall'ni blokladi;
yangi CLI ikkita digest-bound removal plan chiqardi; helper exact plans'ni bajardi;
original foreign hosts bytes tiklandi, owned certificate absent/untrusted bo'ldi,
keyingi inventory bo'sh va preflight successful. Mac'da inactive nontrusting admin
metadata qolishi mumkinligi flag bilan ochiq saqlanadi.[^system-ci][^system-reports]

# Yakuniy Windows dalili va source reconciliation

Workflow43167220218 job131011792557 exact6876c95 frozen CI branch bilan o'tdi.
NSIS install/reinstall/uninstall, denied Program Files target exit5/no directory,
default APPDATA/LOCALAPPDATA sentinel'lari va2137 real project/PG files saqlandi.
ru/uz/en release shell, running-app uninstall block va same-process second launch
tekshirildi. Debug WebView2 real workflow/IPC,2000 queue log line,100 Laravel
request,9 axe cases, close-to-tray/relaunch/Quit va forced-crash recovery o'tdi.
No IPC mocking; production OS credential UI o'rniga fixture adapter chegarasi
alohida saqlanadi. ZIP digest va installer hash mahalliy yuklangan bytes bilan
solishtirildi.[^hosted-windows]

Mac jobs e03eb0c'da, Windows packaging6876c95'da o'tdi. Bu commitlar orasida
apps/desktop frontend/Rust source, crates va Cargo.lock diff'i bo'sh; Windows
installer hooks/template hamda runner/CI argument semantics o'zgargan. Old Mac
workflow umumiy failure bo'lgan, chunki undagi superseded Windows job failed;
ikki Mac job alohida success. Final Windows workflow success. Oldingi failures
baseline deb yashirilmagan; sabab/fix/source va actual head mismatch attempts
record'da.[^attempts]

Exact PHP8.4/8.5 variants, Node22/24, Caddy, PostgreSQL va Composer versiyalari
catalog snapshot bilan support record'da; target variants/arxitekturalari
alohida.9 OS/arch/locale kombinatsiyasi o'tdi.[^support]

# Performance usuli

Release report opt-in `--startup-report <new-file>` bilan preferences/localization
va ikki animation frame'dan keyin yoziladi. HTTP test endpoint yo'q. Cold degani
har locale uchun processning birinchi launch'i; OS disk caches flush qilinmagan.
`shell_ready_ms` Rust run entry'dan; `parent_wall_ms` process spawn va100ms
polling intervalini ham qamraydi. CPU5s sample, RSS/working set faqat desktop main process: WebView helpers va
managed PHP/Node/PostgreSQL bu sonlarga kirmaydi. Hardware/OS/build JSON'da.
M0/M1'dan tasdiqlangan raqamli SLA/budget topilmadi; bular boshlang'ich baseline,
p95 yoki Rust performance kafolati emas.[^local-release]

Download/install,2000 queue log line va100 real Laravel request vaqtida native
frontend frame gap'lari debug-E2E binary'da o'lchandi. Bular input-to-paint yoki
release UX latency deb ko'rsatilmaydi. Local extended GUI report'da barcha uch
workload va actual duration/samples bor.[^local-gui]

# Phase reconciliation va qolgan gate'lar

- **Mos:** source/data ownership, pinned runtimes, three-locale registry, per-project serving va secret boundary saqlangan.
- **O'zgargan:** installable artifacts, bundled CLI/helper, per-user safe NSIS, uninstall plans/guards va second-launch restoration qo'shilgan.
- **Amalga oshirilmagan:** public release, automatic updater, cross-version installer migration/rollback strategy; birinchi alpha uchun same-version replacement tekshirildi.
- **Tekshirilmagan:** minimum macOS13.5 va Windows11 desktop; clean-machine missing prerequisite/network-failure va interrupted-install/power-loss cases; native credential/UI dialogs va optional external editor/file-picker; Mac inactive admin trust metadata jismoniy removal; full provider redistribution/license review.

Root project distribution license hali tanlanmagan; dependency inventory buni
avtomatik hal qilmaydi. Signing identity/credentials mavjud emasligi read-only tekshirildi. Signed public
handoff uchun Developer ID/notarization va Windows Authenticode dalillari alohida
kerak. Unsigned internal acceptance shu gate'lar yopildi degani emas.

[^request]: Joriy M6 ko'rsatmasi; oldingi code/test/OKF/push ketma-ketligi saqlanadi.
[^packaging]: Toolchain va dependency versiyalari amaldagi manifest/lockfile'lardan.
[^acceptance]: Release va debug-E2E qamrovlari alohida.
[^contract]: Ownership va internal/public distribution chegaralari.
[^platform]: Kernel locks active GUI/CLI sessiya umriga bog'langan.
[^cleanup]: Inventory va plan creation privileged apply'dan alohida.
[^desktop]: Single-instance2.5.5, Tauri2.12-compatible; startup optional telemetry.
[^windows]: Template provenance va English/Russian/Uzbek custom messages repoda.
[^core-ci]: Per-OS status source/build/check dalilidir, installer dalili emas.
[^counters]: Dependency crate unit testlari qo'shib hisoblanmaydi.
[^system-ci]: Faqat disposable hosted VM system state o'zgartirildi.
[^system-reports]: Old-new inventory, preservation va removal natijalari.
[^local-release]: Main-process-only measurements va persisted data hash assertions.
[^local-gui]: Debug-only private driver; no IPC mocking; fixture SecretStore alohida.
[^audit]: Incumbent layout saqlangan, yangi visual redesign yoki 1:1 claim yo'q.
[^single-instance]: Native plugin birinchi initialize bo'lishi va existing-window callback kontrakti.

[^hosted-arm64]: Workflow43162493214 job130996693536; internal DMG hash verified.
[^hosted-intel]: Workflow43162493214 job130996693351; x64 native ISA, Rosetta natijasi emas.

[^hosted-windows]: Native Server2025, physical Windows11 minimum emas; ordinary token groups/privileges disabled harness.
[^support]: Catalog versions va actual workload/build measurements snapshot.
[^attempts]: Successful job claims actual tested source'ga bog'langan; failed runs ham saqlangan.

[^production-build]: Build-only shipping check; passed suites qayta ishga tushirilmadi.
