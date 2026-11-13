---
type: Platform Compatibility
title: Windows 10 va Windows 11 mosligi
description: Windows 10 22H2 x64 installer va diagnostika, runtime support chegaralari va haqiqiy OS qabul holati.
status: draft
decision_status: confirmed
tags: [windows, compatibility, distribution, verification]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: Windows 10 topshirigi va test host mavjud emasligi
  - id: installer
    resource: ../../distribution/windows/hooks.nsh
    title: Native x64 va minimum build installer darvozasi
  - id: preflight
    resource: ../../crates/platform/src/distribution.rs
    title: Read-only prerequisite va chegara testlari
  - id: contracts
    resource: ../../scripts/test_distribution.py
    title: Installer va diagnostika floor parity
  - id: guide
    resource: ../../docs/guides/windows-10-tester-uz.html
    title: Ozbekcha Windows 10 qabul qollanmasi
  - id: webview
    resource: https://learn.microsoft.com/en-us/deployedge/microsoft-edge-supported-operating-systems
    title: Microsoft WebView2 OS support
  - id: node
    resource: https://raw.githubusercontent.com/nodejs/node/v24.x/BUILDING.md
    title: Node 24 Windows 10 minimum
  - id: php
    resource: https://www.php.net/manual/en/install.windows.manual.php
    title: PHP Windows minimum va VC runtime
  - id: postgres
    resource: https://www.postgresql.org/download/windows/
    title: EDB PostgreSQL platform qamrovi
  - id: dotnet
    resource: https://learn.microsoft.com/en-us/dotnet/core/install/windows
    title: NET Windows 10 edition lifecycle chegarasi
  - id: file-disposition
    resource: https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/ne-wdm-_file_information_class
    title: Extended disposition Windows 10 1709 dan mavjud
  - id: rust
    resource: https://doc.rust-lang.org/rustc/platform-support.html
    title: Rust Windows MSVC target minimum
  - id: mysql
    resource: https://dev.mysql.com/doc/refman/8.4/en/windows-installation.html
    title: MySQL8.4 Windows x64 va VC prerequisite
  - id: core-evidence
    resource: ../../docs/evidence/windows10/ci-core.json
    title: Joriy source macOS va Windows core build regressiyalari
  - id: local-evidence
    resource: ../../docs/evidence/windows10/local.json
    title: Mahalliy platform kontraktlari va guide browser tekshiruvi
  - id: installer-tests
    resource: ../../scripts/m6_package_smoke.py
    title: Release launch va installer preservation chegaralari
  - id: scan-state
    resource: ../../crates/workspace/src/discovery.rs
    title: Boshqa root uchun hisoblanadigan detached holati
  - id: acceptance-history
    resource: ../../docs/evidence/windows10/acceptance-history.json
    title: Dastlabki failure va harness tuzatishlari tarixi
  - id: installer-evidence
    resource: ../../docs/evidence/windows10/ci-installer.json
    title: Yakuniy native installer qabul source job va digest
  - id: handoff
    resource: ../../docs/evidence/windows10/tester-handoff.json
    title: Yangi Windows10 tester ZIP va checksum
---

# Talab va implementatsiya

User Windows 10 uchun moslashni topshirdi; test kompyuteri/VM mavjud emasligini
bildirdi. Tanlangan implementation minimum: Windows 10 22H2 build19045,
native Intel/AMD x64; Windows 11 qabul qilinadi. 22H2 chegarasi agentning moslik
tanlovi, barcha eski Windows10 nashrlarini qollash tasdigi emas. Oldingi M0/M6
Windows11/build22000 baseline shu topshiriq bilan almashtirildi.[^request]

Installer native AMD64, build>=19045 va VC v14 x64 talab qiladi; en/ru/uz xabarlari
moslandi. Preflight ham native architecture registry qiymatini oqiydi: x64
jarayonning ARM emulation holatini supported deb belgilamaydi. Build/arch yoq,
malformed yoki eski bolsa fail-closed. Per-user install, shared prerequisites,
data preservation va macOS minimum13.5 saqlanadi.[^installer][^preflight]

# Runtime va API auditi

| Qism | Manba va chegara |
| --- | --- |
| WebView2 | Microsoft Windows10 22H2 update supportini kamida2028-10 gacha korsatadi; actual DevForge UI qabuli baribir kerak.[^webview] |
| PHP8.4/8.5 x64 | PHP8.3+ uchun Windows8+ va mos VC runtime; joriy pinlar ozgarmadi.[^php] |
| Node22/24 x64 | Node24 branch Windows10/Server2016+ ni korsatadi; katalog tanlovlari saqlandi.[^node] |
| Rust MSVC x64 | Rasmiy target Windows10+/Server2016+ ni korsatadi; toolchain ozgarmadi.[^rust] |
| MySQL8.4 x64 | Windows64 va VC2019+ talab qilinadi; provider platform lifecycle alohida, Win10 exact archive native qabuli ochiq.[^mysql] |
| PostgreSQL18 | EDB server2022/2025 da test qiladi, comparable desktop uchun expectation bildiradi; bu Win10 certification emas.[^postgres] |
| Garnet/.NET10 | Joriy vendor support Windows10 LTSC/Enterprise bilan cheklangan. Home/Pro22H2 uchun vendor support da'vosi yoq; actual runtime unverified.[^dotnet] |
| Archive purge | Extended disposition1709+ da mavjud. Readonly hardlink source atributlarini ozgartirmaydigan implementatsiya saqlandi; Win10 filesystem xulqi hali native tekshirilmagan.[^file-disposition] |

Caddy, MySQL8.4, Xdebug DLL, credential/Job/ACL/trust adapterlari va native loaderlar
uchun actual19045 qabul ochiq; Server2025 natijasidan Win10 runtime proof
chiqarilmaydi. Paket/SDK versiyalari va user selections ozgarmadi.

