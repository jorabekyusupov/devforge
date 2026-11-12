---
type: Implementation Plan
title: DevForge MVP birinchi bosqich rejasi
description: macOS va Windows hamda rus, o'zbek va ingliz tillari uchun bosqichlar, natijalar va qabul mezonlari.
status: draft
decision_status: proposed
tags: [mvp, implementation-plan, macos, windows, i18n, acceptance]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: MVP uchun aniq reja tuzish topshirig'i
  - id: windows-php
    resource: https://www.php.net/downloads.php?os=windows
    title: Windows PHP builds and requirements
  - id: macos-php
    resource: https://www.php.net/manual/en/install.macosx.php
    title: PHP installation on macOS
  - id: caddy-php
    resource: https://caddyserver.com/docs/caddyfile/directives/php_fastcgi
    title: Caddy PHP FastCGI directive
  - id: caddy-https
    resource: https://caddyserver.com/docs/automatic-https
    title: Caddy local HTTPS and trust
  - id: postgres
    resource: https://www.postgresql.org/download/
    title: PostgreSQL platform packages
  - id: i18next
    resource: https://react.i18next.com/
    title: React i18next integration
  - id: packaging
    resource: https://v2.tauri.app/distribute/
    title: Tauri distribution and signing
---

# Maqsad va hujjat holati

Foydalanuvchi macOS, Windows va uch tilni boshidan hisobga olgan aniq MVP rejasini
so'ragan.[^request] Natija: mavjud Laravel loyihasini qo'shish, o'z PHP/Node/DB
muhitini biriktirish va lokal HTTPS orqali ishlatish mumkin bo'lgan desktop MVP.

Reja tuzilgan paytda scope va texnik yechimlar implementation taklifi edi.
Keyingi alohida M0/M1 topshiriqlari tegishli ishlarni vakolatladi; to'liq MVP
yoki ommaviy release tayyorligi bundan kelib chiqmaydi.

**Keyingi holat:** foydalanuvchi M0 bajarilishini alohida topshirdi.
[M0 runtime natijalari](m0-runtime-findings.md) va evidence'lar yozildi. M0 to'liq
yopilmagan. Keyin M1 alohida topshirildi: [desktop poydevori va natijalari](m1-foundation-results.md)
yozildi; macOS ARM64 native tekshiruvi bor, Windows/Intel gate'lari ochiq.
[M2 runtime yadrosi](m2-runtime-core-results.md) va uning mavjud hostdagi
sinovlari yaratildi; ochiq qabul gate'lari hisobotda ajratilgan.
[M3 loyiha muhitlari](m3-project-environments-results.md) implementation va mavjud
host sinovlari ham qo'shildi. [M4 serving backend](m4-serving-results.md)
yaratildi; Mac Chrome va Windows Edge native acceptance, full test/build va
actual port/trust cheklovlari shu hisobotda qayd etildi. [M5 kundalik boshqaruv](m5-daily-workspace-results.md) va actual macOS/Windows
native GUI/backend acceptance qo'shildi. M6 hali bajarilmagan. Quyidagi bosqich ta'riflari natijalar bilan aralashtirilmaydi.

# 1. MVP chegarasi

| Kiradi | Birinchi bosqichdagi aniq chegara |
| --- | --- |
| Desktop | Tauri + React/TypeScript, Rust yadro, SQLite metama'lumotlari. |
| Platformalar | Taklif etilayotgan targetlar: macOS arm64 va x64, Windows x64; har biriga native build va runtime dalili. |
| Tillar | Rus, o'zbek va ingliz; registry, kataloglar, Settings tanlovi va fallback birinchi ishlaydigan oynadan mavjud. |
| Loyihalar | Mavjud Laravel papkasini import qilish, tekshirish, tahrirlash, ro'yxatdan chiqarish. |
| PHP | Dastlab ikkita mos supported stable versiya; per-project tanlov, Composer va Artisan. |
| Node.js | Dastlab ikkita supported LTS versiya, npm va Vite; qo'shimcha package manager'lar keyin. |
| DB | Bitta supported PostgreSQL major; bitta DevForge instance'ida har loyiha uchun alohida baza va cheklangan role. |
| Web | Caddy ishchi nomzod, per-project .test domeni, HTTP/HTTPS, Laravel public root va Vite HMR. |
| Jarayonlar | PHP/Vite va ixtiyoriy bitta queue worker; start/stop/restart, readiness va loglar. |
| Tarqatish | macOS DMG/app va Windows installer, toza mashinada birinchi ishga tushirish sinovi. |

