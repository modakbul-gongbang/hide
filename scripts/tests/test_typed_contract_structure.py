"""The typed wire boundary must cover extracted runtime submodules."""

import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


class TypedContractStructure(unittest.TestCase):
    def run_checker(self, runtime_module_text, *, path=None):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'scripts').mkdir()
            sources = root / 'herdr-core' / 'src'
            (sources / 'runtime').mkdir(parents=True)
            shutil.copy2(ROOT / 'scripts/check-typed-contract-structure.py',
                         root / 'scripts')
            for name in ('runtime.rs', 'domain.rs', 'sidebar.rs', 'session_sync.rs',
                         'live.rs', 'remote.rs', 'wire.rs', 'herdr_contract.rs'):
                (sources / name).write_text('')
            (sources / 'runtime' / 'events.rs').write_text(runtime_module_text)
            return subprocess.run(
                [sys.executable, 'scripts/check-typed-contract-structure.py'],
                cwd=root, capture_output=True, text=True,
                env={**os.environ, 'PATH': path} if path is not None else None,
            )

    def test_runtime_submodule_cannot_bypass_the_generated_wire_boundary(self):
        result = self.run_checker('use crate::herdr_contract::wire::session;\n')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('runtime/events.rs references generated types', result.stderr)

    def test_runtime_submodule_without_generated_types_passes(self):
        result = self.run_checker('fn project_snapshot() {}\n')
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_checker_does_not_require_rg(self):
        result = self.run_checker('fn project_snapshot() {}\n', path='')
        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == '__main__':
    unittest.main()
