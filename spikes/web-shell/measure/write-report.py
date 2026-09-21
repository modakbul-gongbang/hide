#!/usr/bin/env python3
"""Assemble REPORT.md from S0 run artifacts."""
import json
import os
import subprocess
from pathlib import Path


def load(path: Path):
    if not path.exists():
        return None
    if path.suffix == ".json":
        return json.loads(path.read_text())
    return path.read_text()


def p95(path: Path):
    payload = load(path)
    if not payload:
        return None
    samples = payload.get("samples") if isinstance(payload, dict) else payload
    if not samples:
        return None
    import math
    ordered = sorted(float(value) for value in samples)
    return ordered[max(0, math.ceil(len(ordered) * 0.95) - 1)]


def median(values):
    values = [value for value in values if value is not None]
    if not values:
        return None
    values = sorted(values)
    return values[len(values) // 2]


def main():
    run = Path(os.environ["S0_RUN_DIR"])
    worktree = Path(os.environ["S0_WORKTREE"])
    sha = subprocess.check_output(["git", "-C", str(worktree), "rev-parse", "HEAD"], text=True).strip()
    chrome = subprocess.check_output(
        ["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", "--version"],
        text=True,
    ).strip()
    load_avg = Path("/proc/loadavg").read_text() if Path("/proc/loadavg").exists() else subprocess.check_output(["uptime"], text=True).strip()

    web_p95s = [p95(run / f"echo-web-{i}.json") for i in (1, 2, 3)]
    web_p95 = median(web_p95s)
    swift_p95s = [p95(run / f"echo-swift-{i}.json") for i in (1, 2, 3)]
    swift_p95 = median(swift_p95s)
    rss = load(run / "rss-tab.json") or load(run / "rss-driven.json") or {}
    frames = load(run / "frames-summary.json") or {}
    stats = load(run / "snapshot-stats.json") or {}
    operator_before = load(run / "operator-before.json") or {}
    operator_after = load(run / "operator-after.json") or {}
    idle_hided = load(run / "idle-hided.ps") or ""
    driven_hided = load(run / "driven-hided.ps") or ""

    threshold_echo = None if swift_p95 is None else swift_p95 + 5
    echo_status = "PENDING"
    if web_p95 is not None and threshold_echo is not None:
        echo_status = "PASS" if web_p95 <= threshold_echo else "FAIL"
    elif web_p95 is not None and swift_p95 is None:
        echo_status = "PENDING_SWIFT_BASELINE"

    rss_mb = rss.get("sum_mb")
    mem_status = "PENDING" if rss_mb is None else ("PASS" if rss_mb <= 400 else "FAIL")
    frac = frames.get("fraction")
    frame_status = "PENDING" if frac is None else ("PASS" if frac <= 0.01 else "FAIL")

    measured = [echo_status, mem_status, frame_status]
    hard_fail = [item for item in measured if item == "FAIL"]
    if hard_fail:
        banner = "S0 FAIL - S1 착수 금지"
    elif echo_status == "PASS" and mem_status == "PASS" and frame_status == "PASS":
        banner = "S0 PASS (IME 판정 대기)"
    else:
        banner = "S0 INCOMPLETE - 측정 미완 항목 있음"

    fail_lines = []
    if echo_status == "FAIL":
        fail_lines.append(f"- ② echo p95 {web_p95}ms exceeded Swift {swift_p95}ms + 5ms")
    if mem_status == "FAIL":
        fail_lines.append(f"- ③ RSS sum {rss_mb}MB exceeded 400MB")
    if frame_status == "FAIL":
        fail_lines.append(f"- ④ frame over-budget fraction {frac} exceeded 1%")

    report = f"""# S0 REPORT

{banner}

{os.linesep.join(fail_lines)}

## Gate table

| # | Item | Value | Baseline | Threshold | Result |
| --- | --- | --- | --- | --- | --- |
| ① | Hangul IME V9 four checks | see procedure below | n/a | all four pass in Chrome stable | PENDING_HUMAN |
| ② | key→echo p95 | web {web_p95} ms (trials {web_p95s}) | Swift {swift_p95} ms (trials {swift_p95s}) | Swift p95 + 5ms = {threshold_echo} | {echo_status} |
| ③ | Chrome tab + hided RSS | {rss_mb} MB | n/a | ≤ 400 MB | {mem_status} |
| ④ | 120s driven replay frames >16.7ms | {frames.get("percent")} % ({frames.get("over_16_7ms")}/{frames.get("count")}) | n/a | ≤ 1% | {frame_status} |

## Environment

- commit: `{sha}`
- herdr pin: 0.9.1 (bundle digest in `macos/Sources/HerdrMacOS/Resources/herdr-bundle.json`)
- Chrome: {chrome}
- machine load: {load_avg}
- isolated socket: `/tmp/h-s0.sock`
- operator topology (read-only `herdr api snapshot`): before {json.dumps(operator_before)} after {json.dumps(operator_after)}

## Method (latency)

Both sides use the same driver: `herdr pane send-text` of a short ASCII marker into an isolated pane running `cat`.
t0 is the driver send wall clock.
t1 is screen-buffer arrival: xterm.js `window.__s0WaitFor` for web, Swift `TerminalLatency` `receive_to_draw` completed log timestamp for the Swift app.
Three trials of 50 samples; reported p95 is the median of trial p95s.
Nearest-rank percentiles match `scripts/summarize-terminal-latency.py`.

Rerun: `bash spikes/web-shell/measure/run-s0.sh`

## Swift baseline (idle vs driven)

Idle hided `ps`:

```
{idle_hided}
```

Driven hided `ps`:

```
{driven_hided}
```

Snapshot delta stats (redacted capture): {json.dumps(stats)}

Isolated fixture: 1 workspace, 1 tab, 1 pane (cat then a 120s printer).
Operator live session counts are recorded above and were not attached to.

## Capture

- path: `agents/runs/web-shell-pivot-s0/capture.jsonl` (local only)
- terminal bytes replaced with same-length `x` filler (`bytes_len` retained)
- chrome trace: `agents/runs/web-shell-pivot-s0/chrome-trace.json`
- frames: `agents/runs/web-shell-pivot-s0/frames.json`
- RSS method: `ps -o rss=` summed over the Chrome processes whose command line contains the run-dir user-data-dir, plus hided-spike

## ① IME procedure (PENDING_HUMAN)

Open `http://127.0.0.1:5173/?mode=live` in Chrome stable, then `?mode=ime` for the checklist.
Fill these slots in this run directory; do not let an automated CGEvent pass them.

1. Candidate window follows the cursor while composing Hangul. Slot: `ime-01-candidate-follows-cursor.png`
2. Backspace during composition does not leak DEL to the shell. Slot: `ime-02-backspace-no-del.png`
3. Two or more adjacent Hangul syllables do not overwrite the next cell. Slot: `ime-03-adjacent-hangul.png`
4. ASCII letters echo immediately. Slot: `ime-04-ascii-immediate.png`

## Incomplete items

- IME ① is PENDING_HUMAN
- Swift app baseline is filled only when an isolated worktree bundle was launched; otherwise ② compares against a missing Swift p95
"""
    (run / "REPORT.md").write_text(report)
    print(report)


if __name__ == "__main__":
    main()
