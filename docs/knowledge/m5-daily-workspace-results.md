---
type: Implementation Report
title: M5 kundalik boshqaruv interfeysi
description: Project lifecycle coordinator, native desktop workspaces va M5 verification holati.
status: draft
decision_status: confirmed
tags: [m5, desktop, projects, lifecycle, ui, evidence]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: M5 bosqichini bajarish topshirig'i
  - id: coordinator
    resource: ../../crates/workspace/src/lib.rs
    title: Framework-independent daily workspace orchestration
  - id: ui
    resource: ../../apps/desktop/src/pages/WorkspacePage.tsx
    title: Real project runtime service log controls
  - id: bridge
    resource: ../../apps/desktop/src-tauri/src/workspace.rs
    title: Worker-thread IPC cache cancellation va graceful Quit
  - id: native
    resource: ../../scripts/m5_desktop_smoke.py
    title: Native WKWebView WebView2 UI assertions
  - id: local-native
    resource: ../evidence/m5/macos-native-workspace.json
    title: Task-local real Laravel queue Vite two-project results
  - id: local-desktop
    resource: ../evidence/m5/macos-native-desktop.json
    title: Native WKWebView workflows and nine accessibility cases
  - id: core-ci
    resource: ../evidence/m5/ci-final-checks.json
    title: Final source macOS Windows core tests and native binary builds
  - id: metrics
    resource: ../evidence/m5/test-metrics.json
    title: Per-OS Rust frontend locale test counters
  - id: serving-ci
    resource: ../evidence/m5/ci-serving-regression.json
    title: Preserved native M4 HTTPS Laravel serving contract
  - id: audit
    resource: ../evidence/m5/ui-audit.md
    title: Scoped Web Interface Guidelines and visual audit
  - id: hosted-macos
    resource: ../evidence/m5/hosted-macos-workspace.json
    title: Real hosted macOS backend and WKWebView acceptance
  - id: hosted-windows
    resource: ../evidence/m5/hosted-windows-workspace.json
    title: Real hosted Windows backend and WebView2 acceptance
  - id: native-ci
    resource: ../evidence/m5/ci-native-checks.json
    title: Final native workflow step conclusions
  - id: attempts
    resource: ../evidence/m5/ci-attempts.json
    title: Pause and meaningful CI/capture regressions
  - id: production-build
    resource: ../evidence/m5/local-production-build.json
    title: Local default-feature native binary without E2E instrumentation
  - id: capture-ci
    resource: ../evidence/m5/ci-capture-check.json
    title: Windows capture-only workflow extension and artifact upload
  - id: windows-captures
    resource: ../evidence/m5/windows-capture-verification.json
    title: Preserved Windows native reports and screenshot metadata
  - id: target
    resource: ../../design-system/devforge/pages/projects.md
    title: M5 visual target and incumbent system decisions
---

# Scope va implementatsiya

Foydalanuvchi M5 bosqichini bajarishni topshirdi.[^request] Oldingi yetkazish
ketma-ketligi bo'yicha code/test/OKF va push davom ettiriladi; M6 installer,
signing yoki boot auto-start bu topshiriqqa kirmaydi.

`crates/workspace` real project/runtime/database services'ni birlashtiradi.
Tauri blocking ishlarni worker'da bajaradi; UI snapshot cached, busy va ready
holatlari ajratilgan. Project serving sessiyalari alohida; bitta project Stop
boshqa project Caddy/PHP/Vite/queue yoki shared PostgreSQL'ni stop qilmaydi.
Database Stop active consumer borida rad etiladi. Quit faqat app-owned
sessiya va PostgreSQL'ni graceful to'xtatgach chiqadi.[^coordinator][^bridge]

Projects/details, Runtimes, Services/logs, Settings va Diagnostics bor.
Import read-only, configure running project'ni scoped restart qiladi.
Runtime install/remove/cancel/verify, DB assign/test, own queue va Vite toggles,
actual log filters, browser/VS Code/folder/terminal launch adapterlari ulangan.
Error, empty, loading, disabled, retry va confirmation holatlari uch katalogdan
olinadi. Raw log/path/name/versions tarjima qilinmaydi.[^ui]

Mavjud MASTER.md identity saqlandi. Generated target1487×1058 content pixels;
placeholder data/version/log timestamps real dalil deb ishlatilmadi. Native
same-size va820×620 capture/interaction qamrovi natijalarda ajratiladi.[^target]

# Test va platforma dalillari

Task-local macOS ARM64: Laravel13.42.0 ikki project va alohida PG databases,
real queue job, Vite/hot lifecycle, PHP switch, database consumer guard, log
redaction va reopen persistence o'tdi. FPM ondemand child PID tabiiy almashadi;
isolation testi B master process identity'sini solishtiradi.[^local-native]

Native WKWebView, real IPC: path import validation/retry/input preservation,
runtime Remove/Install, DB Create/Assign/Test, parallel Start/Stop, version
switch, native CloseRequested-to-tray, explicit Quit/reopen va forced app-crash
cleanup/recovery Vite yoqilgan holda o'tdi. Russian/Uzbek/English, light/dark
persistence va minimum window horizontal-overflow tekshirildi.[^local-desktop]

Native axe WCAG2 A/AA va2.1 AA tekshiruvlari9 holatda zero violations: import
error dialog; Runtimes light; Services light; Projects light; minimum Projects
ru/uz/en; Projects dark; Services dark. Bu barcha native screen-reader/OS-dialog
holatlariga certification emas.[^audit]

