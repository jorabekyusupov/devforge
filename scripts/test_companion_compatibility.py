import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from scripts.companion_compatibility import (
    COMPANION, clean_environment, composer_manifest, digest, main, redact_log,
    source_metadata, validate_native_report, verify_managed,
)


class CompanionCompatibilityContracts(unittest.TestCase):
    def fixture(self, root):
        binary = root / "php"
        binary.write_bytes(b"immutable fixture executable")
        package = {"kind": "php", "version": "8.fixture", "target": "macos-arm64",
                   "executables": {"php": "php"}, "sha256": "publisher-archive"}
        receipt = {"schema": 1, "package": package, "hashes": {"php": digest(binary)}}
        (root / ".devforge-receipt.json").write_text(json.dumps(receipt), encoding="utf-8")
        return binary, {"packages": {"php-fixture": package}}

    def test_managed_selection_requires_current_descriptor_and_matching_entrypoint_bytes(self):
        with tempfile.TemporaryDirectory() as folder:
            binary, catalog = self.fixture(Path(folder))
            self.assertEqual(verify_managed(binary, "php", "php", catalog, "macos-arm64")["package_id"], "php-fixture")
            binary.write_bytes(b"changed")
            with self.assertRaisesRegex(ValueError, "hash_mismatch"):
                verify_managed(binary, "php", "php", catalog, "macos-arm64")

    def test_wrong_host_and_removed_catalog_package_are_refused(self):
        with tempfile.TemporaryDirectory() as folder:
            binary, catalog = self.fixture(Path(folder))
            with self.assertRaisesRegex(ValueError, "receipt_mismatch"):
                verify_managed(binary, "php", "php", catalog, "windows-x64")
            with self.assertRaisesRegex(ValueError, "not_in_current_catalog"):
                verify_managed(binary, "php", "php", {"packages": {}}, "macos-arm64")

    def test_unmanaged_php_is_not_accepted(self):
        with tempfile.TemporaryDirectory() as folder:
            binary = Path(folder) / "php"
            binary.write_bytes(b"host binary")
            with self.assertRaisesRegex(ValueError, "receipt_required"):
                verify_managed(binary, "php", "php", {"packages": {}}, "macos-arm64")

    def test_synthetic_manifest_selects_major_without_disabling_advisory_guards(self):
        for major in [11, 12, 13]:
            manifest = composer_manifest(major, Path("/controlled/companion"))
            self.assertEqual(manifest["require"]["laravel/framework"], f"^{major}.0")
            self.assertFalse(manifest["config"]["allow-plugins"])
            self.assertNotIn("audit", manifest["config"])
            self.assertFalse(manifest["repositories"][0]["options"]["symlink"])
        with self.assertRaises(ValueError):
            composer_manifest(14, Path("/controlled/companion"))

    def test_environment_is_private_and_offline_by_default(self):
        with patch.dict(os.environ, {"COMPOSER_AUTH": "PRIVATE", "DB_PASSWORD": "PRIVATE",
                                    "PHP_INI_SCAN_DIR": "/user/php", "SSH_AUTH_SOCK": "/user/agent"}):
            env = clean_environment(Path("/fixture"), Path("/managed/php"), False)
        for key in ["COMPOSER_AUTH", "DB_PASSWORD", "PHP_INI_SCAN_DIR", "SSH_AUTH_SOCK"]:
            self.assertNotIn(key, env)
        self.assertEqual(env["COMPOSER_DISABLE_NETWORK"], "1")
        self.assertEqual(env["HOME"], "/fixture/home")
        self.assertEqual(clean_environment(Path("/fixture"), Path("/managed/php"), True)["COMPOSER_DISABLE_NETWORK"], "0")

    def test_report_cannot_pass_with_wrong_major_missing_contract_or_overclaimed_scope(self):
        checks = {name: True for name in [
            "provider_boot_and_request_middleware", "http_kernel_200_and_500", "sqlite_query_listener",
            "sync_queue_success_and_failure_events", "framework_logging_and_exception", "request_job_and_dd_dumps",
            "private_binding_literal_context_and_message_redaction", "production_environment_opt_out",
            "other_project_capture_empty", "actual_authenticated_smtp_and_mime_store", "attachment_exact_bytes",
            "other_project_inbox_empty",
        ]}
        report = {"schema": 1, "passed": True, "framework": "12.99.1", "checks": checks,
                  "native_desktop_checked": False, "full_skeleton_or_web_server_checked": False}
        validate_native_report(report, 12)
        with self.assertRaisesRegex(ValueError, "wrong_framework_major"):
            validate_native_report(report, 11)
        checks["sqlite_query_listener"] = False
        with self.assertRaisesRegex(ValueError, "contract_missing"):
            validate_native_report(report, 12)
        checks["sqlite_query_listener"] = True
        with self.assertRaisesRegex(ValueError, "overclaim"):
            validate_native_report({**report, "native_desktop_checked": True}, 12)

    def test_diagnostics_hide_proxy_url_userinfo_and_separately_echoed_credentials(self):
        username = "E10_PROXY_USERNAME_CANARY"
        encoded = "E10_PROXY_PASSWORD%40CANARY"
        decoded = "E10_PROXY_PASSWORD@CANARY"
        proxy = f"https://{username}:{encoded}@proxy.example.test:8443"
        text = f"proxy {proxy}; username {username}; password {encoded}; decoded {decoded}"
        result = redact_log(text, Path("/fixture"), {"HTTPS_PROXY": proxy})
        for secret in (proxy, username, encoded, decoded):
            self.assertNotIn(secret, result)
        unknown = redact_log("https://other-user:other-password@other.example.test/path", environment={})
        self.assertNotIn("other-user", unknown)
        self.assertNotIn("other-password", unknown)
        self.assertIn("other.example.test/path", unknown)

    def test_exception_report_uses_the_same_proxy_redaction(self):
        proxy = "http://E10_EXCEPTION_PROXY_USER:E10_EXCEPTION_PROXY_PASSWORD@proxy.example.test:8080"
        with tempfile.TemporaryDirectory() as folder:
            report = Path(folder) / "report.json"
            argv = ["companion_compatibility.py", "--major", "12", "--php", "/fixture/php",
                    "--composer", "/fixture/composer.phar", "--native-harness", "/fixture/native",
                    "--report", str(report)]
            with patch("sys.argv", argv), patch.dict(os.environ, {"HTTP_PROXY": proxy}), \
                    patch("scripts.companion_compatibility.run", side_effect=ValueError(
                        f"request failed {proxy}; password E10_EXCEPTION_PROXY_PASSWORD")), \
                    patch("builtins.print"):
                self.assertEqual(main(), 1)
            diagnostic = json.loads(report.read_text(encoding="utf-8"))["diagnostic"]
            for secret in (proxy, "E10_EXCEPTION_PROXY_USER", "E10_EXCEPTION_PROXY_PASSWORD"):
                self.assertNotIn(secret, diagnostic)

    def test_future_provenance_records_current_native_binary_and_php_fixture_bytes(self):
        with tempfile.TemporaryDirectory() as folder:
            binary = Path(folder) / "native-harness"
            binary.write_bytes(b"first-build")
            first = source_metadata(binary)
            self.assertEqual(first["native_harness_binary_sha256"], digest(binary))
            self.assertEqual(first["source_sha256"]["tests/compatibility.php"],
                             digest(COMPANION / "tests/compatibility.php"))
            binary.write_bytes(b"second-build")
            self.assertNotEqual(first["native_harness_binary_sha256"],
                                source_metadata(binary)["native_harness_binary_sha256"])


if __name__ == "__main__":
    unittest.main()
