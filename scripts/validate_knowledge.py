#!/usr/bin/env python3
"""Read-only OKF 0.2 structure validation with optional DevForge quality gates.

Install scripts/requirements-knowledge.txt into an isolated environment first.
Errors fail validation; --strict also fails on project-quality warnings.
No network requests, Markdown rendering, code execution, or knowledge edits occur.
"""

from __future__ import annotations

import argparse
from dataclasses import asdict, dataclass, field
from datetime import date, datetime, timezone
import json
from pathlib import Path
import re
import sys
from typing import Any, Optional
from urllib.parse import unquote, urlsplit

try:
    import yaml
    from markdown_it import MarkdownIt
except ImportError as exc:
    raise SystemExit(
        "Missing validator dependency: {}. Install scripts/requirements-knowledge.txt "
        "in a virtual environment and run with that environment's Python.".format(exc.name)
    ) from exc


class UniqueSafeLoader(yaml.SafeLoader):
    """Reject ambiguous explicit keys while keeping normal YAML merge overrides."""

    def construct_mapping(self, node, deep=False):
        seen = set()
        for key_node, _ in node.value:
            if key_node.tag == "tag:yaml.org,2002:merge":
                continue
            key = self.construct_object(key_node, deep=deep)
            try:
                duplicate = key in seen
                seen.add(key)
            except TypeError:
                return super().construct_mapping(node, deep=deep)
            if duplicate:
                raise yaml.constructor.ConstructorError(
                    "while reading metadata", node.start_mark,
                    "duplicate key {!r}".format(key), key_node.start_mark,
                )
        return super().construct_mapping(node, deep=deep)


@dataclass
class Issue:
    level: str
    path: str
    line: int
    code: str
    message: str


@dataclass
class Document:
    path: Path
    metadata: dict[str, Any]
    body: str
    body_line: int
    tokens: list[Any] = field(default_factory=list)
    links: list[tuple[str, int]] = field(default_factory=list)


