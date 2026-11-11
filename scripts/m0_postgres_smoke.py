#!/usr/bin/env python3
"""Run a checksummed PostgreSQL test bundle in an isolated single-user cluster.

No Java, installer, TCP listener, existing database, or system configuration is used.
This proves local initdb/SQL viability, not client/server integration or Windows use.
"""

from datetime import datetime, timezone
import argparse
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile
import zipfile

from m0_runtime_audit import digest
from m0_macos_smoke import extract_tar


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--catalog", type=Path, required=True)
    parser.add_argument("--cache", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()
    if platform.system() != "Darwin":
        parser.error("macOS probe; Windows needs a Windows execution environment")
    entries = [e for e in json.loads(args.catalog.read_text()) if e["package"] == "postgresql"
               and e["target"].startswith("macos-") and "Zonky" in e.get("provider", "")]
    results = []
    for entry in entries:
        result = {"artifact_id": entry["id"], "target": entry["target"],
                  "mode": "rosetta" if entry["target"] == "macos-x64" and platform.machine() == "arm64" else "native"}
        with tempfile.TemporaryDirectory(prefix="devforge-m0-postgres-") as directory:
            work = Path(directory)
            try:
                archive = args.cache / (entry["id"] + "-" + Path(entry["url"]).name)
                if not entry.get("sha256") or digest(archive) != entry["sha256"]:
                    raise ValueError("Publisher SHA-256 did not match")
                payload = work / "postgres.txz"
                with zipfile.ZipFile(archive) as source:
                    member = next(m for m in source.infolist() if m.filename.endswith(".txz"))
                    with source.open(member) as stream, payload.open("wb") as output:
                        shutil.copyfileobj(stream, output)
                extract_tar(payload, work / "relocated PostgreSQL Русский")
                postgres = next(p for p in (work / "relocated PostgreSQL Русский").rglob("postgres") if p.is_file())
                prefix = ["/usr/bin/arch", "-x86_64"] if entry["target"] == "macos-x64" else []
                env = {k: os.environ[k] for k in ("HOME", "TMPDIR", "LANG") if k in os.environ}
                env["PATH"] = "/usr/bin:/bin:/usr/sbin:/sbin"
                result["version"] = subprocess.check_output(prefix + [str(postgres), "--version"], env=env, text=True).strip()
                result["dependencies"] = subprocess.check_output(["/usr/bin/otool", "-L", str(postgres)], text=True).replace(str(work), "<temporary-fixture>")
                initialized = subprocess.run(prefix + [str(postgres.parent / "initdb"), "-D", str(work / "data"),
                                                       "--encoding=UTF8", "--no-locale", "--username=devforge_m0", "--auth=trust"],
                                             env=env, capture_output=True, text=True, timeout=90)
                if initialized.returncode:
                    raise RuntimeError(initialized.stdout + initialized.stderr)
                query = subprocess.run(prefix + [str(postgres), "--single", "-D", str(work / "data"), "template1"],
                                       input="SELECT version(), 'devforge_m0_probe'::text, 'Русский O‘zbekcha English'::text;\n",
                                       env=env, capture_output=True, text=True, timeout=30)
                if query.returncode or "devforge_m0_probe" not in query.stdout:
                    raise RuntimeError(query.stdout + query.stderr)
                result.update({"status": "passed", "publisher_sha256_matched": True, "initdb": True,
                               "single_user_sql": True, "unicode_query": "Русский O‘zbekcha English" in query.stdout,
                               "tcp_integration": "not-tested", "system_configuration_changed": False})
            except Exception as exc:
                result.update({"status": "failed", "error": str(exc).replace(str(work), "<temporary-fixture>")})
        results.append(result)
        print(entry["id"], result["status"], result.get("version", result.get("error")), flush=True)
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps({"recorded_at": datetime.now(timezone.utc).isoformat(),
                                      "host_os": platform.mac_ver()[0], "results": results}, indent=2) + "\n")
    return int(not results or any(item["status"] != "passed" for item in results))


if __name__ == "__main__":
    raise SystemExit(main())
