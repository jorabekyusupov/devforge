---
type: Implementation Report
title: M3 loyiha muhiti va PostgreSQL binding
description: Laravel import, per-project runtime/env, DB ownership, CLI va M3 sinov dalillari.
status: draft
decision_status: confirmed
tags: [m3, projects, postgresql, secrets, runtimes, evidence]
sources:
  - id: request
    resource: /references/conversation-decisions.md
    title: M3 bosqichiga o'tish ko'rsatmasi
  - id: registry
    resource: ../../crates/projects/src/project.rs
    title: Project registry va manifest tekshiruvi
  - id: execution
    resource: ../../crates/projects/src/execution.rs
    title: Shared project context va verified tool execution
  - id: database
    resource: ../../crates/projects/src/database.rs
    title: Owned cluster va project DB-role
  - id: env-patch
    resource: ../../crates/projects/src/env_file.rs
    title: Explicit environment patch va private backup
  - id: smoke
    resource: ../evidence/m3/macos-project-smoke.json
    title: Task-local native Mac M3 sinovi
  - id: cli
    resource: ../evidence/m3/cli-registry-smoke.json
    title: Haqiqiy CLI registry va read-only import sinovi
  - id: php-constraints
    resource: https://getcomposer.org/doc/articles/versions.md
    title: Composer version constraint'lari
  - id: pg-hba
    resource: https://www.postgresql.org/docs/18/auth-pg-hba-conf.html
    title: HBA sameuser va authentication qoidalari
  - id: password
    resource: https://docs.rs/postgres-protocol/0.6.12/postgres_protocol/password/index.html
    title: Client-side SCRAM password verifier
---

# Natija va scope

Foydalanuvchi M3'ga o'tishni topshirdi.[^request] `crates/projects` Tauri'dan
mustaqil project services beradi; CLI shu qatlamga ulangan. Desktop Project UI
M5, haqiqiy PHP-FPM/CGI serving/proxy/domain M4. M3 web SAPI version probe va
bir xil INI/env resolver'ni beradi, serving readiness isbotini emas.

# Import va persistent binding

Import canonical loyiha papkasi, `composer.json`, Laravel framework talabi,
`public/index.php` va mavjud bo'lsa `package.json`ni o'qiydi. JSON hajmi cheklangan.
PHP constraint Composer parser, Node constraint npm parser orqali tekshiriladi;
ikkalasining til semantikasi aralashtirilmaydi. Bu declared version tekshiruvi,
to'liq dependency solver yoki extension inventory emas.[^registry][^php-constraints]

Import script/plugin, composer/npm install, migration va `.env` write bajarmaydi.
SQLite `projects.sqlite3` preferences DB'dan alohida schema v1; exact catalog IDs,
root/name, constraint, revision va secret name/reference'lar saqlanadi. Active
duplicate path, stale revision va corrupt/future schema rad etiladi. Unix'da
filesystem dev/inode ham duplicate identity'ga xizmat qiladi. `forget` metadata'ni
inactive qiladi; folder, DB, secret va binding'ni o'chirmaydi.

# Bitta project context

`Context` tanlangan PHP/Node executable lease'lari, project INI/scan/session/cache
yo'llari va explicit environment'ni birlashtiradi. PHP, Composer PHAR, Artisan,
queue prefix, Node/npm va terminal shims shu tanlovdan olinadi. Windows PHP
paketidagi mavjud extension DLL'lari project INI'da yoqiladi; global PHP INI/scan
yoki PATH almashtirilmaydi. Mac'da web resolver mos FPM paketini, Windows'da
aynan tanlangan PHP paketining CGI executable'ini beradi.[^execution]

`runtime/catalog.json` current katalog; M2 snapshot saqlangan. PG `server`
variantlari pg_ctl'ni qo'shadi, Composer artifact esa verified resource sifatida
o'rnatiladi. Eski M2 package identity va receipt'lari o'zgartirilmaydi; reader
yangi optional resource metadata'sini backward-compatible o'qiydi. PHAR native
executable sifatida OS'ga berilmaydi, verified PHP orqali chaqiriladi.

Terminal shims PHP/Composer/Artisan uchun INI prefix beradi; Node/npm selected
paketdan olinadi. macOS Terminal launcher va Windows new-console opener bor;
CLI `shell` kontekst/lease'ni shell tugaguncha saqlaydi. Native shell resolution
probe GUI oynani ko'rish/interaction sinovi o'rniga o'tmaydi.

# Secret va log kontrakti

Custom environment qiymatlari ham OS SecretStore'da; SQLite'da faqat nom/reference.
Loader/PATH/DB control o'zgaruvchilari reserved. CLI env-set qiymatni stdin'dan
oladi. Context credential'lari Debug/Serialize orqali chiqarilmaydi.

Project invocation M2 `start_redacted` yo'lini ishlatadi. Reader chunk chegarasida
bo'lingan ma'lum secret'ni ham memory/disk log'ga yetkazmaydi; pending buffer va
log ring/rotation cheklangan. Exit code endi snapshot'da saqlanadi. Bu hostile
project sandbox'i yoki noma'lum/encoded secret detektori emas; interactive user
terminali DevForge persisted-log redactor'i orqali o'tmaydi.

# PostgreSQL lifecycle va isolation

Cluster data app-owned `databases/<instance>/data` ichida, runtime binary'dan
alohida. Instance metadata/secret pending holatda initdb'dan oldin yoziladi;
initdb faqat yangi owned katalogga ishlaydi. Uzilgan boshlash existing data'ni
avtomatik o'chirib/reinitialize qilmaydi; `ready:false` recovery talab qiladi.
Port loopback'da ajratiladi; band port/instance lock rad etiladi, begona servis
to'xtatilmaydi. Readiness'dan so'ng authenticated admin query data-directory
identity'ni ham tekshiradi.[^database]

