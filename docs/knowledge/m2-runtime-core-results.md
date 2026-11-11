---
type: Implementation Report
title: M2 runtime paketlari va jarayon yadrosi
description: Staged package store, cancellation, process ownership, readiness va tekshirish dalillari.
status: draft
decision_status: confirmed
tags: [m2, runtimes, packages, processes, evidence, windows, macos]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: M2 test va push topshirig'i
  - id: store
    resource: ../../crates/runtime/src/package.rs
    title: Exact package store va executable leases
  - id: extraction
    resource: ../../crates/runtime/src/archive.rs
    title: Bounded portable extraction
  - id: supervisor
    resource: ../../crates/runtime/src/process/mod.rs
    title: Process supervisor va holatlar
  - id: tree
    resource: ../../crates/runtime/src/process/tree.rs
    title: Unix process group va Windows Job adapterlari
  - id: smoke
    resource: ../evidence/m2/macos-catalog-smoke.json
    title: Real macOS paketlari va parallel Node HTTP fixture'lari
  - id: https
    resource: ../evidence/m2/https-cli-smoke.json
    title: Haqiqiy HTTPS download va CLI probe/remove
  - id: checks
    resource: ../evidence/m2/local-checks.json
    title: Mahalliy M2 test va tekshirish xulosasi
  - id: catalog
    resource: ../../runtime/m2-catalog.json
    title: M0 publisher digest va layout'dan hosil qilingan 22 descriptor
  - id: jobs
    resource: https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects
    title: Windows Job Object ownership
  - id: darwin
    resource: https://raw.githubusercontent.com/apple-oss-distributions/xnu/main/bsd/sys/proc_info.h
    title: Darwin process-group member holatlari
---

# Natija

Foydalanuvchi M2 implementation, test, OKF va push'ni topshirdi.[^request]
`crates/runtime` Tauri'dan mustaqil paket va jarayon yadrosini beradi. CLI shu
production qatlamga ulandi. Desktop'da runtime boshqaruv UI/IPC hali yo'q;
Project binding, DB boshqaruvi, DNS/HTTPS va installer M3-M6'ga tegishli.
Bosqichning native platforma va host-crash qamrovi quyida ochiq ajratiladi.

# Paket kontrakti

`Package` kind/version/variant/OS-arch, URL, SHA-256, archive layout va named
executable'larni belgilaydi. Variant macOS PHP CLI/FPM'ni ajratadi. Identity
tuple'ining hash'i katalog nomini beradi; exact versiyalar yonma-yon turadi.
Catalog refresh loyiha tanlovini o'zgartirmaydi; hozir Project binding yo'q.
[22 descriptor](../../runtime/m2-catalog.json) M0 auditidagi publisher digest va
member layout'lardan olindi. Composer PHAR va metadata-only alternativalar
native package modeli ichiga majburan qo'shilmadi. Uchinchi tomon notice/signing
va M0 release gate'lari saqlanadi.[^catalog]

Oqim: HTTPS yoki cached import -> owned staging -> digest -> bounded extraction
-> executable header/arch va hash -> receipt -> same-filesystem rename.
Rename commit nuqtasi; undan oldingi cancel/failure aktiv paketga aylanmaydi.
Mavjud ayni identity'da boshqa descriptor/digest bo'lsa conflict, avtomatik
overwrite yo'q. Takroriy install receipt va executable hash'larini tekshiradi.
Oldingi boshqa versiya saqlanadi. Restartdan keyin store lock ostida faqat
owned `install-*` staging tozalanadi; begona katalog yoki DB data emas.[^store]

HTTPS transport reqwest/rustls bilan certificate validation'ni saqlaydi;
credential URL/plain HTTP rad etiladi. In-flight network future cancellation
bilan bekor qilinadi; file/extraction copy chunk'larida ham cancellation bor.
Default limitlar: 2 GiB download, 4 GiB unpacked, 100000 entry, XZ decoder
uchun 256 MiB memory limit. Network connect 10s va request 300s chegaraga ega.
Progress callback streaming; yadro o'zi cheksiz progress queue yaratmaydi.
Kutubxonaning synchronous install API'si worker thread'da ishlatilishi kerak.

Tar.gz, tar.xz, ZIP va aniq ZIP/JAR ichidagi tar.xz qo'llanadi. Portable path
tekshiruvi traversal, absolute/drive/ADS/device nomlar, duplicate case-insensitive
member va maxsus file type'larni rad etadi. Symlink yozuvlari barcha file write
tugagach faqat in-tree mavjud targetga yaratiladi. Windows'da ichki file alias
copy qilinadi; symlink privilege talab qilinmaydi. Hardlink va noma'lum entry
rad etiladi. Setuid/setgid publisher permission'lari saqlanmaydi.[^extraction]