Windows ARM, boshqa DB dvigatellari, Redis, mail viewer, SQL/dump inspector,
worktree/DB cloning, MCP, plugin market, auto-update, wildcard park, yangi
Laravel yaratish generatori va ommaviy release bu MVP scope'iga kirmaydi.
Keyingi scope foydalanuvchi talabi bilan kengayishi mumkin.

Minimal OS versiyalari va aniq dependency/runtime versiyalari M0'da real
paketlar bilan tanlanib, manifest va support matritsasiga yoziladi. Bu
tanlanmaguncha "barcha macOS/Windows versiyalari" va'da qilinmaydi.

# 2. Boshlang'ich texnik yechimlar

| Masala | Rejadagi ishchi tanlov | Majburiy tekshirish |
| --- | --- | --- |
| Web server | Caddy; Rust dasturi uni alohida boshqariladigan binary sifatida ishlatadi. | Laravel routing, static fayl, FastCGI, HTTPS va Vite WebSocket/HMR. |
| PHP serving | macOS PHP-FPM, Windows NTS PHP-CGI/FastCGI adapteri. | CLI/web versiya va konfiguratsiyasi mosligi, pdo_pgsql va kerakli extension'lar, parallel loyihalar. |
| Runtime tarqatish | OS/arch/version katalogi, checksum va litsenziyasi aniqlangan binarlar. | Toza mashinada global Herd/Homebrew/Node/PHP'ga tayanmasdan ishlashi. |
| Local domen | DevForge boshqaradigan aniq hosts yozuvlari; boshlang'ich domen project.devforge.test. | Mavjud yozuvlar bilan konflikt va aniq ownership; wildcard DNS dastlab kerak emas. |
| HTTPS | Lokal CA va DevForge egaligidagi sertifikat/trust amallari. | OS/browser trust, bekor qilingan elevation va xatodan tiklanish. |
| i18n | i18next + react-i18next va Intl ishchi nomzodi; umumiy locale registry. | Kataloglar, plural, interpolation, live switch, shell menu label'lari va target WebView mosligi. |
| Holat | UI, Rust yadro va supervisor o'rtasida typed command/event/error code. | Request qabul qilinishi "ready" deb ko'rsatilmasligi, xatolar va cancel/stop semantikasi. |

Caddy PHP FastCGI va lokal HTTPS'ni qo'llaydi; trust o'rnatish avtomatik ravishda
har qanday OS/browserda muvaffaqiyatli bo'ladi deb hisoblanmaydi.[^caddy-php][^caddy-https]
Windows PHP uchun NTS/FastCGI va Visual C++ runtime talablari bor; macOS'da
PHP paket manbasi yoki build yo'li alohida hal qilinadi.[^windows-php][^macos-php]
Bu tanlovlar M0'dan o'tgach implementation qarori sifatida qayd etiladi.

# 3. Bajarish ketma-ketligi

M0 -> M1 -> M2 -> M3 -> M4 -> M5 -> M6.
Har bosqich ikkala OS va uch tilni tegishli qatlamda qamrab oladi. Birinchi
platformada hammasini tugatib, ikkinchisini keyin port qilish rejalashtirilmaydi.

## M0. Paket va platforma imkoniyatini isbotlash

**Ishlar:**

- Har uch OS/arch target uchun build muhiti va native test runner/mashinasini belgilang.
- PHP'ning ikki, Node'ning ikki va PostgreSQL'ning bir versiyasi uchun manba,
  dependency, checksum/signature va tarqatish shartlarini tekshiring. PostgreSQL
  turli platforma paketlari mavjud, lekin aniq qayta paketlash yo'li tekshirilishi
  kerak.[^postgres]
- Ayniqsa macOS PHP paketlarining relocatability va extension dependency'larini
  aniqlang. Ishonchli tayyor artifact yetmasa, o'z CI build/packaging yo'lini
  isbotlang; foydalanuvchi kompyuterida yashirin Homebrew install'ga o'tmang.
- Har targetda kichik Laravel fixture'ni PHP + Caddy + PostgreSQL bilan ishga
  tushiring; Node/npm orqali asset build va HMR yo'lini tekshiring.
- Native build uchun minimal OS/WebView/VC runtime matritsasini belgilang.
- Boshlang'ich o'zbek lotin/uz-Latn, system-locale negotiation va ingliz fallback
  ishchi defaultlarini qayd eting; rus/ingliz/o'zbek talabi o'zgarmaydi.

