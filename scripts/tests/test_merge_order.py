"""What `scripts/merge-order.py` says about the open pull requests.

CONTRIBUTING.md is the requirement. "Starting work beside open pull requests":
a conflict is the one two pull requests meet after both merge with main, it
touches code or only documents and design files (`*.md`, `*.pen`, `site/`),
and a code conflict stacks the later one on the one green first, unless that
one is risky or its stack is three layers tall. "Merging into main": the green
pull request that breaks the fewest open ones merges first, red ones uncounted,
a tie going to the stack with more green layers and then to the one green longest.

Each pull request is a branch of a fixture repository; only what GitHub would
answer, the `verify` state, is set by hand.
"""
import importlib.util
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).parents[1]))
SPEC = importlib.util.spec_from_file_location("merge_order", Path(__file__).parents[1] / "merge-order.py")
merge_order = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(merge_order)

START = {"a.rs": "one\n", "b.rs": "one\n", "notes.md": "one\n", "guide.md": "one\n", "Cargo.lock": "one\n"}


class Fixture(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.repo = Path(temporary.name)
        isolated = mock.patch.dict(os.environ, {"GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"})
        isolated.start()
        self.addCleanup(isolated.stop)
        self.git("init", "--quiet", "--initial-branch=main")
        self.git("config", "user.name", "fixture")
        self.git("config", "user.email", "fixture@example.invalid")
        self.start = self.commit("main", START)
        self.main = self.start
        self.pulls = []

    def git(self, *args):
        return subprocess.run(["git", *args], cwd=self.repo, check=True, capture_output=True, text=True).stdout.strip()

    def commit(self, branch, files, start=None):
        if start:
            self.git("checkout", "--quiet", "-B", branch, start)
        for name, text in files.items():
            (self.repo / name).parent.mkdir(parents=True, exist_ok=True)
            (self.repo / name).write_text(text)
        self.git("add", "--all")
        self.git("commit", "--quiet", "-m", f"{branch}: {', '.join(files)}")
        return self.git("rev-parse", "HEAD")

    def advance_main(self, files):
        self.main = self.commit("main", files, start=self.main)

    def pull(self, number, files, on=None, verify="green", green_at="2026-10-16T01:00:00Z"):
        """A pull request from `main` as the fixture started, or stacked on the pull request `on`."""
        branch = f"feat/{number}"
        head = self.commit(branch, files, start=on.head if on else self.start)
        pull = merge_order.Pull(number=number, branch=branch, base=on.branch if on else "main", head=head,
                                verify=verify, green_at=green_at if verify == "green" else None)
        self.pulls.append(pull)
        return pull

    def read(self, target=None):
        merge_order.link_stacks(self.pulls)
        for pull in self.pulls:
            pull.risky = merge_order.risk(pull, self.main, self.repo)
        merge_order.merge_with_main(self.main, self.pulls, self.repo)
        return merge_order.pair_conflicts(self.pulls, self.repo, target=target)


class Conflicts(Fixture):
    def test_each_pair_is_code_documents_only_or_nothing_and_a_stack_line_is_not_compared(self):
        p1 = self.pull(1, {"a.rs": "p1\n"})
        p2 = self.pull(2, {"a.rs": "p2\n", "notes.md": "p2\n"})
        p3 = self.pull(3, {"notes.md": "p3\n"})
        p4 = self.pull(4, {"a.rs": "p4\n"}, on=p1)
        p5 = self.pull(5, {"b.rs": "p5\n"})
        p6 = self.pull(6, {"a.rs": "p6\n", "guide.md": "p6\n"})
        # #1 moves on after #4 branched from it; #4 takes that by merging #1, not as a conflict.
        p1.head = self.commit(p1.branch, {"a.rs": "p1 again\n"}, start=p1.head)
        self.advance_main({"b.rs": "main\n", "guide.md": "main\n"})
        pairs = self.read()

        def between(a, b):
            return merge_order.kind(pairs[(a, b)]) if (a, b) in pairs else None

        self.assertEqual(between(p1, p2), "code")
        # One conflicting code path makes the pair code, whatever documents conflict beside it.
        self.assertEqual(sorted(pairs[(p2, p1)]), ["a.rs"])
        self.assertEqual(between(p2, p3), "docs")
        self.assertIsNone(between(p1, p3))
        # #4 is a layer on #1: what they disagree on is the stack's own business.
        self.assertIsNone(between(p4, p1))
        self.assertEqual(between(p4, p2), "code")
        # #5 conflicts with main itself in code, so it is reported once there and left out of the pairs.
        self.assertEqual(p5.conflicts_with_main, ("b.rs",))
        self.assertFalse(any(p5 in pair for pair in pairs))
        # #6 conflicts with main in a document only, so its code is still compared.
        self.assertEqual(p6.conflicts_with_main, ("guide.md",))
        self.assertEqual(between(p6, p1), "code")

    def test_a_conflict_that_mixes_code_and_documents_counts_as_code(self):
        p1 = self.pull(1, {"a.rs": "p1\n", "notes.md": "p1\n"})
        p2 = self.pull(2, {"a.rs": "p2\n", "notes.md": "p2\n"})
        pairs = self.read()
        self.assertEqual(sorted(pairs[(p1, p2)]), ["a.rs", "notes.md"])
        self.assertEqual(merge_order.kind(pairs[(p1, p2)]), "code")


class MergeOrder(Fixture):
    def test_the_green_pull_request_that_breaks_the_fewest_open_ones_merges_first(self):
        # #1, green longest, breaks green #2 and running #3; #2 breaks #1 alone.
        self.pull(1, {"a.rs": "p1\n", "e.rs": "p1\n"}, green_at="2026-10-16T01:00:00Z")
        self.pull(2, {"a.rs": "p2\n", "notes.md": "p2\n"}, green_at="2026-10-16T02:00:00Z")
        self.pull(3, {"e.rs": "p3\n"}, verify="running")
        # #4 conflicts in code only with red #5, and in documents only with #2: it breaks nothing.
        self.pull(4, {"b.rs": "p4\n", "notes.md": "p4\n"}, green_at="2026-10-16T03:00:00Z")
        self.pull(5, {"b.rs": "p5\n"}, verify="red")
        # #6 breaks nothing and has a green layer on it, so its stack merges more at once.
        p6 = self.pull(6, {"c.rs": "p6\n"}, green_at="2026-10-16T04:00:00Z")
        self.pull(7, {"c.rs": "p7\n"}, on=p6, green_at="2026-10-16T05:00:00Z")
        # #8 is green but conflicts with main, so it is not in the order.
        self.pull(8, {"Cargo.lock": "p8\n"}, green_at="2026-10-16T00:00:00Z")
        self.advance_main({"Cargo.lock": "main\n"})
        pairs = self.read()
        self.assertEqual([pull.number for pull in merge_order.merge_order(self.pulls, pairs)], [6, 4, 2, 1])

    def test_between_two_that_break_only_each_other_the_one_green_longest_goes_first(self):
        self.pull(1, {"a.rs": "p1\n"}, green_at="2026-10-16T02:00:00Z")
        self.pull(2, {"a.rs": "p2\n"}, green_at="2026-10-16T01:00:00Z")
        pairs = self.read()
        self.assertEqual([pull.number for pull in merge_order.merge_order(self.pulls, pairs)], [2, 1])


class Placement(Fixture):
    def place(self, target):
        pairs = self.read(target=target)
        return merge_order.placement(target, self.pulls, pairs)

    def steps(self, answer):
        return {other.number: step for other, _, step, _ in answer["code"]}

    def test_a_pull_request_stacks_on_the_one_green_first_with_the_commands_to_do_it(self):
        a = self.pull(1, {"a.rs": "a\n"})
        target = self.pull(2, {"a.rs": "t\n"}, verify="running")
        answer = self.place(target)
        self.assertEqual(self.steps(answer), {1: "stack"})
        self.assertIn(f"gh pr edit 2 --base {a.branch}; git fetch origin {a.branch} && git merge origin/{a.branch}",
                      answer["do"])

    def test_the_one_green_first_stays_below(self):
        self.pull(1, {"a.rs": "a\n"}, green_at="2026-10-16T02:00:00Z")
        target = self.pull(2, {"a.rs": "t\n"}, green_at="2026-10-16T01:00:00Z")
        answer = self.place(target)
        self.assertEqual(self.steps(answer), {1: "below"})
        self.assertTrue(answer["do"].startswith("Stay as you are."))

    def test_a_risky_pull_request_is_never_stacked_on(self):
        for number, risky in ((1, {"Cargo.lock": "a\n"}), (3, {"contracts/api.json": "a\n"}),
                              (5, {merge_order.PROTOCOL_FILE: "pub const PROTOCOL_VERSION: u32 = 2;\n"})):
            with self.subTest(risky=list(risky)):
                self.pulls = []
                self.pull(number, {"a.rs": "a\n", **risky})
                target = self.pull(number + 1, {"a.rs": "t\n"}, verify="running")
                answer = self.place(target)
                self.assertEqual(self.steps(answer), {number: "alone"})
                self.assertNotIn("gh pr edit", answer["do"])

    def test_a_stack_three_layers_tall_takes_no_new_layer(self):
        one = self.pull(1, {"c.rs": "1\n"})
        two = self.pull(2, {"d.rs": "2\n"}, on=one)
        three = self.pull(3, {"a.rs": "3\n"}, on=two)
        target = self.pull(4, {"a.rs": "t\n"}, verify="running")
        self.assertEqual(self.steps(self.place(target)), {3: "full"})
        self.pulls.remove(three)
        self.pull(5, {"a.rs": "5\n"}, on=one)
        self.assertEqual(self.steps(self.place(target)), {5: "stack"})

    def test_a_documents_only_conflict_stacks_nothing(self):
        self.pull(1, {"notes.md": "a\n", "site/index.html": "a\n"})
        target = self.pull(2, {"notes.md": "t\n", "site/index.html": "t\n"}, verify="running")
        answer = self.place(target)
        self.assertEqual(answer["code"], [])
        self.assertEqual([other.number for other, _ in answer["docs"]], [1])
        self.assertTrue(answer["do"].startswith("Stay as you are"))


if __name__ == "__main__":
    unittest.main()
