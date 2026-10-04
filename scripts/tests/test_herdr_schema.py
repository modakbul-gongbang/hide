"""Run the real checker against a private executable's schema/version outputs."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class SchemaCheck(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "scripts").mkdir()
        (self.root / "contracts").mkdir()
        shutil.copyfile(ROOT / "scripts/check-herdr-schema.py", self.root / "scripts/check-herdr-schema.py")
        self.schema = {"protocol": 7, "methods": {"ping": {"name": "pong"}}}
        (self.root / "contracts/herdr-api.schema.json").write_text(json.dumps(self.schema))
        (self.root / "contracts/herdr-bundle.json").write_text(json.dumps({"version": "fixture-version"}))
        self.binary = self.root / "herdr"
        self.binary.write_text("#!/usr/bin/env python3\nimport json, os, sys\n"
                               "if any(k.startswith('HERDR_') for k in os.environ): sys.exit(41)\n"
                               "if os.environ.get('CLI_FAIL'): sys.exit(42)\n"
                               "if sys.argv[1:] == ['--version']: print('herdr '+os.environ['CLI_VERSION'])\n"
                               "elif sys.argv[1:] == ['api', 'schema', '--json']: print(os.environ['CLI_SCHEMA'])\n"
                               "else: sys.exit(43)\n")
        self.binary.chmod(0o755)
        self.env = {**os.environ, "CLI_SCHEMA": json.dumps(self.schema), "CLI_VERSION": "fixture-version",
                    "HERDR_SOCKET_PATH": "operator", "HERDR_ENV": "1"}

    def check(self, **env):
        return subprocess.run([sys.executable, str(self.root / "scripts/check-herdr-schema.py"),
                               "--herdr-bin", str(self.binary)], env={**self.env, **env},
                              capture_output=True, text=True, timeout=10)

    def test_equal_schema_with_different_json_order_passes_without_server(self):
        result = self.check(CLI_SCHEMA='{"methods":{"ping":{"name":"pong"}},"protocol":7}')
        self.assertEqual(result.returncode, 0, result.stderr)
        record = json.loads(result.stdout)
        self.assertEqual((record["scope"], record["version"], record["protocol"]), ("schema-only", "fixture-version", 7))
        self.assertEqual(len(record["schema_sha256"]), 64)

    def test_protocol_schema_version_and_process_failure_each_block(self):
        for env in [{"CLI_SCHEMA": '{"protocol":8}'}, {"CLI_SCHEMA": json.dumps({**self.schema, "extra": True})},
                    {"CLI_SCHEMA": json.dumps({**self.schema, "protocol": 7.0})}, {"CLI_SCHEMA": "invalid"},
                    {"CLI_VERSION": "different"}, {"CLI_FAIL": "1"}]:
            with self.subTest(env=env):
                result = self.check(**env)
                self.assertEqual(result.returncode, 1)
                self.assertIn("error:", result.stderr)
                self.assertEqual(result.stdout, "")


if __name__ == "__main__":
    unittest.main()
