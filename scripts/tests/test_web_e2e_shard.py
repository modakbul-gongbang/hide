"""How `scripts/web-e2e-shard.py` deals the web end-to-end tests out to shards."""
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).parents[2]
SCRIPT = ROOT / "scripts/web-e2e-shard.py"
SPEC = importlib.util.spec_from_file_location("web_e2e_shard", SCRIPT)
shard = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(shard)

LIST = """Listing tests:
  a.spec.ts:10:1 › one
  a.spec.ts:40:1 › two
  b.spec.ts:7:1 › describe › nested
  c.spec.ts:3:1 › four
  c.spec.ts:30:1 › five
Total: 5 tests in 3 files
"""
TESTS = ["a.spec.ts › one", "a.spec.ts › two", "b.spec.ts › describe › nested", "c.spec.ts › four", "c.spec.ts › five"]


def run(*args, env=None):
    """The script as a process, its stdout as raw bytes: that is what a shard file holds."""
    return subprocess.run([sys.executable, str(SCRIPT), *args], capture_output=True, env=env)


class Listing(unittest.TestCase):
    def test_reads_the_tests_playwright_lists_in_its_order(self):
        self.assertEqual(shard.listed_tests(LIST), TESTS)

    def test_a_list_without_a_test_or_with_a_repeated_one_is_refused(self):
        with self.assertRaises(shard.Refused):
            shard.listed_tests("Listing tests:\nTotal: 0 tests in 0 files\n")
        with self.assertRaises(shard.Refused):
            shard.listed_tests("  a.spec.ts:1:1 › x\n  a.spec.ts:9:1 › x\n")
        self.assertEqual(shard.listed_tests("Total: 0 tests in 0 files\n", allow_none=True), [])


class Splitting(unittest.TestCase):
    def test_fills_the_lightest_shard_with_the_longest_test_first(self):
        table = {"a.spec.ts › one": 50.0, "a.spec.ts › two": 30.0, "b.spec.ts › describe › nested": 25.0,
                 "c.spec.ts › four": 20.0, "c.spec.ts › five": 5.0}
        assigned, load = shard.split(TESTS, table, 2)
        self.assertEqual(assigned, {"a.spec.ts › one": 0, "a.spec.ts › two": 1, "b.spec.ts › describe › nested": 1,
                                    "c.spec.ts › four": 0, "c.spec.ts › five": 1})
        self.assertEqual(load, [70.0, 60.0])

    def test_every_test_lands_in_exactly_one_shard_whatever_the_table_knows(self):
        for table in ({}, {"a.spec.ts › one": 9.0}, {"gone.spec.ts › x": 1.0}):
            for total in (1, 2, 4):
                with self.subTest(table=table, total=total):
                    assigned, load = shard.split(TESTS, table, total)
                    self.assertEqual(sorted(assigned), sorted(TESTS))
                    self.assertTrue(all(0 <= index < total for index in assigned.values()))
                    self.assertEqual(len(load), total)

    def test_a_test_the_table_lacks_weighs_the_tables_median(self):
        assigned, load = shard.split(["x", "y", "z"], {"x": 10.0, "y": 30.0, "k": 20.0}, 2)
        # z is new and weighs 20; y (30) opens a shard, x (10) and z (20) fill the other.
        self.assertEqual(sorted(load), [30.0, 30.0])

    def test_the_same_input_gives_the_same_shards(self):
        table = {test: 5.0 for test in TESTS}
        self.assertEqual(shard.split(TESTS, table, 3), shard.split(list(TESTS), dict(table), 3))

    def test_a_plan_reports_tests_the_table_lacks_and_a_heavy_shard_but_does_not_fail(self):
        self.assertEqual(shard.drift(["a", "b"], {"a": 5.0, "b": 5.0}, [5.0, 5.0]), [])
        notes = shard.drift(["a", "b", "c"], {"a": 5.0, "b": 5.0}, [5.0, 5.0])
        self.assertEqual(len(notes), 1)
        self.assertIn("1 of 3 listed tests are not in shard-durations.json", notes[0])
        notes = shard.drift(["a", "b"], {"a": 30.0, "b": 5.0}, [30.0, 5.0])
        self.assertIn("the heaviest shard plans 30 s against a mean of 18 s", notes[0])

    def test_the_committed_table_names_only_whole_tests_of_existing_specs(self):
        table = json.loads((ROOT / "web/e2e/shard-durations.json").read_text(encoding="utf-8"))
        self.assertGreater(len(table), 100)
        for key, seconds in table.items():
            with self.subTest(key=key):
                match = re.fullmatch(r"([\w.-]+\.spec\.ts) › (.+)", key)
                self.assertTrue(match, "not `file › title`")
                self.assertTrue((ROOT / "web/e2e" / match[1]).is_file(), "no such spec")
                self.assertNotRegex(match[2], r"@[\w-]+$|\(retry #\d+\)$", "a tag or retry mark in the title")
                self.assertGreaterEqual(seconds, 0)


