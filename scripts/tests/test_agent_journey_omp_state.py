"""The journey removes only the omp state it can prove is its own: a wrong
hash leaves its daemon folders in the operator's ~/.omp, or names another's."""

from pathlib import Path
import sys
import tempfile
import time
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from agent_journey import omp_state


class Hash(unittest.TestCase):
    def test_the_hash_is_the_name_omp_gives_a_projects_daemon_folder(self):
        # Folder names omp 18.7.0 made on 2026-10-18 for these two project folders.
        made = {
            "/private/tmp/claude-501/-Users-hoyeonlee--herdr-worktrees-herdr-ide-feat-agent-journey-agents/"
            "2fdf6531-7935-446d-ac67-66a5eb986bd4/scratchpad/ompprobe/omp/repo": "64f3f01c40f0d438",
            "/private/tmp/claude-501/-Users-hoyeonlee--herdr-worktrees-herdr-ide-feat-agent-journey-agents/"
            "2fdf6531-7935-446d-ac67-66a5eb986bd4/scratchpad/probe2/omp/repo": "41f5143888cd43a9",
        }
        for path, name in made.items():
            with self.subTest(path=path):
                self.assertEqual(f"{omp_state.wyhash(path.encode()):016x}", name)

    def test_every_length_branch_runs(self):
        for size in (0, 3, 4, 16, 17, 48, 49, 97):
            with self.subTest(size=size):
                self.assertLess(omp_state.wyhash(b"x" * size), 1 << 64)


class TakeBack(unittest.TestCase):
    def test_only_the_runs_own_state_is_removed(self):
        with tempfile.TemporaryDirectory() as scratch:
            operator, run = Path(scratch) / "home", Path(scratch) / "run"
            project, other = run / "probe/worktrees/omp", Path(scratch) / "operator-project"
            for path in (project, other, operator / ".omp/run/session-owners", operator / ".omp/logs"):
                path.mkdir(parents=True)
            ours, theirs = omp_state.daemon_folder(operator, project), omp_state.daemon_folder(operator, other)
            for folder in (ours, theirs):
                (folder / "clients").mkdir(parents=True)
            owners = operator / ".omp/run/session-owners"
            (owners / "ours-1.lock").write_text("")
            (owners / "theirs-2.lock").write_text("")
            logs = operator / ".omp/logs"
            (logs / "omp.2026-10-18.1.log").write_text(f"cwd {run.resolve()}/probe/worktrees/omp\n")
            (logs / "omp.2026-10-18.2.log").write_text(f"cwd {other}\n")
            (logs / "http-400-requests").mkdir()
            (logs / "http-400-requests/1-a.json").write_text("{}")
            state = omp_state.take_back(operator, [project], ["ours-1"], time.time() - 60)
            self.assertEqual(sorted(Path(path).name for path in state["removed"]), sorted([ours.name, "ours-1.lock"]))
            self.assertTrue(theirs.is_dir())
            self.assertTrue((owners / "theirs-2.lock").is_file())
            # A log or a dump names only a pid or a time: listed, never removed.
            self.assertEqual(len(state["logs_left"]), 3)
            self.assertTrue((logs / "http-400-requests/1-a.json").is_file())
            self.assertTrue((logs / "omp.2026-10-18.1.log").is_file())

    def test_another_omp_running_is_a_reason_not_to_start(self):
        self.assertIsNone(omp_state.busy_refusal([]))
        self.assertIn("pid 41", omp_state.busy_refusal([41]))


if __name__ == "__main__":
    unittest.main()
