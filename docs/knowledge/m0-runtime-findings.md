---
type: Feasibility Report
title: M0 runtime paketlari va dastlabki sinovlar
description: macOS va Windows paketlari, haqiqiy bajarish dalillari, cheklovlar va M0 ochiq gate'lari.
status: draft
decision_status: working-direction
tags: [m0, runtimes, evidence, macos, windows, blockers]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: M0 topshirig'i va Windows muhiti haqidagi javob
  - id: catalog
    resource: ../../runtime/m0-candidates.json
    title: Aniq URL va publisher SHA-256 bilan M0 nomzodlar katalogi
  - id: artifacts
    resource: ../evidence/m0/package-audit.json
    title: Yuklash va statik tekshirish natijalari
  - id: macos-runtime
    resource: ../evidence/m0/macos-arm64-smoke.json
    title: ARM64 Laravel PHP PostgreSQL Caddy va Vite sinovi
  - id: x64-runtime
    resource: ../evidence/m0/macos-x64-rosetta-smoke.json
    title: x64 Rosetta Laravel PHP PostgreSQL Caddy va Vite sinovi
  - id: windows-static
    resource: ../evidence/m0/windows-static-package-audit.json
    title: Windows target paketlarining Mac hostdagi statik auditi
  - id: node-runtime
    resource: ../evidence/m0/node-execution.json
    title: Node npm va Intl native hamda Rosetta sinovlari
  - id: postgres-runtime
    resource: ../evidence/m0/postgres-execution.json
    title: PostgreSQL initdb va single-user SQL sinovlari
  - id: environment
    resource: ../evidence/m0/environment.json
    title: Host muhiti va izolyatsiya dalillari
  - id: staticphp
    resource: https://dl.static-php.dev/v3/php-bin/common/README.txt
    title: StaticPHP v3 common build profili
  - id: node-support
    resource: https://github.com/nodejs/node/blob/v24.22.2/BUILDING.md
    title: Node 24 platforma talablari
  - id: postgres-provider
    resource: https://github.com/zonkyio/embedded-postgres-binaries
    title: Zonky test-bundle manbasi
  - id: windows-php
    resource: https://www.php.net/downloads.php?os=windows
    title: Rasmiy Windows PHP paketlari va prerequisite'lar
---

# Holat

Bu M0 tarixiy snapshot'idir. Keyingi [M1 desktop natijalari](m1-foundation-results.md)
va [M2 runtime natijalari](m2-runtime-core-results.md) toolchain, UI hamda hosted
macOS/Windows package/probe/Node HTTP dalillarini yangilaydi. Windows'dagi to'liq
Laravel/PDO/FastCGI/PG-cluster/TLS/HMR acceptance hali ochiq; quyidagi M0 davridagi
"desktop hali yo'q" yozuvlari joriy source holati deb olinmaydi.

2026-11-11 kuni M0 bajarilishi boshlandi. Runtime paketlari va mavjud hostdagi
sinovlar tekshirildi; **M0 to'liq yopilmagan**. Windows muhiti hozir yo'qligini
foydalanuvchi aniq bildirdi. Intel uchun Rosetta dalili bor, native Intel Mac
dalili yo'q.[^request][^environment]

Bu hisobot paket nomzodlari va o'lchangan xatti-harakatni saqlaydi. Undagi
working-direction status provider/target tanlovlariga tegishli; desktop app,
installer yoki ommaviy tarqatish tayyorligini bildirmaydi.

# Tanlangan aniq nomzodlar

| Qism | Versiya | Paket yo'li va tekshirish |
| --- | --- | --- |
| PHP macOS | 8.4.27, 8.5.12 CLI + FPM | StaticPHP v3 common; har SAPI/arch uchun alohida publisher .sha256. |
| PHP Windows x64 | 8.4.27-1, 8.5.12-1 NTS | PHP rasmiy ZIP va releases.json SHA-256; php.exe, php-cgi.exe va pdo_pgsql DLL tarkibi. |
| Node | v22.23.4, v24.22.2 | Rasmiy darwin arm64/x64 va Windows x64 arxivlari; SHASUMS256.txt. |
| Caddy | v2.11.9 | Rasmiy mac arm64/amd64 va Windows amd64 release asset'lari; publisher SHA-256 metadata. |
| PostgreSQL | 18.6, bundle 18.6.0 | Zonky macOS/Windows test-bundle nomzodi; Maven Central .sha256; JAR ichidagi txz va executable header'lari. |
| Composer | 2.11.2 | Rasmiy PHAR va .sha256sum; mavjud Herd Composer ishlatilmagan. |