class Validator:
    def __init__(self, bundle: Path, repo: Path, now: Optional[datetime] = None):
        self.bundle = bundle.resolve()
        self.repo = repo.resolve()
        self.now = now or datetime.now(timezone.utc)
        self.issues: list[Issue] = []
        self.documents: dict[Path, Document] = {}
        self.markdown = MarkdownIt("commonmark").enable("table")

    def issue(self, path, code, message, line=1, level="warning"):
        try:
            label = path.relative_to(self.repo).as_posix()
        except ValueError:
            label = str(path)
        self.issues.append(Issue(level, label, line, code, message))

    @staticmethod
    def nonempty(value):
        return isinstance(value, str) and bool(value.strip())

    def read(self, path):
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeError) as exc:
            self.issue(path, "DOCUMENT_READ", str(exc), level="error")
            return None
        lines = text.splitlines(keepends=True)
        present = bool(lines and lines[0].strip("\r\n") == "---")
        metadata = {}
        end = -1
        if present:
            end = next((i for i in range(1, len(lines)) if lines[i].strip("\r\n") == "---"), -1)
            if end == -1:
                self.issue(path, "FRONTMATTER_UNCLOSED", "Missing closing ---.", level="error")
                return None
            try:
                metadata = yaml.load("".join(lines[1:end]), Loader=UniqueSafeLoader)
            except yaml.YAMLError as exc:
                mark = getattr(exc, "problem_mark", None)
                line = mark.line + 2 if mark is not None else 1
                self.issue(path, "YAML_INVALID", getattr(exc, "problem", None) or "Invalid YAML.", line, "error")
                return None
            if not isinstance(metadata, dict):
                self.issue(path, "METADATA_MAPPING", "Frontmatter must be a YAML mapping.", level="error")
                return None
        if path.name not in ("index.md", "log.md"):
            if not present:
                self.issue(path, "FRONTMATTER_REQUIRED", "Concept needs YAML frontmatter.", level="error")
            elif not self.nonempty(metadata.get("type")):
                self.issue(path, "TYPE_REQUIRED", "Concept needs a non-empty string type.", level="error")
        elif present and (path != self.bundle / "index.md" or set(metadata) - {"okf_version"}):
            self.issue(path, "RESERVED_FRONTMATTER", "Only the bundle-root index may carry okf_version frontmatter.", level="error")
        body = "".join(lines[end + 1:]) if present else text
        doc = Document(path, metadata, body, end + 2 if present else 1)
        self.parse_markdown(doc)
        return doc

    def parse_markdown(self, doc):
        # Remove code blocks before recognizing footnotes, so examples are not claims.
        lines = doc.body.splitlines(keepends=True)
        for token in self.markdown.parse(doc.body):
            if token.type in ("fence", "code_block", "html_block") and token.map:
                for index in range(*token.map):
                    lines[index] = "\n"
        prose = "".join(lines)
        definitions = re.findall(r"^ {0,3}\[\^([^\]\n]+)\]:", prose, re.M)
        # Footnotes are handled locally, not as CommonMark reference-link definitions.
        prose = re.sub(r"^ {0,3}\[\^([^\]\n]+)\]:[ \t]*", "", prose, flags=re.M)
        doc.tokens = self.markdown.parse(prose)
        used = set()
        for token in doc.tokens:
            line = doc.body_line + (token.map[0] if token.map else 0)
            for child in token.children or []:
                if child.type in ("link_open", "image"):
                    url = child.attrGet("href" if child.type == "link_open" else "src")
                    if url:
                        doc.links.append((url, line))
                elif child.type == "text":
                    used.update(re.findall(r"(?<!\\)\[\^([^\]\n]+)\]", child.content))
        ids = set()
        source_list = doc.metadata.get("sources", [])
        if isinstance(source_list, list):
            ids = {s["id"] for s in source_list if isinstance(s, dict) and self.nonempty(s.get("id"))}
        for label in sorted(used - set(definitions)):
            self.issue(doc.path, "FOOTNOTE_UNDEFINED", "Undefined footnote: {}.".format(label))
        for label in sorted(set(definitions) - ids):
            self.issue(doc.path, "FOOTNOTE_SOURCE", "Footnote has no matching sources[].id: {}.".format(label))
        if len(definitions) != len(set(definitions)):
            self.issue(doc.path, "FOOTNOTE_DUPLICATE", "Duplicate footnote definitions.")

    def local_path(self, doc, value):
        parsed = urlsplit(value)
        if parsed.scheme or parsed.netloc:
            return None
        raw = unquote(parsed.path)
        if not raw:
            return None  # Fragment-only links are not file references.
        return ((self.bundle / raw.lstrip("/")) if raw.startswith("/") else (doc.path.parent / raw)).resolve()

    def check_path(self, doc, value, line=1):
        try:
            target = self.local_path(doc, value)
        except ValueError:
            self.issue(doc.path, "LINK_INVALID", "Malformed path/URI.", line)
            return None
        if target is None:
            return None
        if not any(target.is_relative_to(root) for root in (self.repo, self.bundle)):
            self.issue(doc.path, "PATH_OUTSIDE_ROOT", "Local reference escapes repository/bundle: {}.".format(value), line)
            return None
        if target.is_dir():
            target = target / "index.md"
        if not target.is_file():
            self.issue(doc.path, "LINK_MISSING", "Local target does not exist: {}.".format(value), line)
        return target

    def timestamp(self, doc, value, name):
        try:
            parsed = value if isinstance(value, datetime) else datetime.fromisoformat(str(value).replace("Z", "+00:00"))
            if parsed.tzinfo is None or parsed.utcoffset() is None:
                raise ValueError("Timezone required")
            return parsed
        except (TypeError, ValueError):
            self.issue(doc.path, "TIMESTAMP_INVALID", "{} needs an ISO datetime with timezone.".format(name))
            return None

    def actor(self, doc, value, name):
        if not isinstance(value, str) or not re.fullmatch(r"(?:human:[^\s]+|process:[^\s]+|[^\s/]+/[^\s/]+)", value):
            self.issue(doc.path, "ACTOR_INVALID", "{} needs human:id, process:id, or producer/version.".format(name))

    def metadata(self, doc):
        meta = doc.metadata
        if "status" in meta and meta["status"] not in ("draft", "stable", "deprecated"):
            self.issue(doc.path, "STATUS_INVALID", "Unknown document lifecycle status.")
        if "decision_status" in meta and meta["decision_status"] not in ("confirmed", "working-direction", "proposed"):
            self.issue(doc.path, "DECISION_STATUS_INVALID", "Unknown DevForge decision_status.")
        if "tags" in meta and (not isinstance(meta["tags"], list) or not all(self.nonempty(t) for t in meta["tags"])):
            self.issue(doc.path, "TAGS_INVALID", "tags should be a list of non-empty strings.")
        if "generated" in meta:
            generated = meta["generated"]
            if not isinstance(generated, dict):
                self.issue(doc.path, "GENERATED_INVALID", "generated should be a mapping.")
            else:
                self.actor(doc, generated.get("by"), "generated.by")
                if "at" in generated:
                    self.timestamp(doc, generated["at"], "generated.at")
        if "verified" in meta:
            verified = meta["verified"]
            entries = [verified] if isinstance(verified, dict) else verified
            if not isinstance(entries, list) or not entries:
                self.issue(doc.path, "VERIFIED_INVALID", "verified should be a mapping or non-empty list.")
            else:
                for entry in entries:
                    if not isinstance(entry, dict):
                        self.issue(doc.path, "VERIFIED_INVALID", "Each verification needs by and at.")
                        continue
                    self.actor(doc, entry.get("by"), "verified.by")
                    self.timestamp(doc, entry.get("at"), "verified.at")
        if "stale_after" in meta:
            stale = self.timestamp(doc, meta["stale_after"], "stale_after")
            if stale is not None and self.now >= stale:
                self.issue(doc.path, "DOCUMENT_STALE", "Document has reached stale_after; reassess its claims.")
        if "sources" in meta:
            sources = meta["sources"]
            if not isinstance(sources, list):
                self.issue(doc.path, "SOURCES_INVALID", "sources should be a list.")
                return
            seen = set()
            for source in sources:
                if not isinstance(source, dict) or not self.nonempty(source.get("resource")):
                    self.issue(doc.path, "SOURCE_RESOURCE", "Each source needs a non-empty resource.")
                    continue
                if "id" in source:
                    key = source["id"]
                    if not self.nonempty(key) or key in seen:
                        self.issue(doc.path, "SOURCE_ID", "Source IDs, when present, must be unique non-empty strings.")
                    else:
                        seen.add(key)
                resource = source["resource"]
                # OKF also permits non-retrievable population/scope descriptions.
                looks_like_file = re.search(r"\.(?:md|json|ya?ml|rs|py|tsx?|sql|toml|sh|txt|csv)(?:#.*)?$", resource, re.I)
                if resource.startswith(("/", "./", "../")) or looks_like_file:
                    self.check_path(doc, resource)
                if "last_modified" in source:
                    self.timestamp(doc, source["last_modified"], "sources[].last_modified")

    def reserved(self, doc):
        if doc.path.name == "index.md":
            version = doc.metadata.get("okf_version")
            if version is not None and version != "0.2":
                self.issue(doc.path, "VERSION_UNSUPPORTED", "Declared OKF version differs from this validator's 0.2 rules.")
            if not any(t.type == "heading_open" for t in doc.tokens) or not doc.links:
                self.issue(doc.path, "INDEX_EMPTY", "Index should group entries under headings.")
        if doc.path.name == "log.md":
            dates = []
            for i, token in enumerate(doc.tokens):
                if token.type == "heading_open" and token.tag == "h2":
                    title = doc.tokens[i + 1].content
                    try:
                        if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", title):
                            raise ValueError("Not an ISO date")
                        date.fromisoformat(title)
                        dates.append(title)
                    except ValueError:
                        self.issue(doc.path, "LOG_DATE", "Log date headings must be YYYY-MM-DD.", doc.body_line + token.map[0], "error")
            if not dates:
                self.issue(doc.path, "LOG_EMPTY", "Log has no dated entries.")
            elif dates != sorted(dates, reverse=True):
                self.issue(doc.path, "LOG_ORDER", "Log dates should be newest first.")

    def validate(self):
        for path in sorted(self.bundle.rglob("*.md")):
            if not any(path.resolve().is_relative_to(root) for root in (self.bundle, self.repo)):
                self.issue(path, "PATH_OUTSIDE_ROOT", "Document resolves outside the repository/bundle.")
                continue
            doc = self.read(path)
            if doc is not None:
                self.documents[path.resolve()] = doc
        if not self.documents:
            self.issue(self.bundle, "BUNDLE_EMPTY", "No readable Markdown documents found.", level="error")
        for name in ("index.md", "log.md"):
            if self.bundle / name not in self.documents:
                self.issue(self.bundle / name, "BUNDLE_ENTRY_MISSING", "DevForge expects a root {}.".format(name))
        indexed = set()
        pending = [self.bundle / "index.md"]
        while pending:
            path = pending.pop()
            if path in indexed or path not in self.documents:
                continue
            indexed.add(path)
            doc = self.documents[path]
            if path.name == "index.md":
                for value, _ in doc.links:
                    try:
                        target = self.local_path(doc, value)
                    except ValueError:
                        continue  # The affected-file link check reports the URI issue.
                    if target is not None:
                        pending.append(target / "index.md" if target.is_dir() else target)
        for doc in self.documents.values():
            self.metadata(doc)
            self.reserved(doc)
            for value, line in doc.links:
                self.check_path(doc, value, line)
            if doc.path.name not in ("index.md", "log.md") and doc.path not in indexed:
                self.issue(doc.path, "INDEX_MISSING_CONCEPT", "Concept is not reachable through the root/child indexes.")
        self.issues.sort(key=lambda issue: (issue.path, issue.line, issue.code))
        return self

    def report(self, strict=False):
        errors = sum(issue.level == "error" for issue in self.issues)
        warnings = len(self.issues) - errors
        return {
            "okf_version": "0.2", "strict": strict,
            "documents": len(self.documents), "errors": errors, "warnings": warnings,
            "passed": errors == 0 and (not strict or warnings == 0),
            "issues": [asdict(issue) for issue in self.issues],
        }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo-root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--bundle", type=Path, help="Bundle path, relative to --repo-root unless absolute.")
    parser.add_argument("--strict", action="store_true", help="Fail on project-quality warnings as well as OKF errors.")
    parser.add_argument("--json", action="store_true", help="Emit machine-readable results.")
    args = parser.parse_args(argv)
    repo = args.repo_root.resolve()
    bundle = (repo / (args.bundle or Path("docs/knowledge"))).resolve()
    if not repo.is_dir() or not bundle.is_dir():
        parser.error("Repository and bundle directories must exist.")
    result = Validator(bundle, repo).validate().report(args.strict)
    if args.json:
        print(json.dumps(result, ensure_ascii=True, indent=2))
    else:
        for issue in result["issues"]:
            print("{path}:{line}: {level} {code}: {message}".format(**issue))
        print("{}: {} documents, {} errors, {} warnings{}.".format(
            "PASS" if result["passed"] else "FAIL", result["documents"],
            result["errors"], result["warnings"], " (strict)" if args.strict else "",
        ))
        print("Structural checks only; source truth, human review, and desktop behavior are not verified.")
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
