#!/usr/bin/env python3
"""Nearest-rank echo percentiles and the over-budget frame fraction.

usage: summarize.py echo <echo-*.json...>      -> per-trial p50/p95/p99/max, median of trial p95s
       summarize.py frames <frames.json>       -> count, over 16.7 ms, fraction, complete
"""
import json
import math
import sys
from pathlib import Path

BUDGET_MS = 16.7
SWIFT_BASELINE_P95_MS = 6.228  # S0 REPORT, median of three Swift trials; reused, not re-measured
ECHO_THRESHOLD_MS = SWIFT_BASELINE_P95_MS + 5.0


def percentile(values, fraction):
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * fraction) - 1)] if ordered else None


def echo(paths):
    trials = []
    for path in paths:
        doc = json.loads(Path(path).read_text())
        values = [float(v) for v in doc["samples"]]
        trials.append({
            "file": Path(path).name,
            "count": len(values),
            "p50_ms": percentile(values, 0.50),
            "p95_ms": percentile(values, 0.95),
            "p99_ms": percentile(values, 0.99),
            "max_ms": max(values) if values else None,
            "negative": sum(1 for v in values if v < 0),
            "load": doc.get("load"),
        })
    p95s = sorted(t["p95_ms"] for t in trials)
    median_p95 = p95s[len(p95s) // 2] if p95s else None
    return {
        "trials": trials,
        "median_trial_p95_ms": median_p95,
        "swift_baseline_p95_ms": SWIFT_BASELINE_P95_MS,
        "threshold_ms": ECHO_THRESHOLD_MS,
        "pass": median_p95 is not None and median_p95 <= ECHO_THRESHOLD_MS,
    }


def frames(path):
    doc = json.loads(Path(path).read_text())
    dts = [float(f["dt"]) for f in doc["frames"] if "dt" in f]
    over = sum(1 for v in dts if v > BUDGET_MS)
    complete = bool(dts) and sum(dts) >= 120_000 and doc.get("done") is True
    fraction = (over / len(dts)) if dts else None
    return {
        "count": len(dts),
        "covered_ms": sum(dts),
        "window": doc.get("window"),
        "complete": complete,
        "over_16_7ms": over,
        "fraction": fraction,
        "percent": (100.0 * fraction) if fraction is not None else None,
        "budget_ms": BUDGET_MS,
        "max_dt_ms": max(dts) if dts else None,
        "ws_frames_during_run": doc.get("ws_frames_during_run"),
        "pass": complete and fraction is not None and fraction <= 0.01,
    }


if __name__ == "__main__":
    kind = sys.argv[1]
    result = echo(sys.argv[2:]) if kind == "echo" else frames(sys.argv[2])
    print(json.dumps(result, indent=2))
