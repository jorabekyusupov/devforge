"""Offline regression tests for M0 inspection and isolated extraction helpers."""

from contextlib import redirect_stdout
import io
import hashlib
from pathlib import Path
import struct
import sys
import tarfile
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
from m0_runtime_audit import architectures, audit, digest, finish_explicit_range, inventory, safe_member
from m0_macos_smoke import extract_tar


def macho(cpu=0x0100000C):
    return b"\xcf\xfa\xed\xfe" + struct.pack("<I", cpu) + bytes(64)


class RuntimeAuditTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def test_macho_and_fat_architectures(self):
        self.assertEqual(architectures(macho()), ["arm64"])
        self.assertEqual(architectures(macho(0x01000007)), ["x64"])
        fat = b"\xca\xfe\xba\xbe" + struct.pack(">I", 2)
        fat += struct.pack(">IIIII", 0x01000007, 0, 0, 0, 0)
        fat += struct.pack(">IIIII", 0x0100000C, 0, 0, 0, 0)
        self.assertEqual(architectures(fat), ["arm64", "x64"])

    def test_pe_architecture_and_invalid_headers(self):
        pe = bytearray(256)
        pe[:2] = b"MZ"
        struct.pack_into("<I", pe, 0x3C, 128)
        pe[128:132] = b"PE\0\0"
        struct.pack_into("<H", pe, 132, 0x8664)
        self.assertEqual(architectures(pe), ["x64"])
        for invalid in (b"", b"MZ", b"not executable", b"\xca\xfe\xba\xbe"):
            self.assertEqual(architectures(invalid), [])

    def test_archive_path_rejection(self):
        for name in ("../../outside", "/absolute", "C:\\Windows\\file", "a/../outside", "a\\..\\outside"):
            self.assertFalse(safe_member(name), name)
        self.assertTrue(safe_member("node/bin/node"))

    def test_zip_inventory_does_not_execute_or_extract(self):
        archive = self.root / "fixture.zip"
        with zipfile.ZipFile(archive, "w") as target:
            target.writestr("php.exe", macho(0x01000007))
            target.writestr("ext/php_pdo_pgsql.dll", b"fixture")
            target.writestr("LICENSE", b"fixture license")
        result = inventory(archive)
        self.assertEqual(result["file_count"], 3)
        self.assertTrue(result["windows_pdo_pgsql_present"])
        self.assertEqual(result["binaries"][0]["architectures"], ["x64"])
        self.assertEqual(sorted(p.name for p in self.root.iterdir()), ["fixture.zip"])

    def test_inventory_rejects_traversal(self):
        archive = self.root / "unsafe.zip"
        with zipfile.ZipFile(archive, "w") as target:
            target.writestr("../escape", b"unsafe")
        with self.assertRaises(ValueError):
            inventory(archive)

    def test_nested_postgres_jar_payload_is_inspected_without_extraction(self):
        payload = io.BytesIO()
        with tarfile.open(fileobj=payload, mode="w:xz") as nested:
            member = tarfile.TarInfo("bin/postgres")
            member.size = len(macho())
            nested.addfile(member, io.BytesIO(macho()))
        archive = self.root / "postgres.jar"
        with zipfile.ZipFile(archive, "w") as jar:
            jar.writestr("postgres-darwin.txz", payload.getvalue())
        result = inventory(archive)
        self.assertEqual(result["nested_archives_inspected"], ["postgres-darwin.txz"])
        self.assertEqual(result["binaries"][0]["architectures"], ["arm64"])
        self.assertFalse((self.root / "bin").exists())

    def test_safe_tar_relocation_and_internal_symlink(self):
        archive = self.root / "safe.tar.gz"
        with tarfile.open(archive, "w:gz") as target:
            content = macho()
            member = tarfile.TarInfo("runtime/bin/php")
            member.size = len(content)
            target.addfile(member, io.BytesIO(content))
            link = tarfile.TarInfo("runtime/php")
            link.type = tarfile.SYMTYPE
            link.linkname = "bin/php"
            target.addfile(link)
        destination = self.root / "relocated Русский"
        extract_tar(archive, destination)
        self.assertEqual((destination / "runtime/php").read_bytes(), macho())

    def test_tar_extraction_refuses_escaping_link_before_any_write(self):
        archive = self.root / "unsafe.tar.gz"
        with tarfile.open(archive, "w:gz") as target:
            link = tarfile.TarInfo("escape")
            link.type = tarfile.SYMTYPE
            link.linkname = "../../outside"
            target.addfile(link)
        destination = self.root / "extract"
        with self.assertRaises(ValueError):
            extract_tar(archive, destination)
        self.assertEqual(list(destination.iterdir()), [])

    def call_audit(self, entry):
        with redirect_stdout(io.StringIO()):
            return audit(entry, self.root)

    def test_publisher_hash_verified_without_network_for_cached_artifact(self):
        file = self.root / "fixture-artifact.phar"
        file.write_bytes(b"a non-executed artifact")
        entry = {"id": "fixture", "url": "https://example.invalid/artifact.phar", "sha256": digest(file)}
        with patch("m0_runtime_audit.subprocess.run", side_effect=AssertionError("Unexpected network")):
            result = self.call_audit(entry)
        self.assertEqual(result["integrity"], "publisher-sha256-matched")
        self.assertFalse(result["runtime_executed"])

    def test_matching_hash_does_not_hide_a_wrong_target_architecture(self):
        archive = self.root / "fixture-package.zip"
        with zipfile.ZipFile(archive, "w") as target:
            target.writestr("php.exe", macho())
        entry = {"id": "fixture", "url": "https://example.invalid/package.zip", "target": "windows-x64", "sha256": digest(archive)}
        with patch("m0_runtime_audit.subprocess.run", side_effect=AssertionError("Unexpected network")):
            result = self.call_audit(entry)
        self.assertEqual(result["status"], "failed")
        self.assertFalse(result["target_header_match"])

    def test_computed_hash_is_not_claimed_as_publisher_verification(self):
        (self.root / "fixture-artifact.phar").write_bytes(b"unverified")
        result = self.call_audit({"id": "fixture", "url": "https://example.invalid/artifact.phar", "sha256": None})
        self.assertEqual(result["integrity"], "computed-only-unverified")

    def test_hash_mismatch_never_activates_download(self):
        def download(command, **kwargs):
            Path(command[command.index("--output") + 1]).write_bytes(b"wrong content")
            return SimpleNamespace(returncode=0)
        entry = {"id": "fixture", "url": "https://example.invalid/artifact.zip", "sha256": "0" * 64}
        with patch("m0_runtime_audit.subprocess.run", side_effect=download):
            result = self.call_audit(entry)
        self.assertEqual(result["status"], "failed")
        self.assertEqual(list(self.root.iterdir()), [])

    def test_invalid_url_and_cache_id_are_rejected_without_network(self):
        for entry in ({"id": "fixture", "url": "http://example.invalid/file"},
                      {"id": "../escape", "url": "https://example.invalid/file"},
                      {"id": "fixture", "url": "https://user:secret@example.invalid/file"}):
            with patch("m0_runtime_audit.subprocess.run", side_effect=AssertionError("Unexpected network")):
                result = self.call_audit(entry)
            self.assertEqual(result["status"], "failed")

    def test_metadata_only_candidate_is_not_downloaded_or_executed(self):
        entry = {"id": "fixture", "url": "https://example.invalid/package.zip", "download_policy": "metadata-only"}
        response = SimpleNamespace(returncode=0, stdout="HTTP/2 200\ncontent-length: 1234\n")
        with patch("m0_runtime_audit.subprocess.run", return_value=response) as request:
            result = self.call_audit(entry)
        self.assertIn("--head", request.call_args[0][0])
        self.assertEqual(result["status"], "metadata-only")
        self.assertEqual(result["declared_bytes"], 1234)
        self.assertFalse(result["runtime_executed"])
        self.assertEqual(list(self.root.iterdir()), [])

    def test_interrupted_download_can_resume_and_is_verified_before_activation(self):
        good = b"complete expected content"
        entry = {"id": "fixture", "url": "https://example.invalid/artifact.phar",
                 "sha256": hashlib.sha256(good).hexdigest()}
        def interrupted(command, **kwargs):
            Path(command[command.index("--output") + 1]).write_bytes(b"partial")
            return SimpleNamespace(returncode=28)
        with patch("m0_runtime_audit.subprocess.run", side_effect=interrupted):
            self.assertEqual(self.call_audit(entry)["status"], "failed")
        partial = self.root / "fixture-artifact.phar.part"
        self.assertEqual(partial.read_bytes(), b"partial")
        def resumed(command, **kwargs):
            self.assertIn("--continue-at", command)
            self.assertEqual(partial.read_bytes(), b"partial")
            partial.write_bytes(good)
            return SimpleNamespace(returncode=0)
        with patch("m0_runtime_audit.subprocess.run", side_effect=resumed):
            result = self.call_audit(entry)
        self.assertEqual(result["integrity"], "publisher-sha256-matched")
        self.assertFalse(partial.exists())
        self.assertEqual((self.root / "fixture-artifact.phar").read_bytes(), good)

    def test_explicit_tail_recovery_validates_range_before_appending(self):
        partial = self.root / "artifact.part"
        partial.write_bytes(b"prefix")
        def request(command, **kwargs):
            span = command[command.index("--range") + 1]
            headers = Path(command[command.index("--dump-header") + 1])
            output = Path(command[command.index("--output") + 1])
            if span == "0-0":
                headers.write_text("HTTP/1.1 206\ncontent-range: bytes 0-0/12\n")
                output.write_bytes(b"p")
            else:
                self.assertEqual(span, "6-11")
                headers.write_text("HTTP/1.1 206\ncontent-range: bytes 6-11/12\n")
                output.write_bytes(b"suffix")
            return SimpleNamespace(returncode=0)
        with patch("m0_runtime_audit.subprocess.run", side_effect=request):
            self.assertTrue(finish_explicit_range(partial, "https://example.invalid/package"))
        self.assertEqual(partial.read_bytes(), b"prefixsuffix")

    def test_invalid_tail_range_does_not_corrupt_existing_prefix(self):
        partial = self.root / "artifact.part"
        partial.write_bytes(b"prefix")
        def request(command, **kwargs):
            headers = Path(command[command.index("--dump-header") + 1])
            output = Path(command[command.index("--output") + 1])
            if command[command.index("--range") + 1] == "0-0":
                headers.write_text("HTTP/1.1 206\ncontent-range: bytes 0-0/12\n")
                output.write_bytes(b"p")
            else:
                headers.write_text("HTTP/1.1 206\ncontent-range: bytes 0-5/12\n")
                output.write_bytes(b"wrong!")
            return SimpleNamespace(returncode=0)
        with patch("m0_runtime_audit.subprocess.run", side_effect=request):
            self.assertFalse(finish_explicit_range(partial, "https://example.invalid/package"))
        self.assertEqual(partial.read_bytes(), b"prefix")


if __name__ == "__main__":
    unittest.main()
