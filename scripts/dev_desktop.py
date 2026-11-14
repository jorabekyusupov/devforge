#!/usr/bin/env python3
"""Explicit debug manager preparation. No manager or GUI runs without --launch."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import stat
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
SUPPORTED = {"aarch64-apple-darwin", "x86_64-apple-darwin", "x86_64-pc-windows-msvc"}


def tool_prefix(root: Path, system: str) -> list[str]:
    return [] if system == "Windows" else [str(root / "scripts/with_rust.sh")]


def native_target(system: str, machine: str, environment: dict[str, str]) -> str:
    machine = machine.lower()
    if system == "Darwin":
        if machine in {"arm64", "aarch64"}:
            return "aarch64-apple-darwin"
        if machine in {"x86_64", "amd64"}:
            return "x86_64-apple-darwin"
    if system == "Windows" and machine in {"amd64", "x86_64"}:
        if environment.get("PROCESSOR_ARCHITEW6432", "").lower() not in {"", "amd64", "x86_64"}:
            raise ValueError("Windows emulated/cross architecture is unsupported")
        return "x86_64-pc-windows-msvc"
    raise ValueError("Unsupported native desktop platform")


def choose_target(rustc_output: str, system: str, machine: str, requested: str | None,
                  environment: dict[str, str]) -> str:
    hosts = [line.removeprefix("host: ").strip() for line in rustc_output.splitlines() if line.startswith("host: ")]
    expected = native_target(system, machine, environment)
    if len(hosts) != 1 or hosts[0] not in SUPPORTED or hosts[0] != expected:
        raise ValueError("rustc host does not match the supported native process target")
    if requested is not None and requested != hosts[0]:
        raise ValueError("Cross-target debug manager preparation is unsupported")
    if environment.get("CARGO_BUILD_TARGET", hosts[0]) != hosts[0]:
        raise ValueError("Ambient CARGO_BUILD_TARGET conflicts with the explicit native target")
    if environment.get("TAURI_CONFIG"):
        raise ValueError("Ambient TAURI_CONFIG makes debug layout ambiguous")
    return hosts[0]


def artifact_paths(root: Path, metadata: dict, target: str) -> tuple[Path, Path]:
    if target not in SUPPORTED or Path(metadata["workspace_root"]).resolve() != root.resolve():
        raise ValueError("Unexpected Cargo workspace or target")
    target_dir = Path(metadata["target_directory"])
    if not target_dir.is_absolute():
        raise ValueError("Cargo target directory must be absolute")
    packages = metadata["packages"]
    members = set(metadata["workspace_members"])
    for name, manifest in [("devforge-cli", root / "crates/cli/Cargo.toml"),
                           ("devforge-desktop", root / "apps/desktop/src-tauri/Cargo.toml")]:
        matches = [p for p in packages if p["name"] == name and p["id"] in members]
        if len(matches) != 1 or Path(matches[0]["manifest_path"]).resolve() != manifest.resolve():
            raise ValueError("Unexpected workspace executable package")
    suffix = ".exe" if target.endswith("windows-msvc") else ""
    adjacent = target_dir / target / "debug"
    return adjacent / ("devforge" + suffix), adjacent / ("devforge-desktop" + suffix)


def hash_executable(path: Path) -> str:
    before = path.lstat()
    if (not stat.S_ISREG(before.st_mode) or path.is_symlink()
            or getattr(before, "st_file_attributes", 0) & 0x400):
        raise ValueError("Debug manager must be the fixed regular adjacent executable")
    digest = hashlib.sha256()
    with path.open("rb") as source:
        opened = os.fstat(source.fileno())
        if (opened.st_dev, opened.st_ino) != (before.st_dev, before.st_ino):
            raise ValueError("Debug manager changed before hashing")
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
        after = os.fstat(source.fileno())
    current = path.lstat()
    fields = lambda value: (value.st_dev, value.st_ino, value.st_size, value.st_mtime_ns)
    if fields(before) != fields(after) or fields(after) != fields(current) or not stat.S_ISREG(current.st_mode):
        raise ValueError("Debug manager changed while hashing")
    return digest.hexdigest()


def check_generated_path(path: Path, directory: bool) -> None:
    try:
        metadata = path.lstat()
    except FileNotFoundError:
        return
    valid = stat.S_ISDIR(metadata.st_mode) if directory else stat.S_ISREG(metadata.st_mode)
    if not valid or stat.S_ISLNK(metadata.st_mode) or getattr(metadata, "st_file_attributes", 0) & 0x400:
        raise ValueError("Generated authority path must not be a link, reparse point or wrong file type")


def write_manifest(root: Path, target: str, executable: Path) -> tuple[Path, dict]:
    filename = "devforge.exe" if target.endswith("windows-msvc") else "devforge"
    if target not in SUPPORTED or executable.name != filename:
        raise ValueError("Invalid debug manager identity")
    value = {"schema": 1, "target": target, "filename": filename, "sha256": hash_executable(executable)}
    generated = root / "distribution/generated"
    for directory in [root / "distribution", generated]:
        check_generated_path(directory, directory=True)
    generated.mkdir(parents=True, exist_ok=True)
    manifest = generated / "manager-artifact.json"
    check_generated_path(manifest, directory=False)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=generated, delete=False) as output:
            temporary = Path(output.name)
            json.dump(value, output, indent=2)
            output.write("\n")
            output.flush()
            os.fsync(output.fileno())
        temporary.replace(manifest)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
    return manifest, value


def run(command: list[str], *, cwd: Path, env: dict, capture: bool = False):
    return subprocess.run(command, cwd=cwd, env=env, check=True,
                          capture_output=capture, text=True, encoding="utf-8")


def prepare(root: Path, *, requested: str | None = None, launch: bool = False,
            dry_run: bool = False, environment: dict | None = None,
            system: str | None = None, machine: str | None = None, runner=run) -> dict:
    environment = dict(os.environ if environment is None else environment)
    system, machine = system or platform.system(), machine or platform.machine()
    # Fail before invoking build tools on unsupported systems.
    native_target(system, machine, environment)
    prefix = tool_prefix(root, system)
    cwd = root / "apps/desktop/src-tauri"
    rustc = runner(prefix + ["rustc", "-vV"], cwd=cwd, env=environment, capture=True).stdout
    target = choose_target(rustc, system, machine, requested, environment)
    metadata = json.loads(runner(prefix + ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
                                 cwd=cwd, env=environment, capture=True).stdout)
    executable, gui = artifact_paths(root, metadata, target)
    build = prefix + ["cargo", "build", "--locked", "-p", "devforge-cli", "--bin", "devforge", "--target", target]
    tauri = root / "node_modules/@tauri-apps/cli/tauri.js"
    launch_command = prefix + ["node", str(tauri), "dev", "--target", target, "--", "--locked"]
    if launch and (tauri.is_symlink() or not tauri.is_file()):
        raise ValueError("Install the repository's locked Node dependencies before --launch")
    report = {"mode": "dry_run" if dry_run else "prepared", "target": target,
              "manager": str(executable), "adjacent_gui": str(gui),
              "manifest": str(root / "distribution/generated/manager-artifact.json"),
              "build_command": build, "launch_command": launch_command,
              "launch_build_gate": "DEVFORGE_PACKAGE_MANAGER=1", "launch_requested": launch}
    if dry_run:
        return report
    runner(build, cwd=cwd, env=environment)
    manifest, authority = write_manifest(root, target, executable)
    report["sha256"] = authority["sha256"]
    if launch:
        # The gate is explicitly a build input. GUI runtime uses only its compiled
        # fixed-adjacent descriptor; no CLI PATH/env selection is introduced.
        if hash_executable(executable) != authority["sha256"] or json.loads(manifest.read_text(encoding="utf-8")) != authority:
            raise ValueError("Debug manager authority changed before launch")
        launch_env = dict(environment, DEVFORGE_PACKAGE_MANAGER="1")
        runner(launch_command, cwd=root / "apps/desktop", env=launch_env)
    return report


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--prepare-only", action="store_true", help="Default: compile/hash the adjacent debug manager only")
    mode.add_argument("--launch", action="store_true", help="Explicitly start Tauri dev after preparing the manager")
    parser.add_argument("--target", help="Must equal the supported native rustc host target")
    parser.add_argument("--dry-run", action="store_true", help="Inspect metadata/commands only; no build, write or GUI")
    args = parser.parse_args()
    try:
        print(json.dumps(prepare(ROOT, requested=args.target, launch=args.launch, dry_run=args.dry_run), indent=2))
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        parser.exit(1, f"Debug preparation failed: {error}\n")


if __name__ == "__main__":
    main()
