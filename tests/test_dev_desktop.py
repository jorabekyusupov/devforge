import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import stat
import subprocess
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("dev_desktop", Path(__file__).parents[1] / "scripts/dev_desktop.py")
dev = importlib.util.module_from_spec(spec)
spec.loader.exec_module(dev)
TARGET = "aarch64-apple-darwin"


class Fixture:
    def __init__(self, root, target=TARGET, system="Darwin"):
        self.root, self.target, self.system, self.calls = root, target, system, []
        self.target_dir = root / "custom target & artifacts"
        self.metadata = {"workspace_root": str(root), "target_directory": str(self.target_dir),
                         "workspace_members": ["cli", "desktop"], "packages": [
                             {"id": "cli", "name": "devforge-cli", "manifest_path": str(root / "crates/cli/Cargo.toml")},
                             {"id": "desktop", "name": "devforge-desktop", "manifest_path": str(root / "apps/desktop/src-tauri/Cargo.toml")}]}

    def run(self, command, *, cwd, env, capture=False):
        self.calls.append((command, cwd, dict(env)))
        if "rustc" in command:
            return SimpleNamespace(stdout=f"rustc fixture\nhost: {self.target}\n")
        if "metadata" in command:
            return SimpleNamespace(stdout=json.dumps(self.metadata))
        if "build" in command:
            executable, _ = dev.artifact_paths(self.root, self.metadata, self.target)
            executable.parent.mkdir(parents=True)
            executable.write_bytes(b"exact adjacent debug CLI bytes")
        return SimpleNamespace(stdout="")


