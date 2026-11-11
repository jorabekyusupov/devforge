"""Behavioral fixtures for the read-only knowledge validator; no product services."""

from datetime import datetime, timezone
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

import yaml

SCRIPT = Path(__file__).resolve().parents[1] / "scripts" / "validate_knowledge.py"
sys.path.insert(0, str(SCRIPT.parent))
from validate_knowledge import Validator  # noqa: E402


class KnowledgeValidationTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name)
        self.bundle = self.repo / "docs" / "knowledge"
        self.bundle.mkdir(parents=True)
        self.write("index.md", '---\nokf_version: "0.2"\n---\n# Knowledge\n\n- [Concept](concept.md)\n')
        self.write("log.md", "# History\n\n## 2026-11-11\n\n- Initial fixture.\n")
        self.concept()

    def write(self, name, text):
        path = self.bundle / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        return path

    def concept(self, metadata=None, body="# Concept\n\nUseful knowledge.\n"):
        metadata = {"type": "A Custom Concept"} if metadata is None else metadata
        self.write("concept.md", "---\n" + yaml.safe_dump(metadata, sort_keys=False) + "---\n" + body)

    def report(self, strict=False):
        return Validator(self.bundle, self.repo, datetime(2026, 11, 11, tzinfo=timezone.utc)).validate().report(strict)

    def test_minimal_concept_unknown_type_and_extension_are_valid(self):
        self.concept({"type": "New Vendor Concept", "custom": {"any": [1, 2]}})
        self.assertTrue(self.report(strict=True)["passed"])

    def test_required_frontmatter_and_type_fail(self):
        for text in ("# No frontmatter\n", "---\ntitle: Missing type\n---\n", "---\ntype: ' '\n---\n", "---\ntype: []\n---\n"):
            with self.subTest(text=text):
                self.write("concept.md", text)
                self.assertGreater(self.report()["errors"], 0)

    def test_malformed_unclosed_and_nonmapping_yaml_fail(self):
        for text in ("---\ntype: [broken\n---\n", "---\ntype: Reference\n", "---\n- item\n---\n"):
            with self.subTest(text=text):
                self.write("concept.md", text)
                self.assertGreater(self.report()["errors"], 0)

    def test_duplicate_yaml_keys_are_rejected(self):
        self.write("concept.md", "---\ntype: Reference\ntype: Playbook\n---\n")
        self.assertGreater(self.report()["errors"], 0)

    def test_yaml_merge_override_is_valid(self):
        self.write("concept.md", "---\ndefaults: &defaults\n  type: Reference\n<<: *defaults\ntype: Playbook\n---\n# Concept\n")
        self.assertTrue(self.report(strict=True)["passed"])

    def test_unsafe_yaml_tag_is_not_constructed(self):
        marker = self.repo / "unsafe-marker"
        self.write("concept.md", "---\ntype: !!python/object/apply:os.system ['touch {}']\n---\n".format(marker))
        self.assertGreater(self.report()["errors"], 0)
        self.assertFalse(marker.exists())

    def test_broken_link_is_warning_not_okf_conformance_error(self):
        self.concept(body="# Concept\n\n[Not written](later.md)\n")
        self.assertTrue(self.report()["passed"])
        self.assertEqual(self.report()["errors"], 0)
        self.assertFalse(self.report(strict=True)["passed"])

    def test_links_inside_code_blocks_or_inline_code_are_ignored(self):
        self.concept(body="# Concept\n\n`[inline](missing.md)`\n\n```md\n[example](missing.md)\n[^fake]\n```\n\n    [indented](missing.md)\n")
        self.assertTrue(self.report(strict=True)["passed"])

    def test_reference_links_and_images_are_checked(self):
        for body in ("[Target][ref]\n\n[ref]: missing.md\n", "![Asset](missing.png)\n"):
            with self.subTest(body=body):
                self.concept(body=body)
                self.assertFalse(self.report(strict=True)["passed"])

    def test_encoded_paths_and_parenthesized_filenames_resolve(self):
        self.write("a file.md", "---\ntype: Reference\n---\n")
        self.write("file(v1).md", "---\ntype: Reference\n---\n")
        self.write("index.md", "# Index\n\n- [A](concept.md)\n- [B](a%20file.md)\n- [C](file(v1).md)\n")
        self.concept(body="[A](a%20file.md) and [B](file(v1).md).\n")
        self.assertTrue(self.report(strict=True)["passed"])

    def test_bundle_root_links_and_repository_relative_sources_resolve(self):
        (self.repo / "Cargo.toml").write_text("# fixture\n", encoding="utf-8")
        self.concept({"type": "Reference", "sources": [{"resource": "../../Cargo.toml"}]}, "[Index](/index.md)\n")
        self.assertTrue(self.report(strict=True)["passed"])

    def test_external_urls_scope_descriptions_and_fragments_are_not_fetched(self):
        self.concept({"type": "Reference", "sources": [{"resource": "all queries in project sample.example"}]},
                     "[External](https://invalid.example/not-fetched) [Section](#heading)\n")
        self.assertTrue(self.report(strict=True)["passed"])

    def test_local_path_escape_is_reported(self):
        self.concept(body="[Outside](../../../../outside.md)\n")
        self.assertFalse(self.report(strict=True)["passed"])

    def test_nested_index_reaches_its_concepts(self):
        self.write("sub/index.md", "# Sub\n\n- [Child](child.md)\n")
        self.write("sub/child.md", "---\ntype: Reference\n---\n")
        self.write("index.md", "# Index\n\n- [Concept](concept.md)\n- [Sub](sub/)\n")
        self.assertTrue(self.report(strict=True)["passed"])

    def test_unindexed_concept_warns(self):
        self.write("forgotten.md", "---\ntype: Reference\n---\n")
        self.assertTrue(self.report()["passed"])
        self.assertFalse(self.report(strict=True)["passed"])

    def test_missing_root_index_and_log_are_project_warnings(self):
        (self.bundle / "index.md").unlink()
        (self.bundle / "log.md").unlink()
        self.assertTrue(self.report()["passed"])
        self.assertFalse(self.report(strict=True)["passed"])

    def test_reserved_files_reject_concept_metadata(self):
        for name in ("index.md", "log.md", "sub/index.md"):
            with self.subTest(name=name):
                original = (self.bundle / name).read_text() if (self.bundle / name).exists() else None
                self.write(name, "---\ntype: Reference\n---\n# Wrong\n")
                self.assertGreater(self.report()["errors"], 0)
                if original is None:
                    (self.bundle / name).unlink()
                else:
                    self.write(name, original)

    def test_invalid_log_date_fails_and_reverse_order_warns(self):
        self.write("log.md", "# History\n\n## 2026-02-30\n\n- Invalid.\n")
        self.assertGreater(self.report()["errors"], 0)
        self.write("log.md", "# History\n\n## 2026-11-10\n- Older.\n\n## 2026-11-11\n- Newer.\n")
        self.assertFalse(self.report(strict=True)["passed"])

    def test_matching_source_and_footnote_are_valid_without_verified(self):
        self.concept({"type": "Reference", "sources": [{"id": "source", "resource": "/index.md"}]},
                     "A claim.[^source]\n\n[^source]: Local index.\n")
        self.assertTrue(self.report(strict=True)["passed"])

    def test_unresolved_footnote_and_source_join_warn(self):
        for body in ("A claim.[^absent]\n", "A claim.[^absent]\n\n[^absent]: No source entry.\n"):
            with self.subTest(body=body):
                self.concept(body=body)
                self.assertFalse(self.report(strict=True)["passed"])

    def test_invalid_source_shapes_ids_and_missing_local_resources_warn(self):
        variants = ("not-a-list", [None], [{"resource": ""}],
                    [{"id": "x", "resource": "/index.md"}, {"id": "x", "resource": "/index.md"}],
                    [{"resource": "missing.md"}])
        for sources in variants:
            with self.subTest(sources=sources):
                self.concept({"type": "Reference", "sources": sources})
                self.assertFalse(self.report(strict=True)["passed"])

    def test_lifecycle_and_decision_status_are_independent(self):
        self.concept({"type": "Reference", "status": "draft", "decision_status": "confirmed"})
        self.assertTrue(self.report(strict=True)["passed"])
        self.concept({"type": "Reference", "status": "confirmed", "decision_status": "stable"})
        self.assertFalse(self.report(strict=True)["passed"])

    def test_valid_single_mapping_and_list_verifications_are_accepted(self):
        entry = {"by": "process:fixture-check", "at": "2026-11-11T10:00:00+05:00"}
        for value in (entry, [entry]):
            with self.subTest(value=value):
                self.concept({"type": "Reference", "verified": value})
                self.assertTrue(self.report(strict=True)["passed"])

    def test_invalid_actor_timestamp_and_stale_content_warn(self):
        for metadata in ({"verified": {"by": "anonymous", "at": "2026-11-11"}},
                         {"verified": []}, {"generated": {"by": "producer/v1", "at": "bad"}},
                         {"stale_after": "2026-11-01T00:00:00Z"}):
            with self.subTest(metadata=metadata):
                self.concept({"type": "Reference", **metadata})
                self.assertFalse(self.report(strict=True)["passed"])

    def test_invalid_utf8_and_empty_bundle_fail(self):
        (self.bundle / "concept.md").write_bytes(b"\xff\xfe")
        self.assertGreater(self.report()["errors"], 0)
        for path in self.bundle.glob("*.md"):
            path.unlink()
        self.assertGreater(self.report()["errors"], 0)

    def cli(self, *args):
        return subprocess.run([sys.executable, str(SCRIPT), "--repo-root", str(self.repo), *args],
                              cwd=self.repo, capture_output=True, text=True)

    def test_cli_json_exit_codes_strict_mode_and_read_only_behavior(self):
        before = {p.relative_to(self.repo): p.read_bytes() for p in self.repo.rglob("*") if p.is_file()}
        passed = self.cli("--strict", "--json")
        self.assertEqual(passed.returncode, 0, passed.stderr)
        self.assertTrue(json.loads(passed.stdout)["passed"])
        after = {p.relative_to(self.repo): p.read_bytes() for p in self.repo.rglob("*") if p.is_file()}
        self.assertEqual(before, after)
        self.concept(body="[Broken](missing.md)\n")
        self.assertEqual(self.cli().returncode, 0)
        failed = self.cli("--strict", "--json")
        self.assertEqual(failed.returncode, 1)
        self.assertFalse(json.loads(failed.stdout)["passed"])
        self.write("concept.md", "No metadata\n")
        self.assertEqual(self.cli().returncode, 1)

    def test_cli_missing_bundle_is_usage_error(self):
        result = self.cli("--bundle", "missing")
        self.assertEqual(result.returncode, 2)
        self.assertNotIn("Traceback", result.stderr)


if __name__ == "__main__":
    unittest.main()
