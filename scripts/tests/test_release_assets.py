"""The #343 release contract rejects incomplete, mixed or published packages."""
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
CHECK = ROOT / "scripts/check-release-assets.mjs"


class ReleaseAssetsTest(unittest.TestCase):
    def setUp(self):
        runs = ROOT / "agents/runs/release-assets"
        runs.mkdir(parents=True, exist_ok=True)
        self.scratch = tempfile.TemporaryDirectory(dir=runs)
        self.addCleanup(self.scratch.cleanup)
        self.directory = Path(self.scratch.name) / "packages"
        self.directory.mkdir()
        self.names = [
            "hide-v1.2.3-macos-arm64.zip",
            "hide-v1.2.3-windows-x64.zip",
            "hide-v1.2.3-linux-x64.tar.gz",
        ]
        for name in self.names:
            data = f"fixture for {name}".encode()
            (self.directory / name).write_bytes(data)
            (self.directory / f"{name}.sha256").write_text(f"{hashlib.sha256(data).hexdigest()}  {name}\n")

    def check(self, tag="v1.2.3", release=None):
        args = ["node", str(CHECK), tag, str(self.directory)]
        if release is not None:
            record = Path(self.scratch.name) / "release.json"
            record.write_text(json.dumps(release))
            args.append(str(record))
        return subprocess.run(args, capture_output=True, text=True, timeout=10)

    def test_complete_same_version_packages_pass(self):
        self.assertEqual(self.check().returncode, 0)

    def test_three_checksums_cannot_hide_a_missing_target(self):
        name = self.names[1]
        other = name.replace("windows", "other")
        (self.directory / name).rename(self.directory / other)
        checksum = self.directory / f"{name}.sha256"
        content = checksum.read_text().replace(name, other)
        checksum.unlink()
        (self.directory / f"{other}.sha256").write_text(content)
        self.assertNotEqual(self.check().returncode, 0)

    def test_mixed_version_and_extra_assets_fail(self):
        (self.directory / "hide-v1.2.2-macos-arm64.zip").write_bytes(b"old")
        self.assertNotEqual(self.check().returncode, 0)

    def test_corruption_and_wrong_checksum_filename_fail(self):
        name = self.names[0]
        original = (self.directory / name).read_bytes()
        (self.directory / name).write_bytes(b"changed")
        self.assertNotEqual(self.check().returncode, 0)
        (self.directory / name).write_bytes(original)
        checksum = self.directory / f"{name}.sha256"
        checksum.write_text(checksum.read_text().replace(name, self.names[1]))
        self.assertNotEqual(self.check().returncode, 0)

    def test_symlinks_and_empty_packages_fail(self):
        name = self.names[0]
        archive = self.directory / name
        archive.unlink()
        archive.symlink_to(self.directory / self.names[1])
        self.assertNotEqual(self.check().returncode, 0)
        archive.unlink()
        archive.write_bytes(b"")
        self.assertNotEqual(self.check().returncode, 0)

    def test_stable_tag_required(self):
        for tag in ["v1.2.3-beta.1", "v1.2.3+build", "v01.2.3", "1.2.3"]:
            with self.subTest(tag=tag):
                self.assertNotEqual(self.check(tag).returncode, 0)

    def test_partial_draft_retry_passes_and_public_release_fails(self):
        draft = {"tag_name": "v1.2.3", "draft": True, "prerelease": False,
                 "assets": [{"name": self.names[0]}]}
        self.assertEqual(self.check(release=draft).returncode, 0)
        draft["draft"] = False
        self.assertNotEqual(self.check(release=draft).returncode, 0)

    def test_mixed_or_duplicate_existing_draft_assets_fail(self):
        draft = {"tag_name": "v1.2.3", "draft": True, "prerelease": False,
                 "assets": [{"name": "hide-v1.2.2-windows-x64.zip"}]}
        self.assertNotEqual(self.check(release=draft).returncode, 0)
        draft["assets"] = [{"name": self.names[0]}, {"name": self.names[0]}]
        self.assertNotEqual(self.check(release=draft).returncode, 0)


if __name__ == "__main__":
    unittest.main()
