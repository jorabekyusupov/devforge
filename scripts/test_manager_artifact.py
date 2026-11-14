"""Copied CLI digest authority generation, without a packaging build/download."""
import hashlib
import json
import pathlib
import tempfile
import unittest
from scripts import package_desktop


class ManagerArtifactTests(unittest.TestCase):
    def test_copied_cli_digest_and_fixed_bundle_name_for_each_target(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            sidecars = root / 'sidecars'
            generated = root / 'generated'
            sidecars.mkdir()
            generated.mkdir()
            for target in sorted(package_desktop.MANAGER_TARGETS):
                suffix = '.exe' if 'windows' in target else ''
                payload = ('copied-cli-' + target).encode()
                (sidecars / ('devforge-' + target + suffix)).write_bytes(payload)
                value = package_desktop.manager_manifest(sidecars, generated, target)
                self.assertEqual(value, {'schema': 1, 'target': target,
                    'filename': 'devforge' + suffix,
                    'sha256': hashlib.sha256(payload).hexdigest()})
                self.assertEqual(json.loads((generated / 'manager-artifact.json').read_text()), value)
                self.assertNotIn('path', value)

    def test_unsupported_target_and_missing_cli_do_not_create_manifest(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            for target in ['../arbitrary', 'x86_64-pc-windows-msvc']:
                with self.assertRaises(ValueError):
                    package_desktop.manager_manifest(root, root, target)
                self.assertFalse((root / 'manager-artifact.json').exists())

    def test_changed_copy_changes_authority_on_explicit_regeneration(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            copied = root / 'devforge-aarch64-apple-darwin'
            copied.write_bytes(b'first')
            before = package_desktop.manager_manifest(root, root, 'aarch64-apple-darwin')
            copied.write_bytes(b'second')
            after = package_desktop.manager_manifest(root, root, 'aarch64-apple-darwin')
            self.assertNotEqual(before['sha256'], after['sha256'])

    def test_macos_presigned_copy_avoids_external_resigning_and_preserves_helper(self):
        config = {'bundle': {'externalBin': ['binaries/devforge', 'binaries/devforge-helper'], 'macOS': {'signingIdentity': '-'}}}
        result = package_desktop.manager_bundle_config(config, pathlib.Path('/owned/sidecars'), 'aarch64-apple-darwin')
        self.assertEqual(result['bundle']['externalBin'], ['binaries/devforge-helper'])
        self.assertEqual(result['bundle']['macOS']['files'], {'MacOS/devforge': '/owned/sidecars/devforge-aarch64-apple-darwin'})
        windows = {'bundle': {'externalBin': ['binaries/devforge', 'binaries/devforge-helper']}}
        self.assertEqual(package_desktop.manager_bundle_config(windows, pathlib.Path('/owned'), 'x86_64-pc-windows-msvc'), windows)
        self.assertNotIn('macOS', windows['bundle'])

    def test_postbundle_verification_refuses_mutated_final_payload_on_every_target(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            for target in sorted(package_desktop.MANAGER_TARGETS):
                suffix = '.exe' if 'windows' in target else ''
                copied = root / ('devforge-' + target + suffix)
                copied.write_bytes(b'expected')
                manifest = package_desktop.manager_manifest(root, root, target)
                final = root / ('devforge' + suffix)
                final.write_bytes(b'expected')
                report = package_desktop.verify_manager_payload(final, manifest, target, 'macos-app-bundle' if 'apple' in target else 'nsis-external-binary-source')
                self.assertTrue(report['hash_matches_compiled_authority'])
                self.assertEqual(report['bundled_cli_verified'], 'apple' in target)
                self.assertFalse(report['installed_cli_verified'])
                final.write_bytes(b'mutated-after-signing')
                with self.assertRaises(ValueError):
                    package_desktop.verify_manager_payload(final, manifest, target, 'payload')

    def test_build_refuses_changed_authority_even_when_json_value_is_same(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            path = root / 'manager-artifact.json'
            path.write_text('{"schema":1}')
            captured, value = package_desktop.capture_manager_authority(path)
            self.assertEqual(value, {'schema': 1})
            package_desktop.verify_authority_unchanged(path, captured)
            # JSON-equivalent whitespace still invalidates the captured bytes.
            path.write_text('{"schema": 1}')
            with self.assertRaises(ValueError):
                package_desktop.verify_authority_unchanged(path, captured)
            path.write_text('{"schema":2}')
            with self.assertRaises(ValueError):
                package_desktop.verify_authority_unchanged(path, captured)
            path.write_bytes(b' ' * 1025)
            with self.assertRaises(ValueError):
                package_desktop.capture_manager_authority(path)
            path.unlink()
            with self.assertRaises(ValueError):
                package_desktop.verify_authority_unchanged(path, captured)


if __name__ == '__main__':
    unittest.main()
