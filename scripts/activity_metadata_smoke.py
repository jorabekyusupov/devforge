"""One approved readonly-copy Laravel fixture; no Composer/network/source writes."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
from companion_compatibility import ROOT, COMPANION, digest, host_target, verify_managed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-project", type=Path, required=True)
    parser.add_argument("--php", type=Path, required=True)
    parser.add_argument("--driver", type=Path, required=True)
    parser.add_argument("--cli", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--log", type=Path, required=True)
    args = parser.parse_args()
    source = args.source_project.resolve()
    lock = json.loads((source / "composer.lock").read_text())
    framework = next(package["version"] for package in lock["packages"] if package["name"] == "laravel/framework")
    if framework != "v13.42.0":
        raise ValueError("approved_framework_lock_mismatch")
    if json.loads((source / "composer.json").read_text()).get("autoload", {}).get("files"):
        raise ValueError("project_autoload_files_not_in_approved_copy")
    catalog = json.loads((ROOT / "runtime/catalog.json").read_text())
    runtime = verify_managed(args.php, "php", "php", catalog, host_target())
    # Existing native Unix sockets have a strict path bound; keep this owned
    # fixture short rather than relaxing the product endpoint guard.
    with tempfile.TemporaryDirectory(prefix="df-m13meta-", dir="/tmp") as directory:
        owned = Path(directory)
        project = owned / "project"
        project.mkdir()
        vendor = source / "vendor"
        for path in vendor.rglob("*"):
            if path.is_symlink() and not str(path.relative_to(vendor)).startswith("devforge/laravel-inspector"):
                if not path.resolve().is_relative_to(vendor.resolve()):
                    raise ValueError("unapproved_external_vendor_link")
        def ignore_companion(directory, names):
            return ["laravel-inspector"] if Path(directory) == vendor / "devforge" else []
        shutil.copytree(vendor, project / "vendor", symlinks=False, ignore=ignore_companion)
        shutil.copytree(COMPANION, project / "vendor/devforge/laravel-inspector")
        for name in ["composer.json", "composer.lock"]:
            shutil.copy2(source / name, project / name)
        for folder in ["config", "bootstrap/cache", "storage/logs", "storage/framework/views", "public"]:
            (project / folder).mkdir(parents=True, exist_ok=True)
        (project / "public/index.php").write_text("<?php")
        (project / ".devforge-e10-fixture").write_text("owned-framework-component-fixture\n")
        # Explicit fixture package discovery: only the current companion provider
        # registered by compatibility.php, no source application/package hooks.
        (project / "bootstrap/cache/packages.php").write_text("<?php return [];\n")
        script = (COMPANION / "tests/compatibility.php").read_text()
        # Source Composer used a project-local path repository. Bind only the
        # current copied companion classes in this fresh fixture; never load
        # source .devforge/appdata or regenerate Composer configuration.
        autoload = "require $root.'/vendor/autoload.php';"
        classes = ["Capture", "InspectorServiceProvider", "RequestCapture"]
        mapped = "$loader = require $root.'/vendor/autoload.php';\n$loader->addClassMap([\n" + "\n".join(
            f"    'DevForge\\\\Inspector\\\\{name}' => $root.'/vendor/devforge/laravel-inspector/src/{name}.php'," for name in classes
        ) + "\n]);"
        if autoload not in script:
            raise ValueError("companion_fixture_autoload_boundary_changed")
        script = script.replace(autoload, mapped)
        for before, after in [
            ("'E10 compatibility'", "'M13_MAIL_PRIVATE_CANARY'"),
            ("'E10 local-only body'", "'M13_BODY_PRIVATE_CANARY https://private.example/reset?token=PRIVATE_RESET_TOKEN'"),
            ("'sender@example.test'", "'private-sender@example.test'"),
            ("'recipient@example.test'", "'private-recipient@example.test'"),
            ("'token=E10_LOG_PRIVATE_CANARY'", "'M13_RAW_LOG_PRIVATE_CANARY'"),
        ]:
            if before not in script:
                raise ValueError("current_companion_fixture_contract_changed")
            script = script.replace(before, after)
        fixture_php = owned / "metadata.php"
        fixture_php.write_text(script)
        copied_package = owned / "php-package"
        shutil.copytree(args.php.parent, copied_package)
        php = copied_package / args.php.name
        if digest(php) != runtime["sha256"]:
            raise ValueError("copied_php_hash_mismatch")
        node = shutil.which("node") or ""
        launcher = Path("/tmp/devforge-m13-inspector-2.15.0/node_modules/@modelcontextprotocol/inspector/clients/launcher/build/index.js")
        command = [str(args.driver.resolve()), str(project), str(php), str(fixture_php),
                   str(args.cli.resolve()), str(args.report.resolve()), node, str(launcher)]
        with args.log.open("w") as log:
            result = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, timeout=120, check=False)
        if result.returncode:
            print("owned metadata flow failed; inspect the explicit fixture log")
            return result.returncode
        report = json.loads(args.report.read_text())
        report.update({"framework_lock_version": framework, "framework_lock_sha256": digest(source / "composer.lock"),
                       "php_runtime": runtime, "php_fixture_sha256": digest(fixture_php),
                       "driver_sha256": digest(args.driver), "cli_sha256": digest(args.cli),
                       "copy_scope": "vendor_and_composer_json_lock_only", "global_home_or_config_changed": False})
        args.report.write_text(json.dumps(report, indent=2) + "\n")
    print("one owned Laravel/SMTP public metadata flow passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
