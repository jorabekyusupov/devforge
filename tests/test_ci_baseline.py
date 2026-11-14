import contextlib
import io
from http.client import IncompleteRead, RemoteDisconnected
import json
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch
from urllib.parse import parse_qs, urlsplit

sys.path.insert(0, str(Path(__file__).parents[1] / "scripts"))
import ci_baseline as baseline
import ci_scope
import rust_scope

REPO = "owner/project"
GOOD, PARENT, OTHER = "a" * 40, "b" * 40, "c" * 40


def run(number, sha=GOOD, branch="main"):
    return {"id": number, "head_sha": sha, "path": baseline.WORKFLOW, "event": "push",
            "head_branch": branch, "status": "completed", "conclusion": "success",
            "repository": {"full_name": REPO}}


def jobs(item, rust="success"):
    return [{"name": name, "run_id": item["id"], "head_sha": item["head_sha"],
             "status": "completed", "conclusion": "success",
             "steps": [{"name": baseline.RUST_STEP, "status": "completed", "conclusion": rust}]}
            for name in sorted(baseline.MATRIX)]


class API:
    def __init__(self, runs, by_run):
        self.runs = runs
        self.by_run = by_run
        self.routes = []

    def get(self, route):
        self.routes.append(route)
        path, query = urlsplit(route).path, parse_qs(urlsplit(route).query)
        page = int(query["page"][0])
        size = int(query["per_page"][0])
        if path.endswith("/check.yml/runs"):
            return {"workflow_runs": self.runs[(page - 1) * size: page * size]}
        self.assert_jobs_filter(query)
        number = int(path.split("/")[-2])
        all_jobs = self.by_run[number]
        return {"total_count": len(all_jobs), "jobs": all_jobs[(page - 1) * size: page * size]}

    @staticmethod
    def assert_jobs_filter(query):
        assert query["filter"] == ["latest"]


