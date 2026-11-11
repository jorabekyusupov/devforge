---
type: Implementation Report
title: M4 domen HTTPS va Laravel serving
description: Owned domains, bounded hosts/trust helper, loopback serving va native M4 tekshiruvlari.
status: draft
decision_status: confirmed
tags: [m4, serving, https, domains, laravel, evidence]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: M4 implementation va native test topshirig'i
  - id: serving
    resource: ../../crates/serving/src/manager.rs
    title: Shared serving orchestration
  - id: helper
    resource: ../../crates/serving/src/system.rs
    title: Fixed-target hosts va system trust amallari
  - id: authority
    resource: ../../crates/serving/src/authority.rs
    title: Scoped development CA
  - id: local
    resource: ../evidence/m4/macos-basic-serving.json
    title: macOS PHP routing static TLS smoke
  - id: laravel-local
    resource: ../evidence/m4/macos-laravel-serving.json
    title: macOS real Laravel DB routing va restart sinovi
  - id: browser-driver
    resource: ../../apps/desktop/scripts/m4-browser.mjs
    title: Native browser CLI TLS cookie upload va HMR tekshiruvi
  - id: windows-replace
    resource: https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-replacefilew
    title: Windows ReplaceFileW sharing va ACL kontrakti
  - id: mac-trust
    resource: https://github.com/apple-oss-distributions/Security/blob/main/SecurityTool/macOS/security.1
    title: Apple security trust settings remove kontrakti
  - id: hosted-macos
    resource: ../evidence/m4/hosted-macos-serving.json
    title: Hosted macOS Chrome full native serving acceptance
  - id: hosted-windows
    resource: ../evidence/m4/hosted-windows-serving.json
    title: Hosted Windows Edge full native serving acceptance
  - id: attempts
    resource: ../evidence/m4/ci-attempts.json
    title: Native CI regression urinishlari va tuzatishlari
  - id: core-ci
    resource: ../evidence/m4/ci-final-checks.json
    title: Final macOS Windows checks va embedded desktop build
  - id: metrics
    resource: ../evidence/m4/final-test-metrics.json
    title: Final per-OS test counters
  - id: native-ci
    resource: ../evidence/m4/ci-native-checks.json
    title: Native workflow step conclusions va cleanup
  - id: native
    resource: ../../.github/workflows/serving.yml
    title: Disposable native macOS Windows acceptance workflow
---


# Natija va scope

M4 topshirig'i domen, HTTPS va Laravel serving kodi, ikki OS sinovi, OKF va
push'ni vakolatlaydi.[^request] `crates/serving` va CLI `site` buyruqlari
Tauri'dan mustaqil. Desktop Projects UI va per-project Start/Stop flow'lari M5;
M4 CLI barcha sozlangan site'larni foreground sessiyada boshqaradi.[^serving]

# Serving kontrakti

- Domain faqat lowercase `.devforge.test`; project, `.localhost` va `vite-`
  nomlari o'rtasidagi kolliziya rad etiladi. Bitta sessiyada1-16 site ishlaydi.
- Public root canonical project ichida bo'lishi va `index.php` saqlashi kerak.
  Dot-file route'lar yopiq, request body64MB bilan cheklangan.
- macOS PHP-FPM, Windows selected PHP-CGI ishlatadi. Shared project context
  runtime lease, INI, environment va own DB credential'larini beradi; loyiha
  `.env` fayli serving start'da o'zgartirilmaydi.
- FPM/CGI TCP listener'dan keyin nonce/PHP-version FastCGI probe bilan
  tekshiriladi. Caddy HTTP va pinned-root TLS health probe'dan o'tadi.
  Spawn'ning o'zi readiness emas.
- Faqat127.0.0.1 tinglanadi.80/443 ishlamasa8080/8443 yoki erkin loopback port
  tanlanadi. Actual URL qaytariladi; boshqa Herd/service to'xtatilmaydi.
- DNS yoki CA trust bo'lmasa HTTP `*.localhost` fallback bor. HTTPS certificate
  activation, OS trust, PHP/Vite readiness va actual process holati alohida.
  Stop'dan keyin TLS/worker readiness faol deb saqlanmaydi.
- Vite opt-in: o'rnatilgan project dependency va selected Node kerak. Backend
  loopback HTTP, tashqi URL HTTPS/WSS bo'lishi mumkin; Caddy upgrade'ni proxy qiladi.
  `public/hot` mavjud bo'lsa start rad etiladi; faqat sessiya yaratgan kutilgan
  URL yozuvi cleanup qilinadi. Start dependencies'ni avtomatik o'rnatmaydi.
