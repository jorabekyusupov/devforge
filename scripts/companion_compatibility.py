"""E10 isolated Laravel-major contract runner; no host PHP/Composer discovery.

Build the actual adapter harness once:
  scripts/with_rust.sh cargo build -p devforge-inspector --example companion_compatibility
Then supply catalog-verified managed PHP/Composer paths, --native-harness and
--major 11/12/13. Network is disabled unless --allow-download is explicit.
Only temporary Composer home/cache, synthetic app and product stores are written.
This is Foundation/kernel + SQLite/sync queue/SMTP evidence, not native UI or
full Laravel skeleton/network-web-server acceptance. PG/MySQL remain separate.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import tempfile
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
COMPANION = ROOT / "packages/laravel-inspector"


def digest(path):
    value = hashlib.sha256()
    with Path(path).open("rb") as source:
        for block in iter(lambda: source.read(65536), b""):
            value.update(block)
    return value.hexdigest()


def host_target():
    system, machine = platform.system(), platform.machine().lower()
    if system == "Darwin" and machine in ("arm64", "aarch64"):
        return "macos-arm64"
    if system == "Darwin" and machine in ("x86_64", "amd64"):
        return "macos-x64"
    if system == "Windows" and machine in ("x86_64", "amd64"):
        return "windows-x64"
    raise ValueError("unsupported_native_target")


def normalized_descriptor(package):
    return {**package, "executables": package.get("executables", {}),
            "resources": package.get("resources", {})}


def verify_managed(path, kind, name, catalog, target):
    """Match immutable package receipt to current catalog and selected file hash."""
    path = Path(path).absolute()
    if path.is_symlink() or not path.is_file():
        raise ValueError("managed_file_required")
    # Catalog package entrypoints can be nested (notably Windows distributions).
    for folder in path.parents:
        receipt_path = folder / ".devforge-receipt.json"
        if not receipt_path.is_file():
            continue
        receipt = json.loads(receipt_path.read_text(encoding="utf-8"))
        package = receipt.get("package", {})
        if receipt.get("schema") != 1 or package.get("kind") != kind or package.get("target") != target:
            raise ValueError("managed_receipt_mismatch")
        candidates = [key for key, value in catalog["packages"].items()
                      if normalized_descriptor(value) == normalized_descriptor(package)]
        if len(candidates) != 1:
            raise ValueError("package_not_in_current_catalog")
        resource = kind == "composer"
        entries = package.get("resources" if resource else "executables", {})
        expected = receipt.get("resource_hashes" if resource else "hashes", {}).get(name)
        if name not in entries or (folder / entries[name]).resolve() != path.resolve():
            raise ValueError("managed_entrypoint_mismatch")
        actual = digest(path)
        if actual != expected:
            raise ValueError("managed_entrypoint_hash_mismatch")
        return {"package_id": candidates[0], "version": package["version"], "sha256": actual}
    raise ValueError("managed_receipt_required")


def composer_manifest(major, companion):
    if major not in (11, 12, 13):
        raise ValueError("unsupported_framework_major")
    return {
        "name": "devforge/compatibility-fixture", "type": "project",
        "require": {"laravel/framework": f"^{major}.0", "devforge/laravel-inspector": "0.2.0"},
        "repositories": [{"type": "path", "url": str(companion), "options": {"symlink": False}}],
        "config": {"allow-plugins": False}, "minimum-stability": "stable", "prefer-stable": True,
    }


def clean_environment(root, php, allow_download):
    # Do not inherit Composer auth/home, app credentials, PHP INI, agent tokens,
    # SSH agent, Git auth or a real project's environment.
    environment = {key: os.environ[key] for key in (
        "SystemRoot", "WINDIR", "TEMP", "TMP", "TMPDIR", "SSL_CERT_FILE", "SSL_CERT_DIR",
        "CURL_CA_BUNDLE", "HTTPS_PROXY", "HTTP_PROXY", "NO_PROXY",
        "https_proxy", "http_proxy", "no_proxy",
    ) if key in os.environ}
    environment.update({
        "HOME": str(root / "home"), "USERPROFILE": str(root / "home"),
        "COMPOSER_HOME": str(root / "composer-home"), "COMPOSER_CACHE_DIR": str(root / "composer-cache"),
        "COMPOSER_DISABLE_NETWORK": "0" if allow_download else "1", "COMPOSER_NO_INTERACTION": "1",
        "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1", "GIT_TERMINAL_PROMPT": "0",
        "PATH": str(Path(php).parent) + os.pathsep + os.defpath,
    })
    return environment


def validate_native_report(report, major):
    required = {
        "provider_boot_and_request_middleware", "http_kernel_200_and_500", "sqlite_query_listener",
        "sync_queue_success_and_failure_events", "framework_logging_and_exception", "request_job_and_dd_dumps",
        "private_binding_literal_context_and_message_redaction", "production_environment_opt_out",
        "other_project_capture_empty", "actual_authenticated_smtp_and_mime_store", "attachment_exact_bytes",
        "other_project_inbox_empty",
    }
    if report.get("schema") != 1 or report.get("passed") is not True:
        raise ValueError("native_report_failed")
    if str(report.get("framework", "")).split(".")[0] != str(major):
        raise ValueError("wrong_framework_major")
    if not all(report.get("checks", {}).get(check) is True for check in required):
        raise ValueError("native_contract_missing")
    if report.get("native_desktop_checked") is not False or report.get("full_skeleton_or_web_server_checked") is not False:
        raise ValueError("evidence_scope_overclaim")
    return report


def redact_log(text, fixture=None, environment=None):
    # Composer can echo inherited proxy URLs or credentials separately. Unlike
    # app/SMTP fixture credentials, these values may belong to the host user.
    environment = os.environ if environment is None else environment
    secrets = set()
    for key in ("HTTPS_PROXY", "HTTP_PROXY", "https_proxy", "http_proxy"):
        value = environment.get(key, "")
        if not value:
            continue
        try:
            proxy = urlsplit(value if "://" in value else "//" + value)
            if proxy.username is not None or proxy.password is not None:
                secrets.add(value)
                for credential in (proxy.username, proxy.password):
                    if credential:
                        secrets.update((credential, unquote(credential)))
        except ValueError:
            # A malformed proxy still must not be copied into a diagnostic.
            if "@" in value:
                secrets.add(value)
    for secret in sorted(secrets, key=lambda value: (-len(value), value)):
        text = text.replace(secret, "[REDACTED_PROXY_CREDENTIAL]")
    # Also remove userinfo in diagnostic URLs not present in the environment.
    text = re.sub(r"(?i)\b(https?://)[^/\s@]*@", r"\1[REDACTED_USERINFO]@", text)
    if fixture is not None:
        text = text.replace(str(fixture), "<fixture>")
    return text.replace(str(ROOT), "<repository>")[-6000:]


def source_metadata(native_harness):
    sources = {p.name: digest(p) for p in sorted((COMPANION / "src").glob("*.php"))}
    sources["tests/compatibility.php"] = digest(COMPANION / "tests/compatibility.php")
    return {"source_sha256": sources, "native_harness_binary_sha256": digest(native_harness)}


def run(args):
    catalog = json.loads((ROOT / "runtime/catalog.json").read_text(encoding="utf-8"))
    target = host_target()
    php = verify_managed(args.php, "php", "php", catalog, target)
    composer = verify_managed(args.composer, "composer", "composer", catalog, target)
    report = {"schema": 1, "requested_major": args.major, "target": target, "status": "unverified",
              "runtime": {"php": php, "composer": composer}, "network_explicitly_allowed": args.allow_download,
              **source_metadata(args.native_harness),
              "native_desktop_checked": False, "full_skeleton_or_web_server_checked": False,
              "database_coverage": ["sqlite"], "queue_coverage": ["sync"],
              "unverified": ["postgresql", "mysql", "async_queue", "full_laravel_skeleton", "network_http_server", "native_desktop"]}
    with tempfile.TemporaryDirectory(prefix=f"devforge-e10-laravel{args.major}-") as folder:
        fixture = Path(folder)
        for directory in ("home", "composer-home", "config", "bootstrap/cache", "storage/logs", "storage/framework/views"):
            (fixture / directory).mkdir(parents=True, exist_ok=True)
        (fixture / ".devforge-e10-fixture").write_text("isolated-companion-contract-v1\n", encoding="utf-8")
        if args.composer_cache:
            shutil.copytree(args.composer_cache, fixture / "composer-cache", symlinks=False)
        environment = clean_environment(fixture, args.php, args.allow_download)
        (fixture / "composer.json").write_text(json.dumps(composer_manifest(args.major, COMPANION), indent=2), encoding="utf-8")
        command = [str(args.php), "-n", "-d", "date.timezone=UTC", str(args.composer), "update",
                   "--no-interaction", "--no-scripts", "--no-plugins", "--no-progress", "--prefer-dist"]
        try:
            resolved = subprocess.run(command, cwd=fixture, env=environment, stdout=subprocess.PIPE,
                                      stderr=subprocess.STDOUT, timeout=args.timeout, check=False)
        except subprocess.TimeoutExpired:
            report.update(status="blocked", blocker="composer_timeout")
            return report
        if resolved.returncode:
            diagnostic = resolved.stdout.decode("utf-8", errors="replace")
            report.update(status="blocked", blocker="composer_resolution_or_install_failed",
                          diagnostic=redact_log(diagnostic, fixture, environment))
            return report
        lock = json.loads((fixture / "composer.lock").read_text(encoding="utf-8"))
        packages = {package["name"]: package["version"] for package in lock["packages"]}
        if packages.get("laravel/framework", "").lstrip("v").split(".")[0] != str(args.major):
            raise ValueError("resolved_wrong_framework_major")
        for source in (COMPANION / "src").glob("*.php"):
            if digest(source) != digest(fixture / "vendor/devforge/laravel-inspector/src" / source.name):
                raise ValueError("installed_companion_source_changed")
        report.update(composer_lock_sha256=digest(fixture / "composer.lock"), resolved_packages=packages)
        native_result = fixture / "native-result.json"
        native = subprocess.run([str(args.native_harness), str(args.php), str(fixture),
                                 str(COMPANION / "tests/compatibility.php"), str(native_result)],
                                cwd=fixture, env=environment, stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT, timeout=120, check=False)
        if native.returncode:
            report.update(status="failed", blocker="native_contract_failed",
                          diagnostic=redact_log(native.stdout.decode("utf-8", errors="replace"), fixture, environment))
            return report
        report.update(status="passed", contract=validate_native_report(
            json.loads(native_result.read_text(encoding="utf-8")), args.major))
        return report


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--major", type=int, required=True, choices=[11, 12, 13])
    parser.add_argument("--php", type=Path, required=True)
    parser.add_argument("--composer", type=Path, required=True, help="managed composer.phar, not host executable")
    parser.add_argument("--native-harness", type=Path, required=True)
    parser.add_argument("--composer-cache", type=Path, help="optional owned fixture cache; copied before use")
    parser.add_argument("--allow-download", action="store_true")
    parser.add_argument("--timeout", type=int, default=300)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    for name in ("php", "composer", "native_harness", "composer_cache", "report"):
        path = getattr(args, name)
        if path is not None:
            setattr(args, name, path.absolute())
    try:
        report = run(args)
    except (ValueError, OSError, subprocess.TimeoutExpired) as error:
        # No raw environment/credential values enter exception reports.
        report = {"schema": 1, "requested_major": args.major, "status": "blocked",
                  "blocker": type(error).__name__, "diagnostic": redact_log(str(error))[-2000:]}
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"Laravel {args.major}: {report['status']}")
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
