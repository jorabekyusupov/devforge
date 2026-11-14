"""Select Rust workspace consumers conservatively; execute Cargo using argv arrays."""
from __future__ import annotations

import json
import os
from pathlib import Path
import re
import subprocess
import sys

from ci_scope import changed_paths, comparison_base, is_documentation

_NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9_-]*\Z")


def full_scope() -> dict:
    return {"mode": "full", "packages": [], "frontend": True}


def affected_scope(paths: list[str], metadata: dict, root: Path) -> dict:
    if not paths:
        return full_scope()
    root = root.resolve()
    members = set(metadata["workspace_members"])
    packages = [package for package in metadata["packages"] if package["id"] in members]
    if not packages or len(packages) != len(members):
        return full_scope()
    by_path, reverse = {}, {}
    desktop = None
    for package in packages:
        name = package["name"]
        if not isinstance(name, str) or not _NAME.fullmatch(name) or name in reverse:
            return full_scope()
        directory = Path(package["manifest_path"]).parent.resolve()
        relative = directory.relative_to(root).as_posix()
        if relative == "." or relative in by_path:
            return full_scope()
        by_path[relative] = name
        reverse[name] = set()
        if relative == "apps/desktop/src-tauri":
            desktop = name
    if desktop is None:
        return full_scope()
    absolute = {(root / path).resolve(): name for path, name in by_path.items()}
    for package in packages:
        for dependency in package["dependencies"]:
            # cargo metadata --no-deps retains every dev/build/target dependency.
            # Never filter these edges by the current runner's cfg.
            path = dependency.get("path")
            if path:
                source = absolute.get(Path(path).resolve())
                if source:
                    reverse[source].add(package["name"])
                else:
                    # Unknown local dependency means the graph is incomplete.
                    return full_scope()
    # Runtime-only edge: desktop launches packaged CLI/terminal helpers even
    # where no Rust dependency records that protocol/distribution coupling.
    cli = by_path.get("crates/cli")
    if cli:
        reverse[cli].add(desktop)
    selected = set()
    for path in paths:
        if is_documentation(path):
            continue
        if (not isinstance(path, str) or "\\" in path or not path.endswith(".rs")
                or any(ord(char) < 32 for char in path)
                or any(part in ("", ".", "..") for part in path.split("/"))):
            return full_scope()
        owners = [name for directory, name in by_path.items() if path.startswith(directory + "/")]
        if len(owners) != 1:
            return full_scope()
        selected.add(owners[0])
    if not selected:
        return {"mode": "none", "packages": [], "frontend": False}
    pending = list(selected)
    while pending:
        for consumer in reverse[pending.pop()]:
            if consumer not in selected:
                selected.add(consumer)
                pending.append(consumer)
    return {"mode": "affected", "packages": sorted(selected), "frontend": desktop in selected}


def select_scope(event_name: str, event: dict, diff, metadata, root: Path, *,
                 validated_base: str | None = None, baseline_required: bool = False) -> dict:
    try:
        base = comparison_base(event_name, event, validated_base=validated_base,
                               baseline_required=baseline_required)
        if base is None:
            return full_scope()
        return affected_scope(diff(base), metadata(), root)
    except (OSError, ValueError, TypeError, KeyError, AttributeError, subprocess.SubprocessError):
        return full_scope()


def valid_scope(scope: dict) -> bool:
    if not isinstance(scope, dict) or set(scope) != {"mode", "packages", "frontend"}:
        return False
    names = scope["packages"]
    if not isinstance(names, list) or not isinstance(scope["frontend"], bool):
        return False
    if scope["mode"] == "none":
        return names == [] and scope["frontend"] is False
    if scope["mode"] == "full":
        return names == [] and scope["frontend"] is True
    return scope["mode"] == "affected" and bool(names) and all(
        isinstance(name, str) and _NAME.fullmatch(name) for name in names
    )


def cargo_command(scope: dict, action: str) -> list[str] | None:
    if action not in {"test", "clippy", "fmt"}:
        raise ValueError("unknown action")
    if not valid_scope(scope):
        scope = full_scope()
    if scope.get("mode") == "none":
        return None
    names = scope.get("packages")
    affected = scope.get("mode") == "affected" and isinstance(names, list) and bool(names)
    affected = affected and all(isinstance(name, str) and _NAME.fullmatch(name) for name in names)
    command = ["cargo", action]
    if affected:
        for name in sorted(set(names)):
            command.extend(["-p", name])
    else:
        command.append("--all" if action == "fmt" else "--workspace")
    if action == "fmt":
        command.extend(["--", "--check"])
    if action == "clippy":
        command.extend(["--all-targets", "--", "-D", "warnings"])
    return command


def workspace_metadata() -> dict:
    output = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"],
        check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    ).stdout
    return json.loads(output)


def main() -> None:
    action = sys.argv[1] if len(sys.argv) == 2 else ""
    scope_file = Path(os.environ["DEVFORGE_CI_RUST_SCOPE"])
    if action == "plan":
        scope = full_scope()
        try:
            event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text(encoding="utf-8"))
            from ci_baseline import validated_base_from_environment
            scope = select_scope(os.environ.get("GITHUB_EVENT_NAME", ""), event,
                                 changed_paths, workspace_metadata, Path.cwd(),
                                 validated_base=validated_base_from_environment(), baseline_required=True)
        except (OSError, ValueError, KeyError):
            pass
        scope_file.write_text(json.dumps(scope), encoding="utf-8")
        print(json.dumps(scope, sort_keys=True))
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
            output.write(f"frontend={'true' if scope['frontend'] else 'false'}\n")
            output.write(f"mode={scope['mode']}\n")
    else:
        scope = full_scope()
        try:
            loaded = json.loads(scope_file.read_text(encoding="utf-8"))
            if isinstance(loaded, dict):
                scope = loaded
        except (OSError, ValueError):
            pass
        command = cargo_command(scope, action)
        if command:
            print(json.dumps(command), flush=True)
            subprocess.run(command, check=True)


if __name__ == "__main__":
    main()
