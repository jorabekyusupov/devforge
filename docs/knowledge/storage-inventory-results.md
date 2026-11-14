---
type: Implementation Result
title: E05.1 read-only owned storage inventory
description: Bounded registry va metadata-only filesystem inventory, ownership/reference coverage va logical size chegaralari.
status: draft
decision_status: working-direction
tags: [storage, e05, ownership, references, bounds]
sources:
  - id: plan
    resource: /integrated-execution-plan.md
    title: E05 inventory oldin cleanup keyin
  - id: inventory
    resource: ../../crates/workspace/src/storage.rs
    title: Metadata-only owned resource inventory
  - id: metadata
    resource: ../../crates/projects/src/storage_metadata.rs
    title: Bounded read-only registry projection
---

# Bajarilgan qism

Workspace::storage_inventory(StorageLimits) stable IDs/category/ownership evidence,
project/workspace references, retained reason, measurement/reference completeness va
stable problem codes qaytaradi. Runtime catalog+receipt, instance/cache registry,
backup/config/debug/log namespaces, mail marker, typed worktree/source-registry
receipt hamda app/tooling artifact namespace'lari tekshiriladi. Unknown ownership
traversed emas; inventory cleanup/deletion authority yoki eligibility bermaydi.

Hajm logical apparent bytes, allocated/freeable/recoverable emas. Hardlink/sparse
file va shared DB generation uchun per-resource attributable bytes ishonchsiz;
retained generation logical_bytes=None va unattributed sifatida ko'rsatiladi.
Active refs selected binding/pins ekanini process-running bilan aralashtirmaydi.
Archived/process lease reference coverage to'liq olinmasa explicit unknown.

No payload/env/log/mail/private-key content yoki native keychain o'qilmaydi.
Faqat bounded nonsecret marker/receipt metadata o'qiladi. Entry/resource/depth,
registry rows/perrecord UTF8bytes/global JSONbytes limitlari bor. Oversize/corrupt/
changed/truncated metadata incomplete va unknown refs, missing refs uchun0 degani emas.

ProjectStore storage_metadata first rowid+optimized octet_length(record) streamini
oladi. Perrecord/remaining-total budgetdan keyin rowid lookup boundedrecordni SQL'da
qayta tekshiradi, actual qaytganbytes hisoblanadi. Metadata API deliberately Serialize
emas: consumer faqat kerakli nonsecret fieldsni tanlaydi. Budgets returned records/
JSON memoryni bound qiladi, SQLite VMning barcha CPU/query allocator ishlari yoki
transactional snapshot kafolati emas.

Final metadata handles Unix nofollow/cloexec/nonblock yoki Windows open-reparse flag
bilan ochiladi; fstat nonregular/reparse/oversize bo'lsa read boshlanmaydi. Har
ancestor known symlink/junction tekshiriladi. Concurrent ancestor replacement uchun
check/open race va non-atomic snapshot qoladi; hostile filesystem sandbox yoki
cleanup plan sifatida taqdim etilmaydi.

# Tegishli tekshiruvlar

Initial storage8fixture passed: owned/unowned markers, runtime pins, active/retained
DB metadata refs, worktree receipt ownership, symlink/depth/entry/resource budget,
corrupt registry unknown coverage va read-only behavior. DB SQLseed registry
accounting fixture, actual initialized/native DB connection proof emas.

Initial bounded metadata1pass; QA SQL byte-size sourcefindingdan keyin metadata2+
Unicode-byte1, affected corrupt-reference1/DBaccounting1 qayta o'tdi.8MiB TEXT fixture
cache-miss bound bilan oversized payload readsni tekshiradi; bu fixture eski CASE
yozuvda ham o'tgan, originalCASTissue aynan reproduce qilindi deyilmaydi. Finalhandle
open1 va affectedlink1 passed. Scoped projects/workspace/storage clippy va fmt/diff
o'tdi. Unchanged tests reused, whole product/native runtime suites rerun qilinmadi.

Actual production data skan qilinmadi. GUI, reviewed cleanup/retention, restore drill
va external scoped inventory API bu kesimga kirmaydi. Windows-specific reparse/FS
execution yangi source CI/targetda alohida, macOS result undan dalil emas.
