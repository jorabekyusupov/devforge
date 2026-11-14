import importlib.util
from pathlib import Path
import subprocess
import sys
import unittest

SCRIPTS = Path(__file__).parents[1] / "scripts"
sys.path.insert(0, str(SCRIPTS))
spec = importlib.util.spec_from_file_location("rust_scope", SCRIPTS / "rust_scope.py")
rust_scope = importlib.util.module_from_spec(spec)
spec.loader.exec_module(rust_scope)

ROOT = Path("/fixture").resolve()


def package(name, path, dependencies=()):
    return {"id": name, "name": name, "manifest_path": str(ROOT / path / "Cargo.toml"),
            "dependencies": list(dependencies)}


def edge(path, kind=None, target=None):
    return {"path": str(ROOT / path), "kind": kind, "target": target}


def graph():
    packages = [
        package("leaf", "crates/leaf"),
        package("independent", "crates/independent"),
        package("service", "crates/service", [edge("crates/leaf", "dev", "cfg(windows)")]),
        package("devforge-desktop", "apps/desktop/src-tauri", [edge("crates/service", "build")]),
    ]
    return {"workspace_members": [p["id"] for p in packages], "packages": packages}


class RustScopeTests(unittest.TestCase):
    def select(self, paths, metadata=None):
        return rust_scope.affected_scope(paths, metadata or graph(), ROOT)

    def test_reverse_transitive_closure_includes_dev_build_and_other_os_edges(self):
        scope = self.select(["crates/leaf/src/lib.rs"])
        self.assertEqual(scope, {"mode": "affected", "packages": ["devforge-desktop", "leaf", "service"],
                                 "frontend": True})
        self.assertNotIn("independent", scope["packages"])

    def test_leaf_without_desktop_consumer_avoids_frontend_and_binary(self):
        scope = self.select(["crates/independent/src/lib.rs", "README.md"])
        self.assertEqual(scope, {"mode": "affected", "packages": ["independent"], "frontend": False})

    def test_cli_runtime_consumer_desktop_is_added_without_cargo_edge(self):
        metadata = graph()
        metadata["packages"].append(package("devforge-cli", "crates/cli"))
        metadata["workspace_members"].append("devforge-cli")
        scope = self.select(["crates/cli/src/main.rs"], metadata)
        self.assertEqual(scope["packages"], ["devforge-cli", "devforge-desktop"])
        self.assertTrue(scope["frontend"])

    def test_root_manifests_package_manifests_unknown_and_nonrust_keep_full(self):
        for path in ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "crates/leaf/Cargo.toml",
                     "package.json", "apps/desktop/src/main.tsx", "runtime/catalog.json",
                     "scripts/ci_scope.py", ".github/workflows/check.yml", "unknown/code.rs",
                     "crates/leaf/src/config.json", "crates/../leaf/src/lib.rs"]:
            with self.subTest(path=path):
                self.assertEqual(self.select(["crates/leaf/src/lib.rs", path]), rust_scope.full_scope())

    def test_renamed_or_deleted_sources_map_both_paths(self):
        # ci_scope Git diff --no-renames supplies old and new paths.
        self.assertEqual(self.select(["crates/independent/src/old.rs", "crates/independent/src/new.rs"])
                         ["packages"], ["independent"])
        self.assertEqual(self.select(["crates/removed/src/old.rs", "docs/moved.md"]), rust_scope.full_scope())
        self.assertEqual(self.select(["crates/leaf/src/old.rs", "crates/independent/src/new.rs"])
                         ["packages"], ["devforge-desktop", "independent", "leaf", "service"])

    def test_docs_only_none_but_empty_comparison_is_full(self):
        self.assertEqual(self.select(["docs/knowledge/log.md"])["mode"], "none")
        self.assertEqual(self.select([]), rust_scope.full_scope())

    def test_manual_missing_history_metadata_failure_are_full(self):
        def failure(*_):
            raise subprocess.CalledProcessError(128, "git")
        for name, event in [("workflow_dispatch", {}), ("push", {"before": "0" * 40}),
                            ("pull_request", {}), ("push", [])]:
            self.assertEqual(rust_scope.select_scope(name, event, failure, graph, ROOT), rust_scope.full_scope())
        self.assertEqual(rust_scope.select_scope("push", {"before": "a" * 40}, failure, graph, ROOT), rust_scope.full_scope())
        self.assertEqual(rust_scope.select_scope("push", {"before": "a" * 40}, lambda _: ["README.md"], failure, ROOT), rust_scope.full_scope())

    def test_pr_selects_base_push_selects_before(self):
        seen = []
        def diff(base):
            seen.append(base)
            return ["crates/independent/src/lib.rs"]
        event = {"before": "a" * 40, "pull_request": {"base": {"sha": "b" * 40}}}
        for name in ["push", "pull_request"]:
            self.assertEqual(rust_scope.select_scope(name, event, diff, graph, ROOT)["mode"], "affected")
        self.assertEqual(seen, ["a" * 40, "b" * 40])

    def test_incomplete_metadata_or_unknown_local_dependency_keep_full(self):
        metadata = graph()
        metadata["packages"][0]["dependencies"] = [edge("outside-member")]
        self.assertEqual(self.select(["crates/leaf/src/lib.rs"], metadata), rust_scope.full_scope())
        metadata = graph()
        metadata["workspace_members"].append("unresolved")
        self.assertEqual(self.select(["crates/leaf/src/lib.rs"], metadata), rust_scope.full_scope())

    def test_argument_arrays_and_injection_rejected_to_full(self):
        scope = self.select(["crates/independent/src/lib.rs"])
        self.assertEqual(rust_scope.cargo_command(scope, "test"), ["cargo", "test", "-p", "independent"])
        self.assertEqual(rust_scope.cargo_command(scope, "clippy"),
                         ["cargo", "clippy", "-p", "independent", "--all-targets", "--", "-D", "warnings"])
        self.assertEqual(rust_scope.cargo_command(scope, "fmt"), ["cargo", "fmt", "-p", "independent", "--", "--check"])
        for name in ["--workspace", "x; echo bad", "$(bad)", "a\nb", ""]:
            malformed = {"mode": "affected", "packages": [name]}
            self.assertEqual(rust_scope.cargo_command(malformed, "test"), ["cargo", "test", "--workspace"])
        self.assertIsNone(rust_scope.cargo_command({"mode": "none", "packages": [], "frontend": False}, "test"))
        for malformed in [{"mode": "none"}, {"mode": "none", "packages": [], "frontend": True}, {}, {"mode": "other"}]:
            self.assertEqual(rust_scope.cargo_command(malformed, "test"), ["cargo", "test", "--workspace"])
        with self.assertRaises(ValueError):
            rust_scope.cargo_command(scope, "shell")


if __name__ == "__main__":
    unittest.main()
