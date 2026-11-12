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
---

# Joriy holat

Foydalanuvchi M6 bosqichiga o'tishni topshirdi.[^request] Ichki paketlash va
qabul dalillari ustida ish davom etmoqda. Public release va automatic updater
bu topshiriq tarkibiga kiritilmadi.

Release CLI/helper sidecar, dependency notice matnlari va CycloneDX inventory
bilan app/DMG yoki per-user NSIS yaratish oqimi qo'shildi.[^packaging]
Native installer/reinstall/uninstall va uch tildagi release shell o'lchovlari
alohida test bilan tekshiriladi; hali test o'tgan deb belgilanmaydi.[^acceptance]

Windows app-data deletion varianti chiqarildi. Desktop lifecycle lock uninstall
oldidan Quit'ni talab qiladi. Cleanup rejalari faqat owned hosts/CA uchun mavjud
helper digest kontraktidan foydalanadi; user loyihalari va DB data saqlanadi.
Mac Finder Trash operatsiyasi cleanup hook chaqirmaydi; CLI preflight zarur.
Signing credentials mavjud emas. Minimum OS, signed notarized distribution,
provider licensing review va Mac inactive trust metadata jismoniy olib tashlash
qabul gate'lari alohida ochiq.[^contract]

[^request]: Joriy M6 ko'rsatmasi; oldingi code/test/OKF/push ketma-ketligi saqlanadi.
[^packaging]: Toolchain va dependency versiyalari amaldagi lockfile'lardan olinadi.
[^acceptance]: Release va debug-E2E o'lchovlari aralashtirilmaydi.
[^contract]: Ichki unsigned/ad-hoc distributiv public signed release emas.
