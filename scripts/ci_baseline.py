"""Use an ancestor validated by both native CI matrix Rust-test steps, or full CI.

Only GitHub Actions metadata is read. A docs-only successful workflow is not a
product baseline. Failure or incomplete evidence never falls back to push.before.
"""
from __future__ import annotations

import json
from http.client import HTTPException
import os
from pathlib import Path
import re
import subprocess
import time
from urllib.parse import quote, urlencode
from urllib.request import HTTPRedirectHandler, Request, build_opener

SHA = re.compile(r"[0-9a-fA-F]{40}\Z")
PART = re.compile(r"[A-Za-z0-9_.-]+\Z")
WORKFLOW = ".github/workflows/check.yml"
MATRIX = {"check (macos-latest)", "check (windows-latest)"}
RUST_STEP = "Rust test for selected scope"
RUN_PAGES, RUN_PAGE_SIZE, JOB_PAGES, JOB_PAGE_SIZE, CANDIDATES = 3, 30, 2, 100, 10


class Unavailable(Exception):
    """No bounded, complete validation evidence; caller must select full CI."""


def valid_sha(value) -> bool:
    return isinstance(value, str) and bool(SHA.fullmatch(value)) and set(value) != {"0"}


def repo_route(repository: str) -> str:
    parts = repository.split("/")
    if len(parts) != 2 or not all(PART.fullmatch(part) and part not in {".", ".."} for part in parts):
        raise Unavailable("repository")
    return "/repos/" + "/".join(quote(part, safe="") for part in parts)


def branch_for(event_name: str, event: dict) -> str | None:
    if event_name == "push":
        ref = event.get("ref")
        branch = ref[len("refs/heads/"):] if isinstance(ref, str) and ref.startswith("refs/heads/") else None
    elif event_name == "pull_request":
        branch = event.get("pull_request", {}).get("base", {}).get("ref")
    else:
        return None
    if not isinstance(branch, str) or not branch or len(branch) > 1024 or any(ord(c) < 32 for c in branch):
        return None
    return branch


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        # Never forward a credential to a redirected destination.
        return None


class GitHub:
    def __init__(self, token: str):
        if not token:
            raise Unavailable("token")
        self.token = token
        self.deadline = time.monotonic() + 30
        self.opener = build_opener(NoRedirect())

    def get(self, route: str) -> dict:
        if not route.startswith("/repos/") or time.monotonic() >= self.deadline:
            raise Unavailable("api_budget")
        request = Request("https://api.github.com" + route, headers={
            "Accept": "application/vnd.github+json", "Authorization": "Bearer " + self.token,
            "X-GitHub-Api-Version": "2026-03-10", "User-Agent": "DevForge-CI-Baseline",
        })
        with self.opener.open(request, timeout=10) as response:
            # Metadata only, with a bound before JSON decoding.
            raw = response.read(8 * 1024 * 1024 + 1)
        if len(raw) > 8 * 1024 * 1024:
            raise Unavailable("api_size")
        value = json.loads(raw)
        if not isinstance(value, dict):
            raise Unavailable("api_shape")
        return value


def git_ancestor(sha: str) -> bool:
    if not valid_sha(sha):
        raise Unavailable("sha")
    try:
        result = subprocess.run(["git", "merge-base", "--is-ancestor", sha, "HEAD"],
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                check=False, timeout=10)
    except (OSError, subprocess.SubprocessError) as error:
        raise Unavailable("history") from error
    if result.returncode not in {0, 1}:
        raise Unavailable("history")
    return result.returncode == 0


