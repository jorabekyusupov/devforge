"""Select synthetic debugger integration tests from the validated CI base."""
from __future__ import annotations

import json
import os
from pathlib import Path
import subprocess

from ci_baseline import validated_base_from_environment
from ci_scope import changed_paths, comparison_base


def relevant(paths: list[str]) -> bool:
    prefixes = ("crates/debugger/", "crates/workspace/src/debugging", "crates/workspace/tests/debug_facade")
    shared = {
        "Cargo.lock", "Cargo.toml", "rust-toolchain.toml",
        "crates/manager/Cargo.toml", "crates/manager/src/actor.rs",
        "crates/manager/src/controller.rs", "crates/manager/src/diagnostics.rs",
        "crates/manager/src/service_control.rs", "crates/manager/src/external_auth.rs",
        "crates/manager/tests/service_control.rs", "crates/control/src/lib.rs",
        "crates/control/src/mutations.rs", "crates/control/src/tool-inventory.json",
        "crates/control-adapter/src/lib.rs", ".github/workflows/check.yml",
        "scripts/a09_ci_scope.py",
    }
    return any(path in shared or path.startswith(prefixes) for path in paths)


def main() -> None:
    enabled = True  # Missing or untrusted history must not hide affected tests.
    try:
        event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text(encoding="utf-8"))
        base = comparison_base(os.environ.get("GITHUB_EVENT_NAME", ""), event,
                               validated_base=validated_base_from_environment(), baseline_required=True)
        if base is not None:
            enabled = relevant(changed_paths(base))
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
        pass
    value = "true" if enabled else "false"
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        output.write(f"enabled={value}\n")
    print(f"debugger_synthetic={value}")


if __name__ == "__main__":
    main()
