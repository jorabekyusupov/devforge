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
| PostgreSQL18 | EDB server2022/2025 da test qiladi, comparable desktop uchun expectation bildiradi; bu Win10 certification emas.[^postgres] |
| Garnet/.NET10 | Joriy vendor support Windows10 LTSC/Enterprise bilan cheklangan. Home/Pro22H2 uchun vendor support da'vosi yoq; actual runtime unverified.[^dotnet] |
| Archive purge | Extended disposition1709+ da mavjud. Readonly hardlink source atributlarini ozgartirmaydigan implementatsiya saqlandi; Win10 filesystem xulqi hali native tekshirilmagan.[^file-disposition] |

Caddy, MySQL8.4, Xdebug DLL, credential/Job/ACL/trust adapterlari va native loaderlar
uchun actual19045 qabul ochiq; Server2025 natijasidan Win10 runtime proof
chiqarilmaydi. Paket/SDK versiyalari va user selections ozgarmadi.

# Dalillar va phase reconciliation

| Claim | Holat | Dalil |
| --- | --- | --- |
| Installer/preflight19045 floor | changed | NSIS/Rust source va parity regression.[^installer][^preflight][^contracts] |
| Win10/11 x64 positive, old/malformed/ARM negative | matching | Mahalliy platform suite10 passed/1 credential ignored;7 distribution contracts passed. |
| macOS13.5 va data-preserving lease | matching | Source unchanged; minimum/lease testlari otdi. |
| Windows release build/installer regression | unverified | Joriy source uchun hosted CI keyingi tekshiruvda qayd qilinadi. |
| Haqiqiy Windows10 22H2 desktop/runtime | unverified | Host/VM mavjud emas.[^request] |
| Signed/public release | not implemented | Avvalgi M15 gate saqlanadi; internal unsigned paket. |

Windows10 qabul jarayoni va edition cheklovi ozbekcha HTMLda.[^guide]
OKF validatsiyasi runtime/OS ishlashini isbotlamaydi.

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