Har bir aniq URL, digest va provider [katalogda](../../runtime/m0-candidates.json),
yuklab olingan baytlar va header natijalari esa [auditda](../evidence/m0/package-audit.json).
Checksum mosligi release signature yoki code signing tekshiruvi deb ko'rsatilmaydi.[^catalog][^artifacts]

Yakuniy audit: **25 nomzoddan 23 tasi yuklandi va publisher SHA-256'ga mos keldi**;
ikkita muqobil manba metadata-only qoldi. Tekshirilgan executable header'lari
katalog target'lariga mos. Windows target uchun
[alohida statik audit](../evidence/m0/windows-static-package-audit.json) mavjud;
uning execution host'i Mac, Windows runtime bajarilishi emas.[^windows-static]

Oldingi uzilgan yuklamalarda checksum rad etilishi va HTTP/2/partial-transfer
xatolari qayd etilgan. Yaroqsiz fayl bajarilmadi. Toza yuklash hamda tekshirilgan
explicit-end Range recovery'dan keyin final digest'lar mos keldi; oldingi holat
[checksum snapshot](../evidence/m0/pre-retry-package-audit.json),
[transport snapshot](../evidence/m0/pre-range-recovery-package-audit.json) va
[recovery receipt](../evidence/m0/intel-range-recovery-package-audit.json) bilan
ajratilgan. Bu muammolar final artifact-integrity gate'i sifatida ochiq qolmadi.

StaticPHP eski `/static-php-cli/` katalogida 8.4.23/8.5.8 topilgan; u yangi
default sifatida olinmadi. Joriy `/v3/php-bin/common/` yo'li 8.4.27/8.5.12 va
publisher digest'larini beradi. Tool v3 yo'nalishi va uchinchi tomon build profili
shipping/reproducibility ko'rib chiqishidan hali o'tkazilishi kerak.[^staticphp]

Postgres.app DMG va EDB Windows ZIP muqobil manba sifatida katalogda qoldi;
ushbu pass'da ular metadata-only, bajarilgan runtime dalili emas. Zonky
bundle'i test uchun mo'ljallangan uchinchi tomon artifact'idir; Java ishlatilmadi,
ichki native binarlar tekshirildi.[^postgres-provider]

# Haqiqiy bajarish matritsasi

| Qism | macOS ARM64 | macOS x64 | Windows x64 |
| --- | --- | --- | --- |
| Node 22/24 va npm | Native o‘tdi. | Rosetta orqali o‘tdi. | Paket/hash/PE tekshirildi; bajarilmadi. |
| Caddy | Native version probe va Laravel HTTP/TLS o‘tdi. | Rosetta version probe va Laravel HTTP/TLS o‘tdi. | Paket/hash/PE tekshirildi; bajarilmadi. |
| PostgreSQL 18.6 | initdb, SQL va Laravel PDO/TCP o‘tdi. | Rosetta initdb, single-user SQL va Laravel PDO/TCP o‘tdi. | Paket/hash/nested PE tekshirildi; bajarilmadi. |
| PHP 8.4/8.5 | CLI, extension, Unicode yo‘lga ko‘chirish, FPM va Laravel HTTP/TLS o‘tdi. | Xuddi shu zanjir Rosetta orqali o‘tdi; native Intel dalili emas. | Rasmiy NTS ZIP/hash/PE va pdo_pgsql DLL tekshirildi; bajarilmadi. |
| Vite | Node 22/24 build, client HTTP 200, direct WebSocket va file-change notification o‘tdi. | Shu tekshiruvlar Rosetta orqali o‘tdi. | Hali tekshirilmagan. |

ARM64 sinovida Laravel skeleton v13.11.0 dan Laravel Framework 13.42.0
o'rnatildi. PHP 8.4.27 va 8.5.12 uchun PostgreSQL PDO so'rovi va HTTPS tekshirildi.
Vite HMR natijasi serverning direct WebSocket transporti va fayl o'zgargandagi
xabarga tegishli; brauzerda JS yangilanishi yoki Caddy HTTPS reverse-proxy HMR
deb talqin qilinmaydi.[^macos-runtime]

