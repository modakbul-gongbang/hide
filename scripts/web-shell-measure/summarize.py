#!/usr/bin/env python3
"""Nearest-rank echo percentiles and the over-budget frame fraction.

usage: summarize.py echo <echo-*.json...>      -> per-trial p50/p95/p99/max, median of trial p95s
       summarize.py frames <frames.json>       -> count, over 16.7 ms, fraction, complete
       summarize.py topology <topology.json>   -> per operation p50/p95/max to screen and to frame
       summarize.py timing <core.jsonl>        -> per operation stage times from pane_op.timing lines,
                                                  with Herdr's share and Hide's (layout event to sent)
       summarize.py resources <resources.json> -> hided's median one-second CPU and its largest RSS
"""
import json
import math
import sys
from pathlib import Path

BUDGET_MS = 16.7
S0_BASELINE_P95_MS = 6.228  # S0 REPORT, median of three trials of the native shell the web replaced; reused, not re-measured
ECHO_THRESHOLD_MS = S0_BASELINE_P95_MS + 5.0


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
        "s0_baseline_p95_ms": S0_BASELINE_P95_MS,
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


def topology(path):
    doc = json.loads(Path(path).read_text())
    operations = {}
    for sample in doc["samples"]:
        operations.setdefault(sample["kind"], []).append(sample)
    result = {}
    for kind, samples in operations.items():
        row = {"count": len(samples), "timeouts": sum(1 for s in samples if s.get("timeout"))}
        for stage in ("screen_ms", "frame_ms"):
            values = [float(s[stage]) for s in samples if s.get(stage) is not None]
            row[stage] = {
                "p50": percentile(values, 0.50),
                "p95": percentile(values, 0.95),
                "max": max(values) if values else None,
            }
        result[kind] = row
    return {"load": doc.get("load"), "rounds": doc.get("rounds"), "operations": result}


def stats(values):
    return {
        "count": len(values),
        "p50": percentile(values, 0.50),
        "p95": percentile(values, 0.95),
        "max": max(values) if values else None,
    }


def resources(path):
    doc = json.loads(Path(path).read_text())
    samples = [s["hided"] for s in doc["samples"] if "hided" in s]
    cpu = sorted(float(s["cpu_percent"]) for s in samples)
    return {
        "samples": len(samples),
        "hided_cpu_median_percent": percentile(cpu, 0.50),
        "hided_cpu_p95_percent": percentile(cpu, 0.95),
        "hided_rss_max_kb": max((int(s["rss_kb"]) for s in samples), default=None),
        "method": doc.get("method"),
    }


def timing(path):
    records = {}
    for line in Path(path).read_text().splitlines():
        try:
            entry = json.loads(line)
        except json.JSONDecodeError:
            continue
        record = entry.get("event", entry)
        if record.get("kind") != "pane_op.timing":
            continue
        records.setdefault(record["op"], []).append(record)
    result = {}
    for op, rows in records.items():
        stages = {}
        for stage in ("herdr_ack_ms", "drawn_ms", "first_event_ms", "layout_event_ms", "applied_ms", "sent_ms", "first_frame_ms"):
            stages[stage] = stats([float(r[stage]) for r in rows if stage in r])
        herdr = [float(r["layout_event_ms"]) - float(r["herdr_ack_ms"]) for r in rows if "layout_event_ms" in r and "herdr_ack_ms" in r]
        hide = [float(r["sent_ms"]) - float(r["layout_event_ms"]) for r in rows if "sent_ms" in r and "layout_event_ms" in r]
        outcomes = {}
        for r in rows:
            outcomes[r.get("outcome")] = outcomes.get(r.get("outcome"), 0) + 1
        result[op] = {"records": len(rows), "outcomes": outcomes, "stages": stages, "herdr_share_ms": stats(herdr), "hide_share_ms": stats(hide)}
    return result


if __name__ == "__main__":
    kind = sys.argv[1]
    if kind == "echo":
        result = echo(sys.argv[2:])
    elif kind == "topology":
        result = topology(sys.argv[2])
    elif kind == "timing":
        result = timing(sys.argv[2])
    elif kind == "resources":
        result = resources(sys.argv[2])
    else:
        result = frames(sys.argv[2])
    print(json.dumps(result, indent=2))