- Serving APP_URL va secure-cookie qiymatini primary scheme'dan oladi. Laravel
  application config/cache semantics saqlanadi; start avtomatik Artisan cache
  clear yoki migration bajarmaydi.[^serving]

CLI: `site list|configure|remove|authority|system-plan|serve`.
`system-plan hosts|hosts-remove|trust|trust-remove <new-file>` reviewable plan va
SHA-256 digest beradi. `serve` Ctrl+C/stdin `quit` bilan owned process'larni
stop qiladi. Boshqa process'lar yoki project/database data o'chirilmaydi.

# CA, helper va OS chegarasi

Active DevForge CA key production'da OS SecretStore'da; helper private key'ni
olmaydi. Public root certificate va private session leaf files app data ichida.
CA10 yil, leaf30 kun; leaf har serving start'da qayta chiqariladi. Critical DNS
name constraints `devforge.test` va `localhost`; arbitrary domain uchun cert
chiqarilmaydi. Expired CA install rad etiladi, exact-owned removal esa private
key va amaldagi validity'ni talab qilmaydi.[^authority]

`devforge-helper apply <plan> <digest>` fresh elevation bilan ishlaydigan bir
martalik executable. Supervisor root/admin bo'lsa start rad etiladi. Helper
faqat fixed system hosts va exact branded/scoped CA install/remove qabul qiladi;
arbitrary target, project command, shell IPC yoki global DNS daemon yo'q.
Input bounded regular file; schema/unknown fields/digest tekshiriladi. Hosts
before-hash CAS, lock va file identity tekshiruvi bor; owner block tashqarisidagi
baytlar saqlanadi. Mac metadata/ACL va Windows ReplaceFileW DACL preservation
ishlatiladi. Windows mandatory lock ichida o'qish aynan locked handle orqali,
replacement handle esa ReplaceFileW'dan oldin yopiladi.[^helper][^windows-replace]

Mac system trust System.keychain, Windows LocalMachine Root store'da. OS changes
explicit; oddiy serving start user'ni elevate qilmaydi. Caddy default PKI uchun
trust installation ham explicit o'chirilgan. Mac security amallari noninteractive
stdin, bounded output va10s deadline bilan bajariladi.[^helper]

Mac headless runner'da DeleteTrustSettings authorization kutishida qolgan.
Removal exact CA'ni system-default/unspecified holatiga qaytarib, certificate'ni
hash orqali keychain'dan olib tashlaydi; keyin absent va untrusted holati
tekshiriladi. OS authorization siyosati o'zgartirilmaydi. Ishonch bermaydigan
admin metadata qolishi mumkinligi helper `may_retain_nontrusting_record` field'ida
ko'rsatiladi. Uning jismoniy removal'i GUI/uninstall gate'ida qoladi. Windows
exact DER certificate context'ini olib tashlaydi.[^helper][^mac-trust]

# Dalillar va test chegarasi

Mahalliy Mac ARM64: selected PHP8.4/8.5 FPM, HTTP/static/pinned-root TLS,
haqiqiy Laravel13.42, ikkita PG sentinel, Vite startup, conflict restart,
hot cleanup va untrusted HTTP fallback o'tdi. Herd80/443 band/permission holati
8080/8443 bilan ishladi. User Mac hosts/trust o'zgartirilmadi; task-local test CA
key va stopped fixture keyin olib tashlandi.[^local][^laravel-local]

Native workflow43020318091, source `6f3033e` ikkala OS'da o'tdi:

| Target | Browser | Actual default-port natijasi | Natija |
| --- | --- | --- | --- |
| macOS26.7.2 ARM64 | Chrome | HTTP8080, HTTPS8443;80/443 permission fallback | passed |
| Windows Server2025 x64 | Edge | HTTP8080 fallback; HTTPS443 portsiz URL | passed |

Mac'da9, Windows'da7 pinned package o'rnatildi; Laravel13.42 ikki
project bilan PHP8.4/8.5, PG own databases, nested routing/static/dot-file block,
verified TLS, secure/HttpOnly cookie, real multipart upload, Vite WSS HMR
**document reloadsiz**, restart va hot cleanup o'tdi. Foreign listener'lar
saqlandi, explicit conflict fallback tekshirildi. Fresh untrusted CA bilan
`*.localhost` HTTP browser fallback va denied helper permission tekshirildi.
Requested0 port bilan clean/free-port case alohida ishladi. Mac80/443 uchun
portsiz URL ishladi deb ko'rsatilmaydi.[^hosted-macos][^hosted-windows]

