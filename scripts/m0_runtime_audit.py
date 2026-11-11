#!/usr/bin/env python3
"""M0 diagnostics: download candidates, compare publisher hashes, inspect archives.

This is not a production package installer. It never executes downloaded binaries,
extracts archives, edits system configuration, or treats a computed hash as proof
that a publisher authenticated an artifact. Catalog updates require source review.
"""

from __future__ import annotations

import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timezone
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import platform
import re
import struct
import subprocess
import sys
import tarfile
import tempfile
from urllib.parse import unquote, urlsplit
import zipfile


def architectures(header):
    """Identify PE/Mach-O headers without running foreign executables."""
    if header[:2] == b"MZ" and len(header) >= 64:
        offset = struct.unpack_from("<I", header, 0x3C)[0]
        if offset + 6 <= len(header) and header[offset:offset + 4] == b"PE\0\0":
            machine = struct.unpack_from("<H", header, offset + 4)[0]
            return {0x8664: ["x64"], 0xAA64: ["arm64"], 0x14C: ["x86"]}.get(machine, ["unknown-pe"])
    endian = {b"\xcf\xfa\xed\xfe": "<", b"\xfe\xed\xfa\xcf": ">"}.get(header[:4])
    cpu_names = {0x01000007: "x64", 0x0100000C: "arm64"}
    if endian and len(header) >= 8:
        return [cpu_names.get(struct.unpack_from(endian + "I", header, 4)[0], "unknown-mach-o")]
    if header[:4] in (b"\xca\xfe\xba\xbe", b"\xca\xfe\xba\xbf") and len(header) >= 8:
        count = struct.unpack_from(">I", header, 4)[0]
        stride = 32 if header[:4] == b"\xca\xfe\xba\xbf" else 20
        return sorted({cpu_names.get(struct.unpack_from(">I", header, 8 + i * stride)[0], "unknown-mach-o")
                       for i in range(min(count, (len(header) - 8) // stride))})
    return []


def safe_member(name):
    path = PurePosixPath(name.replace("\\", "/"))
    return not path.is_absolute() and ".." not in path.parts and not re.match(r"^[A-Za-z]:", str(path))


def finish_explicit_range(partial, url):
    """Recover one interrupted transfer using validated, explicitly bounded ranges."""
    base = ["curl", "--http1.1", "--fail", "--silent", "--show-error", "--location", "--proto", "=https",
            "--connect-timeout", "15"]
    with tempfile.TemporaryDirectory(prefix="devforge-m0-range-") as directory:
        root = Path(directory)
        headers, probe, tail = root / "headers", root / "probe", root / "tail"
        response = subprocess.run(base + ["--max-time", "30", "--max-filesize", "1048576", "--range", "0-0",
                                         "--dump-header", str(headers), "--output", str(probe), url], capture_output=True, text=True)
        ranges = re.findall(r"(?im)^content-range:\s*bytes\s+(\d+)-(\d+)/(\d+)", headers.read_text() if headers.exists() else "")
        if response.returncode or not ranges or ranges[-1][:2] != ("0", "0") or probe.stat().st_size != 1:
            return False
        total, offset = int(ranges[-1][2]), partial.stat().st_size
        if offset == total:
            return True  # Caller still verifies the complete publisher hash.
        if offset > total:
            return False
        response = subprocess.run(base + ["--max-time", "300", "--max-filesize", str(total - offset),
                                         "--range", "{}-{}".format(offset, total - 1), "--dump-header", str(headers),
                                         "--output", str(tail), url], capture_output=True, text=True)
        ranges = re.findall(r"(?im)^content-range:\s*bytes\s+(\d+)-(\d+)/(\d+)", headers.read_text() if headers.exists() else "")
        expected = (str(offset), str(total - 1), str(total))
        if response.returncode or not ranges or ranges[-1] != expected or tail.stat().st_size != total - offset:
            return False
        with partial.open("ab") as destination, tail.open("rb") as source:
            for chunk in iter(lambda: source.read(1024 * 1024), b""):
                destination.write(chunk)
        return True


def inventory(path):
    selected = {"php", "php-fpm", "php.exe", "php-cgi.exe", "node", "node.exe",
                "caddy", "caddy.exe", "postgres", "postgres.exe", "initdb", "initdb.exe"}
    binaries, licenses, names, nested_archives = [], [], [], []

    def inspect(name, opener):
        names.append(name)
        if not safe_member(name):
            raise ValueError("Unsafe archive member path")
        basename = PurePosixPath(name).name.lower()
        if basename in selected:
            with opener() as stream:
                binaries.append({"member": name, "architectures": architectures(stream.read(8192))})
        if basename.startswith(("license", "copying", "copyright", "notice")):
            licenses.append(name)

    if zipfile.is_zipfile(path):
        with zipfile.ZipFile(path) as archive:
            for member in archive.infolist():
                if not member.is_dir():
                    inspect(member.filename, lambda m=member: archive.open(m))
                    if member.filename.endswith((".txz", ".tar.gz", ".tgz")) and member.file_size <= 128 * 1024 * 1024:
                        nested_archives.append(member.filename)
                        with archive.open(member) as stream:
                            payload = io.BytesIO(stream.read())
                        with tarfile.open(fileobj=payload) as nested:
                            for child in nested:
                                if not safe_member(child.name):
                                    raise ValueError("Unsafe nested archive member path")
                                if child.isfile():
                                    inspect(member.filename + "!/" + child.name, lambda m=child: nested.extractfile(m))
        kind = "zip"
    elif tarfile.is_tarfile(path):
        with tarfile.open(path) as archive:
            for member in archive:
                if member.isfile():
                    inspect(member.name, lambda m=member: archive.extractfile(m))
                elif not safe_member(member.name):
                    raise ValueError("Unsafe archive member path")
        kind = "tar"
    else:
        kind = "dmg" if path.name.endswith(".dmg") else "single-file"
    return {"archive_kind": kind, "file_count": len(names), "binaries": binaries,
            "nested_archives_inspected": nested_archives,
            "license_members": licenses[:30],
            "windows_pdo_pgsql_present": any(name.lower().endswith("php_pdo_pgsql.dll") for name in names)}


def audit(entry, cache):
    result = {**entry, "runtime_executed": False, "signature_verification": "not-performed"}
    try:
        parsed = urlsplit(entry["url"])
        if parsed.scheme != "https" or parsed.username or parsed.password:
            raise ValueError("Candidate URL must use HTTPS without credentials")
        if not re.fullmatch(r"[A-Za-z0-9._-]+", entry["id"]):
            raise ValueError("Unsafe candidate id")
        if entry.get("download_policy") == "metadata-only":
            response = subprocess.run(
                ["curl", "--fail", "--silent", "--show-error", "--head", "--location", "--proto", "=https",
                 "--connect-timeout", "10", "--max-time", "30", entry["url"]], capture_output=True, text=True,
            )
            if response.returncode:
                raise RuntimeError("Metadata request failed (curl exit {})".format(response.returncode))
            sizes = re.findall(r"(?im)^content-length:\s*(\d+)", response.stdout)
            statuses = re.findall(r"(?im)^HTTP/\S+\s+(\d+)", response.stdout)
            result.update({"status": "metadata-only", "integrity": "not-downloaded",
                           "http_status": int(statuses[-1]) if statuses else None,
                           "declared_bytes": int(sizes[-1]) if sizes else None})
            print(entry["id"] + ": metadata-only (no download or execution)", flush=True)
            return result
        name = PurePosixPath(unquote(parsed.path)).name
        if not name or not safe_member(name):
            raise ValueError("Unsafe archive filename")
        path = cache / (entry["id"] + "-" + name)
        expected = entry.get("sha256")
        if expected is not None and not re.fullmatch(r"[a-f0-9]{64}", expected):
            raise ValueError("Invalid expected SHA-256")
        cached_hash = digest(path) if path.is_file() else None
        if not path.is_file() or (expected and cached_hash != expected):
            partial = path.with_name(path.name + ".part")
            if not (expected and partial.is_file() and digest(partial) == expected):
                command = ["curl", "--http1.1", "--fail", "--silent", "--show-error", "--location", "--proto", "=https",
                           "--connect-timeout", "20", "--max-time", "1800", "--continue-at", "-",
                           "--output", str(partial), entry["url"]]
                downloaded = subprocess.run(command, capture_output=True, text=True)
                if downloaded.returncode == 33:
                    # Some publisher endpoints do not support Range requests.
                    partial.unlink(missing_ok=True)
                    downloaded = subprocess.run(command, capture_output=True, text=True)
                recovered = downloaded.returncode in (18, 92) and partial.is_file() and finish_explicit_range(partial, entry["url"])
                if recovered:
                    result["transport_recovery"] = "validated-explicit-end-range; complete publisher hash checked afterwards"
                if downloaded.returncode and not recovered:
                    raise RuntimeError("Download incomplete (curl exit {}): {}; task-owned partial retained".format(
                        downloaded.returncode, getattr(downloaded, "stderr", "").strip()[:200]))
            actual = digest(partial)
            if expected and actual != expected:
                result["computed_sha256"] = actual
                result["integrity"] = "publisher-sha256-mismatch"
                partial.unlink(missing_ok=True)
                raise ValueError("Publisher SHA-256 mismatch")
            partial.replace(path)
        actual = digest(path)
        inspected = inventory(path)
        expected_arch = "x64" if entry.get("target", "").endswith("-x64") else "arm64" if entry.get("target", "").endswith("-arm64") else None
        result.update({"cache_file": path.name, "bytes": path.stat().st_size, "computed_sha256": actual,
                       "integrity": "publisher-sha256-matched" if expected else "computed-only-unverified",
                       "inspection": inspected, "status": "inspected"})
        if expected_arch and inspected["binaries"]:
            result["target_header_match"] = all(expected_arch in item["architectures"] for item in inspected["binaries"])
            if not result["target_header_match"]:
                raise ValueError("Executable headers do not match catalog target")
    except (OSError, ValueError, RuntimeError, tarfile.TarError, zipfile.BadZipFile) as exc:
        result.update({"status": "failed", "error": str(exc)})
    print("{}: {} ({}){}".format(entry["id"], result["status"], result.get("integrity", "not-verified"),
                                 "; " + result["error"] if result.get("error") else ""), flush=True)
    return result


def digest(path):
    checksum = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            checksum.update(chunk)
    return checksum.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--catalog", required=True, type=Path)
    parser.add_argument("--cache", required=True, type=Path)
    parser.add_argument("--report", required=True, type=Path)
    parser.add_argument("--target", help="Inspect one catalog target plus related portable/universal helpers.")
    parser.add_argument("--jobs", type=int, default=3, choices=range(1, 5))
    args = parser.parse_args()
    try:
        entries = json.loads(args.catalog.read_text())
    except (OSError, ValueError) as exc:
        parser.error("Cannot read candidate catalog: {}".format(exc))
    if not isinstance(entries, list) or not all(isinstance(entry, dict) and all(isinstance(entry.get(key), str) and entry[key] for key in ("id", "url")) for entry in entries):
        parser.error("Catalog must be a list of entries with non-empty id and url strings")
    ids = [entry["id"] for entry in entries]
    if len(ids) != len(set(ids)):
        parser.error("Catalog contains duplicate ids")
    if args.target:
        if args.target not in {entry.get("target") for entry in entries}:
            parser.error("Requested target is not present in the catalog")
        universal = args.target.split("-", 1)[0] + "-universal"
        entries = [entry for entry in entries if entry.get("target") in (args.target, universal, "php-portable")]
    args.cache.mkdir(parents=True, exist_ok=True)
    report = {"recorded_at": datetime.now(timezone.utc).isoformat(),
              "execution_host": {"os": platform.system(), "architecture": platform.machine()},
              "scope": "Artifact integrity and static inspection, not foreign-OS runtime verification",
              "expected_artifacts": len(entries), "status": "running", "artifacts": []}
    args.report.parent.mkdir(parents=True, exist_ok=True)
    def save():
        temporary = args.report.with_name(args.report.name + ".tmp")
        temporary.write_text(json.dumps(report, indent=2) + "\n")
        temporary.replace(args.report)
    save()
    with ThreadPoolExecutor(max_workers=args.jobs) as pool:
        futures = [pool.submit(audit, entry, args.cache) for entry in entries]
        for future in as_completed(futures):
            report["artifacts"].append(future.result())
            save()
    results = report["artifacts"]
    report["status"] = "completed"
    save()
    return int(any(result["status"] == "failed" for result in results))


if __name__ == "__main__":
    sys.exit(main())
