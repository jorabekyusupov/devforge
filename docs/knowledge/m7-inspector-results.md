---
type: Implementation Report
title: M7 Laravel debugging va profiling
description: Opt-in collector companion package va native Inspector birinchi ishlaydigan qatlami.
status: draft
decision_status: confirmed
tags: [m7, inspector, profiling, laravel, evidence]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: M7 ni boshlash topshirig'i
  - id: core
    resource: ../../crates/inspector/src/lib.rs
    title: Authenticated loopback bounded capture store
  - id: companion
    resource: ../../packages/laravel-inspector/src/InspectorServiceProvider.php
    title: Laravel events va dump handler integration
  - id: coordinator
    resource: ../../crates/workspace/src/lib.rs
    title: Project setup capture lifecycle va capability probe
  - id: ui
    resource: ../../apps/desktop/src/pages/InspectorPage.tsx
    title: Real Inspector event/context UI
  - id: local
    resource: ../evidence/m7/local-native.json
    title: Actual local Laravel PG request SQL dump dd job exception acceptance
---

# Scope va hozirgi holat

Foydalanuvchi qayta tartiblangan roadmap bo'yicha M7'ni boshlashni topshirdi.
M7.1 opt-in event capture, Laravel companion, request/query profiling va desktop
Inspector ustida ish bajarilmoqda. M7 to'liq yopildi deb belgilanmaydi: Xdebug
capability aniqlanadi, native extension/debugger/function profiler boshqaruvi
uchun mos provider va alohida tekshirish keyingi qismda qoladi. M15 yakuniy beta
bosqichi oxirida qoladi; doimiy test va data preservation to'xtatilmaydi.[^request]

# Kod kontrakti

`crates/inspector` Tauri'dan mustaqil. Collector loopback ephemeral port, loyiha
bo'yicha random credential va project-id tekshiruvi bilan ishlaydi. Origin'li
browser so'rovlari rad etiladi; request body128KiB, batch200 event, concurrency8.
Session JSON private app-data'da; UI/CLI DTO token bermaydi. Kernel owner lease va
instance nonce stale config'ni boshqa collector session'ida qo'llashni oldini oladi.
Event data bounded/sanitized, SQLite ring4000 events va16MiB payload budget,
visible24h/200 events/256KiB. History saqlanadi, capture app sessiyasiga bog'liq.[^core]

Companion local/testing va DevForge endpoint/token ENV bo'lgandagina ulanadi.
Request route template, query duration, job lifecycle, log, dump va exception
konteksti olinadi. Request headers/body, SQL bindings va job payload olinmaydi;
SQL literal/comments normalized, known secrets va sensitive keys mask qilinadi.
Bu hostile code sandbox yoki barcha encoded/private qiymatlarni topish kafolati
emas; capture opt-in va lokal test ma'lumotiga mo'ljallangan. Collector unavailable
bo'lsa bounded transport va per-context circuit breaker application'ni davom
ettiradi. dd exit semantikasi saqlanadi, no-active-scope dump default handler'ga
qaytariladi. QueryException raw bindingsli message uzatilmaydi.[^companion]

Install amali user confirmation bilan .devforge/laravel-inspector, composer.json/
lock/vendor'ga ta'sir qiladi, .env'ni o'zgartirmaydi. Existing conflicting files
refuse qilinadi; package atomic staging bilan yoziladi. Composer require/reinstall
scripts/plugins'siz, keyin explicit package:discover ishlaydi. Running project avval
Stop qilinadi. Capture enable/pause faqat selected project'ni restart qiladi;
clear faqat uning Inspector history'sini o'chiradi.[^coordinator]

SQL pattern repetitions potential N+1 hint; actual source/query execution yoki
N+1 proof emas. Profiler request/job duration, captured SQL total/count va peak PHP
memory'ni ko'rsatadi. Xdebug probe loaded/version/mode haqiqiy selected PHP'dan;
local PHP8.5.12'dagi loaded=false extension bor degan claim'ga aylantirilmaydi.[^local]

# Dastlabki dalil va qolgan ish

Local native Laravel/PostgreSQL HTTP, normalized/slow/repeated SQL, dump/dd,
queue-job va exception, sentinel masking, project isolation/scoped clear,
enableB preservingA PIDs, pause va history persistence o'tdi. Companion Laravel13
fixture'da tekshirildi; declared11/12 constraints hali runtime dalili emas.[^local]

Desktop controls/filter/context, three catalogs va native WKWebView/WebView2 GUI
sinovlari davom etmoqda. First local audit capture-button contrast muammosini
topdi; incumbent M5 control styles bilan tuzatilmoqda. Backend/collector6 test,
frontend16 baseline+Inspector test oldingi pass; yakuniy counters/source CI dalili
bilan alohida yangilanadi. Yangilangan UI paging uchun qo'shimcha regression bor.[^ui]

Bu implementatsiya public publish, cloud deploy yoki Windows signing ruxsati
emas. Code/evidence reconciliation, strict OKF va native target natijalari
qolgan ishlar tugagach qayd etiladi.

[^request]: «m7dan boshla»; M7 debugging/profiling birinchi, M15 beta oxirgi.
[^core]: Token never Debug/DTO; no SQL execution API.
[^companion]: Local Composer path package, source binariesni o'zgartirmaydi; native PHP extension emas.
[^coordinator]: Managed runtime/INI/env va worker boundary reused.
[^ui]: Generated target design-system'da illustrative; product mock data bilan to'ldirilmagan.
[^local]: Actual isolated task fixture; user Herd/hosts/trust o'zgartirilmagan.
