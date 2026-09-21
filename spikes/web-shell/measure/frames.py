#!/usr/bin/env python3
"""16.7ms-exceeded fraction from rAF dt samples."""
import json
import sys
from pathlib import Path

BUDGET_MS = 16.7


def main():
    path = Path(sys.argv[1])
    payload = json.loads(path.read_text())
    frames = payload.get("frames") if isinstance(payload, dict) else payload
    dts = [float(frame["dt"]) for frame in frames if "dt" in frame]
    if dts and dts[0] > 1000:
        # performance.now() deltas should already be ms. Guard accidental seconds.
        dts = [value * 1000 for value in dts]
    over = sum(1 for value in dts if value > BUDGET_MS)
    result = {
        "count": len(dts),
        "over_16_7ms": over,
        "fraction": (over / len(dts)) if dts else None,
        "percent": (100.0 * over / len(dts)) if dts else None,
        "budget_ms": BUDGET_MS,
        "max_dt_ms": max(dts) if dts else None,
    }
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
