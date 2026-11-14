---
okf_version: "0.2"
---

# DevForge bilim bazasi

Ushbu OKF bundle 2026-11-11 kungi DevForge suhbatidan tuzildi. Avval kerakli
mavzuni tanlang, so'ng faqat tegishli hujjatlar va ularning manbalarini o'qing.

## Qarorlar va yo'nalishlar

- [Mahsulot chegarasi](product-scope.md) - macOS va Windows uchun desktop dastur, loyiha nomi va repozitoriy.
- [Arxitektura yo'nalishi](architecture.md) - Rust/Tauri stack va yadro, interfeys, CLI hamda OS adapterlari chegaralari.
- [Loyiha muhitlari](project-environments.md) - loyiha bo'yicha PHP, Node.js va DB tanlovi, umumiy paketlar va izolyatsiya.
- [Muhandislik siyosati](engineering-policy.md) - AI bilan ishlab chiqish, vaziyatga mos SOLID/OOP/pattern tanlovi va versiya siyosati.
- [UI/UX siyosati](ui-ux-policy.md) - to'rtta UI skill'ning vazifasi, ishga qarab ketma-ketligi va desktop tekshiruvi.
- [Ko'p tillilik](localization.md) - rus, o'zbek va ingliz tili talabi hamda yangi tillar uchun kengayadigan i18n kontrakti.

## Taklif va ochiq masalalar

- [Birlashtirilgan bajarish rejasi](integrated-execution-plan.md) - yetti rol va tashqi agentlar rejasi, aniq dependency, parallel sub-agentlar, maqsadli test siyosati va Mac mini UI skill routing.

- [M13 tashqi agentlar integratsiyasi](m13-external-agents-plan.md) - MCP/JSON CLI, yagona manager, imkoniyatlar va M13.0–M13.9 qabul rejasi; ichki AI chat/provider ulash scope'da emas.

- [M6 dan keyingi roadmap](post-mvp-roadmap.md) - yangi M7 debugging/profiling bilan boshlanadi; yakuniy beta/tarqatish M15'ga ko'chirilgan, eski-yangi raqamlar mosligi berilgan.

- [MVP birinchi bosqich rejasi](mvp-phase-1-plan.md) - macOS va Windows hamda rus, o'zbek va ingliz tillari uchun bosqichlar, natijalar va qabul mezonlari.
- [Birinchi MVP taklifi](mvp-proposal.md) - tavsiya etilgan birinchi ish jarayoni, funksiyalar va hali tasdiqlanmagan chegaralar.

## Manba va yuritish

- [M13.4 reviewed actions](../architecture/reviewed-service-actions.md) - exact native approval bilan service, runtime, env/YAML va DB bind/restore; domain/HTTPS keyingi kesim.

- [M13.3 stdio MCP](../architecture/mcp-stdio-contract.md) - 5 read-only tools, bound summary resource, modern/legacy protocol va actual client evidence.

- [M13.2 JSON CLI](../architecture/json-cli.md) - versioned envelope, no-write help, bounded credential stdin va 6 cheklangan live buyruq.
- [External control boundary](../architecture/manager-external-control.md) - grant-derived project/client, native admin ajratilishi va own-operation isolation.

- [M13.1 native GUI cutover](native-manager-gui-results.md) - actual WebView13 checks, one owner/two clients, Close/Quit va target gates.

- [Native bridge prerequisite](../architecture/manager-native-bridge.md) - Rust-only client state va incremental snapshot; native handler adoption natijasi alohida qayd etilgan.
- [Native subject advice](../architecture/manager-subject-advice.md) - IDs-only advisory lookup, admission/execution authority saqlanadi.
- [Debug E2E composition](../architecture/cli-e2e-fixture-composition.md) - explicit feature/flag, bound fresh fixture root va production fallback yo‘qligi.

- [E05.2 cleanup preview](cleanup-preview-results.md) - physical-key refs, read-only lease observation va E06 retained receipt inventory; apply/delete yo‘q.

- [Private desktop facade](../architecture/manager-desktop-facade.md) - owner read/access, bounded images, ephemeral polling, operations va trusted recovery.
- [Thin-client migration gates](../architecture/manager-desktop-migration.md) - 20 handler parity, implemented client lease va qolgan UI/auth/native adoption.

- [CI validated baseline](../architecture/ci-validated-baseline.md) - failed/queued commitsdan qolgan scope yo‘qolmasligi, ikki platforma successful ancestor va FULL fallback.

- [Atomic settings patches](../architecture/atomic-settings-patches.md) - independent GUI/manager preference writers va None-only folder initialization.

- [External grant foundation](../architecture/external-grants.md) - owner-issued project scopes, independent information rights, hash-only credentials va durable revoke/expiry.

- [Terminal snapshots](../architecture/terminal-snapshots.md) - exact foreground shell coexistence, private runtime config va frozen metadata.

- [E02 environment metadata](../architecture/environment-report.md) - repository expected versus registered configuration, portability va explicitly unverified health.

- [E06.1 restore drill](restore-drill-results.md) - activation qilmaydigan DB restore verification, durable receipts va retained cleanup.

- [Bundled manager authority](../architecture/bundled-manager-authority.md) - compiled exact CLI digest, macOS signing order va final-bundle verification.

- [E01 first-success checklist](e01-first-success-results.md) - existing project detail, truthful last-observed/manual states va scoped RU/UZ/EN browser evidence.

- [E07.1 offline metadata](../architecture/offline-readiness.md) - exact pin, bounded receipt/file evidence va unknown full-offline readiness.

- [E10 framework compatibility](companion-compatibility-results.md) - actual Laravel12/13 Collector/mail, Laravel11 fresh-resolution advisory blocker, no guard bypass va unverified matrix.

- [E05.1 storage inventory](storage-inventory-results.md) - read-only ownership/logical size/refs, bounded registry/FS metadata va keyingi cleanup gate.

- [Configure preflight refusal](configure-preflight-results.md) - invalid name/debug PHP-change sessionni stop qilmasligi; actual native red-green va target chegaralari.

- [E03 exact Composer](composer-binding-results.md) - schema8, persisted selection, terminal/YAML/worktree va credential-free tooling; focused test/native chegara.

- [Execution holati va E00 inventari](execution-status.md) - source/platform evidence, barcha work ID holati, scoped test va qoldiq tashqi gate.

- [Windows 10/11 mosligi](windows-compatibility.md) - 22H2 x64 minimum, installer/preflight va actual Windows10 qabul chegarasi.

- [M7–M12 completion](m7-m12-completion.md) - qolgan funksional imkoniyatlar yakuni, uch native target natijalari, saqlangan source va M15 chegaralari.

- [M7.2 Xdebug](m7-xdebug-results.md) - optional native runtime, interactive debugger va function profiles; uch native target qabuli, artifact/source mapping va M15 chegaralari.
- [Avtomatik loyiha aniqlash](project-discovery.md) - default `~/Devforge`, sozlanadigan root, safe discovery/registry; ARM64/Intel/Windows actual backend va native UI qabuli, hashlar; main/M7 va joriy upstream katalog integratsiyasi ham native qabuldan o‘tdi.

- [M12 workspaces](m12-workspaces-results.md) - reviewed Git worktree projectlari, alohida muhit/DB va arxivlovchi cleanup; ARM64/Intel/Windows native backend/CLI/UI qabuli va source chegarasi bilan.

- [M11 mail va loglar](m11-mail-logs-results.md) - per-project local SMTP, safe MIME preview va structured log/exception filters; ARM64/Intel/Windows backend va native UI qabuli, source chegarasi va ochiq release gate‘lari bilan.

- [M10 takrorlanadigan muhit](m10-reproducible-results.md) - portable YAML, exact pins, local secret links, reviewed CLI setup va recovery;3 native target qabul va scoped cheklovlar bilan.

- [M9 baza va xizmatlar](m9-services-results.md) - MySQL, per-project Garnet, Laravel service profiles, staged backup va retained connections;3 native target qabul, Intel MySQL va M15 gate chegaralari bilan.

- [M8 loyihani tayyorlash](m8-project-setup-results.md) - Laravel/Git wizard, PHP/env/dependency setup, recent/favorites va HTTPS review uch native targetda o'tdi; system-dialog/release gate'lari alohida.

- [M7 Inspector](m7-inspector-results.md) - M7.1 Laravel capture/profiling uch native targetda o'tgan tarixiy dalil; M7.2 yakuni alohida yuqorida.

- [M6 distributiv va qabul](m6-distribution-results.md) - ichki paketlash, data preservation, Windows tester HTML qo'llanmasi va ochiq release gate holati.
- [M5 kundalik boshqaruv](m5-daily-workspace-results.md) - real desktop controls, project lifecycle va macOS/Windows native GUI dalillari.
- [M4 domen va serving](m4-serving-results.md) - domain/HTTPS/hosts helper, Caddy/PHP/Vite orchestration va macOS Chrome/Windows Edge native dalillari.
- [M3 loyiha muhitlari](m3-project-environments-results.md) - import, scoped PHP/Node/Composer, PostgreSQL DB/role, secret references va explicit env patch dalillari.
- [M2 runtime yadrosi](m2-runtime-core-results.md) - paket store, cancellation, process ownership/readiness/restart/loglar, CLI va platforma sinovlari.
- [M1 desktop natijalari](m1-foundation-results.md) - Rust/Tauri poydevori, uch tilli UI, SQLite/keychain, bajarilgan test va native macOS dalillari hamda ochiq platforma gate'lari.
- [M0 runtime natijalari](m0-runtime-findings.md) - macOS va Windows paketlari, haqiqiy bajarish dalillari, cheklovlar va M0 ochiq gate'lari.
- [Bilim bazasini yuritish](knowledge-policy.md) - OKF tuzilishi, qaror holatlari, manba, yangilash va tekshirish qoidalari.
- [OKF validator](validation-tooling.md) - read-only tekshirish, strict rejim, o'rnatish, testlar va qamrov cheklovlari.
- [Suhbat manbasi](references/conversation-decisions.md) - foydalanuvchi ko'rsatmalari va assistant takliflarining tanlangan qaydi.
- [O'zgarishlar tarixi](log.md) - mazmunli bilim yangilanishlari.

## Holatni to'g'ri talqin qilish

`decision_status` loyiha kengaytmasi:

| Qiymat | Ma'nosi |
| --- | --- |
| `confirmed` | Foydalanuvchining aniq ko'rsatmasi yoki bevosita tasdiqlangan ish qoidasi. |
| `working-direction` | Suhbat va amaldagi skill'dagi ishchi tavsiya; alohida yakuniy tasdiq deb talqin qilinmaydi. |
| `proposed` | Taklif; bajarish yoki mahsulot hajmini kengaytirishga ruxsat bermaydi. |

OKF `status: draft` dastlabki hujjat inson tomonidan ko'rib chiqilmaganini
bildiradi. Bu `decision_status` bilan bir xil emas. `verified` metama'lumoti
kiritilmadi: yozuvlar inson tekshirgan deb ko'rsatilmaydi.

Bundle yaratilgan paytda repozitoriyda AGENTS.md va skill fayllari bor edi;
ilova manifestlari, runtime boshqaruvi yoki tayyor installer mavjudligi
aniqlanmadi. Quyidagi hujjatlar mahsulotning ishlayotganini isbotlamaydi.