Executable resolve receipt/hash'ni tekshiradi va shared OS lease saqlaydi.
Paket removal exclusive lease ololmasa `busy` qaytaradi; ishlayotgan paket
o'chirilmaydi. Removal faqat shu store package directory'siga tegadi. Bu lease
DevForge operatsiyalarini muvofiqlashtiradi, malicious same-user filesystem
o'zgarishlariga qarshi privilege sandbox emas.

# Jarayon kontrakti

`ProcessSpec` verified executable, argument array, working directory, explicit
environment, readiness, grace, restart va log limitlarini oladi. `env_clear`
global env tasodifan o'tishini cheklaydi; CLI kerakli OS path/temp qiymatlarini
aniq uzatadi. Shell interpolation yo'q. Process ID uchun in-memory guard va OS
file lock boshqa supervisor instance bilan duplicate launch'ni rad etadi.
Supervisor lifetime oynadan alohida bo'lishi uchun uni composition root egallaydi;
M2 desktop shell'ga hali ulanmagan.[^supervisor]

Holatlar: Starting -> Ready yoki timeout/failure; crash'dan keyin bounded Backoff
va yangi attempt; Stop -> Stopping -> Stopped. One-shot successful-exit probe
Succeeded bo'ladi. Spawn Ready degani emas: stdout/stderr marker yoki muvaffaqiyatli
exit kerak. Cancel startup/readiness/backoff'ni ham to'xtatadi. Default avtomatik
restart yo'q; maksimum10, configured backoff maksimum60s. Maksimum64 process
handle va 32 transition history; tugagan handle'ni stop orqali bo'shatish mumkin.

macOS: yangi process group; WNOWAIT root PID'ni group cleanup tugaguncha saqlab,
PID/PGID reuse xavfini cheklaydi. Stdin quit va SIGTERM'dan keyin grace, so'ng
SIGKILL va wait. Darwin zombie-only guruhining EPERM holati libproc orqali
member holatlari bilan ajratiladi; live denial yashirilmaydi.[^tree][^darwin]

Windows: CREATE_SUSPENDED orqali spawn, kill-on-close Job Object'ga assign,
so'ng primary thread resume. Child assign'dan oldin ishlamaydi. Stdin quit
cooperative protokol; uni qo'llamaydigan runtime grace'dan keyin faqat owned job
orqali tugatiladi. Headless GUI'da shared console bor deb Ctrl+C yuborilmaydi.
Bu barcha runtime'lar uchun application-level graceful shutdown isboti emas.[^jobs]

Root child crash qilganda qolgan owned descendant'lar attempt bilan cleanup
qilinadi. Supervisor handle Stop/Drop ham cleanup/join qiladi. **Abrupt macOS
supervisor SIGKILL/power-loss va atayin setsid bilan guruhdan chiqadigan daemon**
uchun guardian/recovery hali yo'q; foreground runtime kontrakti kerak. Windows
job handle yopilishi uchun OS mexanizmi yozilgan, alohida host-crash acceptance
sinovi hali yo'q. Saqlangan PID orqali begona process'ni keyinroq kill qiladigan
recovery yozilmadi; bu xavfsiz tasdiqlanmagan cleanup o'rniga ochiq gapdir.

Har stdout/stderr reader 4KiB chunk ishlatadi. Default memory ring64KiB,
persisted log 1MiB x3, sequence history32. Loglar raw output; credential chiqarish
mumkin emas. Unix store/log root0700. Log write failure stable code bilan attempt'ni
to'xtatadi; cheksiz output/string/event to'planmaydi.

# CLI va tillar

`runtime catalog`, `runtime install <id> [cached-archive]`,
`runtime probe <id> <executable>`, `runtime remove <id>` yaratildi. Machine JSON
status/error code beradi. `DEVFORGE_DATA_DIR` vaqtinchalik test ildizini ajratadi.
Renderer uchun arbitrary shell endpoint qo'shilmagan. Barcha18 runtime error
code uchun en/ru/uz-Latn `runtime.errors` kataloglari bor; native UI'da M2 error
flow hali ko'rsatilmagan. Key parity va code->catalog qamrovi test qilingan.

# Sinov va dalillar

