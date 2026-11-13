# Source/evidence reconciliation

Product implementation is frozen at e5ce0e7. Full acceptance checkout0fb0eca
adds test infrastructure only; c33eadc adds an Intel-only workflow selector and
one original runtime cache entry. Application/core/catalog and all runner source
files have the same tree digest across0fb0eca/c33eadc; see product-source-equivalence.json.

| Phase | Classification | Owning source and evidence |
| --- | --- | --- |
| 1 Settings | matching | core SettingsPatch; platform SQLite/resolve tests; native save/default/custom/relaunch/invalid-path assertions |
| 2 Scanner | matching | projects/discovery bounded direct-child walk, cancellation/cap/permission/reservation/symlink tests; native no-runtime/permission/junction checks |
| 3 Registry | matching | schema6 ledger and identity; transactional suppression/first-scan Forget/Restore tests; native preserved manual profiles and original ID/runtime |
| 4 Monitor | changed with evidence | workspace/discovery: Windows collection-root handle replaces descendant handles; canonical/verbatim path normalization; pathless loss flag recovery; bounded worker/generations/30s fallback and actual resource measurements |
| 5 UI | matching | localized Settings/Projects/New Project; actual system picker/Tab/focus/unsaved draft; three locales x light/dark and minimum viewport; runner waits for route field mount |
| 6 Native | matching — all3 native targets passed | ARM64/Windows completed at0fb0eca; Intel repeats identical code atc33eadc after upstream archive replacement; public ZIP digests bind source and reports |
| 7 Harden | matching — full audit and strict validator passed | core CIs pass;60 original criteria audited individually; final native matrix, screenshots and strict knowledge close after Intel passes |

## Deliberate boundaries

- Delivered:one shallow Laravel root; compatible installed runtimes or needs-runtime;
  explicit Prepare/Start; retained project data on root loss/change/Forget.
- Not implemented by this feature:multiple roots, arbitrary web-driver framework,
  automatic scripts/downloads/migrations, new DNS/CA daemon, signed public release.
- Not certified by these checks:Windows ARM, network/removable mount behavior,
  minimum OS/fresh installer. Existing M15 gates remain separate.
- Upstream PHP archives were rebuilt without a version change. Current catalog
  pins were retained. Native acceptance uses original public archives verified
  against those pins and stored as hash-bound temporary Git blob cache objects;
  no runtime binary files are committed into the branch tree. Fresh public-URL
  availability requires separate catalog maintenance; no changed archive was run.

## Completion rule

A structural phase/knowledge check does not prove native behavior. Intel native backend/GUI passed. The60-criterion audit, per-target metrics and
artifact hashes agree; the strict knowledge validator passed28 documents/0 errors/0 warnings.
