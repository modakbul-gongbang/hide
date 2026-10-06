#!/usr/bin/env python3
"""Deal the web end-to-end tests out to CI shards by how long each one takes.

Playwright's own `--shard` cuts the tests in file order into runs of equal
count, and the suite's tests range from 1 second to a minute, so one shard held
11 minutes of work while another held 6. `split` reads the tests Playwright
lists (`playwright test --list`) and `web/e2e/shard-durations.json`, gives each
test its recorded seconds (a test the table does not know gets the table's
median), and fills the shards longest test first, each into the shard with the
least work so far. The same list and table always give the same shards, and
every listed test lands in exactly one shard. The output is a file for
`playwright test --test-list`.

`durations` writes the table from the job logs of earlier runs: the median
seconds of each test's passing line in the list reporter's output. docs/TESTING.md,
"Balancing the web e2e shards", says when and how to refresh it.
"""
import argparse
import json
import re
import statistics
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TABLE = ROOT / "web/e2e/shard-durations.json"

# `playwright test --list`: "  file.spec.ts:45:1 › [describe › ]title"; a run's
# list reporter adds the `e2e/` directory and a leading mark, number and
# duration, and a test's tags after its title:
# "✓   1 e2e/file.spec.ts:45:1 › title @platform (4.9s)".
LISTED = re.compile(r"^\s+(?:e2e/)?(?P<file>[^\s:]+\.spec\.ts):\d+:\d+ › (?P<title>.+?)\s*$")
PASSED = re.compile(r"✓\s+\d+\s+(?:e2e/)?(?P<file>[^\s:]+\.spec\.ts):\d+:\d+ › (?P<title>.+?)(?: @[\w-]+)* \((?P<value>\d+(?:\.\d+)?)(?P<unit>ms|s|m)\)\s*$")
UNITS = {"ms": 0.001, "s": 1.0, "m": 60.0}


def listed_tests(text):
    """The tests `playwright test --list` printed, as `file › title`, in its order."""
    tests = []
    for line in text.splitlines():
        match = LISTED.match(line)
        if match:
            tests.append(f"{match['file']} › {match['title']}")
    if not tests:
        raise ValueError("the list names no test")
    if len(set(tests)) != len(tests):
        raise ValueError("two listed tests share a file and title")
    return tests


def split(tests, table, shards):
    """Shard index (0-based) for each test: longest first into the lightest shard."""
    if shards < 1:
        raise ValueError("a run has at least one shard")
    unknown = statistics.median(table.values()) if table else 1.0
    weight = {test: float(table.get(test, unknown)) for test in tests}
    load = [0.0] * shards
    assigned = {}
    for test in sorted(tests, key=lambda test: (-weight[test], test)):
        lightest = min(range(shards), key=lambda index: (load[index], index))
        assigned[test] = lightest
        load[lightest] += weight[test]
    return assigned, load


def parse_shard(value):
    match = re.fullmatch(r"(\d+)/(\d+)", value)
    if not match or not 1 <= int(match[1]) <= int(match[2]):
        raise argparse.ArgumentTypeError(f"expected CURRENT/TOTAL, got {value!r}")
    return int(match[1]), int(match[2])


def durations(logs):
    """Median seconds per test across the passing lines of the given logs."""
    seen = {}
    for text in logs:
        for line in text.splitlines():
            match = PASSED.search(line)
            if match:
                key = f"{match['file']} › {match['title']}"
                seen.setdefault(key, []).append(float(match["value"]) * UNITS[match["unit"]])
    return {key: round(statistics.median(values), 1) for key, values in sorted(seen.items())}


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    commands = parser.add_subparsers(dest="command", required=True)
    cut = commands.add_parser("split", help="print the tests of one shard, one per line")
    cut.add_argument("--shard", type=parse_shard, required=True, help="CURRENT/TOTAL, one-based")
    cut.add_argument("--table", type=Path, default=TABLE)
    cut.add_argument("list", type=Path, help="the output of `playwright test --list`")
    refresh = commands.add_parser("durations", help="print the duration table from job logs")
    refresh.add_argument("logs", type=Path, nargs="+")
    arguments = parser.parse_args(argv)

    if arguments.command == "durations":
        table = durations(path.read_text(encoding="utf-8", errors="replace") for path in arguments.logs)
        if not table:
            raise SystemExit("no passing test line in the logs")
        json.dump(table, sys.stdout, ensure_ascii=False, indent=2)
        sys.stdout.write("\n")
        return 0

    current, total = arguments.shard
    tests = listed_tests(arguments.list.read_text(encoding="utf-8"))
    table = json.loads(arguments.table.read_text(encoding="utf-8"))
    assigned, load = split(tests, table, total)
    mine = [test for test in tests if assigned[test] == current - 1]
    if not mine:
        raise SystemExit(f"shard {current}/{total} would run no test; {len(tests)} tests listed")
    print(
        f"shard {current}/{total}: {len(mine)} of {len(tests)} tests, "
        f"{load[current - 1]:.0f} s of {sum(load):.0f} s planned; every shard: "
        + ", ".join(f"{seconds:.0f} s" for seconds in load),
        file=sys.stderr,
    )
    sys.stdout.write("".join(f"{test}\n" for test in mine))
    return 0


if __name__ == "__main__":
    sys.exit(main())
