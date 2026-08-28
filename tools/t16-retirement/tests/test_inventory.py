from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

PACKAGE_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(PACKAGE_ROOT))

from t16_retirement.inventory import inventory  # noqa: E402


class InventoryTests(unittest.TestCase):
    def test_inventory_requires_historical_prds_and_keeps_retirement_disabled(self) -> None:
        root = Path("/Users/hoyeonlee/projects/herdr-ide")
        pet = Path("/Users/hoyeonlee/projects/herdr-pet")
        if not (root / "agents/prd/herdr-lightweight-ide/prd.md").is_file():
            self.skipTest("repository fixtures are not available")
        result = inventory(root, pet)
        self.assertEqual(result["status"], "PASS")
        self.assertEqual(len(result["supersession"]["old_prds"]), 2)
        self.assertFalse(result["retirement"]["executed"])
        self.assertFalse(result["supersession"]["sealed_inputs_modified"])


if __name__ == "__main__":
    unittest.main()
