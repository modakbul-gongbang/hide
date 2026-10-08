"""What `scripts/premerge-check.py` lets merge into main without another `verify`.

CONTRIBUTING.md, "Merging into main", is the requirement: a green pull request
behind main merges when nothing conflicts and the fast checks pass on the
merge result itself, and the latest `verify` run on its head decides whether
it is green.
"""
import importlib.util
import io
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location("premerge", Path(__file__).parents[1] / "premerge-check.py")
premerge = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(premerge)
ROOT = Path(__file__).parents[2]

# Every name `uses.txt` lists must be one `exports.txt` declares: two changes
# can each pass it and fail it together, as an import and a removed export do.
SYMBOLS = ("symbols", ".", ["sh", "-c", 'for s in $(cat uses.txt 2>/dev/null); do grep -qx "$s" exports.txt || exit 1; done'])
NEVER = ("never runs", ".", ["false"])


class MergeResult(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.repo = Path(temporary.name) / "repo"
        self.worktrees = Path(temporary.name) / "repo.worktrees"
        isolated = mock.patch.dict(os.environ, {"GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"})
        isolated.start()
        self.addCleanup(isolated.stop)
        self.git("init", "--quiet", "--initial-branch=main", str(self.repo), cwd=temporary.name)
        self.git("config", "user.name", "fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.base = self.commit("main", {"exports.txt": "foo\nbar\n"})

    def git(self, *args, cwd=None):
        return subprocess.run(["git", *args], cwd=cwd or self.repo, check=True,
                              capture_output=True, text=True).stdout.strip()

    def commit(self, branch, files, start=None):
        if start:
            self.git("checkout", "--quiet", "-B", branch, start)
        for name, text in files.items():
            (self.repo / name).write_text(text)
        self.git("add", "--all")
        self.git("commit", "--quiet", "-m", f"{branch}: {', '.join(files)}")
        return self.git("rev-parse", "HEAD")

    def check(self, main, head, checks=(SYMBOLS,)):
        out = io.StringIO()
        premerge.check_merge(main, head, checks, self.worktrees, "pr1", repo=self.repo, out=out)
        return out.getvalue()

    def assert_no_worktree_left(self):
        self.assertEqual(self.git("worktree", "list", "--porcelain").count("worktree "), 1)
        self.assertEqual(list(self.worktrees.glob("*")), [])

    def test_a_conflict_with_main_refuses_and_names_the_file(self):
        main = self.commit("main", {"exports.txt": "foo\nbar\nbaz\n"})
        head = self.commit("pr", {"exports.txt": "foo\nbar\nqux\n"}, start=self.base)
        with self.assertRaisesRegex(premerge.Refused, "conflicts with main in exports.txt"):
            self.check(main, head)
        self.assert_no_worktree_left()

    def test_two_changes_that_each_pass_and_fail_together_are_refused(self):
        main = self.commit("main", {"exports.txt": "bar\n"})
        head = self.commit("pr", {"uses.txt": "foo\n"}, start=self.base)
        for alone in (main, head):
            self.git("checkout", "--quiet", alone)
            subprocess.run(SYMBOLS[2], cwd=self.repo, check=True)
        with self.assertRaisesRegex(premerge.Refused, "symbols fails on the merge with main"):
            self.check(main, head)
        self.assert_no_worktree_left()

    def test_a_clean_merge_that_passes_may_merge(self):
        main = self.commit("main", {"exports.txt": "foo\nbar\nbaz\n"})
        head = self.commit("pr", {"uses.txt": "bar\n"}, start=self.base)
        self.assertIn("ok  symbols", self.check(main, head))
        self.assert_no_worktree_left()

    def test_a_branch_that_contains_main_runs_no_check(self):
        main = self.commit("main", {"exports.txt": "bar\n"})
        head = self.commit("pr", {"uses.txt": "bar\n"}, start=main)
        self.assertIn("already ran on this merge", self.check(main, head, checks=(NEVER,)))
        self.assert_no_worktree_left()


def run(run_id, created_at, status="completed", conclusion="success", name="verify"):
    return {"id": run_id, "name": name, "created_at": created_at, "status": status,
            "conclusion": conclusion, "html_url": f"https://example.invalid/runs/{run_id}"}


class VerifyVerdict(unittest.TestCase):
    def test_the_latest_verify_run_on_the_head_decides(self):
        passed_then_failed = [run(1, "2026-10-07T01:00:00Z"), run(2, "2026-10-07T02:00:00Z", conclusion="failure")]
        with self.assertRaisesRegex(premerge.Refused, "ended failure"):
            premerge.require_verify_passed(passed_then_failed)
        failed_then_passed = [run(1, "2026-10-07T01:00:00Z", conclusion="failure"), run(2, "2026-10-07T02:00:00Z")]
        self.assertEqual(premerge.require_verify_passed(failed_then_passed), "https://example.invalid/runs/2")

    def test_a_running_or_missing_verify_refuses(self):
        with self.assertRaisesRegex(premerge.Refused, "still running"):
            premerge.require_verify_passed([run(1, "2026-10-07T01:00:00Z", status="in_progress", conclusion=None)])
        with self.assertRaisesRegex(premerge.Refused, "No `verify` run"):
            premerge.require_verify_passed([run(1, "2026-10-07T01:00:00Z", name="Design contract")])


class PolicyLaneParity(unittest.TestCase):
    def test_every_structural_check_of_the_policy_lane_runs_before_a_merge(self):
        workflow = (ROOT / ".github" / "workflows" / "pr.yml").read_text()
        step = workflow.split("name: Structural checks", 1)[1].split("\n\n", 1)[0]
        lane = [line.strip() for line in step.splitlines() if line.strip().startswith(("bash ", "zsh ", "python3 "))]
        self.assertTrue(lane)
        checked = {" ".join(command) for _, _, command in premerge.CHECKS}
        self.assertEqual([command for command in lane if command not in checked], [])


if __name__ == "__main__":
    unittest.main()
