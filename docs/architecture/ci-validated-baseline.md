# CI scope accumulates from a validated ancestor

Status: TP03 implementation; remote execution after integration remains distinct
from the local selector tests and read-only API sample below.

## Problem

A per-push `before..HEAD` comparison can omit unvalidated changes. For example, a
platform change may fail during formatting, before its tests run. A following
desktop-only formatting fix must still select the platform tests. Docs-only
successful workflows are not product validation and must not reset that history.

## Selection contract

One `scope-baseline` job resolves the comparison SHA before the macOS/Windows
matrix. Both the docs/product classifier and the Rust dependency selector consume
that same explicit output. They recheck its local ancestry and never substitute
the event parent when validated history is unavailable.

The producer uses GitHub's authenticated read-only REST metadata for this exact
`.github/workflows/check.yml`. It considers successful, completed push runs on the
push branch, or on a pull request's base branch. A candidate must be an ancestor
of the checkout HEAD, belong to the same repository/workflow/branch and include
both exact `check (macos-latest)` and `check (windows-latest)` jobs. Both jobs and
their `Rust test for selected scope` steps must be completed successfully.
Missing, failed, skipped or duplicated matrix/step evidence does not authorize a
baseline. `filter=latest` prevents combining jobs from older executions of a run.
A docs-only green run with skipped Rust tests therefore cannot advance the base.

Diffing the validated SHA to HEAD accumulates failed and queued intermediate
changes. The existing reverse-dependency/package selection then applies to that
entire accumulated diff. Changes to selector scripts, manifests/lockfiles,
workflow/toolchain or unknown files still select the full product checks.

Manual dispatch, missing token, API/auth/rate-limit failure, missing history,
malformed metadata or exhausted search budgets select full checks. Discovery's
permissions are only `contents: read` and `actions: read` on its own job; matrix
jobs retain their previous permissions. If discovery itself fails unexpectedly,
the uncancelled product matrix can still run with full fallback. Explicit user
cancellation remains respected. Existing manual native-suite routing is retained.

The REST search is bounded to3 pages of30 runs,10 ancestor candidates, and2 pages
of100 jobs per candidate, with bounded response bodies, per-request timeout and
an overall request-admission deadline. Branch/query and repository path inputs
are encoded and validated. Git operations use argument arrays with validated
40-hex SHAs. Requests stay on `api.github.com`; redirects are rejected, and token,
request headers and raw API errors are never printed.

This optimization trusts previously successful CI product runs as its starting
evidence. It does not turn a green source test into minimum-OS/native GUI/signing
acceptance. Selection-changing code itself takes full scope, establishing a new
broad baseline when integrated. On the first TP03 run, the workflow/selector diff
forces FULL even if the historical checkpoint came from the older parent-diff
selector. Those migration changes remain accumulated until a successful broad
run exists. Historical green CI metadata is a checkpoint, not immutable artifact
attestation; it cannot by itself prove every earlier partial selector was sound.

## Verification

Local verification:29 selector tests passed (11 baseline,8 docs/product,10 Rust
closure/argv tests); workflow YAML permissions/base propagation and scoped diff
checks passed. Local mock API/metadata and selector tests cover docs-only exclusion, incomplete
platform validation, run/job identity, ancestry/missing history, PR base branch,
branch query encoding, bounded pagination, API/token failure, a common validated
base accumulating missed platform changes, and explicit full fallback instead of
parent fallback. Existing TP01/TP02 classifier/closure tests remain applicable.
No product native/full test suite was started for this selector change.

A read-only actual GitHub API sample selected ancestor
`5b9930cbf277f0c846ca57ed98a421a73082cda4` for main. Run43576221093's latest macOS
and Windows matrix jobs both reported a successful Rust-test step. This confirms
the API selection on the inspected history; it is not a run of the new workflow.

## Official API references

- [Workflow runs](https://docs.github.com/en/rest/actions/workflow-runs#list-workflow-runs-for-a-workflow)
- [Workflow jobs and latest-execution filter](https://docs.github.com/en/rest/actions/workflow-jobs#list-jobs-for-a-workflow-run)
- [Status check functions](https://docs.github.com/en/actions/reference/workflows-and-actions/expressions#status-check-functions)
