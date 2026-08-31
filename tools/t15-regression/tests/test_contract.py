from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

PACKAGE_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(PACKAGE_ROOT))

from t15_regression.model import CheckSpec, ContractError, load_manifest  # noqa: E402
from t15_regression.runtime import CommandResult, RegressionRunner  # noqa: E402


def write_manifest(root: Path, *, mutate=None) -> Path:
    raw = {
        "schema": "herdr.ide.t15-regression.manifest.v1",
        "run_id": "test-run",
        "verification_profile": "test-profile",
        "working_directory": ".",
        "output_dir": "evidence/test-run",
        "checks": [
            {"id": "pass", "mode": "automated behavior", "required": True, "can_block": False, "command": ["true"]},
            {"id": "blocked", "mode": "remote runtime", "required": True, "can_block": True, "status": "BLOCKED", "reason": "fixture unavailable"},
        ],
    }
    if mutate is not None:
        mutate(raw)
    path = root / "manifest.json"
    path.write_text(json.dumps(raw), encoding="utf-8")
    return path


class FakeCommandRunner:
    def __init__(self, *, exit_code: int = 0) -> None:
        self.exit_code = exit_code
        self.calls: list[tuple[str, ...]] = []

    def run(self, command, *, cwd, environment, timeout_seconds):
        self.calls.append(command)
        return CommandResult(command, self.exit_code, "ok", "", 1.0)


class ContractTests(unittest.TestCase):
    def test_check_spec_rejects_shell_syntax_and_run_without_command(self) -> None:
        with self.assertRaises(ContractError):
            CheckSpec.from_raw({"id": "x", "mode": "automated behavior", "command": ["sh", "-c", "echo ok; echo bad"]}, 0)
        with self.assertRaises(ContractError):
            CheckSpec.from_raw({"id": "x", "mode": "automated behavior"}, 0)

    def test_manifest_rejects_duplicate_check_ids(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = write_manifest(root, mutate=lambda raw: raw["checks"].append(raw["checks"][0]))
            with self.assertRaisesRegex(ContractError, "unique"):
                load_manifest(path, root=root)

    def test_manifest_keeps_root_relative_output_when_generated_target_is_sibling_symlink(self) -> None:
        with tempfile.TemporaryDirectory() as directory, tempfile.TemporaryDirectory() as cache:
            root = Path(directory)
            (root / "target").symlink_to(Path(cache), target_is_directory=True)
            manifest = load_manifest(
                write_manifest(root, mutate=lambda raw: raw.update({"output_dir": "target/evidence/test-run"})),
                root=root,
            )
            self.assertEqual(manifest.output_dir, root.resolve() / "target/evidence/test-run")

    def test_runner_keeps_required_blocked_rows_out_of_pass(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = load_manifest(write_manifest(root), root=root)
            runner = RegressionRunner(manifest, command_runner=FakeCommandRunner())
            result = runner.run()
            self.assertEqual(result["status"], "PARTIAL")
            self.assertEqual(result["required_summary"]["blocked"], 1)

    def test_runner_reports_required_failure_and_preserves_redacted_command_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            path = write_manifest(root, mutate=lambda raw: raw["checks"].pop())
            manifest = load_manifest(path, root=root)
            runner = RegressionRunner(manifest, command_runner=FakeCommandRunner(exit_code=7))
            result = runner.run()
            self.assertEqual(result["status"], "FAIL")
            self.assertEqual(result["required_summary"]["fail"], 1)
            self.assertTrue((manifest.output_dir / "matrix.json").is_file())

    def test_dry_run_does_not_call_command_runner(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            manifest = load_manifest(write_manifest(root), root=root)
            fake = FakeCommandRunner()
            result = RegressionRunner(manifest, command_runner=fake).run(execute=False)
            self.assertEqual(result["status"], "PARTIAL")
            self.assertEqual(fake.calls, [])

    def test_result_preserves_user_accepted_deferred_task_without_calling_it_machine_pass(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)

            def mutate(raw):
                raw["task_status"] = {
                    "T5": {
                        "status": "user-accepted/verification-deferred",
                        "machine_pass": False,
                        "note": "physical Option+F verification is deferred",
                    }
                }

            manifest = load_manifest(write_manifest(root, mutate=mutate), root=root)
            result = RegressionRunner(manifest, command_runner=FakeCommandRunner()).run(execute=False)
            self.assertEqual(result["task_status"]["T5"]["status"], "user-accepted/verification-deferred")
            self.assertFalse(result["task_status"]["T5"]["machine_pass"])


if __name__ == "__main__":
    unittest.main()
