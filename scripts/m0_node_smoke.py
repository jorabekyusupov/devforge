#!/usr/bin/env python3
"""Execute verified Node/npm artifacts on the current OS and probe Intl locales.

macOS x64 on ARM is labeled Rosetta. A successful Intl probe is not evidence
that the DevForge interface has translations or that Vite HMR works.
"""

import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile
import zipfile

from m0_runtime_audit import digest, safe_member
from m0_macos_smoke import extract_tar


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--catalog", type=Path, required=True)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    host = platform.system()
    if host not in ("Darwin", "Windows"):
        parser.error("This M0 probe targets macOS or Windows")
    target_prefix = "macos-" if host == "Darwin" else "windows-"
    results = []
    for entry in json.loads(args.catalog.read_text()):
        if entry["package"] != "node" or not entry["target"].startswith(target_prefix):
            continue
        prefix = ["/usr/bin/arch", "-x86_64"] if host == "Darwin" and entry["target"] == "macos-x64" else []
        mode = "rosetta" if prefix and platform.machine() == "arm64" else "native"
        if host == "Windows" and platform.machine().lower() in ("arm64", "aarch64"):
            mode = "windows-x64-emulation"
        result = {"artifact_id": entry["id"], "mode": mode,
                  "scope": "Standalone Node/npm and Intl; not application translation or Vite HMR proof"}
        with tempfile.TemporaryDirectory(prefix="devforge-m0-node-") as directory:
            work = Path(directory)
            try:
                archive = args.cache / (entry["id"] + "-" + Path(entry["url"]).name)
                if not entry.get("sha256") or digest(archive) != entry["sha256"]:
                    raise ValueError("Publisher SHA-256 mismatch")
                if host == "Darwin":
                    extract_tar(archive, work)
                    node = next(p for p in work.rglob("node") if p.is_file())
                    npm = node.parent.parent / "lib/node_modules/npm/bin/npm-cli.js"
                else:
                    with zipfile.ZipFile(archive) as source:
                        if not all(safe_member(m.filename) for m in source.infolist()):
                            raise ValueError("Unsafe archive path")
                        source.extractall(work)
                    node = next(work.rglob("node.exe"))
                    npm = node.parent / "node_modules/npm/bin/npm-cli.js"
                keep = ("HOME", "TMPDIR", "LANG", "SystemRoot", "WINDIR", "TEMP", "TMP", "USERPROFILE")
                env = {k: os.environ[k] for k in keep if k in os.environ}
                env["PATH"] = str(node.parent) + os.pathsep + os.defpath
                code = "JSON.stringify({version:process.version,platform:process.platform,arch:process.arch,icu:process.versions.icu,locales:Intl.DateTimeFormat.supportedLocalesOf(['en','ru','uz-Latn']),number:new Intl.NumberFormat('ru').format(1234.5),plural:new Intl.PluralRules('ru').select(2)})"
                data = json.loads(subprocess.check_output(prefix + [str(node), "-p", code], env=env, encoding="utf-8", timeout=30))
                if len(data["locales"]) != 3:
                    raise RuntimeError("One or more required Intl locales are unsupported")
                result.update(data)
                result["npm"] = subprocess.check_output(prefix + [str(node), str(npm), "--version"], env=env, encoding="utf-8", timeout=30).strip()
                result.update({"status": "passed", "publisher_sha256_matched": True})
            except Exception as exc:
                result.update({"status": "failed", "error": str(exc).replace(str(work), "<temporary-fixture>")})
        results.append(result)
        print(entry["id"], result["status"], result.get("version", result.get("error")), flush=True)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps({"recorded_at": datetime.now(timezone.utc).isoformat(),
                                      "host_os": platform.mac_ver()[0] if host == "Darwin" else platform.release(),
                                      "host_platform": host, "results": results}, indent=2) + "\n")
    return int(not results or any(item["status"] != "passed" for item in results))


if __name__ == "__main__":
    raise SystemExit(main())
