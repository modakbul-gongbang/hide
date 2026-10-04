"""Real command failures retain sampled resources and their original reason.

These short POSIX probes exercise the external command boundary. Windows
requires its owning runner; no mock substitutes for native job accounting.
"""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "check-delivery-os-contract.py"
SPEC = importlib.util.spec_from_file_location("delivery_command_resources", SCRIPT)
GATE = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = GATE
SPEC.loader.exec_module(GATE)


@unittest.skipUnless(sys.platform in ("darwin", "linux"), "native POSIX process accounting")
class FailedCommandResources(unittest.TestCase):
    def assert_failure_resources(self, code, seconds, reason):
        runs = GATE.ROOT / "agents/runs/ci-platform-completion"
        runs.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=runs) as folder:
            diagnostic = io.StringIO()
            with contextlib.redirect_stderr(diagnostic):
                with self.assertRaisesRegex(RuntimeError, reason):
                    GATE.run_command([sys.executable, "-c", code],
                                     time.monotonic() + seconds, Path(folder) / "command.log")
            resources = json.loads(diagnostic.getvalue().splitlines()[0])
            self.assertEqual(resources["status"], "fail")
            self.assertEqual(resources["measurement_status"], "available")
            self.assertIsNone(resources["measurement_error"])
            self.assertGreater(resources["peak_owned_processes"], 0)
            self.assertGreater(resources["peak_sampled_rss_bytes"], 0)
            # The caller must reap its guardian, even on these early failures.
            with self.assertRaises(ChildProcessError):
                os.waitpid(-1, os.WNOHANG)

    def test_output_cap_failure_keeps_final_samples_and_original_error(self):
        self.assert_failure_resources(
            "import os,time; memory=bytearray(16*1024*1024); time.sleep(.15); "
            "os.write(1,b'x'*(8*1024*1024+1)); time.sleep(1)",
            3, "command output exceeds the 8 MiB cap")

    def test_shared_deadline_failure_keeps_final_samples_and_original_error(self):
        self.assert_failure_resources(
            "import time; memory=bytearray(16*1024*1024); time.sleep(2)",
            .3, "contract check exceeded shared 570s deadline")


if __name__ == "__main__":
    unittest.main()