class Command(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name)
        self.table = self.path / "table.json"
        self.table.write_text(json.dumps({"a.spec.ts › one": 50, "a.spec.ts › two": 30, "c.spec.ts › four": 20}), encoding="utf-8")

    def file(self, name, text):
        path = self.path / name
        path.write_text(text, encoding="utf-8")
        return str(path)

    def split(self, current, total, listing=LIST, env=None):
        return run("split", "--shard", f"{current}/{total}", "--table", str(self.table), self.file("list.txt", listing), env=env)

    def test_the_shards_together_run_every_listed_test_once(self):
        results = [self.split(current, 2) for current in (1, 2)]
        for result in results:
            self.assertEqual(result.returncode, 0, result.stderr)
        lines = [line for result in results for line in result.stdout.decode("utf-8").splitlines()]
        self.assertEqual(sorted(lines), sorted(TESTS))
        self.assertIn("shard 1/2", results[0].stderr.decode())

    def test_the_shard_file_is_utf8_whatever_encoding_the_runner_gives_stdout(self):
        # A Windows runner's stdout is cp1252, which writes `›` as the byte
        # 0x9b; Playwright reads the file as UTF-8 and would match no test.
        env = {**os.environ, "PYTHONIOENCODING": "cp1252", "PYTHONUTF8": "0"}
        result = self.split(1, 1, env=env)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "".join(f"{test}\n" for test in TESTS).encode("utf-8"))
        self.assertIn("›".encode("utf-8"), result.stdout)
        self.assertNotIn(b"\r", result.stdout)

    def test_fewer_tests_than_shards_leaves_the_extra_shards_an_empty_file(self):
        one = "  a.spec.ts:10:1 › one\n"
        first, second = self.split(1, 4, listing=one), self.split(2, 4, listing=one)
        self.assertEqual((first.returncode, first.stdout), (0, b"a.spec.ts \xe2\x80\xba one\n"))
        self.assertEqual((second.returncode, second.stdout), (0, b""))

    def test_a_problem_with_an_input_is_one_line_not_a_traceback(self):
        for result in (
            self.split(1, 2, listing="Listing tests:\nTotal: 0 tests in 0 files\n"),
            self.split(1, 2, listing="  a.spec.ts:1:1 › x\n  a.spec.ts:9:1 › x\n"),
            run("split", "--shard", "1/2", "--table", str(self.path / "gone.json"), self.file("l.txt", LIST)),
            run("split", "--shard", "1/2", "--table", self.file("bad.json", "{"), self.file("l.txt", LIST)),
            run("split", "--shard", "1/2", "--table", self.file("bad2.json", '{"a.spec.ts › one": "slow"}'), self.file("l.txt", LIST)),
        ):
            with self.subTest(stderr=result.stderr):
                self.assertEqual(result.returncode, 1)
                self.assertNotIn(b"Traceback", result.stderr)
                self.assertTrue(result.stderr.startswith(b"web-e2e-shard: "))
                self.assertEqual(len(result.stderr.splitlines()), 1)

    def test_a_shard_outside_the_total_is_refused(self):
        for value in ("0/4", "5/4", "4", "a/b"):
            with self.subTest(value=value):
                self.assertNotEqual(run("split", "--shard", value, "unused").returncode, 0)

    def check(self, current, total, listed):
        planned = self.split(current, total)
        self.assertEqual(planned.returncode, 0, planned.stderr)
        return run("check", "--shard", f"{current}/{total}", "--table", str(self.table),
                   self.file("list.txt", LIST), self.file("shard-list.txt", listed))

    def test_check_passes_when_playwright_lists_the_planned_tests(self):
        planned = self.split(1, 2).stdout.decode("utf-8").splitlines()
        listed = "Listing tests:\n" + "".join(f"  {test.replace(' › ', ':1:1 › ', 1)}\n" for test in planned) + "Total\n"
        result = self.check(1, 2, listed)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_check_fails_when_the_shard_file_matches_nothing_or_the_wrong_tests(self):
        for listed, words in (
            ("Listing tests:\nTotal: 0 tests in 0 files\n", "would run 0 tests, not the"),
            ("  a.spec.ts:10:1 › one\n  c.spec.ts:30:1 › five\n  b.spec.ts:7:1 › describe › nested\n  c.spec.ts:3:1 › four\n", "unplanned tests are selected"),
        ):
            with self.subTest(words=words):
                result = self.check(1, 2, listed)
                self.assertEqual(result.returncode, 1)
                self.assertIn(words, result.stderr.decode())
                self.assertNotIn(b"Traceback", result.stderr)

    def test_check_passes_for_a_shard_dealt_no_test(self):
        one = "  a.spec.ts:10:1 › one\n"
        result = run("check", "--shard", "2/4", "--table", str(self.table), self.file("list.txt", one), self.file("shard-list.txt", "Total: 0 tests in 0 files\n"))
        self.assertEqual(result.returncode, 0, result.stderr)


