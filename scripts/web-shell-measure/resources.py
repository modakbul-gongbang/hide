#!/usr/bin/env python3
"""Twenty one-second CPU deltas and RSS samples for owned process trees.

usage: resources.py <hided-pid> <herdr-pid> <chrome-pid> [--memory-series]
CPU is one-core percent from process CPU-time deltas, not ps's lifetime average.
No command line, environment, or operator socket is read.
"""
import json
import subprocess
import sys
import time


def snapshot():
    raw = subprocess.check_output(["ps", "-axo", "pid=,ppid=,time=,rss="], text=True)
    rows = {}
    for line in raw.splitlines():
        pid, parent, cpu, rss = line.split()
        days, separator, clock = cpu.partition("-")
        seconds = 0.0
        for part in (clock if separator else cpu).split(":"):
            seconds = seconds * 60 + float(part)
        if separator:
            seconds += int(days) * 86400
        rows[int(pid)] = (int(parent), seconds, int(rss))
    return rows


def tree(root, rows):
    if root not in rows:
        raise RuntimeError(f"owned process {root} ended before sampling finished")
    found = {root}
    while True:
        grown = found | {pid for pid, row in rows.items() if row[0] in found}
        if grown == found:
            return found
        found = grown


if len(sys.argv) not in (4, 5) or (len(sys.argv) == 5 and sys.argv[4] != "--memory-series"):
    raise SystemExit("usage: resources.py <hided-pid> <herdr-pid> <chrome-pid> [--memory-series]")
roots = dict(zip(("hided", "herdr", "chrome"), map(int, sys.argv[1:4])))
if len(sys.argv) == 5:
    origin = time.monotonic()
    samples = []
    for minute in range(11):
        time.sleep(max(0, origin + minute * 60 - time.monotonic()))
        rows = snapshot()
        sample = {"seconds": time.monotonic() - origin}
        for name, root in roots.items():
            pids = tree(root, rows)
            sample[name] = {"rss_kb": sum(rows[pid][2] for pid in pids), "pids": sorted(pids)}
        samples.append(sample)
    print(json.dumps({"method": "eleven one-minute RSS samples over ten minutes; owned process trees", "samples": samples}))
    raise SystemExit(0)
before, started = snapshot(), time.monotonic()
samples = []
for _ in range(20):
    time.sleep(1)
    after, ended = snapshot(), time.monotonic()
    sample = {"seconds": ended - started}
    for name, root in roots.items():
        pids = tree(root, after)
        cpu = sum(max(0, after[pid][1] - before.get(pid, (0, 0, 0))[1]) for pid in pids)
        sample[name] = {"cpu_percent": cpu / (ended - started) * 100,
                        "rss_kb": sum(after[pid][2] for pid in pids), "processes": len(pids)}
    samples.append(sample)
    before, started = after, ended
print(json.dumps({"method": "20 one-second CPU-time deltas; RSS sums; owned process trees", "samples": samples}))
