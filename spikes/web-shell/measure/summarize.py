#!/usr/bin/env python3
"""Nearest-rank percentiles for echo samples. Same definition as scripts/summarize-terminal-latency.py."""
import json
import math
import sys
from pathlib import Path


def percentile(values, fraction):
    if not values:
        return None
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * fraction) - 1)]


def summarize(samples):
    values = sorted(float(sample) for sample in samples)
    return {
        "count": len(values),
        "p50_ms": percentile(values, 0.50),
        "p95_ms": percentile(values, 0.95),
        "p99_ms": percentile(values, 0.99),
        "max_ms": values[-1] if values else None,
    }


def main():
    path = Path(sys.argv[1])
    rows = json.loads(path.read_text())
    samples = rows if isinstance(rows, list) else rows.get("samples", [])
    print(json.dumps(summarize(samples), indent=2))


if __name__ == "__main__":
    main()