class Durations(unittest.TestCase):
    LOG = (
        "2026-10-06T00:35:07.4913637Z   ✓   1 e2e/s3.spec.ts:262:1 › editing a document @platform (11.9s)\n"
        "2026-10-06T00:35:09.0000000Z   ✓   2 e2e/a.spec.ts:10:1 › one (850ms)\n"
        "2026-10-06T00:35:09.0000000Z   ✘   3 e2e/a.spec.ts:40:1 › two (1.2s)\n"
        "2026-10-06T00:35:09.0000000Z   ✓   4 e2e/b.spec.ts:7:1 › describe › nested (1.5m)\n"
        "2026-10-06T00:35:09.0000000Z unrelated line (3.0s)\n"
    )
    LISTED = ["a.spec.ts › one", "b.spec.ts › describe › nested", "s3.spec.ts › editing a document", "z.spec.ts › never ran"]

    def test_reads_passing_lines_without_their_tags_and_in_seconds(self):
        self.assertEqual(
            shard.durations([self.LOG], self.LISTED),
            {"a.spec.ts › one": 0.8, "b.spec.ts › describe › nested": 90.0, "s3.spec.ts › editing a document": 11.9},
        )

    def test_a_passing_retry_is_the_test_not_a_test_named_after_the_retry(self):
        log = (
            "  ✓  14 e2e/a.spec.ts:10:1 › one (retry #1) (4.2s)\n"
            "  ✓  15 e2e/a.spec.ts:40:1 › two @platform (retry #2) (6.0s)\n"
        )
        self.assertEqual(shard.durations([log], ["a.spec.ts › one", "a.spec.ts › two"]), {"a.spec.ts › one": 4.2, "a.spec.ts › two": 6.0})

    def test_a_windows_runners_ok_mark_counts_as_a_pass(self):
        log = "  ok   1 e2e/a.spec.ts:10:1 › one (2.0s)\n  ok   2 e2e/a.spec.ts:40:1 › look ok (3.0s)\n"
        self.assertEqual(shard.durations([log], ["a.spec.ts › one", "a.spec.ts › look ok"]), {"a.spec.ts › one": 2.0, "a.spec.ts › look ok": 3.0})

    def test_keeps_only_the_tests_the_list_names(self):
        log = "  ✓   1 e2e/a.spec.ts:10:1 › one (4.0s)\n  ✓   2 e2e/a.spec.ts:40:1 › renamed away (9.0s)\n"
        self.assertEqual(shard.durations([log], ["a.spec.ts › one"]), {"a.spec.ts › one": 4.0})

    def test_takes_the_median_across_logs(self):
        logs = [f"  ✓   1 e2e/a.spec.ts:10:1 › one ({seconds}s)\n" for seconds in (4.0, 100.0, 6.0)]
        self.assertEqual(shard.durations(logs, ["a.spec.ts › one"]), {"a.spec.ts › one": 6.0})

    def test_a_listed_title_matches_the_title_a_run_prints(self):
        # `--list` omits the tags a run prints, so both name a test the same way.
        listed = shard.listed_tests("  s3.spec.ts:262:1 › editing a document\n")
        self.assertEqual(list(shard.durations([self.LOG], listed)), listed)

    def test_the_command_writes_utf8_and_names_the_tests_it_found_no_line_for(self):
        with tempfile.TemporaryDirectory() as directory:
            listing = Path(directory) / "list.txt"
            listing.write_text("  a.spec.ts:10:1 › one\n  a.spec.ts:40:1 › two\n", encoding="utf-8")
            log = Path(directory) / "job.log"
            log.write_text("  ✓   1 e2e/a.spec.ts:10:1 › one (4.0s)\n", encoding="utf-8")
            env = {**os.environ, "PYTHONIOENCODING": "cp1252", "PYTHONUTF8": "0"}
            result = run("durations", "--list", str(listing), str(log), env=env)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout.decode("utf-8")), {"a.spec.ts › one": 4.0})
        self.assertIn("1 of 2 listed tests have no passing line", result.stderr.decode())


if __name__ == "__main__":
    unittest.main()
