"""Cheap subprocess-boundary checks; no cargo build or package install.

The external commands record the argv and environment received by CI, and
fail on demand. Real build freshness remains test_verification_builds.py's job.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class EntryPoints(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        (self.root / "scripts").mkdir()
        self.bin = self.root / "bin"
        self.bin.mkdir()
        for name in ["verify-cargo.sh", "verify-web.sh", "toolchain-env.sh"]:
            shutil.copyfile(ROOT / "scripts" / name, self.root / "scripts" / name)
        for name in ["cargo", "pnpm"]:
            file = self.bin / name
            file.write_text("#!/usr/bin/env python3\n"
                            "import json, os, sys\n"
                            "with open(os.environ['RECORD'], 'a') as f:\n"
                            " f.write(json.dumps({'argv': sys.argv[1:], 'cwd': os.getcwd(), 'env': dict(os.environ)})+'\\n')\n"
                            "sys.exit(int(os.environ.get('FAIL_COMMAND', '0')))\n")
            file.chmod(0o755)
        self.record = self.root / "record.jsonl"
        self.env = {**os.environ, "PATH": str(self.bin) + os.pathsep + os.environ["PATH"],
                    "RECORD": str(self.record), "CARGO_TARGET_DIR": str(self.root / "foreign"),
                    "HERDR_SOCKET_PATH": "operator-server", "HERDR_ENV": "1",
                    "HCOORD_HOME": "operator-legacy-state",
                    "HIDE_E2E_HERDR_BIN": "explicit-test-binary"}

    def run_entry(self, script, *args, **env):
        return subprocess.run(["bash", str(self.root / "scripts" / script), *args],
                              env={**self.env, **env}, capture_output=True, text=True, timeout=10)

    def records(self):
        return [json.loads(line) for line in self.record.read_text().splitlines()]

    def test_scoped_cargo_keeps_target_and_drops_operator_identity(self):
        result = self.run_entry("verify-cargo.sh", "test-scoped", "-p", "hide-platform", "--", "--ignored")
        self.assertEqual(result.returncode, 0, result.stderr)
        received = self.records()[0]
        self.assertEqual(received["argv"], ["test", "--locked", "-p", "hide-platform", "--", "--ignored"])
        self.assertEqual(received["cwd"], str(self.root))
        self.assertEqual(received["env"]["CARGO_TARGET_DIR"], str(self.root / "target"))
        self.assertFalse(any(key.startswith(("HERDR_", "HCOORD_")) for key in received["env"]))
        self.assertEqual(received["env"]["HIDE_E2E_HERDR_BIN"], "explicit-test-binary")
        self.assertEqual(self.run_entry("verify-cargo.sh", "check", "--workspace", FAIL_COMMAND="23").returncode, 23)

    def test_invalid_cargo_mode_or_artifact_redirection_never_calls_cargo(self):
        for args in [("unknown",), ("build", "--target-dir", "elsewhere"),
                     ("check", "--config=build.target-dir=elsewhere"), ("clippy", "--manifest-path=../Cargo.toml"),
                     ("test-scoped", *(["arg"] * 129))]:
            with self.subTest(args=args[:2]):
                self.assertEqual(self.run_entry("verify-cargo.sh", *args).returncode, 2)
        self.assertFalse(self.record.exists())

    def test_sealed_test_and_lint_arguments_remain_unchanged(self):
        self.assertEqual(self.run_entry("verify-cargo.sh", "test", "selected", "--", "--ignored").returncode, 0)
        self.assertEqual(self.records()[0]["argv"], ["test", "--locked", "--workspace", "selected", "--", "--ignored"])
        self.assertEqual(self.run_entry("verify-cargo.sh", "lint").returncode, 0)
        self.assertEqual([r["argv"] for r in self.records()[1:]],
                         [["fmt", "--all", "--check"], ["clippy", "--locked", "--workspace", "--all-targets", "--", "-D", "warnings"]])

    def test_web_scoped_step_forwards_playwright_failure_and_package(self):
        result = self.run_entry("verify-web.sh", "web", "e2e", "--shard=1/4", FAIL_COMMAND="19")
        self.assertEqual(result.returncode, 19)
        self.assertEqual(self.records()[0]["argv"], ["--dir", "web", "exec", "playwright", "test", "--shard=1/4"])
        self.assertEqual(self.run_entry("verify-web.sh", "desktop", "package").returncode, 0)
        self.assertEqual(self.records()[1]["argv"], ["--dir", "desktop", "package"])

    def test_web_install_is_locked_and_invalid_package_actions_fail(self):
        self.assertEqual(self.run_entry("verify-web.sh", "install", "--ignore-scripts").returncode, 0)
        self.assertEqual(self.records()[0]["argv"], ["install", "--frozen-lockfile", "--ignore-scripts"])
        for args in [("install", "--no-frozen-lockfile"), ("elsewhere", "test"),
                     ("web", "package"), ("web", "test:e2e"), ("retired", "e2e"), ("desktop", "unknown")]:
            with self.subTest(args=args):
                self.assertEqual(self.run_entry("verify-web.sh", *args).returncode, 2)
        self.assertEqual(len(self.records()), 1)

    def test_electron_acquisition_runs_once_and_propagates_the_installer_failure(self):
        result = self.run_entry("verify-web.sh", "desktop", "electron-install", FAIL_COMMAND="17")
        self.assertEqual(result.returncode, 17)
        self.assertEqual([record["argv"] for record in self.records()],
                         [["--dir", "desktop", "exec", "install-electron", "--no"]])
        for args in [("web", "electron-install"), ("other", "electron-install"),
                     ("desktop", "electron-install", "--force")]:
            with self.subTest(args=args):
                self.assertEqual(self.run_entry("verify-web.sh", *args).returncode, 2)
        self.assertEqual(len(self.records()), 1)


if __name__ == "__main__":
    unittest.main()
