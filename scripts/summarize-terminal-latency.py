#!/usr/bin/env python3
"""Summarize TerminalLatency signpost end messages from `log stream --style ndjson`.

Capture with --predicate 'subsystem == "me.grab.hide" AND category == "TerminalLatency"'.
The recorded refresh rate comes from the view's display link, not automation timing.
Zero Hz means the display clock had not ticked; it is reported as unknown.
"""

import argparse
from datetime import datetime
import json
import math
import re
from pathlib import Path


INTERVALS = ("key_to_send", "receive_to_draw", "wheel_to_draw", "tab_to_first_draw")
MARK = re.compile(
    r"hide_latency interval=(\S+) pane=(\S+) milliseconds=([\d.]+) hz=([\d.]+) outcome=(\S+)"
)


def summarize(lines, started_after=None):
    samples = {name: [] for name in INTERVALS}
    excluded = {name: {} for name in INTERVALS}
    rates = set()
    panes = set()
    for line in lines:
        # Raw JSON escapes its closing quote, so the mark is read from the
        # decoded event message rather than from the line.
        event = json.loads(line) if line.lstrip().startswith("{") else None
        match = MARK.search(event.get("eventMessage", "") if event else line)
        if not match:
            continue
        name, pane, duration, rate, outcome = match.groups()
        if name not in samples:
            raise ValueError(f"Unknown latency interval: {name}")
        if started_after is not None:
            if event is None or "timestamp" not in event:
                raise ValueError("--started-after requires timestamped JSON log records")
            # macOS log uses +0900; Python 3.9 requires the ISO colon.
            stamp = re.sub(r"([+-]\d{2})(\d{2})$", r"\1:\2", event["timestamp"])
            timestamp = datetime.fromisoformat(stamp).timestamp()
            if timestamp - float(duration) / 1000 < started_after:
                outcome = "began_before_window"
        if outcome != "completed":
            excluded[name][outcome] = excluded[name].get(outcome, 0) + 1
            continue
        samples[name].append(float(duration))
        panes.add(pane)
        if float(rate) > 0:
            rates.add(float(rate))

    def percentile(values, fraction):
        return values[max(0, math.ceil(len(values) * fraction) - 1)] if values else None

    intervals = {}
    for name, values in samples.items():
        values.sort()
        intervals[name] = {
            "count": len(values),
            "p50_ms": percentile(values, 0.50),
            "p95_ms": percentile(values, 0.95),
            "max_ms": values[-1] if values else None,
            "excluded": excluded[name],
        }
    return {"intervals": intervals, "refresh_rates_hz": sorted(rates), "panes": sorted(panes)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace", type=Path)
    parser.add_argument("--started-after", type=float, help="Only intervals beginning after this Unix timestamp")
    args = parser.parse_args()
    with args.trace.open() as stream:
        result = summarize(stream, args.started_after)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
