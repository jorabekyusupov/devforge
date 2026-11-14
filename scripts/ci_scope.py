"""Conservative docs-only CI selection; uncertainty always keeps product checks."""
from __future__ import annotations

import json
import os
from pathlib import Path
import re
import subprocess
from typing import Callable

_SHA = re.compile(r"[0-9a-fA-F]{40}\Z")


def is_documentation(path: str) -> bool:
    if not path or any(ord(char) < 32 for char in path) or "\\" in path:
        return False
    parts = path.split("/")
    if any(part in ("", ".", "..") for part in parts):
        return False
    if path in {"README.md", "CONTRIBUTING.md"}:
        return True
    # Examples, JSON evidence, binary assets and tooling stay product scope.
    return path.startswith("docs/") and (
        path.endswith(".md")
        or (path.startswith("docs/guides/") and path.endswith(".html"))
    )


def needs_product_checks(paths: list[str]) -> bool:
    # Empty or unavailable comparisons are not sufficient evidence to skip.
    return not paths or not all(is_documentation(path) for path in paths)


def comparison_base(event_name: str, event: dict, *, validated_base: str | None = None,
                    baseline_required: bool = False) -> str | None:
    if event_name not in {"push", "pull_request"}:
        return None
    if baseline_required:
        return validated_base if isinstance(validated_base, str) and _SHA.fullmatch(validated_base) and set(validated_base) != {"0"} else None
    if event_name == "push":
        base = event.get("before")
    elif event_name == "pull_request":
        base = event.get("pull_request", {}).get("base", {}).get("sha")
    else:
        return None
    if not isinstance(base, str) or not _SHA.fullmatch(base) or set(base) == {"0"}:
        return None
    return base


def select_product_checks(
    event_name: str, event: dict, diff: Callable[[str], list[str]], *,
    validated_base: str | None = None, baseline_required: bool = False,
) -> bool:
    try:
        base = comparison_base(event_name, event, validated_base=validated_base,
                               baseline_required=baseline_required)
        return True if base is None else needs_product_checks(diff(base))
    except (OSError, ValueError, TypeError, AttributeError, subprocess.SubprocessError):
        return True


def changed_paths(base: str) -> list[str]:
    # --no-renames includes both old and new paths so moving code into docs
    # cannot hide the deleted product source. No shell or event text execution.
    output = subprocess.run(
        ["git", "diff", "--name-only", "-z", "--no-renames", base, "HEAD", "--"],
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    ).stdout
    return [part.decode("utf-8", errors="surrogateescape") for part in output.split(b"\0") if part]


def main() -> None:
    product = True
    try:
        event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text(encoding="utf-8"))
        from ci_baseline import validated_base_from_environment
        product = select_product_checks(os.environ.get("GITHUB_EVENT_NAME", ""), event, changed_paths,
                                        validated_base=validated_base_from_environment(), baseline_required=True)
    except (OSError, ValueError, KeyError):
        pass
    result = f"product={'true' if product else 'false'}\n"
    output_path = os.environ.get("GITHUB_OUTPUT")
    if output_path:
        with open(output_path, "a", encoding="utf-8") as output:
            output.write(result)
    print(result.strip())


if __name__ == "__main__":
    main()