def validated_jobs(api, prefix: str, run: dict) -> bool:
    jobs = []
    for page in range(1, JOB_PAGES + 1):
        result = api.get(prefix + f"/actions/runs/{run['id']}/jobs?" + urlencode({
            "filter": "latest", "per_page": JOB_PAGE_SIZE, "page": page,
        }))
        batch, count = result.get("jobs"), result.get("total_count")
        if (not isinstance(batch, list) or type(count) is not int or count < len(batch)
                or len(batch) > JOB_PAGE_SIZE):
            raise Unavailable("jobs_shape")
        jobs.extend(batch)
        if len(jobs) >= count:
            break
    else:
        raise Unavailable("jobs_pages")
    matched = {}
    for job in jobs:
        name = job.get("name")
        if name not in MATRIX:
            continue
        if name in matched or job.get("run_id") != run["id"] or job.get("head_sha") != run["head_sha"]:
            raise Unavailable("jobs_identity")
        steps = [step for step in job.get("steps", []) if step.get("name") == RUST_STEP]
        matched[name] = (job.get("status") == "completed" and job.get("conclusion") == "success"
                         and len(steps) == 1 and steps[0].get("status") == "completed"
                         and steps[0].get("conclusion") == "success")
    return set(matched) == MATRIX and all(matched.values())


def find_baseline(event_name: str, event: dict, repository: str, api, ancestor=git_ancestor) -> str | None:
    """Return a validated ancestor, or None. All uncertainty selects full checks."""
    try:
        branch = branch_for(event_name, event)
        if branch is None:
            return None
        prefix = repo_route(repository)
        examined = 0
        for page in range(1, RUN_PAGES + 1):
            result = api.get(prefix + "/actions/workflows/check.yml/runs?" + urlencode({
                "branch": branch, "event": "push", "status": "success",
                "per_page": RUN_PAGE_SIZE, "page": page,
            }))
            runs = result.get("workflow_runs")
            if not isinstance(runs, list) or len(runs) > RUN_PAGE_SIZE:
                raise Unavailable("runs_shape")
            for run in runs:
                if (run.get("path") != WORKFLOW or run.get("event") != "push"
                        or run.get("head_branch") != branch or run.get("status") != "completed"
                        or run.get("conclusion") != "success"
                        or run.get("repository", {}).get("full_name", "").lower() != repository.lower()
                        or type(run.get("id")) is not int or run["id"] <= 0
                        or not valid_sha(run.get("head_sha"))):
                    raise Unavailable("run_identity")
                if not ancestor(run["head_sha"]):
                    continue
                examined += 1
                if examined > CANDIDATES:
                    raise Unavailable("candidate_budget")
                if validated_jobs(api, prefix, run):
                    return run["head_sha"]
            if len(runs) < RUN_PAGE_SIZE:
                return None
        return None
    except (OSError, HTTPException, ValueError, TypeError, KeyError, AttributeError, Unavailable):
        return None


def validated_base_from_environment() -> str | None:
    # The matrix consumes only the producer's explicit, validated output. A failed
    # or absent producer may not silently downgrade to an immediate parent diff.
    value = os.environ.get("DEVFORGE_CI_VALIDATED_BASE")
    if os.environ.get("DEVFORGE_CI_BASELINE_READY") != "true" or not valid_sha(value):
        return None
    try:
        return value if git_ancestor(value) else None
    except (OSError, Unavailable):
        return None


def main() -> None:
    baseline = None
    try:
        token = os.environ.get("GITHUB_TOKEN", "")
        if token:
            event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text(encoding="utf-8"))
            baseline = find_baseline(os.environ.get("GITHUB_EVENT_NAME", ""), event,
                                     os.environ.get("GITHUB_REPOSITORY", ""), GitHub(token))
    except (OSError, HTTPException, ValueError, KeyError, Unavailable):
        pass
    outputs = f"sha={baseline or ''}\navailable={'true' if baseline else 'false'}\n"
    if os.environ.get("GITHUB_OUTPUT"):
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
            output.write(outputs)
    # Fixed status and public commit metadata only; never log request/token/error repr.
    print(json.dumps({"baseline": baseline, "selection": "validated_ancestor" if baseline else "full_fallback"}))


if __name__ == "__main__":
    main()
