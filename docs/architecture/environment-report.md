# E02: expected versus registered environment metadata

Status: initial library report implemented; no CLI, UI or agent endpoint added.
[Integrated E02 plan](../knowledge/integrated-execution-plan.md) and
[M10 environment contract](../knowledge/m10-reproducible-results.md) define scope.

`Workspace::environment_report(id, ReportLimits)` is an immutable metadata read.
It composes existing `offline_readiness` artifact/receipt evidence instead of
creating a second runtime collector. It deliberately does not call manifest_plan:
that existing planner may read credentials and create transaction lock directories.
The report never calls SecretStore, PHP/Composer/Node/Git, DNS/network, package
installation, env patching or service start/stop. A future external adapter must
check its own authenticated project grant before invoking this local library API.

## What is compared

- Repository exact PHP/Node/Composer family versions and target package IDs against
  persisted selections. Logical versions use the existing Manifest::resolve rules:
  PHP build suffixes and non-PHP leading `v` are normalized for family comparison.
  Exact package-ID comparison and nested offline evidence retain full provider
  identity; normalization cannot silently replace a stored selection.
- Project name and required environment-name registration, selected DB engine/version, cache/queue drivers and cache runtime versions;
  site domain/HTTPS/Vite, development queue, mail and explicit project PHP settings.
- Portable manifest resolution and pin digest checks for ARM64/Intel macOS and
  Windows x64. catalog_support describes package metadata only. native_support is
  always Unverified here; OS edition, prerequisites and actual runtime acceptance
  remain separate gates.
- Local secret aliases and registered environment-name metadata. Alias registration
  does not prove that a credential exists, is readable or matches the desired alias.
  Missing name registration is known configuration drift, not proof that the
  credential itself is absent. Availability remains Unverified without a credential query. Actual secret-store
  key strings and credential values are not serialized.
- Direct require/require-dev ext-* names from bounded composer.json. Optional cached
  PHP extension observations have no source revision/timestamp in the current
  cache and remain explicitly stale/unverified. They never become a current native
  pass. Transitive composer.lock extension requirements, version constraints and
  actual Composer platform check remain unverified.

matches_recorded_configuration is about the compared stored settings, not a
running Laravel application. environment_ready never becomes true from metadata;
known missing artifacts, pin errors or drift can establish false, otherwise it
remains unknown. A mismatched version/package does not trigger fallback or change
any project pin. Missing/invalid manifest, incomplete registry, identity conflict
and budgets are explicit report problems. Incomplete DB/cache tables do not prove
absence; those facts are marked Unverified.

## Read boundaries

Registry projection reuses bounded storage_metadata. The offline report and local
fact pass can observe different moments; project revision mismatch is reported and
the comparison is stopped. This is not an atomic transaction or hostile-filesystem
sandbox. The registered project root must still match canonical path and filesystem
identity before repository metadata is visited. Final-component links/reparse files
are rejected using the existing metadata-file reader; serving root is checked too.
The report uses only devforge.yaml, composer.json and the owned sites registry.
.env content is never read by this module.

Defaults:2048 registry rows per metadata pass,128 combined comparison/secret/extension
facts,128 direct extension requirements,64KiB per file and256KiB aggregate local file
reads. Limits are validated and bounded. Nested offline evidence has its own existing
receipt/registry limits and provenance. Comparison values are capped256 bytes;
unknown/oversized values are not echoed. No arbitrary filesystem path or raw source
exception is part of the report. User-chosen metadata is not a universal secret
scanner/redaction guarantee.

## Focused evidence

The integration fixtures cover immutable reporting with credential methods that
panic if called, private .env/reference canaries, full tree content preservation,
version versus exact-ID drift, manifest pin tampering, missing target family,
registry/file budgets, linked manifest refusal and replaced root identity. A unit
fixture proves cached extension observations never establish current support.
These fixtures do not run a native PHP/DB service or establish Windows/minimum-OS
runtime acceptance. No broad product suite is necessary for this isolated report.