**Natija:** runtime compatibility jadvali, minimal OS/arch jadvali va nomzod
paketlar bilan smoke dalillari. Bu bosqich UI chizishdan oldin tarqatish xavfini
aniqlaydi. Windows yoki Intel target dalili yo'q bo'lsa, o'sha target ochiq qoladi;
reja uni tasdiqlangan deb belgilamaydi.

**Chiqish mezoni:** har targetda runtime zanjiri ishlaydi yoki konkret blocker
va uni hal qilish yo'li qayd etilgan. Paket manbasi isbotlanmagan target uchun
release-ready scope bilan davom etilmaydi.

## M1. Repozitoriy, desktop shell va i18n poydevori

**Ishlar:**

- Cargo workspace, Tauri frontend va tekshiruv buyruqlarini yarating. Yadro,
  platform adapterlari, supervisor va shell/CLI chegaralarini minimal ajrating.
- macOS/Windows CI matrix: Rust fmt/clippy/test, frontend typecheck/build,
  OKF strict validator va artifact build. CI remote workflow'larini ulash
  implementation vazifasi doirasida amalga oshiriladi; hozir ulanish yo'q.
- SQLite schema/migration, OS app-data yo'llari va secret store adapterini yarating.
- Birinchi Settings oynasidan locale registry va en/ru/uz kataloglarini ishlating.
  User tanlovi saqlansin, fallback va locale-aware formatter ishlasin.
- UI skill routing bo'yicha dizayn tizimi va kerakli vizual nishonni tayyorlang;
  keyin haqiqiy semantic UI'ni yozing. Uch til uchun uzun matn boshidan hisobga olinsin.

**Natija:** har targetda ishga tushadigan desktop shell, Settings va diagnostika
oynasi. i18next React tarjima qatlamiga nomzod sifatida ishlatiladi; native
menu/tray shu canonical kataloglardan olinadi, qo'lda parallel tarjima ro'yxati
yaratilmaydi.[^i18next]

**Chiqish mezoni:** uch tilda switch/re-launch/fallback testlari, accessibility
nomlari va test-only to'rtinchi locale'ni registry+katalog bilan qo'shish sinovi.
App tili almashtirilganda hech qanday loyiha servisi qayta ishga tushmaydi.

## M2. Runtime paketlari va jarayon yadrosi

**Ishlar:**

- Paket identifikatori: kind/version/OS/arch; install, staging, checksum,
  activate va failure recovery holatlarini yarating.
- Download progress/cancel, buzilgan arxiv, disk to'lishi, noto'g'ri arxitektura
  va takroriy install holatlarini boshqaring. Error code'lar uch tilda ko'rsatilsin.
- Jarayonlarni executable+argument list+env bilan boshlang; readiness probe,
  grace-period stop, owned process-tree cleanup va bounded restart qo'shing.
- Bir loyiha uchun PHP serving jarayoni ajratilsin; bir xil binary paketi umumiy
  bo'lishi mumkin. Windows job/process-tree va macOS process-group farqlari adapterda.
- Log queue/buffer va fayl rotation chegaralarini belgilang.

**Natija:** test fixture'lar va CLI smoke orqali tekshiriladigan runtime manager.
UI hali tugallanmagan bo'lsa ham yadro mustaqil ishlaydi.

**Chiqish mezoni:** toza mashinada tanlangan paketlar ishlaydi; start/stop/cancel
va crashdan so'ng begona jarayon o'chmaydi, cheksiz restart yoki orphan qolmaydi.
Runtime versiyasini olib tashlash loyiha yoki DB ma'lumotlarini o'chirmaydi.

## M3. Loyiha importi, muhit va PostgreSQL

**Ishlar:**

- Mavjud papka, composer.json, public/index.php va package.json'ni tekshiring;
  noma'lum loyiha yoki incompatible PHP/Node talabi uchun tushunarli xabar bering.
- Importning o'zi composer/npm script, migration yoki mavjud .env yozishni boshlamasin.
- Loyiha PHP/Node tanlovi, process env va konfiguratsiyasini saqlang.
- PostgreSQL instance uchun data directory va credential ownership'ni yarating;
  loyiha uchun alohida DB/role, connection test va boshqa loyiha bazasiga
  ruxsatsiz kirishni rad etish sinovini qo'shing.
