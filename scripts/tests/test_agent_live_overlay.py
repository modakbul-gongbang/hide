"""Existing login copies stay private, bounded, disposable and never write back."""

import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from agent_live_check.overlay import prepare
from agent_live_check.protection import MAX_BACKUP_BYTES, ProtectionError
from agent_live_check.processes import OwnedProcesses
from agent_live_check.runtime import Runtime


class PrivateConfiguration(unittest.TestCase):
    def test_protocol_cleanup_failure_does_not_retain_credential_copies(self):
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            runtime = Runtime.__new__(Runtime)
            runtime.probe, runtime.short, runtime.state = root / "probe", root / "short", root / "state"
            for folder in (runtime.probe, runtime.short, runtime.state):
                folder.mkdir(mode=0o700)
            credential = runtime.probe / "config-fixture"
            credential.mkdir(mode=0o700)
            (credential / "auth.json").write_bytes(b"private-fixture-token")
            runtime.credential_roots = {credential}
            runtime.owner = OwnedProcesses()
            runtime.workspaces, runtime.started, runtime.servers = {"own-workspace"}, False, []
            def refused(_):
                raise RuntimeError("injected_protocol_cleanup_failure")
            runtime.close_workspace = refused
            result = runtime.close()
            self.assertFalse(result["confirmed"])
            self.assertTrue(result["credential_copies_removed"])
            self.assertFalse(credential.exists())
            self.assertTrue(result["processes_confirmed"])
            self.assertFalse(runtime.probe.exists())
            self.assertFalse(runtime.short.exists())

    def recipe(self, source="auth.json"):
        return {"id": "fixture", "overlay": {"env": {"FIXTURE_CONFIG_ROOT": "."},
                "copies": [[source, source]], "settings": ["settings.json"], "sessions": "sessions",
                "provenance": "fixture-only contract"}}

    def test_private_copy_preserves_source_and_reports_no_values(self):
        with tempfile.TemporaryDirectory() as name:
            operator, probe = Path(name) / "operator", Path(name) / "probe"
            operator.mkdir(mode=0o700)
            probe.mkdir(mode=0o700)
            source = operator / "auth.json"
            content = b'{"test-only-credential":"sensitive-fixture"}'
            source.write_bytes(content)
            result = prepare(self.recipe(), probe, operator)
            copied = probe / "config-fixture/auth.json"
            self.assertEqual(copied.read_bytes(), content)
            self.assertEqual(copied.stat().st_mode & 0o777, 0o600)
            copied.write_bytes(b"refreshed-in-private-copy")
            self.assertEqual(source.read_bytes(), content)
            self.assertNotIn("HOME", result["env"])
            self.assertNotIn("sensitive-fixture", json.dumps(result["copies"]))

    def test_alias_uncheckpointed_database_and_oversized_copy_are_refused(self):
        for kind in ("alias", "database", "oversized"):
            with self.subTest(kind=kind), tempfile.TemporaryDirectory() as name:
                operator, probe = Path(name) / "operator", Path(name) / "probe"
                operator.mkdir(mode=0o700)
                probe.mkdir(mode=0o700)
                source = operator / ("auth.db" if kind == "database" else "auth.json")
                if kind == "alias":
                    target = operator / "other.json"
                    target.write_bytes(b"other")
                    source.symlink_to(target)
                else:
                    source.write_bytes(b"fixture")
                    if kind == "database":
                        Path(str(source) + "-wal").write_bytes(b"pending")
                    else:
                        with source.open("r+b") as stream:
                            stream.truncate(MAX_BACKUP_BYTES + 1)
                with self.assertRaises(ProtectionError):
                    prepare(self.recipe(source.name), probe, operator)
                self.assertFalse((probe / "config-fixture" / source.name).exists())


if __name__ == "__main__":
    unittest.main()