Disposable VM'larda exact hosts/CA plans apply qilindi. Yakunda hosts baytlari
oldingi hash'ga qaytdi, certificate olib tashlandi va OS trust false bo'ldi.
Mac effect nontrusting metadata qolishi mumkinligini true, Windows false
qaytardi. Bu user Mac'iga system trust o'rnatilganini bildirmaydi.

Final core workflow43020283271 shu source'da ikkala OS uchun passed:
51 Rust passed,0 failed,1 native credential fixture intentionally ignored;
8 frontend va3 locale regression testi,3 katalog coverage, typecheck,
fmt/clippy, embedded frontend bilan native desktop binary build va OKF strict
passed. Bu phase'da native credential UI testi bajarildi deb ko'rsatilmaydi.
Final docs/evidence o'zgargach product suite qayta bajarilmaydi; faqat yangi
knowledge uchun strict validator ishlaydi.[^core-ci][^metrics][^native-ci]

Native harness CA/private environment'ni **test-only private filesystem SecretStore
adapteri** orqali saqlaydi; production NativeSecretStore kodi bundan alohida.
Native CA keychain item roundtrip/credential UI bu fixture bilan isbotlanmaydi.
Browser pinned Playwright CLI, named isolated session va
`ignoreHTTPSErrors:false` bilan ishlaydi; OS trust'ni TLS bypass bilan
almashtirmaydi.[^browser-driver][^native]

# Regressiyalar va reconciliation

CI urinishlari lock/sharing, Windows fixture token/default DACL, verbatim paths,
WinSock SystemRoot, Edge ProgramFiles discovery, Laravel cookie naming va upload
locator xatolarini ko'rsatdi. Product/backend fix'lar fixture fix'laridan ajratildi;
failed/cancelled urinishlar acceptance pass deb ko'rsatilmaydi.[^attempts] Minimal Windows
base env'da standard OS folder path'lari saqlanadi; global PATH almashtirilmaydi.
Native Windows test-only restricted token hisob, filesystem ACL yoki UAC/security
siyosatini o'zgartirmaydi.

- **Mos:** shared runtime/context, owned domain/CA, narrow elevation boundary,
  loopback/fallback, actual readiness va secret/log chegaralari.
- **O'zgargan:** M3'da rejalashtirilgan serving backend endi mavjud; Mac removal
  nontrusting metadata haqidagi explicit result bilan bajariladi.
- **Tekshirilmagan:** minimum OS/physical Windows10-11, native Intel Mac,
  Safari/Firefox, actual CA-key credential UI va uzoq muddatli session renewal.
- **Amalga oshirilmagan:** per-project desktop flow'lari, installer-managed
  elevation UI, standalone daemon, automatic leaf renewal va physical trust
  metadata cleanup. Bular M5/M6 gate'lari; M4 native test ularni yopmaydi.

[^request]: Joriy foydalanuvchi M4 topshirig'i va uning scope'i.
[^serving]: Shared serving source, CLI va runtime/context integratsiyasi.
[^helper]: Fixed-target OS adapters va digest-bound operation; failure yashirilmaydi.
[^authority]: Root/leaf generation, key-match va certificate validation.
[^local]: Task-local custom-root HTTP/TLS sinovi; user OS trust o'zgartirilmagan.
[^laravel-local]: Real Laravel/PHP/PostgreSQL/Vite fixture, HTTP fallback va restart.
[^browser-driver]: Native browser CLI TLS/cookie/upload/WSS-HMR assertions.
[^windows-replace]: Microsoft native replacement sharing, DACL va metadata kontrakti.
[^mac-trust]: Apple trust settings va keychain certificate alohida obyekt ekanini hujjatlashtiradi.
[^native]: Disposable macOS/Windows CI workflow; faqat actual natija verification hisoblanadi.

[^hosted-macos]: Real hosted Mac/Chrome .test, OS trust, upload/HMR va cleanup result; custom TLS bypass ishlatilmagan.
[^hosted-windows]: Real hosted Windows/Edge, CGI/Laravel/PG, HTTPS443, fallback va cleanup result.
[^attempts]: Har meaningful CI regression va uning source/test fix'lari; final successful source SHA ajratilgan.

[^core-ci]: Source6f3033e uchun macOS/Windows full checks va native binary build success.
[^metrics]: Job loglaridan chiqarilgan51 Rust/8 frontend/3 locale per-OS counters; ignored fixture alohida.
[^native-ci]: Native7th suite'ning barcha relevant step'lari, shu jumladan cleanup success.
