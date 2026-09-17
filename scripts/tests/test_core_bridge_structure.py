"""The CoreBridge split stays reviewable and cannot silently regress."""

import importlib.util
import shutil
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CHECKER_PATH = ROOT / "scripts" / "check-core-bridge-structure.py"
SPEC = importlib.util.spec_from_file_location("core_bridge_structure", CHECKER_PATH)
assert SPEC is not None and SPEC.loader is not None
CHECKER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECKER)


class CoreBridgeStructure(unittest.TestCase):
    def test_current_modules_match_the_responsibility_inventory(self):
        self.assertEqual(CHECKER.check(ROOT), [])

    def test_snapshot_dto_moved_back_into_bridge_is_rejected(self):
        with self.fixture_root() as root:
            bridge = root / "macos" / "Sources" / "HerdrMacOS" / "CoreBridge.swift"
            bridge.write_text(bridge.read_text() + "\nstruct CoreRestSnapshot {}\n")

            issues = CHECKER.check(root)

        self.assertTrue(
            any("CoreBridge.swift" in issue and "unexpected" in issue for issue in issues),
            issues,
        )

    def test_policy_in_the_snapshot_module_is_rejected(self):
        with self.fixture_root() as root:
            snapshot = root / "macos" / "Sources" / "HerdrMacOS" / "CoreBridgeSnapshot.swift"
            snapshot.write_text(snapshot.read_text() + "\nenum CoreDispatchOutcome {}\n")

            issues = CHECKER.check(root)

        self.assertTrue(
            any("CoreBridgeSnapshot.swift" in issue and "unexpected" in issue for issue in issues),
            issues,
        )

    def test_snapshot_access_level_drift_is_rejected(self):
        with self.fixture_root() as root:
            snapshot = root / "macos" / "Sources" / "HerdrMacOS" / "CoreBridgeSnapshot.swift"
            snapshot.write_text(snapshot.read_text().replace("struct CoreSnapshot {", "private struct CoreSnapshot {", 1))

            issues = CHECKER.check(root)

        self.assertTrue(
            any("CoreBridgeSnapshot.swift" in issue and "private struct CoreSnapshot" in issue for issue in issues),
            issues,
        )

    @staticmethod
    def fixture_root():
        temporary = tempfile.TemporaryDirectory(prefix="core-bridge-structure-")
        root = Path(temporary.name)
        source = ROOT / "macos" / "Sources" / "HerdrMacOS"
        destination = root / "macos" / "Sources" / "HerdrMacOS"
        destination.mkdir(parents=True)
        for path in source.glob("CoreBridge*.swift"):
            shutil.copy2(path, destination / path.name)
        return _TemporaryRoot(temporary, root)


class _TemporaryRoot:
    def __init__(self, temporary: tempfile.TemporaryDirectory, root: Path):
        self.temporary = temporary
        self.root = root

    def __enter__(self) -> Path:
        return self.root

    def __exit__(self, exc_type, exc_value, traceback):
        self.temporary.cleanup()


if __name__ == "__main__":
    unittest.main()
