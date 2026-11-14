---
type: Development Milestone
title: E01 first-success checklist boshlangich UI
description: Mavjud snapshot va amallarga tayangan checklist, RU UZ EN va scoped browser dalili.
status: draft
decision_status: confirmed
tags: [e01, onboarding, ui, localization, verification]
sources:
  - id: plan
    resource: /integrated-execution-plan.md
    title: E01 va UI skill routing talablari
  - id: checklist
    resource: ../../apps/desktop/src/components/FirstProjectChecklist.tsx
    title: Saved va observed faktlar checklist projection
  - id: integration
    resource: ../../apps/desktop/src/pages/WorkspacePage.tsx
    title: Mavjud project detail integratsiyasi
  - id: tests
    resource: ../../apps/desktop/src/first-project.test.tsx
    title: Truthful status va navigation contract testlari
---

# Natija va scope

E01 uchun mavjud project detail ichiga collapsed `First project checks` disclosure
qo'shildi. Bu ikkinchi wizard emas; mavjud PHP/prepare, runtime/configuration,
services, browser va Inspector amallariga olib boradi. Project/snapshot faktlaridan
har renderda hosil qilinadi; alohida completion storage yoki avtomatik setup yo'q.
Saved holat reopen/retry'da mavjud source'dan ko'rinadi. UI checklist qo'shilgani
real user pilot yoki butun E01 acceptance yakunlangani degani emas.[^plan]

# Fakt va noma'lum chegarasi

- PHP/Node faqat aynan tanlangan installed, problemsiz ID bilan tekshiriladi.
- Exact Composer ID optional TS modelda aks etadi; eski/yo'q DTO qiymatida unknown,
  boshqa installed Composer bilan muvaffaqiyat hisoblanmaydi.
- Preparation faqat mavjud PHP diagnostikasidagi vendor/env/key faktlariga tayangan;
  diagnostika yo'q bo'lsa unknown. Platform requirementsning to'liq health proof'i emas.
- Snapshot/check freshness timestamp'i yo'q: UI oxirgi mavjud holat deb ochiq yozadi;
  fresh measurement yoki to'liq sog'lomlik deb ko'rsatmaydi.
- DB konfiguratsiyasi yoki database driver talabi, queue/Vite/mail tanlovi bor bo'lsa
  tegishli qadam chiqadi. DB binding/mail tanlovi configured, actual behavior hali verify.
- HTTP response va yangi Inspector request Row'da mavjud emas; ikkalasi ham manual
  verification holatida qoladi, running/PHP readiness bilan completed qilinmaydi.
- Busy/dirty holatda mutation shortcutlar disabled; mavjud unsaved navigation guard,
  error/cancel/retry va preparation data-preservation oqimi qayta ishlatiladi.

# UI manbalari va tekshiruv

Installed ui-ux-pro-max'da checklist feedback va React derived-state guidance,
ui-taste polish/operate/craft, uizze-ui local workflow va fresh
web-design-guidelines affected source review qo'llandi. Existing MASTER va tarixiy
M5 macOS Projects light1833x1344 baseline ko'rildi; yangi design system, tasvir
konsepti, paid Uizze material yoki native fidelity da'vosi yo'q. Scoped CSS mavjud
token, typography, focus va disclosure patternidan foydalanadi.

Focused `first-project.test.tsx` va existing `workspace.test.tsx`:12 passed
(6 yangi,6 existing). TypeScript va uch til locale validator o'tdi. Dastlabki
4 failure nested text test matcheriga tegishli edi; explicit status spanlardan
keyin focused testlar o'tdi. Product-wide Rust/frontend suite qayta bajarilmadi.

Agent-desktop PATH va odatiy local install manzillarida topilmadi. Native GUI
permissions o'zgartirilmadi. Owned1421 portda temporary isolated browser fixture
mavjud WorkspacePage va checklistni synthetic snapshot bilan render qildi.
Bu Tauri IPC/backend/native service ishini tekshirmaydi. Browser tekshiruvi:

| Holat | Qamrov va natija |
| --- | --- |
| EN/light1180x800 | Expanded checklist, summary→Tab runtime link keyboard focus, readable inherited tokens |
| RU/dark820x620 | Optional queue/Vite/mail, long text, expanded disclosure; horizontal overflow yo'q |
| UZ/light820x620 | Expanded disclosure, text wrapping; horizontal overflow yo'q; scoped axe WCAG2A/AA/2.1AA0 violation |

Uch element screenshot vizual ko'rildi: `output/playwright/e01/en-light-checks.png`,
`ru-dark-minimum-checks.png`, `uz-light-minimum-checks.png`. Bular local browser
artifactlar; committed/native dalil deb olinmaydi. Temporary preview HTML/TSX
va owned server tekshiruvdan keyin olib tashlandi. Console'dagi favicon404 fixturega
tegishli; product exception kuzatilmadi. Fresh guidelines affected component/CSS
va integration scope review'da hal qilinmagan finding qolmadi; butun app audit emas.

# Qolgan acceptance

Actual macOS ARM64/Intel va Windows Tauri interaction, production credentials,
minimum OS, six theme/locale combinations, observed first-success user pilot va
ona tilidagi human copy review hali unverified. Mac browser screenshots Windows
native proof emas. Userning yangi HTTP javobi yoki Inspector eventini avtomatik
verified qilish uchun kelajakdagi backend evidence kerak; ushbu slice buni yasamaydi.

[^plan]: [Birlashtirilgan bajarish rejasi](integrated-execution-plan.md).
[^checklist]: [Checklist source](../../apps/desktop/src/components/FirstProjectChecklist.tsx).
[^integration]: [Mavjud workspace](../../apps/desktop/src/pages/WorkspacePage.tsx).
[^tests]: [Focused tests](../../apps/desktop/src/first-project.test.tsx).