class DebugDesktopTests(unittest.TestCase):
    def test_prepare_only_hashes_exact_adjacent_binary_preserves_generated_files_and_never_launches(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture = Fixture(root)
            generated = root / "distribution/generated"
            generated.mkdir(parents=True)
            (generated / "sentinel").write_bytes(b"preserved")
            report = dev.prepare(root, environment={}, system="Darwin", machine="arm64", runner=fixture.run)
            self.assertEqual(len(fixture.calls), 3)
            self.assertNotIn("node", [part for call, _, _ in fixture.calls for part in call])
            command, cwd, env = fixture.calls[-1]
            self.assertEqual(command[-9:], ["cargo", "build", "--locked", "-p", "devforge-cli", "--bin", "devforge", "--target", TARGET])
            self.assertEqual(cwd, root / "apps/desktop/src-tauri")
            self.assertNotIn("DEVFORGE_PACKAGE_MANAGER", env)
            self.assertEqual(report["manager"], str(fixture.target_dir / TARGET / "debug/devforge"))
            self.assertEqual(Path(report["adjacent_gui"]).parent, Path(report["manager"]).parent)
            manifest = json.loads(Path(report["manifest"]).read_text())
            self.assertEqual(manifest, {"schema": 1, "target": TARGET, "filename": "devforge", "sha256": hashlib.sha256(b"exact adjacent debug CLI bytes").hexdigest()})
            self.assertEqual((generated / "sentinel").read_bytes(), b"preserved")
            self.assertFalse(report["launch_requested"])

    def test_dry_run_has_only_read_tool_queries_no_build_write_or_launch(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture = Fixture(root)
            report = dev.prepare(root, dry_run=True, environment={}, system="Darwin", machine="arm64", runner=fixture.run)
            self.assertEqual(report["mode"], "dry_run")
            self.assertEqual(len(fixture.calls), 2)
            self.assertFalse((root / "distribution").exists())
            self.assertFalse(fixture.target_dir.exists())

    def test_explicit_launch_uses_same_target_debug_adjacent_file_and_build_only_gate(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture = Fixture(root)
            tauri = root / "node_modules/@tauri-apps/cli/tauri.js"
            tauri.parent.mkdir(parents=True)
            tauri.write_text("fixture")
            original = {"TEST_ENV": "unchanged"}
            dev.prepare(root, launch=True, environment=original, system="Darwin", machine="arm64", runner=fixture.run)
            command, cwd, env = fixture.calls[-1]
            self.assertEqual(command, [str(root / "scripts/with_rust.sh"), "node", str(tauri), "dev", "--target", TARGET, "--", "--locked"])
            self.assertEqual(cwd, root / "apps/desktop")
            self.assertEqual(env["DEVFORGE_PACKAGE_MANAGER"], "1")
            self.assertEqual(original, {"TEST_ENV": "unchanged"})

    def test_reject_cross_unsupported_ambient_or_ambiguous_host(self):
        for host, system, machine, requested, env in [
            (TARGET, "Darwin", "arm64", "x86_64-apple-darwin", {}),
            ("x86_64-apple-darwin", "Darwin", "arm64", None, {}),
            (TARGET, "Linux", "arm64", None, {}),
            (TARGET, "Darwin", "arm64", None, {"CARGO_BUILD_TARGET": "wasm32-unknown-unknown"}),
            (TARGET, "Darwin", "arm64", None, {"TAURI_CONFIG": "{}"}),
            ("x86_64-pc-windows-msvc", "Windows", "AMD64", None, {"PROCESSOR_ARCHITEW6432": "ARM64"}),
        ]:
            with self.subTest(host=host, environment=env), self.assertRaises(ValueError):
                dev.choose_target(f"host: {host}\n", system, machine, requested, env)
        with self.assertRaises(ValueError):
            dev.choose_target(f"host: {TARGET}\nhost: {TARGET}", "Darwin", "arm64", None, {})

    def test_windows_has_native_exe_layout_and_no_posix_tool_wrapper(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture = Fixture(root, "x86_64-pc-windows-msvc", "Windows")
            report = dev.prepare(root, dry_run=True, environment={}, system="Windows", machine="AMD64", runner=fixture.run)
            self.assertEqual(fixture.calls[0][0], ["rustc", "-vV"])
            self.assertEqual(Path(report["manager"]).name, "devforge.exe")
            self.assertEqual(Path(report["adjacent_gui"]).name, "devforge-desktop.exe")

    def test_hash_streams_regular_file_and_refuses_linked_authority(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            executable = root / "devforge"
            executable.write_bytes(b"x" * (2 * 1024 * 1024 + 3))
            with patch.object(Path, "read_bytes", side_effect=AssertionError("must stream")):
                digest = dev.hash_executable(executable)
            self.assertEqual(digest, hashlib.sha256(b"x" * (2 * 1024 * 1024 + 3)).hexdigest())
            if not dev.os.name == "nt":
                linked = root / "link"
                linked.symlink_to(executable)
                with self.assertRaises(ValueError):
                    dev.hash_executable(linked)

    def test_authority_rejects_reparse_metadata_and_unix_link_canary(self):
        metadata = SimpleNamespace(st_mode=stat.S_IFDIR | 0o700, st_file_attributes=0x400)
        with patch.object(Path, "lstat", return_value=metadata), self.assertRaises(ValueError):
            dev.check_generated_path(Path("fixed/generated"), directory=True)
        if dev.os.name == "nt":
            return  # The reparse rejection above runs without OS permission changes.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            executable = root / "devforge"
            executable.write_bytes(b"owned executable fixture")
            generated = root / "distribution/generated"
            generated.mkdir(parents=True)
            outside = root / "outside"
            outside.write_bytes(b"outside canary")
            (generated / "manager-artifact.json").symlink_to(outside)
            with self.assertRaises(ValueError):
                dev.write_manifest(root, TARGET, executable)
            self.assertEqual(outside.read_bytes(), b"outside canary")
            self.assertTrue((generated / "manager-artifact.json").is_symlink())

    def test_failed_build_does_not_hash_stale_binary_or_change_manifest(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture = Fixture(root)
            generated = root / "distribution/generated"
            generated.mkdir(parents=True)
            manifest = generated / "manager-artifact.json"
            manifest.write_text("old authority")
            def failed(command, **kwargs):
                if "build" in command:
                    raise subprocess.CalledProcessError(1, command)
                return fixture.run(command, **kwargs)
            with self.assertRaises(subprocess.CalledProcessError):
                dev.prepare(root, environment={}, system="Darwin", machine="arm64", runner=failed)
            self.assertEqual(manifest.read_text(), "old authority")

    def test_changed_adjacent_binary_after_preparation_never_launches(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture = Fixture(root)
            tauri = root / "node_modules/@tauri-apps/cli/tauri.js"
            tauri.parent.mkdir(parents=True)
            tauri.write_text("fixture")
            original = dev.write_manifest
            def tamper(root, target, executable):
                result = original(root, target, executable)
                executable.write_bytes(b"changed binary")
                return result
            with patch.object(dev, "write_manifest", side_effect=tamper), self.assertRaises(ValueError):
                dev.prepare(root, launch=True, environment={}, system="Darwin", machine="arm64", runner=fixture.run)
            self.assertEqual(len(fixture.calls), 3)

    def test_metadata_layout_and_target_validation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            fixture = Fixture(root)
            fixture.metadata["target_directory"] = "relative"
            with self.assertRaises(ValueError):
                dev.artifact_paths(root, fixture.metadata, TARGET)
            fixture.metadata["target_directory"] = str(root / "target")
            fixture.metadata["packages"][0]["manifest_path"] = str(root / "other/Cargo.toml")
            with self.assertRaises(ValueError):
                dev.artifact_paths(root, fixture.metadata, TARGET)


if __name__ == "__main__":
    unittest.main()
