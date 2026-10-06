"""How `scripts/web-e2e-shard.py` deals the web end-to-end tests out to shards."""
import importlib.util
import json
from pathlib import Path
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


def run(*args):
    return subprocess.run([sys.executable, str(SCRIPT), *args], capture_output=True, text=True)


class Listing(unittest.TestCase):
    def test_reads_the_tests_playwright_lists_in_its_order(self):
        self.assertEqual(shard.listed_tests(LIST), TESTS)

    def test_a_list_without_a_test_or_with_a_repeated_one_is_refused(self):
        with self.assertRaises(ValueError):
            shard.listed_tests("Listing tests:\nTotal: 0 tests in 0 files\n")
        with self.assertRaises(ValueError):
            shard.listed_tests("  a.spec.ts:1:1 › x\n  a.spec.ts:9:1 › x\n")


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

    def test_the_committed_table_deals_the_four_shards_within_five_percent(self):
        table = json.loads((ROOT / "web/e2e/shard-durations.json").read_text(encoding="utf-8"))
        _, load = shard.split(sorted(table), table, 4)
        self.assertLessEqual(max(load), sum(load) / 4 * 1.05)
        self.assertGreater(min(load), sum(load) / 4 * 0.95)


class Command(unittest.TestCase):
    def split_files(self, total):
        with tempfile.TemporaryDirectory() as directory:
            listing = Path(directory) / "list.txt"
            listing.write_text(LIST, encoding="utf-8")
            table = Path(directory) / "table.json"
            table.write_text(json.dumps({"a.spec.ts › one": 50, "a.spec.ts › two": 30, "c.spec.ts › four": 20}), encoding="utf-8")
            results = [run("split", "--shard", f"{current}/{total}", "--table", str(table), str(listing)) for current in range(1, total + 1)]
        return results

    def test_the_shards_together_run_every_listed_test_once(self):
        results = self.split_files(2)
        for result in results:
            self.assertEqual(result.returncode, 0, result.stderr)
        lines = [line for result in results for line in result.stdout.splitlines()]
        self.assertEqual(sorted(lines), sorted(TESTS))
        self.assertIn("shard 1/2", results[0].stderr)

    def test_a_shard_with_no_test_fails_instead_of_running_none(self):
        results = self.split_files(8)
        self.assertNotEqual(results[-1].returncode, 0)
        self.assertEqual(results[-1].stdout, "")
        self.assertIn("would run no test", results[-1].stderr)

    def test_a_shard_outside_the_total_is_refused(self):
        for value in ("0/4", "5/4", "4", "a/b"):
            with self.subTest(value=value):
                self.assertNotEqual(run("split", "--shard", value, "unused").returncode, 0)


class Durations(unittest.TestCase):
    LOG = (
        "2026-10-06T00:35:07.4913637Z   ✓   1 e2e/s3.spec.ts:262:1 › editing a document @platform (11.9s)\n"
        "2026-10-06T00:35:09.0000000Z   ✓   2 e2e/a.spec.ts:10:1 › one (850ms)\n"
        "2026-10-06T00:35:09.0000000Z   ✘   3 e2e/a.spec.ts:40:1 › two (1.2s)\n"
        "2026-10-06T00:35:09.0000000Z   ✓   4 e2e/b.spec.ts:7:1 › describe › nested (1.5m)\n"
        "2026-10-06T00:35:09.0000000Z unrelated line (3.0s)\n"
    )

    def test_reads_passing_lines_without_their_tags_and_in_seconds(self):
        self.assertEqual(
            shard.durations([self.LOG]),
            {"a.spec.ts › one": 0.8, "b.spec.ts › describe › nested": 90.0, "s3.spec.ts › editing a document": 11.9},
        )

    def test_takes_the_median_across_logs(self):
        logs = [f"  ✓   1 e2e/a.spec.ts:10:1 › one ({seconds}s)\n" for seconds in (4.0, 100.0, 6.0)]
        self.assertEqual(shard.durations(logs), {"a.spec.ts › one": 6.0})

    def test_a_listed_title_matches_the_title_a_run_prints(self):
        # `--list` omits the tags a run prints, so both name a test the same way.
        listed = shard.listed_tests("  s3.spec.ts:262:1 › editing a document\n")
        self.assertEqual(listed, list(shard.durations([self.LOG]))[-1:])


if __name__ == "__main__":
    unittest.main()
