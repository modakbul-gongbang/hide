"""Run the external HTTP release-writer contract in the required script suite."""
from pathlib import Path
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[2]


class ReleaseDraftTest(unittest.TestCase):
    def test_guarded_append_only_writer(self):
        result = subprocess.run(
            ["node", "--test", "scripts/tests/release-draft.test.mjs"],
            cwd=ROOT, text=True, capture_output=True, timeout=60,
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