Final app/test source `e501b29` core workflow43149466472 ikkala OS'da passed:53 Rust,
13 frontend va3 locale regression testi,3 katalog coverage, typecheck,
fmt/clippy va embedded frontend bilan native desktop build. Native credential
fixture1 ignored; u bajarildi deb hisoblanmaydi.[^core-ci][^metrics]

Serving regression workflow43147311309 ikkala target'da passed: M4 HTTP/TLS,
CA/hosts permission boundary, secure cookie/upload va WSS HMR saqlangan. Tested
Rust source cb236b3 va final source orasida faqat UI CSS/harness o'zgargan;
serving/runtime Rust farqi yo'q.[^serving-ci]

Native workflow43149548247 ikkala OS'da passed. Mac ARM64 WKWebView va Windows
x64 WebView2 actual window/IPC bilan to'liq UI siklini bajardi: real runtime
Remove/Install, DB Create/Assign/Test, project controls/queue/Vite, B master PID
preservation, CloseRequested-to-tray, normal Quit/reopen, forced app termination
cleanup va recovery, ru/uz/en, dark persistence hamda raw log filtering.
Har bir target'da9 axe holati zero violations bilan o'tdi.[^hosted-macos][^hosted-windows][^native-ci]

Hosted Mac display requested1487×1058 content'ni1487×645 bilan chekladi; test
actual size va matched=false'ni yozdi. Target-size fidelity u yerda da'vo
qilinmaydi. Local Mac va Windows target1487×1058 content'ga yetdi. Minimum native
window820×620; titlebar/border sabab content viewport Mac820×588, Windows804×561
bo'lishi mumkin. Actual dimensions/scale report'larda; scroll/overflow assertion
bu physical viewport'da o'tdi.[^local-desktop][^hosted-macos][^hosted-windows]

Final local binary default production features bilan qayta build qilindi:
`target/debug/devforge-desktop` E2E server/fixture SecretStore bilan qoldirilmadi.
Bu debug, unsigned/no-bundle build; signed distributive emas.[^production-build]

Windows captures workflow-only e430e4d commit'da qayta saqlandi; app/test code
e501b29 bilan bir xil. Upload allowlist faqat PNG va public result JSON oladi,
fixture credentials/datadir olinmaydi. Large Windows capture Working holatini
ushlagan; stable local target-size va Windows minimum/Services captures alohida
ko'rildi. Ready-state Windows pixel-fidelity da'vosi yo'q.[^capture-ci][^windows-captures]

Test-only filesystem SecretStore production keychain/credential UI o'rniga
o'tmaydi. Hosted Windows Server2025 va macOS hosted target real OS/WebViews'dir;
physical Windows10/11, minimum OS va native Intel coverage alohida M6 gate.
No IPC mocking; screenshot/compile natijasi native workflow assertion'ini
almashtirmaydi.[^native]

# Lifecycle va verification chegarasi

macOS owned process group uchun private stdin-pipe watchdog app o'lganda cleanup
qiladi; normal Stop uni release qiladi. Windows Kill-on-close Job ishlatadi.
Vite hot file crash recovery faqat private owner receipt, unchanged file stamp/
URL, per-project lock va free private listener holatida amalga oshadi; foreign
hot file yoki live port saqlanadi. App reopen auto-start qilmaydi.

E2E feature debug-only; release instrumentation rad etiladi. Isolated data root
majburiy, WebDriver127.0.0.1'da. IPC mock qilinmaydi. Page-load marker faqat
test build'da; production build'da HTTP test endpoint va fixture SecretStore yo'q.
GUI observations source/unit/browser-preview bilan aralashtirilmaydi.

# Phase reconciliation

- **Mos:** own resource/process boundary, shared project context va three-locale registry.
- **O'zgargan:** M4 foreground-all CLI bilan birga per-project desktop lifecycle bor.
- **Tekshirilmagan:** minimum OS/Intel, native credential UI, file-picker interaction va optional external app availability alohida gate.
- **Amalga oshirilmagan:** M6 signed installers, boot auto-start, long-term leaf renewal va physical trust metadata cleanup.

[^request]: Joriy M5 ko'rsatmasi.
[^coordinator]: Shared services va per-project ownership invariants.
[^bridge]: Mutex serialization, nonblocking cached status, cancellation va Quit.
[^ui]: Typed real backend commands; illustrative frontend mocks faqat unit fixture'larida.
[^native]: Actual native WebViews; isolated acceptance build'dan tashqarida driver yo'q.
[^target]: ui-ux-pro-max, ui-taste/uizze-ui target/craft va scoped source review.

[^local-native]: Actual task-local package/queue/database assertions; user Herd/hosts/trust o'zgarmagan.
[^local-desktop]: Native window/IPC with isolated fixture credentials; captures record actual content viewport and scale.
[^core-ci]: Final-source full check/build matrix,0 failed.
[^metrics]: Job logs:53 Rust/13 frontend/3 locale per OS, credential fixture ignored alohida.
[^serving-ci]: Platform-sensitive serving regressions and cleanup passed on both disposable hosts.
[^audit]: Scoped source review, visual comparison, accessibility findings/fixes and remaining boundaries.

[^hosted-macos]: Final-source Mac backend and real WKWebView workflow, display cap clearly recorded.
[^hosted-windows]: Final-source Windows backend and real WebView2 workflow; nine axe cases passed.
[^native-ci]: Final source workflow43149548247, both target job conclusions success.
[^attempts]: Cancelled user pause and the hosted capture-size limitation are separate from product failures.
[^production-build]: Local binary uses production defaults; no E2E endpoint, no fixture credentials; release signing not claimed.

[^capture-ci]: Windows-only artifact-preservation workflow passed; product code changed emas.
[^windows-captures]: Downloaded native results and inspected Windows images; busy-state boundary ochiq.