Shu fixture x64 paketlari bilan Rosetta orqali ham o'tdi; dalil alohida
[x64 report'da](../evidence/m0/macos-x64-rosetta-smoke.json). Rosetta natijasi
minimum OS yoki fizik Intel Mac sinovi o'rniga o'tmaydi.[^x64-runtime]

Node/npm/Intl dalillarida en, ru va uz-Latn mavjud. Bu DevForge interfeysi,
tarjima kataloglari, menu/tray matnlari yoki fallback ishlaganini isbotlamaydi;
ular M1 va keyingi UI bosqichlarida yoziladi. Node ICU natijasi target WebView'ning
Intl qamrovi bilan bir xil deb taxmin qilinmaydi.[^node-runtime]

# Izolyatsiya va cheklovlar

- Host: macOS 27.2, arm64; x64 bajarish Rosetta orqali. Minimal qo'llab-quvvatlash
  versiyasi faqat shu host natijasidan chiqarilmaydi.
- Herd read-only runtime diagnostikasida Nginx/DNS va FPM faol/sog'lom edi;
  extra service inventory Herd Pro talab qilgani uchun mavjud emas.
- 80/443 va mavjud PostgreSQL portiga tegilmadi. Fixture'lar Herd papkasidan
  tashqarida, vaqtinchalik katalog va bo'sh loopback portlarda ishladi.
- Hosts, system CA trust, global PATH, mavjud database va servislar o'zgartirilmadi.
  TLS uchun task-local CA klientga aniq berildi; oddiy browser trust tekshirilmadi.
- Sinovdan keyingi Herd read-only holatida FPM sog'lom, Nginx va DNS faol edi;
  bu qayta tekshirish host daliliga qo'shildi.
- PHP common profilida pdo_pgsql bor, `intl` va `sodium` yo'q. Oddiy Laravel
  fixture o'tdi, lekin bu Filament yoki shu extension'larni talab qiluvchi har
  qanday loyiha qo'llanadi degani emas. Release PHP profili alohida tanlanadi.
- Zonky nested paketlarida LICENSE/NOTICE fayllari topilmadi. PHP va PostgreSQL
  provider'lari uchun bundled kutubxona/litsenziya-notice to'plami hali tayyor emas.
  Diagnostika muvaffaqiyati redistribution tayyorligi deb ko'rsatilmaydi.

Dalillar: [host](../evidence/m0/environment.json),
[PostgreSQL standalone](../evidence/m0/postgres-execution.json),
[native stack](../evidence/m0/macos-arm64-smoke.json).[^environment][^postgres-runtime]

# Minimal OS va ochiq gate'lar

Node 24 rasmiy matritsasi macOS uchun 13.5+ va Windows x64 uchun Windows 10+
ko'rsatadi. Bu bitta dependency chegarasi; DevForge uchun tayyor support sertifikati
emas.[^node-support] Windows PHP NTS/FastCGI va VC++ runtime talablari ham
alohida hisobga olinadi.[^windows-php]

| Gate | Holat va keyingi ish |
| --- | --- |
| Windows runtime | Blocker: Windows VM/test kompyuter/runner mavjud emas. Native PHP-CGI + Caddy + PostgreSQL + Laravel/Vite sinovi kerak. |
| Intel Mac | To'liq runtime zanjiri Rosetta orqali o'tdi; native Intel va minimum OS hostida tekshirish yo'q. |
| Minimum OS | macOS 13.5+ va Windows 11 x64 ishchi baseline nomzodi; minimum OS'da yakuniy runtime/build hali tekshirilmagan. |
| Release paketlar | PHP extension profili, provider build reproducibility, notice/SBOM va signing talablari ochiq. |
| Domen/browser HTTPS/HMR | Hosts/system trust/browser va HTTPS-proxy HMR hali tekshirilmagan; M4 sinovlari bilan yopiladi. |
| Desktop/i18n | Rust/Cargo joriy PATH'da topilmadi; Tauri/uch tilli UI hali yo'q. M1 uchun toolchain va native build muhiti kerak. |

M0 natijasi: mavjud Mac'da asosiy runtime zanjiri mumkinligi isbotlandi;
Windows support'i tasdiqlanmadi. M1 poydevor ishlarini boshlash keyingi alohida
topshiriq bo'ladi, release/support gate'lari esa ochiq qoladi.

Mashina o'qiydigan [target snapshot](../../runtime/m0-targets.json) ham partial
holatni va unverified gate'larni saqlaydi. M0 tooling/validator bo'yicha **44 ta
test** macOS'da o'tdi; bu platforma runtime va release gate'larini almashtirmaydi.

# Kod va bilimni solishtirish

| Da'vo | Tekshirilgan source yoki dalil | Holat |
| --- | --- | --- |
| Paket integrity va header tekshiruvi bor. | [Audit kodi](../../scripts/m0_runtime_audit.py), [regression testlar](../../tests/test_m0_runtime_audit.py) va final artifact report. | Mos; bu production installer emas. |
| Laravel/PHP/PG/Caddy/Vite runtime zanjiri ishlaydi. | [Smoke runner](../../scripts/m0_macos_smoke.py) va ikkita alohida native/Rosetta report. | Mos, faqat qayd etilgan host va fixture doirasida. |
| DevForge Rust/Tauri desktop va uch tilli UI tayyor. | Repozitoriyda app Cargo/package/Tauri manbalari hali yo'q. | Amalga oshirilmagan; M1+. |
| Windows va minimum OS support'i tasdiqlangan. | Real Windows/minimum OS hosti yo'q; package metadata va Mac natijasi buni isbotlamaydi. | Tekshirilmagan. |
| Portable package release'ga tayyor. | Extension, notice/SBOM, signing va provider build gate'lari ochiq. | Tekshirilmagan; shipping qarori chiqarilmagan. |

Tooling testi tayyorlangan Python muhitida `python -m unittest discover -s tests
-p 'test_*.py' -v` bilan bajarildi: 17 ta M0 helper testi va 27 ta OKF validator
testi. Hujjatlar yakunida strict OKF tekshiruvi alohida bajariladi; testlar
Windows'da o'tgan deb ko'rsatilmaydi.

# Qayta bajarish

Quyidagi buyruqlar repozitoriy ildizidan, mos Python bilan bajariladi. Cache
repo'dan tashqarida yoki `.m0-cache` ichida bo'lishi mumkin; binarlar Git'ga qo'shilmaydi.

```sh
python3 scripts/m0_runtime_audit.py --catalog runtime/m0-candidates.json --cache .m0-cache --report docs/evidence/m0/local-artifact-audit.json
python3 scripts/m0_node_smoke.py --catalog runtime/m0-candidates.json --cache .m0-cache --report docs/evidence/m0/local-node-smoke.json
python3 scripts/m0_postgres_smoke.py --catalog runtime/m0-candidates.json --cache .m0-cache --report docs/evidence/m0/local-postgres-smoke.json
python3 scripts/m0_macos_smoke.py --catalog runtime/m0-candidates.json --cache .m0-cache --report docs/evidence/m0/local-stack-smoke.json --architecture arm64
```

Node va PostgreSQL standalone probe'lari hozir macOS'dagi ikkala arch'ni tanlaydi;
ularni bajarishdan oldin kerakli ikkala target arxivini ham cache'ga yuklang.
`m0_node_smoke.py` Windows yo'liga ega, lekin Windows'da hali bajarilmagan.
Mac stack runner'i Windows'ni qabul qilmaydi; FPM'ni Windows'da ishlatishga urinmaydi.

Windows runner mavjud bo'lganda `--target windows-x64` artifact auditi va Node
probe'idan boshlanadi; keyin task-local PHP-CGI, VC runtime, PostgreSQL va Caddy
fixture'i bilan native sinov qilinadi. Mac/Rosetta natijalari u yerga ko'chirilmaydi.

[^request]: Foydalanuvchi M0'ni boshlashni topshirgan va Windows muhiti yo'qligini javobda tasdiqlagan.
[^catalog]: Aniq versiyalar M0 snapshot'idir; skill'ga doimiy major/minor pin kiritilmagan.
[^artifacts]: Audit download/hash va executable header'larini tekshiradi; signature va foreign OS execution tekshiruvi emas.
[^macos-runtime]: Task-local konfiguratsiya, CA, cluster va fixture bilan qayd etilgan native ARM64 natija.
[^x64-runtime]: Xuddi shu zanjirning x64/Rosetta natijasi, native Intel sifatida ko'rsatilmagan.
[^windows-static]: Windows artifact'lari SHA va PE tarkibi Mac'da tekshirildi, bajarilmadi.
[^node-runtime]: Publisher hash'i tekshirilgan to'rtta macOS Node artifact'ining haqiqiy CLI/Intl natijalari.
[^postgres-runtime]: Ikki arch uchun initdb va SQL; x64 natijasi Rosetta sifatida belgilangan.
[^environment]: Mavjud host, Herd read-only holati va task izolyatsiyasi.
[^staticphp]: Common profilining e'lon qilingan extension tarkibi va yangi artifact yo'li.
[^node-support]: Node 24.22.2 BUILDING.md dagi provider matritsasi; minimum OS'da DevForge sinovi emas.
[^postgres-provider]: Native test-bundle loyihasi va uning rasmiy manbalari; redistribution bo'yicha barcha notice'lar hali jamlanmagan.
[^windows-php]: Rasmiy Windows paket va prerequisite hujjati; Windows mashinasida tekshirish hali yo'q.
