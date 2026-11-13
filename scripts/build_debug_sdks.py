#!/usr/bin/env python3
"""CI wrapper for the same private source builder embedded by the application."""
import argparse, json, pathlib, subprocess, sys, zipfile, hashlib

p = argparse.ArgumentParser()
p.add_argument("target")
p.add_argument("output", type=pathlib.Path)
a = p.parse_args()
a.output.mkdir(parents=True, exist_ok=True)
root = pathlib.Path(__file__).resolve().parent.parent
for version in ["8.4.27", "8.5.12"]:
    archive = a.output / f"debug-{version}.zip"
    if not archive.exists():
        build_archive = a.output / "build outputs" / archive.name
        try:
            subprocess.run(
                [
                    sys.executable,
                    str(root / "crates/debugger/src/provider/build_runtime.py"),
                    str(root / "crates/debugger/src/provider/recipe.json"),
                    version,
                    a.target,
                    str(build_archive),
                ],
                check=True,
            )
        except subprocess.CalledProcessError:
            log = build_archive.with_suffix(".build.log")
            if log.exists():
                print(log.read_text(errors="replace")[-12000:], flush=True)
            raise
        build_archive.replace(archive)
    with zipfile.ZipFile(archive) as z:
        provenance = json.loads(z.read("provenance.json"))
    assert provenance["version"] == version and provenance["target"] == a.target
    provenance["archive_sha256"] = hashlib.sha256(archive.read_bytes()).hexdigest()
    (a.output / f"debug-{version}-provenance.json").write_text(
        json.dumps(provenance, indent=2)
    )
    print("M7_SDK:" + json.dumps(provenance), flush=True)
