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
  - id: native
    resource: ../../.github/workflows/serving.yml
    title: Disposable native macOS Windows acceptance workflow
---

# Natija va hozirgi dalil

M4 implementation va ikki OS sinovi foydalanuvchi tomonidan topshirildi.[^request]
`crates/serving` va CLI `site` buyruqlari Tauri'dan mustaqil qo'shildi.
Desktop Projects UI M5 scope'ida qoladi.[^serving]

Mahalliy macOS ARM64 smoke ikki project PHP 8.4/8.5, FPM identity, Caddy,
HTTP routing/static va custom CA orqali tekshirilgan HTTPS bilan o'tdi.
Herd egallagan 80/443 o'rniga 8080/8443 tanlandi; boshqa servis o'zgartirilmadi.
Mahalliy hosts va OS trust o'zgartirilmadi.[^local]

# Kontrakt

Domain faqat lowercase `.devforge.test`; project va `vite-` alias'lari kolliziyasi
rad etiladi. `.localhost` HTTP muqobil manzili DNS/trust/elevation bo'lmaganda
ishlatiladi. Loopback-only listener'lar va `public/index.php` canonical containment
tekshiruvi bor. PHP web process tanlangan project context/INI/DB env bilan
ishlaydi. Caddy HTTP va HTTPS alohida route qiladi. Vite ixtiyoriy project-local
Node bilan ishlaydi, reverse proxy WSS'ni o'tkazadi.[^serving]

CA key production'da OS SecretStore'da, public root certificate private app
papkasida. Leaf key/cert private session papkasida. Root critical DNS name
constraints `devforge.test` va `localhost` bilan cheklangan. CA activation va
system trust alohida status; trust yo'q bo'lsa default URL HTTP bo'ladi.[^authority]

`devforge-helper apply <plan> <digest>` bir martalik elevated executable.
Faqat fixed system hosts va aynan DevForge-branded/scoped CA install/remove
amallarini qabul qiladi. Caller digesti plan'ga bog'langan; hosts before-hash CAS
va owner block tashqarisidagi baytlar saqlanadi. Arbitrary target/command yoki
project code root/admin sifatida bajarilmaydi. Supervisor elevated bo'lsa
start rad etiladi. Automatic OS elevation yoki global DNS daemon yo'q.[^helper]

# Tekshiruv holati

Mahalliy47 Rust testi,8 frontend va3 locale testi o'tdi; keyingi TCP readiness
va native hosts fixture regressiyalari ham alohida o'tdi. Haqiqiy Laravel13.42
ikki project uchun PG sentinel, selected PHP, nested routing/static/TLS, Vite
startup, conflict restart va untrusted HTTP fallback bilan Mac'da o'tdi.
Windows'da hosts/CA install va exact cleanup o'tdi, ammo fixture elevated
process'da tayyorlangani uchun unprivileged child uning metadata'sini yoza olmadi.
Prepare ham run kabi test-only restricted token'da bajarishga o'tkazildi;
Composer'ga yuboriladigan Windows canonical verbatim path normal formaga keltirildi;
OS hisob/ACL/security siyosati o'zgartirilmaydi. Edge channel lookup restricted project environment'da PROGRAMFILES yo'qligi
sabab installed browser'ni topmagan. Standard OS ProgramFiles path'lari minimal
base env allowlist'iga qo'shildi; global PATH o'zgartirilmadi. Upload fixture
file-input/submit accessible nomlari ajratildi. Browser acceptance hali davom etmoqda.[^laravel-local][^browser-driver]

Dastlabki native CI Windows mandatory file-lock semantics va Windows-only lint
regressiyasini topdi. Hosts read/recheck endi locked handle orqali bajariladi,
file identity ham solishtiriladi. `ReplaceFileW` replacement faylini no-sharing
rejimida ochgani uchun staged write handle avval yopiladi; ACL saqlash uchun
almashtirish API'si saqlanadi.[^windows-replace] Windows TCP fixture'iga WinSock provider lookup
uchun explicit SystemRoot beriladi; product context uni allaqachon beradi. Mac'da CA keychain item'ini o'chirish admin
trust settings'ni qoldirishi aniqlandi; cleanup endi exact DER uchun admin trust
settings va keychain item'ini alohida o'chiradi.[^mac-trust] Browser CLI chaqiruvidagi
subcommand xatosi ham tuzatildi. Mac security chaqiruvlari local-only trust
check, noninteractive stdin, bounded output va10s deadline bilan bajariladi;
helper inputi bounded regular file. Serving context primary URL scheme'iga mos explicit secure-cookie true/false
beradi. Browser test avval Laravel13 standart `-session` nomini `_session` deb
qidirgan; fixture endi aniq SESSION_COOKIE beradi va atributlar tekshiriladi. `auto_https off` holatida Caddy default PKI'ni
keyinchalik yaratib trust install qilishga uringan; explicit local PKI va
skip_install_trust birgalikda berilib, bu yo'l o'chirildi. Expired CA install rad etiladi, exact-owned
CA removal esa private key va amaldagi validity'ni talab qilmaydi. Mac headless runner'da `remove-trusted-cert` hamda `delete-certificate -t`
authorization kutishida qolgan;10s timeout bilan error qaytishi tasdiqlandi.
Yangi removal exact CA'ni system-default/unspecified holatiga qaytarib, keychain
certificate'ini hash orqali o'chiradi va untrusted/absent holatini tekshiradi.
OS authorization siyosati o'zgartirilmaydi. Nontrusting admin metadata qolishi
mumkinligi helper effect'ida `may_retain_nontrusting_record` bilan ochiq beriladi;
metadata'ni jismonan tozalash GUI/uninstall gate'ida qoladi. Yangi usulning
Mac'da yangi removal hosts restore, certificate absence va untrusted holati bilan
o'tdi. TLS, secure cookie va WSS HMR ham upload bosqichigacha o'tdi; to'liq
suite yakuni hali pending. Bu ilk run'lar to'liq acceptance pass emas. Hosted macOS Chrome va Windows Edge .test/OS trust/
secure cookie/upload/Vite HMR natijalari faqat actual CI tugagach shu hujjatga
kiritiladi. Workflow mavjudligi uning muvaffaqiyatli o'tganiga dalil emas.[^native]

# Bosqich reconciliation

- **Mos:** shared runtime/context, privilege boundary, loopback/fallback, CA key ownership.
- **O'zgargan:** M3'da rejalashtirilgan serving backend endi mavjud; GUI hali M5.
- **Tekshirilmagan:** native hosted browser acceptance va clean 80/443 qamrovi hozir pending.
- **Amalga oshirilmagan:** installer-managed elevation UI, standalone daemon, per-project GUI Start/Stop va M5 interaction'lari.

[^request]: Suhbatdagi M4 topshirig'i.
[^serving]: Shared serving source va CLI caller.
[^helper]: Fixed-target OS adapters va digest-bound operation.
[^authority]: Root/leaf generation va validation.
[^local]: Mahalliy custom-root HTTP/TLS sinovi; system trust acceptance o'rniga o'tmaydi.
[^native]: Hosted disposable VM workflow; natijasi alohida qayd etiladi.

[^laravel-local]: Task-local Mac actual Laravel/PHP/PostgreSQL/Vite serving fixture; OS trust o'zgartirilmagan.
[^browser-driver]: Pinned Playwright CLI, named session va ignoreHTTPSErrors false; native natija alohida evidence'da.
[^windows-replace]: Microsoft native file replacement sharing, DACL va metadata semantikasi.
[^mac-trust]: Apple admin trust settings va keychain certificate alohida obyekt ekanini hujjatlashtiradi.
