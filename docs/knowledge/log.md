# DevForge bilim bazasi tarixi

## 2026-11-11

- **M1 poydevori**: Cargo workspace, Tauri/React desktop shell, SQLite migration, OS secret adapter, CLI doctor, shared en/ru/uz-Latn kataloglari va CI matrix yaratildi. [M1 hisobotida](m1-foundation-results.md) oldin o'tgan testlar, native macOS app/menu/re-launch dalillari va Windows/Intel/minimum OS cheklovlari ajratildi. Foydalanuvchining qayta test qilmasdan push ko'rsatmasi sabab avvalgi test suite'lari qayta bajarilmadi; bu push uchun CI skip belgisi ishlatiladi.
- **M0 dalillari**: [Runtime natijalari](m0-runtime-findings.md), aniq artifact/target kataloglari va izolyatsiyalangan smoke runner'lar qo'shildi. 23 artifact publisher SHA-256'ga mos keldi; ikki muqobil manba metadata-only qoldi. ARM64 va x64/Rosetta'da ikki PHP, ikki Node, Laravel/PDO/HTTP/TLS/Vite transport sinovlari o'tdi. 44 tooling/validator testi o'tdi; Windows, native Intel/minimum OS va release gate'lari ochiq saqlandi.
- **MVP reja**: Platforma/runtime feasibility'dan installer va uch tilli qabul matritsasigacha M0-M6 [bajarish rejasi](mvp-phase-1-plan.md) yaratildi. Scope va yangi texnik tanlovlar taklif, implementation hali boshlanmagan.
- **Til talabi**: Rus, o'zbek va ingliz tillari hamda kengayadigan locale registry/catalog modeli [i18n kontraktiga](localization.md) yozildi; skill va tekshirish qoidalari bog'landi. Ilova tarjimalari va runtime hali yaratilmagan.
- **Validator**: [read-only skript](../../scripts/validate_knowledge.py), uning development dependency'lari va regression testlari qo'shildi; [foydalanish yo'riqnomasi](validation-tooling.md) indeksga bog'landi.
- **Yuritish**: Muhim vazifa yakunidagi knowledge-impact tekshiruvi, strict validatsiya va bosqich oxirida kod/test dalillari bilan solishtirish [siyosatga](knowledge-policy.md) hamda skill'ga qo'shildi.
- **Yaratish**: OKF 0.2 formatida [indeks](index.md) va mavzular bo'yicha bilim yozuvlari yaratildi.
- **Qarorlar**: [mahsulot talablari](product-scope.md), [muhandislik siyosati](engineering-policy.md) va [UI/UX siyosati](ui-ux-policy.md) qayd etildi.
- **Ajratish**: [arxitektura](architecture.md) va [loyiha muhitlari](project-environments.md) ishchi yo'nalish, [MVP](mvp-proposal.md) esa tasdiqlanmagan taklif sifatida belgilandi.
- **Manba**: [suhbat qaydi](references/conversation-decisions.md) yaratildi; inson tekshiruvi yoki runtime natijasi o'ylab topilmadi.
- **Integratsiya**: AGENTS.md va DevForge skill'iga bilim bazasini o'qish, tegishli yozuvni yangilash va statuslarni ajratish qoidasi qo'shildi.