class BaselineTests(unittest.TestCase):
    def find(self, api, ancestor=lambda _: True, event=None, name="push", repository=REPO):
        return baseline.find_baseline(name, event or {"ref": "refs/heads/main"}, repository, api, ancestor)

    def test_last_green_with_both_real_rust_steps_excludes_docs_and_partial(self):
        docs, partial, good = run(3, OTHER), run(2, PARENT), run(1)
        incomplete = jobs(partial)
        incomplete[1]["steps"][0]["conclusion"] = "skipped"
        api = API([docs, partial, good], {3: jobs(docs, "skipped"), 2: incomplete, 1: jobs(good)})
        self.assertEqual(self.find(api), GOOD)

    def test_missing_failed_or_duplicate_rust_step_never_authorizes_baseline(self):
        item = run(1)
        for mutate in [lambda data: data[0].update(steps=[]),
                       lambda data: data[0]["steps"][0].update(conclusion="failure"),
                       lambda data: data.append(dict(data[0]))]:
            data = jobs(item)
            mutate(data)
            self.assertIsNone(self.find(API([item], {1: data})))

    def test_nonancestor_is_skipped_missing_history_falls_full(self):
        newer, older = run(2, OTHER), run(1)
        api = API([newer, older], {1: jobs(older)})
        self.assertEqual(self.find(api, lambda sha: sha == GOOD), GOOD)
        def missing(_):
            raise baseline.Unavailable("history")
        self.assertIsNone(self.find(api, missing))

    def test_branch_repository_workflow_and_job_identity_are_checked(self):
        for field, value in [("path", ".github/workflows/other.yml"), ("head_branch", "other"),
                             ("head_sha", "HEAD;echo secret"), ("event", "pull_request"),
                             ("repository", {"full_name": "other/repo"}), ("id", True)]:
            item = run(1)
            item[field] = value
            self.assertIsNone(self.find(API([item], {})))
        item = run(1)
        data = jobs(item)
        data[0]["head_sha"] = OTHER
        self.assertIsNone(self.find(API([item], {1: data})))

    def test_pr_uses_base_branch_and_url_encodes_branch_without_query_injection(self):
        branch = "feature/a&status=evil"
        item = run(1, branch=branch)
        api = API([item], {1: jobs(item)})
        event = {"pull_request": {"base": {"ref": branch}, "head": {"ref": "wrong"}}}
        self.assertEqual(self.find(api, event=event, name="pull_request"), GOOD)
        query = parse_qs(urlsplit(api.routes[0]).query)
        self.assertEqual(query["branch"], [branch])
        self.assertEqual(query["status"], ["success"])
        self.assertIsNone(self.find(api, repository="owner/repo?token=bad"))

    def test_manual_missing_event_api_errors_and_absent_evidence_fall_full(self):
        api = API([], {})
        self.assertIsNone(self.find(api, name="workflow_dispatch"))
        self.assertIsNone(self.find(api, event={"ref": "refs/tags/v1"}))
        self.assertIsNone(self.find(api))
        class Failed:
            def get(self, _):
                raise OSError("private authorization or rate-limit details")
        self.assertIsNone(self.find(Failed()))

    def test_truncated_or_disconnected_http_response_falls_full_without_error_log(self):
        for error in [IncompleteRead(b"partial private metadata", 100), RemoteDisconnected("response closed")]:
            class Failed:
                def get(self, _):
                    raise error
            with contextlib.redirect_stdout(io.StringIO()) as out:
                self.assertIsNone(self.find(Failed()))
                self.assertEqual(out.getvalue(), "")

    def test_pagination_is_bounded_and_truncated_jobs_do_not_qualify(self):
        item = run(1)
        api = API([item], {1: jobs(item) + [{"name": "other"}] * 201})
        self.assertIsNone(self.find(api))
        self.assertLessEqual(len(api.routes), 1 + baseline.JOB_PAGES)
        many = [run(i, GOOD) for i in range(1, 100)]
        api = API(many, {item["id"]: jobs(item, "skipped") for item in many})
        self.assertIsNone(self.find(api))
        self.assertLessEqual(len(api.routes), baseline.RUN_PAGES + baseline.CANDIDATES)

    def test_both_selectors_accumulate_failed_parent_changes_from_same_baseline(self):
        seen = []
        def diff(sha):
            seen.append(sha)
            return ["crates/platform/src/lib.rs", "apps/desktop/src-tauri/src/lib.rs"]
        event = {"before": PARENT}
        root = Path("/fixture").resolve()
        metadata = {"workspace_members": ["platform", "desktop"], "packages": [
            {"id": "platform", "name": "platform", "manifest_path": str(root / "crates/platform/Cargo.toml"), "dependencies": []},
            {"id": "desktop", "name": "desktop", "manifest_path": str(root / "apps/desktop/src-tauri/Cargo.toml"), "dependencies": [{"path": str(root / "crates/platform")}]}]}
        self.assertTrue(ci_scope.select_product_checks("push", event, diff, validated_base=GOOD, baseline_required=True))
        scope = rust_scope.select_scope("push", event, diff, lambda: metadata, root, validated_base=GOOD, baseline_required=True)
        self.assertEqual(scope["packages"], ["desktop", "platform"])
        self.assertEqual(seen, [GOOD, GOOD])
        # A known immediate parent is insufficient when validated history is absent.
        def forbidden(_):
            self.fail("must not fall back to event parent")
        self.assertTrue(ci_scope.select_product_checks("push", event, forbidden, baseline_required=True))
        self.assertEqual(rust_scope.select_scope("push", event, forbidden, lambda: metadata, root, baseline_required=True), rust_scope.full_scope())

    def test_environment_consumer_requires_ready_valid_ancestor(self):
        with patch.dict("os.environ", {"DEVFORGE_CI_BASELINE_READY": "false", "DEVFORGE_CI_VALIDATED_BASE": GOOD}), patch.object(baseline, "git_ancestor") as ancestor:
            self.assertIsNone(baseline.validated_base_from_environment())
            ancestor.assert_not_called()
        with patch.dict("os.environ", {"DEVFORGE_CI_BASELINE_READY": "true", "DEVFORGE_CI_VALIDATED_BASE": GOOD}), patch.object(baseline, "git_ancestor", return_value=True):
            self.assertEqual(baseline.validated_base_from_environment(), GOOD)
        with patch.dict("os.environ", {"DEVFORGE_CI_BASELINE_READY": "true", "DEVFORGE_CI_VALIDATED_BASE": GOOD}), patch.object(baseline, "git_ancestor", side_effect=baseline.Unavailable()):
            self.assertIsNone(baseline.validated_base_from_environment())

    def test_missing_token_never_calls_api_or_logs_credentials(self):
        with patch.dict("os.environ", {"GITHUB_TOKEN": ""}, clear=True), patch.object(baseline, "GitHub") as client, contextlib.redirect_stdout(io.StringIO()) as out:
            baseline.main()
            client.assert_not_called()
            self.assertIn("full_fallback", out.getvalue())
        with self.assertRaises(baseline.Unavailable):
            baseline.GitHub("")


if __name__ == "__main__":
    unittest.main()
