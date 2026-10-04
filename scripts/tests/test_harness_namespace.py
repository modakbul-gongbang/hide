"""Check the local-only contract with staged files in real Git fixtures."""

from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


CHECK = Path(__file__).resolve().parents[1] / "check-harness-ignore-anchor.sh"


class HarnessNamespaceTests(unittest.TestCase):
    def setUp(self):
        runs = CHECK.parent.parent / "agents" / "runs" / "privacy-tests"
        runs.mkdir(parents=True, exist_ok=True)
        temporary = tempfile.TemporaryDirectory(dir=runs)
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.git("init", "--quiet")
        (self.root / "scripts").mkdir()
        shutil.copyfile(CHECK, self.root / "scripts" / CHECK.name)
        (self.root / ".gitignore").write_text("/agents/\n")
        (self.root / "AGENTS.md").write_text(
            "The ignore file carries one anchored line, `/agents/`.\n")
        self.git("add", ".gitignore", "AGENTS.md", "scripts")

    def git(self, *args):
        return subprocess.run(["git", "-C", str(self.root), *args], check=True,
                              capture_output=True, timeout=10)

    def write(self, path, content="local fixture\n"):
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content)
        return target

    def check(self):
        return subprocess.run(["bash", str(self.root / "scripts" / CHECK.name)],
                              cwd=self.root, capture_output=True, text=True,
                              timeout=10)

    def test_local_prd_config_and_runs_are_preserved_and_untracked(self):
        paths = ("agents/prd/example/prd.md", "agents/config.json",
                 "agents/rules/INDEX.md", "agents/runs/example/receipt.json")
        targets = [self.write(path) for path in paths]
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.git("ls-files", "--", "agents").stdout, b"")
        self.assertTrue(all(target.read_text() == "local fixture\n"
                            for target in targets))

    def test_force_added_prd_config_rules_and_evidence_each_fail(self):
        for path in ("agents/prd/example/prd.md", "agents/config.json",
                     "agents/rules/INDEX.md", "agents/runs/example/receipt.json"):
            with self.subTest(path=path):
                target = self.write(path)
                self.git("add", "-f", "--", path)
                result = self.check()
                self.assertEqual(result.returncode, 1)
                self.assertIn("zero indexed paths", result.stderr)
                self.assertEqual(target.read_text(), "local fixture\n")
                # Fixture-only removal proves the gate examines the index.
                self.git("rm", "--cached", "--", path)
                self.assertEqual(self.check().returncode, 0)
                self.assertTrue(target.exists())

    def test_unanchored_ignore_is_rejected_even_for_a_tracked_subagent(self):
        self.write(".claude/agents/simplify-scout.md")
        self.git("add", "--", ".claude/agents/simplify-scout.md")
        (self.root / ".gitignore").write_text("agents/\n")
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("still matches .claude/agents/", result.stderr)

    def test_repo_subagent_can_remain_tracked(self):
        self.write(".claude/agents/simplify-scout.md")
        self.git("add", "--", ".claude/agents/simplify-scout.md")
        result = self.check()
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_missing_root_ignore_is_rejected(self):
        (self.root / ".gitignore").write_text("node_modules/\n")
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("not ignored", result.stderr)

    def test_partial_run_ignore_does_not_cover_the_namespace(self):
        (self.root / ".gitignore").write_text("/agents/runs/\n")
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("not ignored", result.stderr)

    def test_failed_collateral_query_is_not_a_success(self):
        target = self.root / "subagent-source"
        target.mkdir()
        try:
            (self.root / ".claude").symlink_to(target, target_is_directory=True)
        except OSError as error:
            self.skipTest(f"directory symlink unavailable: {error.__class__.__name__}")
        result = self.check()
        self.assertEqual(result.returncode, 1)
        self.assertIn("cannot inspect", result.stderr)
        self.assertNotIn("zero indexed paths", result.stdout)


if __name__ == "__main__":
    unittest.main()
