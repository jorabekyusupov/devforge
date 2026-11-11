---
type: Development Tool
title: DevForge OKF validator
description: Read-only tekshirish, strict rejim, o'rnatish, testlar va qamrov cheklovlari.
status: draft
decision_status: confirmed
tags: [okf, validation, tooling, tests]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: Validator va bilimlarni doimiy yuritish topshirig'i
  - id: validator
    resource: ../../scripts/validate_knowledge.py
    title: Validator implementatsiyasi
  - id: tests
    resource: ../../tests/test_validate_knowledge.py
    title: Validator regression fixture'lari
---

# Vazifa va qamrov

Foydalanuvchi validator yaratish va muhim vazifa/bosqich yakunida bilimlarni
yangilash qoidasini qo'shishni topshirgan.[^request] Skript YAML va Markdown'ni
parse qiladi, fayllarni o'qiydi; ularni yozmaydi, tashqi URL'larni chaqirmaydi
yoki bilim ichidagi buyruqlarni bajarmaydi.[^validator]

| Tekshiruv | Natija turi |
| --- | --- |
| UTF-8, YAML mapping, yopilgan frontmatter, takroriy YAML kalitlari va konseptning type maydoni | Format xatosi. |
| Rezerv index/log metadata va log sanasi | Format xatosi. |
| Mahalliy havola/manba, root/child indeks orqali konseptga yetishish, footnote/source ID bog'lanishi | Sifat ogohlantirishi. |
| Lifecycle, DevForge decision_status, optional trust metadata, timezone va stale_after | Sifat ogohlantirishi. |

OKF ruxsat beradigan noma'lum type/qo'shimcha metadata qabul qilinadi.
Ixtiyoriy maydon yoki index/log yo'qligi minimal OKF konseptini buzilgan deb
hisoblamaydi. DevForge root index/log kutadi; bunday sifat muammolari strict
rejimda tekshiruvni yiqitadi. Markdown kod bloklari va inline code ichidagi
namuna havolalar bilim havolasi deb tekshirilmaydi.[^validator]

# O'rnatish

Dependency'lar faqat ushbu development vositasiga tegishli; desktop ilova uchun
Python runtime majburiyati yaratmaydi. Mos Python 3 interpreter va alohida venv
ishlating. Versiyalar skill matnida emas,
[requirements faylida](../../scripts/requirements-knowledge.txt) belgilangan.

macOS/Linux terminalida, repozitoriy ildizidan:

```sh
python3 -m venv .venv-knowledge
.venv-knowledge/bin/python -m pip install -r scripts/requirements-knowledge.txt
.venv-knowledge/bin/python scripts/validate_knowledge.py --strict
```

Windows PowerShell uchun ekvivalent buyruqlar:

```powershell
py -3 -m venv .venv-knowledge
.venv-knowledge\Scripts\python.exe -m pip install -r scripts/requirements-knowledge.txt
.venv-knowledge\Scripts\python.exe scripts/validate_knowledge.py --strict
```

Oldindan tayyorlangan tooling interpreter bo'lsa, o'shani ishlating; global
Python muhitiga dependency o'rnatish yoki mavjud venv'ni qayta yaratish kerak emas.
Skill'dagi `python3` shu tayyor interpreter'ni anglatadi.

# Ishlatish

```sh
python3 scripts/validate_knowledge.py
python3 scripts/validate_knowledge.py --strict --json
python3 scripts/validate_knowledge.py --repo-root /path/to/devforge
python3 scripts/validate_knowledge.py --repo-root /path/to/repo --bundle docs/other-bundle
```

Default repo ildizi skript joylashuvidan aniqlanadi, shell working directory'dan
emas. `--bundle` nisbiy bo'lsa `--repo-root`ga nisbatan ishlaydi. Oddiy rejimda
faqat format xatolari, `--strict` rejimida ogohlantirishlar ham exit 1 beradi.
Muvaffaqiyat exit 0; noto'g'ri CLI yoki yo'q bundle exit 2. Yetishmayotgan
dependency uchun aniq o'rnatish xabari beriladi. `--json` CI uchun mo'ljallangan;
hozir repozitoriyga CI workflow qo'shilmagan.[^validator]

# Regression tekshiruvlari

Tayyorlangan interpreter bilan:

```sh
python3 -m unittest discover -s tests -p 'test_validate_knowledge.py' -v
```

Fixture'lar noto'g'ri YAML, xavfsiz parsing, minimal/unknown type, ixtiyoriy
metadata, indekslar, relative/reference linklar, kod namunalari, manbalar,
footnote'lar, CLI exit code va read-only xatti-harakatni qamrab oladi.[^tests]

2026-11-11 kungi tekshiruv: macOS'da izolatsiyalangan Python muhitida 27 ta test
o'tdi. Windows'da ishga tushirish hali tekshirilmagan; yuqoridagi PowerShell
buyruqlari tekshirilgan Windows natijasi sifatida ko'rsatilmaydi.

# Cheklovlar va bosqich tekshiruvi

Validator format va bog'lanishlarni tekshiradi. U qarorning to'g'riligi, inson
tasdig'i, to'liq Markdown/HTML semantikasi, anchor nomlari, tashqi URL mavjudligi,
runtime va kod/hujjat mosligini isbotlamaydi. Optional metadata uchun asosiy
shape tekshiruvlari bor; u OKF'ning barcha ixtiyoriy oilalari uchun to'liq schema
sertifikatori emas.

Bosqich oxirida kod, config va real test natijalari bilan solishtirishni agent
[knowledge policy](knowledge-policy.md) bo'yicha alohida bajaradi. Shu vazifadagi
solishtirish skript CLI'si, qoidalar, dependency'lar va testlarga tegishli;
DevForge desktop runtime'i hali yaratilmagan va tekshirilmagan.

[^request]: Suhbat manbasining validator va doimiy yuritish bo'limi.
[^validator]: Repozitoriydagi read-only validator kodi va uning CLI interfeysi.
[^tests]: Repozitoriydagi izolatsiyalangan vaqtinchalik fixture'lar bilan regression testlari.
