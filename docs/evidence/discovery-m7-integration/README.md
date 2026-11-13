# Discovery/M7/current catalog integration acceptance

Accepted source:`38847ce4eea44508e3a5c6828b9789a76a16d9bc`.
Main baseline:`65948e0f769f8def543d056544ad620abc2228d5`.
Feature parent:`e5c5f6469d8ed999b06cbba11210196c1c78b53d`.

## Result

Default/custom `~/Devforge` discovery is integrated with M7 debugger/profiler.
Both schema6 histories migrate additively to schema7: debugger/profile/runtime/ID
and existing discovery ledger/suppression are preserved. Explicit setup of the
new discovered fixture's PHP, database and managed env precedes debugging; source
discovery never copies another project's secret bindings or runs migrations.

Current reviewed Intel PHP8.5 CLI/FPM build families were added to the existing
M7 superseded/rebuild model. All48 existing package descriptors are unchanged.
Old installed package IDs/receipts and explicit manifest pins remain valid. Fresh
unpinned setup selects reviewed replacement families. Matching CLI/FPM versions
stay paired. Current upstream checksums and actual bytes/architectures were checked;
normal production downloader installs were exercised in each native job. The old
Git-blob workaround is not used by this acceptance or the product installer.

## Actual native evidence

| Target | Backend / CLI | Discovery + debugger UI checks | Axe cases / findings | 100-project event → UI |
| --- | --- | --- | --- | --- |
| macos-arm64 | PASS | 12 + 8 | 22 / 0 | 1657ms |
| macos-x64 | PASS | 12 + 8 | 22 / 0 | 1899ms |
| windows-x64 | PASS | 12 + 8 | 22 / 0 | 1681ms |

Each target ran actual Laravel13.42/PHP8.4.27 and8.5.12 with Xdebug3.5.3. Discovery
watcher/recovery/identity/default/custom/picker tests pass. The M7 suite runs on an
AUTO-discovered new project: actual HTTP breakpoints, steps, masked variables,
profiles/export/delete, other-project isolation, Off recovery, process cleanup,
then Forget/Restore preserving its debugger configuration. Native UI runs both
pages with three locales, light/dark/minimum window, keyboard/focus and relaunch.
Raw credential/DB/env/profile files are excluded from uploaded artifacts.

- [macos-arm64](https://github.com/jorabekyusupov/devforge/actions/runs/43436944740/job/131924407272): ZIP SHA256 `e40d1970f4116861c61edff2711b773e382fbcae7684a39a36bd3a1e787f7bf9`.
- [macos-x64](https://github.com/jorabekyusupov/devforge/actions/runs/43436950302/job/131924424311): ZIP SHA256 `ce183e63d525e06ef3ef98a3044ed92f1061e32583e81185f92d60e99b6bfe67`.
- [windows-x64](https://github.com/jorabekyusupov/devforge/actions/runs/43436840855/job/131924041031): ZIP SHA256 `46e9a675e31b8630faabd9e9d09ec00e3009991fc21bad659ee8e3224d0f5c01`.

All ZIP digests match GitHub metadata. Reports and immutable source-archive SHA256
are in [provenance](provenance.json). Actual screenshots were inspected. Ordinary
default-feature desktop and CLI builds passed; acceptance uses explicit e2e builds.
[Core CI](core.json):124Mac/120Win Rust tests,1 opt-in OS credential fixture ignored
each;42frontend,6packaging, locale/typecheck/fmt/clippy and default desktop builds.
The native recipe builds only in owned private prefixes; no system PHP/Homebrew/
Herd or OS security-permission changes. Existing Inspector and SDK builder/recipe
are unchanged:see [source boundaries](source-boundaries.json).

## Preserved failures and fixes

Initial Windows archive audit failed on nodejs.org DNS; it passed on the new
runner at unchanged application code. The combined fixture first assumed PHP8.5
instead of its discovered PHP8.4 default; explicit scenario setup corrected this.
Then cloned sources lacked the original project's managed APP_KEY/session env and
returned500 before reaching breakpoints; new per-project managed fixture env and
normal HTTP/migration preparation corrected setup. These failures are retained
in [attempts](attempts.json); no integrity or functional assertion was removed.

## Source and delivery boundary

Core/runtime/application code was frozen after merge4ff3411. Follow-ups corrected
acceptance fixture preparation and focused retry selection; final native reports
above all use the SAME source38847ce. Final documentation-only commit contains no
application changes. Main promotion is authorized and occurs only after native
and strict knowledge validation, with remote SHA verified in the final handoff.
Public signing/notarization, fresh/minimum OS certification and M15 distribution
gates remain separate. M7.1 and other Herd projects were not changed.