# Installer tekshiruvi chegarasi

Dastlabki13.6MB installer build, native prerequisite va uch locale startup otdi;
butun metadata fayli hashini uch GUI launchdan oldingi holat bilan solishtirish
preservation assertionni yiqitdi. Replay faqat `discovery_entries` farqini aniqladi:
fixture seed project unutilgan, app boshqa default rootni scan qilganda entry
`state`ini `detached`ga yangilashi source kontrakti. Bu yangi Win10 product xatosi
deb korsatilmaydi.[^acceptance-history][^scan-state]

Harness launch va installer amallarini ajratadi. Launch orasida mavjud barcha
jadval/yozuvlar saqlanadi; faqat discovery recordning hisoblanadigan `state` va
`problem` maydonlari normalize qilinadi. Identity, path, project binding va
`ignored` flag ozgarishi regression testda rad etiladi. Boshqa mavjud project/DB
fayllari launchda ham byte-identical; installer install/reinstall/uninstall uchun
post-launch snapshotning barcha fayl hashlarida qat'iy tenglik saqlanadi. Python
SQLite handlelari Windows cleanup uchun aniq yopiladi. Product binary ozgarmadi:
replay faqat helper/workflow/doc farqiga ruxsat berib, original installer hashini
tekshiradi.[^installer-tests][^contracts][^acceptance-history]

# Dalillar va phase reconciliation

| Claim | Holat | Dalil |
| --- | --- | --- |
| Installer/preflight19045 floor | changed | NSIS/Rust source va parity regression.[^installer][^preflight][^contracts] |
| Win10/11 x64 positive, old/malformed/ARM negative | matching | Mahalliy platform suite10 passed/1 credential ignored;7 distribution contracts passed. |
| macOS13.5 va data-preserving lease | matching | Source unchanged; minimum/lease testlari otdi. |
| Joriy core va desktop binary | matching | bb763e6: macOS135/Windows132 Rust passed, har OS44 frontend/7 distribution;1 native credential ignored/OS. fmt/clippy/locales/native build va31-doc strict otdi.[^core-evidence] |
| Windows release build/installer regression | matching | Installer bb763e6, test661b76b: Server2025 build26100 ordinary token,11 acceptance check/3 locale,2140 project/DB file preservation va10 harness contract otdi.[^installer-evidence] |
| Haqiqiy Windows10 22H2 desktop/runtime | unverified | Host/VM mavjud emas.[^request] |
| Signed/public release | not implemented | Avvalgi M15 gate saqlanadi; internal unsigned paket. |

Windows10 qabul jarayoni va edition cheklovi ozbekcha HTMLda.[^guide] Guide1360x900
va390x844 loopback browserda korildi: overflow0, broken internal links0,
external assets0; ikkala screenshot vizual tekshirildi.[^local-evidence]
OKF validatsiyasi runtime/OS ishlashini isbotlamaydi.

Installer `af159ea75aa99b0207403a6756442e3bb31118fe930c00e209f469ca5ef5734f`.
Yangi tester ZIP13,591,163 byte: HTML, installer, BUILD-INFO va SHA256SUMS birga;
CRC, har bir file hash va guide/source tengligi tekshirildi.[^handoff]
Initial run43467683180 build qildi; installer recompile qilinmasdan exact source
parity bilan run43474984058 da yakuniy qabuldan otdi. Test-only native fixture
helper debug profileda, o'rnatilgan desktop va sidecarlar original release.
Core sourcebb763e6 va final code orasida faqat uch helper/workflow fayli farqi;
product crate/app/runtime/lock/config unchanged. Yakuniy knowledge/source
solishtirishdan keyin docs-only push uchun product suite takrorlanmaydi.[^installer-evidence]

[^request]: [Topshiriq qaydi](references/conversation-decisions.md).
[^installer]: [NSIS hook](../../distribution/windows/hooks.nsh).
[^preflight]: [Platform preflight](../../crates/platform/src/distribution.rs).
[^contracts]: [Distribution contracts](../../scripts/test_distribution.py).
[^guide]: [Tester HTML](../guides/windows-10-tester-uz.html).
[^webview]: [Microsoft OS support](https://learn.microsoft.com/en-us/deployedge/microsoft-edge-supported-operating-systems).
[^node]: [Node24 platform matrix](https://raw.githubusercontent.com/nodejs/node/v24.x/BUILDING.md).
[^php]: [PHP Windows talablari](https://www.php.net/manual/en/install.windows.manual.php).
[^postgres]: [PostgreSQL Windows](https://www.postgresql.org/download/windows/).
[^dotnet]: [NET Windows talablari](https://learn.microsoft.com/en-us/dotnet/core/install/windows).
[^file-disposition]: [Microsoft file class](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdm/ne-wdm-_file_information_class).
[^rust]: [Rust platform support](https://doc.rust-lang.org/rustc/platform-support.html).
[^mysql]: [MySQL Windows talablari](https://dev.mysql.com/doc/refman/8.4/en/windows-installation.html).
[^core-evidence]: [Core CI source/job natijalari](../evidence/windows10/ci-core.json).
[^local-evidence]: [Local/guide dalili](../evidence/windows10/local.json).
[^installer-tests]: [Installer native smoke](../../scripts/m6_package_smoke.py).
[^scan-state]: [Discovery reconcile](../../crates/workspace/src/discovery.rs).
[^acceptance-history]: [Failure va fix tarixi](../evidence/windows10/acceptance-history.json).
[^installer-evidence]: [Installer qabul dalili](../evidence/windows10/ci-installer.json).
[^handoff]: [Tester ZIP qaydi](../evidence/windows10/tester-handoff.json).