Mahalliy host: macOS27.2 ARM64. M2 fixture testlari vaqtinchalik store/loglar,
test executable va task-local process'larda ishladi; mavjud Herd, 80/443/5432,
hosts/trust va real project/database o'zgartirilmadi.

- M2: 3 unit + 11 package + 7 process =21 test o'tdi. Checksum, arch, cancel,
  duplicate, traversal/link/limits, stale staging, executable tamper, removal lease,
  disk-full error mapping, pending-network cancel, three-language coverage;
  argv/env/cwd, readiness, timeout, bounded restart/backoff cancel, forced stop,
  bounded logs, crash/Drop descendant cleanup va unrelated process preservation.
- `cargo test --workspace`:28 o'tdi; native credential fixture odatiy rejimda1 ignored.
- `cargo clippy --workspace --all-targets -- -D warnings`: o'tdi. Rustfmt qo'llandi.
- Frontend Vitest8 va locale validator Node testlari3 o'tdi; catalog lint3 til
  kalit/placeholder/plural kontraktini tasdiqladi.
- [Real package smoke](../evidence/m2/macos-catalog-smoke.json):8 paket install,
  11 probe (PHP CLI/FPM, Node/npm, Caddy, PostgreSQL/initdb). Ikki Node versiyasi
  parallel loopback HTTP200 berdi; running removal Busy, stop'dan keyin removal o'tdi.
- [HTTPS CLI smoke](../evidence/m2/https-cli-smoke.json): Caddy publisher URL'dan
  real TLS download, checksum/install, managed version probe va removal —3 buyruq
  exit0. Alohida vaqtinchalik data directory ishlatildi.[^https]
- Strict OKF:16 hujjat,0 error,0 warning. Bu product testi emas.[^checks]

Disk-full sinovi haqiqiy diskni to'ldirmaydi: failing writer orqali error mapping
tekshirildi. Pending-network sinovi cancellation future'ni tekshiradi; cached
install cancel alohida real file streaming'da tekshirildi. Smoke clean machine
yoki release redistribution testi emas.

# Kod va bilim solishtiruvi

| Da'vo | Holat va dalil |
| --- | --- |
| Immutable exact package store va safe activation mavjud. | Mos: package/archive source, test va 8 paket smoke. |
| Process spawn avtomatik Ready emas. | Mos: output/exit readiness va delayed/timeout fixture testlari. |
| Root process crash, Stop/Drop descendant'larni tozalaydi. | Mos, qayd etilgan Mac foreground fixture qamrovida; unrelated child saqlandi. |
| Barcha supervisor hard-crash/daemon holatlarida orphan yo'q. | Amalga oshirilmagan/to'liq tekshirilmagan; yuqoridagi guardian gap'i ochiq. |
| Runtime error'lari uch tilda mavjud. | Mos: catalogs + mapping/lint; M2 native error screen flow hali yo'q. |
| Windows runtime manager native acceptance'dan o'tgan. | CI natijasi alohida dalil bilan yangilanadi; mahalliy Windows host yo'q. |
| Project runtime/DB binding va desktop service boshqaruvi bor. | Amalga oshirilmagan; M3-M5. |
| Minimum OS/Intel/signing/release tayyor. | Tekshirilmagan; M0/M1 gate'lari saqlanadi. |

GitHub Actions yoqilganligi tekshirildi; M2 push uchun macOS/Windows workflow
ishlatiladi. Remote natijalar local Mac natijasi bilan aralashtirilmaydi. Bu
hisobotning lifecycle `draft` holati inson tasdig'i emas; OKF validator semantik
runtime isbotini bermaydi.

[^request]: M2 kod/test/OKF/push ko'rsatmasi.
[^store]: Synchronous worker API, cancellation va same-filesystem activation.
[^extraction]: Faqat owned staging, bounded va portable path qoidalari.
[^supervisor]: Background attempt worker va bounded state/log publication.
[^tree]: Platformaga xos ownership va cleanup; taskkill/process-name matching yo'q.
[^smoke]: Yangi Rust yadrosi orqali M0 cached paketlarining haqiqiy bajarilishi.
[^https]: Cached import'dan alohida real HTTPS va CLI integratsiyasi.
[^checks]: Agent-curated buyruq natijalari; mustaqil attestation emas.
[^catalog]: Provider sha/layout snapshot; release-ready deklaratsiyasi emas.
[^jobs]: Windows'ning process tree ownership primitivlari haqidagi rasmiy hujjat.
[^darwin]: Apple proc_info strukturalari va guruh member holatini tekshirish API'si.
