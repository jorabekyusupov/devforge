---
type: Design Policy
title: UI UX skill'lardan foydalanish
description: To'rtta UI skill'ning vazifasi, ishga qarab ketma-ketligi va desktop tekshiruvi.
status: draft
decision_status: confirmed
tags: [ui, ux, accessibility, desktop, skills]
sources:
  - id: conversation
    resource: /references/conversation-decisions.md
    title: To'rtta UI UX skill'ni o'rganish va ulardan foydalanish topshirig'i
---

# Tasdiqlangan qoida

Foydalanuvchi DevForge UI/UX ishlarida `ui-ux-pro-max`, `ui-taste`, `uizze-ui`
va `web-design-guidelines` skill'larini o'rganib, qachon qaysi biri birinchi
ishlatilishini belgilashni so'ragan. Routing loyiha skill'iga qo'shilgan.[^conversation]

| Skill | Vazifa |
| --- | --- |
| ui-ux-pro-max | UX, design-system, accessibility va tegishli stack bo'yicha qarorlar. |
| ui-taste | Mahsulotga mos vizual yo'nalish, rasmli nishon, polish va craft. |
| uizze-ui | Interfeysni amalga oshirish/tekshirish va kerakli referens yoki asset'lar. |
| web-design-guidelines | WebView UI kodi bo'yicha amaldagi qoida auditi. |

# Routing qarori

Yangi ekran uchun tartib: `ui-ux-pro-max` -> `ui-taste` -> `uizze-ui` ->
`web-design-guidelines` -> vizual va interaction tekshiruvi. Mavjud rasm,
polish, accessibility xatosi va read-only audit uchun boshlanish nuqtasi farq
qiladi; har safar to'rtta skill'ning barcha bosqichlarini bajarish talab emas.

To'liq ketma-ketlik, tashqi skill yo'llari va istisnolar uchun yagona amaliy
manba: [UI/UX workflow](../../.agents/skills/devforge/references/ui-ux-workflow.md).
Ushbu bilim yozuvi tashqi skill matnlarining nusxasi emas.

# Muhim chegaralar

- Tasdiqlangan rasm qayta yaratilmaydi. Yangi vizual ishda mavjud va ruxsatli
  image-generation vositasi bilan bitta tekshirilgan nishon yaratiladi.
- Kichik texnik tuzatish yoki read-only audit uchun yangi konsept rasmi kerak emas.
- Uizze'ning pullik MCP resurslari ixtiyoriy; ularning yo'qligi lokal ishni to'xtatmaydi.
- Desktop vazifasiga mobil navigation yoki safe-area qoidalari avtomatik yuklanmaydi.
- Rasm, kod auditi, brauzer preview'i va real Tauri tekshiruvi turli dalillar.
- Vizual qarorlar backend imkoniyati yoki ishlayotgan servis mavjudligini o'ylab topishga asos emas.

[^conversation]: Foydalanuvchi sanab bergan to'rtta skill va ulardan foydalanish tartibini DevForge skill'iga qo'shish topshirig'i.
