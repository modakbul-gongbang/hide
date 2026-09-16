#!/usr/bin/env python3
import hashlib
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PUBLISH = ROOT / "scripts" / "publish-dist-artifacts.sh"


class DistArtifactRetentionTests(unittest.TestCase):
    def ready_inputs(self, root: Path, version: str = "2.0.0"):
        bundle = root / "ready" / "hide.app"
        bundle.mkdir(parents=True)
        (bundle / "marker").write_text("new bundle")
        archive = root / "ready" / f"hide-v{version}-macos-arm64.zip"
        archive.write_bytes(b"new archive")
        checksum = archive.with_suffix(archive.suffix + ".sha256")
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        checksum.write_text(f"{digest}  {archive.name}\n")
        return bundle, archive, checksum

    def run_publish(
        self,
        bundle: Path,
        archive: Path,
        checksum: Path,
        dist: Path,
        env=None,
    ):
        return subprocess.run(
            ["zsh", str(PUBLISH), str(bundle), str(archive), str(checksum), str(dist)],
            text=True,
            capture_output=True,
            env={
                **os.environ,
                "LC_ALL": "C",
                "LC_CTYPE": "C",
                "LANG": "C",
                **(env or {}),
            },
        )

    def test_success_keeps_only_the_current_hide_archive_pair(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            dist = root / "dist"
            dist.mkdir()
            (dist / "hide-v1.0.0-macos-arm64.zip").write_bytes(b"old")
            (dist / "hide-v1.0.0-macos-arm64.zip.sha256").write_text("old")
            (dist / "notes.txt").write_text("preserve me")
            (dist / "hide.app").mkdir()
            (dist / "hide.app" / "marker").write_text("old bundle")
            bundle, archive, checksum = self.ready_inputs(root)

            result = self.run_publish(bundle, archive, checksum, dist)

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(
                sorted(path.name for path in dist.glob("hide-v*-macos-arm64.zip*")),
                ["hide-v2.0.0-macos-arm64.zip", "hide-v2.0.0-macos-arm64.zip.sha256"],
            )
            self.assertEqual((dist / "hide.app" / "marker").read_text(), "new bundle")
            self.assertEqual((dist / "notes.txt").read_text(), "preserve me")

    def test_incomplete_ready_set_preserves_the_previous_success(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            dist = root / "dist"
            dist.mkdir()
            old_archive = dist / "hide-v1.0.0-macos-arm64.zip"
            old_archive.write_bytes(b"old")
            old_checksum = dist / "hide-v1.0.0-macos-arm64.zip.sha256"
            old_checksum.write_text("old")
            old_bundle = dist / "hide.app"
            old_bundle.mkdir()
            (old_bundle / "marker").write_text("old bundle")
            bundle, archive, checksum = self.ready_inputs(root)
            checksum.unlink()

            result = self.run_publish(bundle, archive, checksum, dist)

            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(old_archive.read_bytes(), b"old")
            self.assertEqual(old_checksum.read_text(), "old")
            self.assertEqual(
                (old_bundle / "marker").read_text(),
                "old bundle",
                f"stderr={result.stderr!r}; dist={[path.name for path in dist.iterdir()]}",
            )

    def test_publish_failure_restores_the_previous_complete_set(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            dist = root / "dist"
            dist.mkdir()
            old_archive = dist / "hide-v1.0.0-macos-arm64.zip"
            old_archive.write_bytes(b"old archive")
            old_checksum = dist / "hide-v1.0.0-macos-arm64.zip.sha256"
            old_checksum.write_text("old checksum")
            old_bundle = dist / "hide.app"
            old_bundle.mkdir()
            (old_bundle / "marker").write_text("old bundle")
            bundle, archive, checksum = self.ready_inputs(root)

            fake_bin = root / "fake-bin"
            fake_bin.mkdir()
            failure_marker = root / "checksum-move-failed"
            checksum_target = dist / checksum.name
            real_mv = shutil.which("mv")
            self.assertIsNotNone(real_mv)
            (fake_bin / "mv").write_text(
                "#!/bin/sh\n"
                "last=''\n"
                "for argument in \"$@\"; do last=$argument; done\n"
                f"if [ \"$last\" = '{checksum_target}' ] && "
                f"[ ! -e '{failure_marker}' ]; then\n"
                f"  : > '{failure_marker}'\n"
                "  exit 73\n"
                "fi\n"
                f"exec '{real_mv}' \"$@\"\n"
            )
            (fake_bin / "mv").chmod(0o755)

            result = self.run_publish(
                bundle,
                archive,
                checksum,
                dist,
                env={"PATH": f"{fake_bin}:{os.environ['PATH']}"},
            )

            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(
                (old_bundle / "marker").read_text(),
                "old bundle",
                f"stderr={result.stderr!r}; dist={[path.name for path in dist.iterdir()]}",
            )
            self.assertEqual(old_archive.read_bytes(), b"old archive")
            self.assertEqual(old_checksum.read_text(), "old checksum")
            self.assertFalse((dist / archive.name).exists())
            self.assertFalse((dist / checksum.name).exists())
            self.assertEqual(
                [path.name for path in dist.iterdir() if path.name.startswith(".")],
                [],
            )


if __name__ == "__main__":
    unittest.main()