- .env o'zgarishi kerak bo'lsa aniq ko'rsatilgan user amalida, backup va cheklangan
  patch bilan qo'llang; aloqasiz qiymatlar va secret'larni saqlang.
- Minimal CLI: project list, doctor va project-scoped exec. "Open terminal"
  shu env bilan ochilsin; butun tizim PATH'ini almashtirmang.

**Natija:** ikki import qilingan loyiha mustaqil tanlov va DB bilan boshqariladi.

**Chiqish mezoni:** web/Composer/Artisan/queue PHP tanlovi va terminal/node/npm
tanlovi bir xil project context'ga amal qiladi. A loyihadagi o'zgarish B'ga
ta'sir qilmaydi; DB credential SQLite yoki loglarda ochiq chiqmaydi.

## M4. Domen, HTTPS va Laravel serving

**Ishlar:**

- Per-project hosts/domain yozuvlari, public root va Caddy routing yarating.
- CA/trust/hosts kabi zarur elevated amallarni narrow helper orqali bajaring;
  oddiy UI va supervisor'ni admin/root sifatida ishlatmang.
- Port/hostname konflikti mavjud Herd yoki boshqa servisni avtomatik to'xtatmasin.
  Default 80/443 band bo'lsa configurable loopback port va aniq URL ko'rsatilsin.
- HTTPS aktivatsiyasi, sertifikat saqlash va ishonchni tekshirishni ajrating.
  Ruxsat rad etilsa lokalizatsiyalangan recovery va foydalaniladigan alternativ URL bering.
- Laravel routing, static files, upload, secure-cookie va Vite HTTPS/HMR'ni tekshiring.

**Natija:** loyihani HTTP/HTTPS orqali ochish va real holatini ko'rish.

**Chiqish mezoni:** macOS'dagi asosiy browser va Windows/Edge'da .test + HTTPS,
trust va HMR dalili bor. Clean-port va mavjud-servis-konflikti holatlari alohida
tekshiriladi; yuqori port fallback'i "portsiz URL ishladi" deb hisoblanmaydi.

## M5. Kundalik foydalanish interfeysi

**Ishlar:**

- Ekranlar: Projects, Project details, Runtimes, Services/Logs va Settings.
- Add project, Start/Stop/Restart, Open browser/editor/terminal, versiya tanlash,
  DB test va log filtrlashni haqiqiy kontraktlarga ulang.
- Vite va bitta queue worker'ni loyiha uchun ixtiyoriy boshqaring. Project Stop
  boshqa loyihalar ishlatayotgan umumiy PostgreSQL yoki web-serverni o'chirmasin.
- MVP window close tray'da boshqarishni davom ettiradi; aniq Quit DevForge-owned
  supervisor va servislarni graceful to'xtatadi. Crash recovery M2 bilan bog'lanadi.
  Boot paytida auto-start keyingi scope; qayta ochishda sozlamalar saqlanadi.
- Ru/uz/en matnlar, empty/loading/error/recovery/disabled holatlar, keyboard,
  dialog focus va oynani kichraytirishni tekshiring. Request pending va ready
  holatlari ko'rinishda ajralsin.
- UI routing bo'yicha affected-file audit, vizual target comparison va real
  desktop interaction tekshiruvi bajarilsin.

**Chiqish mezoni:** uch tilda foydalanuvchi loyihani qo'shib, versiya/DB tanlab,
ishga tushirib, xatodan tiklanib va qayta ochib asosiy vazifani bajara oladi.
Haqiqiy log, yo'l va loyiha nomi tarjima qilinmaydi; tushuntirish matnlari tarjima qilinadi.

## M6. Paketlash va MVP qabul sinovi

**Ishlar:**

- Har target uchun installable artifact, prerequisite detection va owned
  resurslar cleanup'ini tayyorlang. DevForge uninstall loyihalar/DB data'ni
  avtomatik o'chirmasin.
- macOS signing/notarization va Windows signing'ni ichki test paketlaridan
  ajrating. Ommaviy tarqatish uchun zarur signing dalillari kerak;
  unsigned ichki build ommaviy signed release emas.[^packaging]
- Quyidagi qabul matritsasini to'liq o'tkazing; cold start, idle CPU/RAM,
  package install va log oqimidagi UI javobini o'lchang. M0/M1'da belgilangan
  reference environment va mavjud budget'lar bilan solishtiring. M0/M1'da raqamli
  budget tasdiqlanmagan; M6 boshlang'ich o'lchovlari baseline bo'ladi, SLA emas.