Har project uchun bir xil generated DB/role nomi alohida yaratiladi. Role
NOSUPERUSER/NOCREATEDB/NOCREATEROLE/NOREPLICATION/NOBYPASSRLS/NOINHERIT.
HBA admin'ga faqat loopback, project role'ga `sameuser` DB, qolganiga reject;
DB PUBLIC rights ham bekor qilinadi. Yangi DB connections disabled holatda
yaratilib ACL/comment o'rnatilgach yoqiladi.[^pg-hba]

Password OS store'da; CREATE ROLE SQL'iga client-side SCRAM verifier beriladi,
plaintext password emas. Ownership comment va pending ledger idempotent
provision/recovery'ni tekshiradi; unowned role/DB egallanmaydi. DB connection
loopback SCRAM bilan, NoTLS transport; tashqi DB server ulash scope'i yo'q.[^password]

`db-serve` explicit foreground owner: yangi server boot/system service qilib
ro'yxatga olinmaydi. Boshqa CLI buyruqlari shu ishlayotgan instance'ga private
admin/project authentication bilan murojaat qiladi; har project exec alohida
server ochib-yopmaydi. Shared instance'ni explicit stop qilish barcha uning
project'lariga ta'sir qiladi. Stop oldidan pg_ctl target PID/owned data tekshiriladi;
pg_ctl fast shutdown tugagach generic process-group/Job fallback ishlaydi.
Controller stop resource/lock'ni ham bo'shatadi; shu handle tirik bo'lsa ham
restart qilish mumkin. Abrupt SIGKILL/power-loss/standalone guardian M2'dagi
ochiq gate sifatida qoladi.

# `.env` faqat explicit apply

`env-plan` faqat key nomlarini ko'rsatadi. `env-apply` selected DB key'larni
yozadi, DB URL override'larni aniq bo'shatadi, private exact backup saqlaydi va
atomic replacement ishlatadi. Other key/content va CRLF saqlanadi. Duplicate
DB key, symlink, invalid/oversize file rad etiladi. O'qilgandan keyin o'zgargan
file optimistic check'da rad etiladi; bu har qanday tashqi editor bilan kernel
darajasidagi compare-and-swap kafolati emas.[^env-patch]

# Haqiqiy tekshirishlar

- Mahalliy macOS27.2 ARM64 workspace:39 Rust test o'tdi/1 opt-in credential ignored.
- Full workspace Clippy `-D warnings` va locale lint3 katalog o'tdi.
- [CLI smoke](../evidence/m3/cli-registry-smoke.json): import/list/doctor/forget/list,
  persisted registry va original `.env` o'zgarmagani tekshirildi.[^cli]
- [Native Mac fixture](../evidence/m3/macos-project-smoke.json):8 verified paket,
  2 project,7 tool probe har birida va2 terminal shell probe. Real PG DB/role,
  PDO SQL, cross-DB deny, idempotent provision, graceful stop/restart+row persistence,
  known-secret redaction va explicit env backup/change rejection o'tdi.[^smoke]

Native fixture'da secret store in-memory adapter; production native keyring'ni
almashtirmaydi. Real Laravel migration/empty-worker va hosted Mac/Windows smoke
alohida `--laravel` flag bilan bajariladi; uning natijalari yakunida qo'shiladi.
Fixture'dagi artisan script faqat interpreter/context selection'ni tekshiradi,
Laravel framework worker isboti deb ko'rsatilmaydi.

Native sinovda DB comment lookup shared-object metadata'dan olinishi va stop'da
instance lock release qilinishi tuzatildi; final Mac restart/data tekshiruvi o'tdi.
Global Herd/DB, hosts/trust, shell profiles va real loyiha `.env`'iga tegilmadi.

# Phase reconciliation

| Da'vo | Holat |
| --- | --- |
| Persistent per-project PHP/Node selection/context bor. | Mos: registry testlari, actual native interpreter/tool probe'lari. |
| Import o'zi project code/config'ni o'zgartirmaydi. | Mos: script sentinel, env/content va CLI testlari. |
| Alohida DB/role va connection isolation bor. | Mos: actual owned PG/PDO va cross-database negative test. |
| Credentials SQLite/persisted project logs'da plaintext emas. | Mos: refs-only metadata, SCRAM verifier, chunk redaction va scan testlari. |
| GUI Project CRUD va HTTP/FastCGI/proxy serving tayyor. | Amalga oshirilmagan; M4-M5. |
| Real Laravel migration/worker va Windows DB lifecycle verified. | Hosted dalil pending; natija kelgach yangilanadi. |
| Native terminal oynasi va minimum OS/Intel/release verified. | Tekshirilmagan; shell probe va build buning o'rnini bosmaydi. |

[^request]: M3'ga o'tish haqidagi joriy user ko'rsatmasi.
[^registry]: Domain inspection va SQLite ownership; manifestlarni bajarish yo'q.
[^execution]: Framework'dan mustaqil context va verified argv/environment.
[^database]: Foreground cluster, native initdb/postgres/pg_ctl va ownership ledger.
[^env-patch]: Explicit patch, exact backup va file-change checks.
[^smoke]: Task-local real runtimes/database; in-memory credential fixture deb aniq ajratilgan.
[^cli]: Haqiqiy CLI parsing va registry roundtrip, installed runtime talab qilinmaydigan scope.
[^php-constraints]: Composer range semantikasi npm tilidan alohida.
[^pg-hba]: PostgreSQL18 HBA user/database matching qoidalari.
[^password]: Client-side SCRAM cleartext'ni SQL/log'ga yubormaslik uchun qo'llanadi.
