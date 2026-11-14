import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("ci_scope", Path(__file__).parents[1] / "scripts/ci_scope.py")
ci_scope = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci_scope)


class ScopeTests(unittest.TestCase):
    def test_documentation_only_avoids_product_checks(self):
        self.assertFalse(ci_scope.needs_product_checks([
            "README.md", "CONTRIBUTING.md", "docs/knowledge/execution-status.md",
            "docs/guides/windows-tester-uz.html",
        ]))

    def test_code_tooling_configs_unknown_and_mixed_changes_keep_checks(self):
        for path in ["crates/core/src/lib.rs", "scripts/ci_scope.py", "Cargo.lock",
                     "docs/examples/devforge.yaml", "docs/evidence/native.json",
                     "docs/evidence/screen.png", ".github/workflows/check.yml", "AGENTS.md"]:
            with self.subTest(path=path):
                self.assertTrue(ci_scope.needs_product_checks(["README.md", path]))

    def test_moved_code_still_keeps_checks_and_deleted_docs_are_safe(self):
        self.assertTrue(ci_scope.needs_product_checks(["crates/core/src/lib.rs", "docs/old.md"]))
        self.assertFalse(ci_scope.needs_product_checks(["docs/removed.md"]))

    def test_empty_and_ambiguous_paths_keep_checks(self):
        self.assertTrue(ci_scope.needs_product_checks([]))
        for path in ["", "/docs/a.md", "docs/../code.md", "docs//a.md", "docs/a\nb.md", "docs\\a.md"]:
            self.assertTrue(ci_scope.needs_product_checks([path]), path)

    def test_push_uses_before_and_pr_uses_base_not_before(self):
        before, base = "a" * 40, "b" * 40
        event = {"before": before, "pull_request": {"base": {"sha": base}}}
        seen = []
        def diff(value):
            seen.append(value)
            return ["README.md"]
        self.assertFalse(ci_scope.select_product_checks("push", event, diff))
        self.assertFalse(ci_scope.select_product_checks("pull_request", event, diff))
        self.assertEqual(seen, [before, base])

    def test_manual_new_branch_missing_base_and_malformed_events_keep_checks(self):
        def no_diff(_):
            self.fail("comparison should not be attempted")
        for name, event in [
            ("workflow_dispatch", {"before": "a" * 40}),
            ("push", {"before": "0" * 40}), ("push", {}),
            ("push", {"before": "HEAD;echo unsafe"}),
            ("pull_request", {"pull_request": None}), ("push", []),
        ]:
            self.assertTrue(ci_scope.select_product_checks(name, event, no_diff))

    def test_real_git_diff_and_github_output_for_docs_and_mixed_pushes(self):
        script = Path(__file__).parents[1] / "scripts/ci_scope.py"
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            def git(*args):
                return subprocess.run(["git", *args], cwd=root, check=True,
                                      stdout=subprocess.PIPE, stderr=subprocess.PIPE).stdout.decode().strip()
            def commit():
                git("add", ".")
                git("-c", "user.name=CI Fixture", "-c", "user.email=ci@example.invalid",
                    "commit", "-m", "fixture")
                return git("rev-parse", "HEAD")
            git("init")
            (root / "README.md").write_text("baseline", encoding="utf-8")
            before = commit()
            (root / "README.md").write_text("documentation", encoding="utf-8")
            commit()
            event_path = root / "event.json"
            output_path = root / "github-output"
            event_path.write_text(json.dumps({"before": before}), encoding="utf-8")
            env = dict(os.environ, GITHUB_EVENT_NAME="push", GITHUB_EVENT_PATH=str(event_path),
                       GITHUB_OUTPUT=str(output_path), DEVFORGE_CI_BASELINE_READY="true",
                       DEVFORGE_CI_VALIDATED_BASE=before)
            subprocess.run([sys.executable, str(script)], cwd=root, env=env, check=True,
                           stdout=subprocess.PIPE)
            self.assertEqual(output_path.read_text(encoding="utf-8"), "product=false\n")
            (root / "Cargo.toml").write_text("[workspace]", encoding="utf-8")
            commit()
            subprocess.run([sys.executable, str(script)], cwd=root, env=env, check=True,
                           stdout=subprocess.PIPE)
            self.assertEqual(output_path.read_text(encoding="utf-8"), "product=false\nproduct=true\n")

    def test_unavailable_history_and_git_failure_keep_checks(self):
        def failed(_):
            raise subprocess.CalledProcessError(128, ["git", "diff"])
        self.assertTrue(ci_scope.select_product_checks("push", {"before": "a" * 40}, failed))


if __name__ == "__main__":
    unittest.main()