- Knowledge'ni code/config/tests bilan solishtirib yakuniy bosqich xulosasi va
  ochiq gaplarni yozing; strict validator va tegishli build/testlarni o'tkazing.

**Chiqish mezoni:** installable MVP, qo'llab-quvvatlash matritsasi, tekshirilgan
versiyalar, qabul dalillari va qolgan cheklovlar birga topshiriladi. Publish
alohida scope; rejaning o'zi release yoki GitHub push ruxsati emas.

# 4. Ma'lumot va ownership modeli

Boshlang'ich obyektlar: Project, RuntimePackage, DatabaseInstance, DatabaseBinding,
ProcessSession va AppSettings. Project runtime versiya/instance'ga havola qiladi;
DatabaseBinding secret qiymatini emas OS secret store reference'ini saqlaydi.
AppSettings locale tanlovi loyiha env'idan mustaqil. SQLite schema migration'lari
bo'ladi; aniq jadval va crate bo'linishi implementatsiyada vaziyatga mos tanlanadi.

# 5. Yakuniy qabul matritsasi

Har targetda uch til: ru, o'zbek ishchi katalogi va en. Uch target o'tsa jami
to'qqiz OS/arch/til kombinatsiyasi tekshiriladi; fixture locale real to'rtinchi
til chiqarilganini bildirmaydi.

| Sinov | Kutiladigan natija |
| --- | --- |
| Toza mashinaga install | Tashqi Herd/global PHP/Node talab qilmaydi; kerakli OS prerequisite aniq ko'rsatiladi. |
| A va B loyiha | Turli PHP/Node, alohida DB/role bilan parallel ishlaydi. |
| Version switch | A web/CLI/queue tanlovi yangilanadi, B o'zgarmaydi. |
| Vite/queue | Boshlash, readiness, log, stop va crash recovery tekshiriladi. |
| HTTP/HTTPS | Domen, certificate trust, secure-cookie va HMR ishlaydi; rad etilgan elevation recovery'ga olib keladi. |
| Restart/re-launch | Tanlovlar, locale va DB data saqlanadi; duplicate jarayon qolmaydi. |
| Til | UI, menu/tray, notification va errors uch tilda; plural/fallback/key coverage va text expansion tekshiriladi. |
| Konflikt | Mavjud Herd/DB/port/hosts yozuvi begona resurs sifatida saqlanadi. |
| Nosozlik | Buzilgan download, disk full, invalid path/config va unavailable package boshqariladi. |
| Remove/uninstall | Loyiha katalogi va DB ma'lumotlari saqlanadi; faqat tegishli owned sozlamalar cleanup qilinadi. |

# 6. Har bosqichda hisobot

Bosqich holati, o'zgargan kod/hujjatlar, real buyruq/test natijasi, macOS/Windows
qamrovi, ru/uz/en holati va blocker'lar ko'rsatiladi. Knowledge status'lari kod
dalili bilan yangilanadi; [yuritish siyosati](knowledge-policy.md) bajariladi.
Read-only planning tekshiruvlari desktop implementatsiya dalili deb ko'rsatilmaydi.

**Keyingi implementation yo'nalishi:** M5 kundalik workspace native acceptance
yakunlandi. Alohida topshiriqda M6 packaging/release matritsasiga o'tish; minimum
OS, signing/installer, native credential UI va trust metadata gate'lari saqlanadi.
M0 release/runtime gate'lari ham ochiq qoladi; asoslanmagan muddat va'dasi yo'q.

[^request]: Suhbat manbasining aniq MVP reja bo'limi.
[^windows-php]: Windows PHP rasmiy build'lari NTS/FastCGI va VC runtime talablarini tushuntiradi.
[^macos-php]: PHP rasmiy qo'llanmasi macOS paket manbasi yoki source build talabini bayon qiladi.
[^caddy-php]: Caddy php_fastcgi PHP so'rovlarini FastCGI backend'ga yo'naltiradi.
[^caddy-https]: Caddy lokal CA va trust xatti-harakati haqidagi rasmiy qo'llanma.
[^postgres]: PostgreSQL rasmiy sayti platformalar bo'yicha paket va source manbalarini ko'rsatadi.
[^i18next]: react-i18next i18next'ning React integration'idir; aniq versiya implementation vaqtida tanlanadi.
[^packaging]: Tauri platforma distributivlari va signing jarayonini hujjatlashtiradi.
