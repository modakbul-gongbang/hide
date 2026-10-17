"""The operator files' trust entries for a run's paths; no agent runs."""

import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from agent_journey.local import operator_trust
from agent_live_check.protection import ProtectionError


class OperatorTrust(unittest.TestCase):
    def setUp(self):
        folder = tempfile.TemporaryDirectory()
        self.addCleanup(folder.cleanup)
        self.operator = Path(folder.name) / "home"
        self.run = Path(folder.name) / "run"
        (self.operator / ".codex").mkdir(parents=True)
        self.run.mkdir()

    def test_only_entries_for_the_runs_paths_are_named(self):
        worktree = self.run / "probe/worktrees/codex"
        (self.operator / ".codex/config.toml").write_text(
            f'[projects."/elsewhere"]\ntrust_level = "trusted"\n[projects.{json.dumps(str(worktree))}]\ntrust_level = "trusted"\n')
        (self.operator / ".claude.json").write_text(json.dumps({"projects": {
            str(worktree): {"hasTrustDialogAccepted": True}, str(self.run / "a"): {"allowedTools": []},
            "/elsewhere": {"hasTrustDialogAccepted": True}}}))
        found = operator_trust(self.operator, self.run)
        self.assertEqual(found[str(self.operator / ".codex/config.toml")], [f"projects.{json.dumps(str(worktree))}"])
        self.assertEqual(found[str(self.operator / ".claude.json")],
                         [f"projects.{json.dumps(str(worktree))}.hasTrustDialogAccepted"])

    def test_a_file_that_cannot_be_read_fails_and_a_missing_one_is_empty(self):
        self.assertEqual(operator_trust(self.operator, self.run), {})
        (self.operator / ".codex/config.toml").write_text("[projects\n")
        with self.assertRaisesRegex(ProtectionError, "operator_codex_config_unreadable"):
            operator_trust(self.operator, self.run)


if __name__ == "__main__":
    unittest.main()
