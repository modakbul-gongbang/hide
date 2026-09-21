#!/usr/bin/env python3
"""Score only complete, registered-shape measurements; never invent a baseline."""
import json
import os
from pathlib import Path
import statistics
import subprocess
from summarize import summarize


def echo_samples(trial, side):
    end = 'write_ms' if side == 'web' else 'draw_ms'
    # Re-score existing observations without rewriting raw files or discarding
    # signed values if a completion ever precedes CLI return.
    return [row[end] - row['cli_return_ms'] for row in trial['hops']]


def main():
    run = Path(os.environ['S0_RUN_DIR'])
    worktree = Path(os.environ['S0_WORKTREE'])
    read = lambda name: json.loads((run / name).read_text())
    trials = {side: [read(f'echo-{side}-{i}.json') for i in (1, 2, 3)] for side in ('web', 'swift')}
    for side, values in trials.items():
        if any(len(v['hops']) < 50 for v in values):
            raise SystemExit(f'{side}: incomplete sample count')
    summaries = {side: [summarize(echo_samples(v, side)) for v in values] for side, values in trials.items()}
    med = {side: statistics.median(v['p95_ms'] for v in values) for side, values in summaries.items()}
    rss = read('rss-tab.json')
    frames = read('frames-summary.json')
    stats = read('snapshot-stats.json')
    statuses = [
        'PASS' if med['web'] <= med['swift'] + 5 else 'FAIL',
        'PASS' if rss['sum_mb'] <= 400 else 'FAIL',
        ('PASS' if frames['fraction'] <= .01 else 'FAIL') if frames.get('complete') else 'INCOMPLETE',
    ]
    banner = 'S0 FAIL - S1 착수 금지' if 'FAIL' in statuses else ('S0 PASS (IME 판정 대기)' if all(s == 'PASS' for s in statuses) else 'S0 INCOMPLETE - S1 착수 금지')
    hop_rows = []
    for i, trial in enumerate(trials['web'], 1):
        sent = {}
        for line in (run / f'logs/hided-{i}.log').read_text().splitlines():
            if line.startswith('{'):
                row = json.loads(line)
                if row.get('event') == 'ws_sent': sent[row['requested_ms']] = row['completed_ms']
        for row in trial['hops']:
            row = dict(row)
            row['ws_completed_ms'] = sent[row['requested_ms']]
            hop_rows.append(row)
    boundaries = [
        ('Driver -> on_change', 't0_ms', 'notified_ms'),
        ('on_change -> snapshot request', 'notified_ms', 'requested_ms'),
        ('Owner queue', 'requested_ms', 'owner_started_ms'),
        ('Core snapshot + copy', 'owner_started_ms', 'owner_finished_ms'),
        ('Envelope preparation', 'owner_finished_ms', 'ws_send_ms'),
        ('WS send start -> completed', 'ws_send_ms', 'ws_completed_ms'),
        ('WS send start -> message event', 'ws_send_ms', 'arrival_ms'),
        ('Message event -> xterm write callback', 'arrival_ms', 'write_ms'),
    ]
    hops = {name: summarize([row[b] - row[a] for row in hop_rows if row[a] is not None]) for name, a, b in boundaries}
    for side, values in trials.items():
        hops[f'{side} CLI spawn -> return (overlaps downstream)'] = summarize([row['cli_return_ms']-row['t0_ms'] for trial in values for row in trial['hops']])
    (run/'hop-summary.json').write_text(json.dumps(hops, indent=2)+'\n')
    sha = subprocess.check_output(['git', '-C', str(worktree), 'rev-parse', 'HEAD'], text=True).strip()
    chrome = subprocess.check_output(['/Applications/Google Chrome.app/Contents/MacOS/Google Chrome','--version'],text=True).strip()
    table = '\n'.join(f'| {name} | {s["count"]} | {s["p50_ms"]:.3f} | {s["p95_ms"]:.3f} | {s["p99_ms"]:.3f} | {s["max_ms"]:.3f} |' for name,s in hops.items())
    distributions = '\n'.join(f'| {side} {i} | {s["count"]} | {s["p50_ms"]:.3f} | {s["p95_ms"]:.3f} | {s["p99_ms"]:.3f} | {s["max_ms"]:.3f} | {trials[side][i-1]["load"]} |' for side in trials for i,s in enumerate(summaries[side],1))
    ps = '\n'.join(f'{side} {phase} trial {i}:\n{(run/f"{phase}-{side}-{i}.ps").read_text()}' for side in ('swift','hided') for phase in ('idle','driven') for i in (1,2,3))
    report = f'''# S0 REPORT

{banner}

## Gate table

| # | Item | Value | Baseline | Threshold | Result |
| --- | --- | --- | --- | --- | --- |
| ① | Hangul IME V9 | four human checks below | n/a | all four pass | PENDING_HUMAN |
| ② | CLI return -> echo p95 | web {med['web']:.3f} ms | Swift {med['swift']:.3f} ms | Swift + 5 = {med['swift']+5:.3f} ms | {statuses[0]} |
| ③ | Chrome tab renderer + hided RSS | {rss['sum_mb']:.2f} MiB | n/a | <= 400 MB | {statuses[1]} |
| ④ | Frames over 16.7 ms | {frames['percent']:.4f}% ({frames['over_16_7ms']}/{frames['count']}); {frames['covered_ms']/1000:.3f}s | n/a | <= 1% over full 120s | {statuses[2]} |

## Environment and method

- Source at report generation: `{sha}`; the verification report binds the final committed source.
- Chrome: {chrome}.
- Swift baseline: this worktree's signed `macos/build/assembled/hide-web-shell-pivot-s0.app`, debug Swift shell linked to its release core archive, launched with private state and `--verification-background`.
- hided-spike: debug build; no product-tree changes.
- Fixture: one disposable repository, workspace, tab and cat pane; private socket/XDG/state/HOME; operator socket read only.
- Web uses 84x46 terminal grid to match the native candidate's observed settled grid.
- Each client is stopped before the other attaches; trials alternate web and Swift, 3x50 each.
- Both send `sNNNN` + LF through the same pinned `herdr pane send-text` into `stty -echo -icanon; cat`.
- Observer-authorized t0 is `cli_return_ms`, the send-text CLI return timestamp, used identically for Swift and web; t1 is the screen-side completion described below.
- The fixed threshold remains Swift p95 + 5ms. CLI spawn-to-return is excluded from gate ② and retained as its own measured distribution; raw `hops.t0_ms` retains the original pre-spawn origin.
- CLI return is an acknowledged handoff proxy, not instrumentation at the exact socket write. Echo can theoretically precede CLI return; signed differences are retained without clamping.
- Web t1 is xterm write completion after the marker is present in the parsed buffer; the associated WS message timestamp is captured at event entry before JSON decoding.
- Swift t1 is the next completed receive_to_draw log timestamp from the exact candidate PID, with only one outstanding input and an 80ms quiet interval.
- This is a software echo/draw proxy, not physical key-to-compositor latency; Swift does not expose marker identity in its trace.
- Nearest-rank percentiles, median of three trial p95s; no outlier removal.

| Trial | n | p50 ms | p95 ms | p99 ms | max ms | contemporaneous load |
| --- | --- | --- | --- | --- | --- | --- |
{distributions}

Negative corrected samples: web {sum(v < 0 for trial in trials['web'] for v in echo_samples(trial, 'web'))}/150; Swift {sum(v < 0 for trial in trials['swift'] for v in echo_samples(trial, 'swift'))}/150.
Pre-spawn medians of trial p95s, retained for comparison and not gate ②: web {statistics.median(summarize([row['write_ms']-row['t0_ms'] for row in trial['hops']])['p95_ms'] for trial in trials['web']):.3f}ms; Swift {statistics.median(summarize([row['draw_ms']-row['t0_ms'] for row in trial['hops']])['p95_ms'] for trial in trials['swift']):.3f}ms.

## Per-hop timings

Timestamp units are epoch milliseconds on this machine; within-process high-resolution clocks retain sub-millisecond precision.
Each matched browser message carries the notification, owner queue and snapshot timestamps; server send-completion logs join by request timestamp.
WS send completion and browser arrival can overlap across threads and are reported as separate intervals from send start.
Notification time identifies the burst that caused the snapshot, not a new per-byte notification.

| Boundary | n | p50 ms | p95 ms | p99 ms | max ms |
| --- | --- | --- | --- | --- | --- |
{table}

## Spike defects and interpretation

The protocol has no client ACK or next-delta request requirement: on_change broadcasts immediately, and the WS task immediately asks the owner thread for a snapshot.
The hop table measures its actual wake, owner queue and snapshot costs instead of attributing the first pass's 255ms to the product core.
The first pass used a mutable latest-chunk timestamp, polled for a raw-text match, and called term.write without waiting for completion.
Its web driver sent LF while its Swift driver did not, and the Swift trials had only 15 samples.
The corrected driver arms one sample, uses the parsed terminal buffer (ANSI compression can hide a literal marker in raw bytes), and timestamps the actual message event and write callback independently of when the driver reads them.
The old samples have no per-hop timestamps, so the precise contribution to their 255ms cannot be recovered retrospectively.
Replay now starts explicitly after Performance recording begins, measures from the first rAF through at least 120000ms, and treats missing coverage as INCOMPLETE.
The frame summarizer no longer converts a long millisecond stall into seconds or accepts a short window as PASS.
Cleanup stops all clients before waiting, stops the private server, and supervises process groups when the run owner exits.

## Idle and driven observations

`ps` CPU is the process-lifetime average at each recorded point, not an interval-only CPU sample.
Idle is before echo; driven is immediately after the 50-sample sequence.

```
{ps}
```

120s delta capture statistics: {json.dumps(stats)}.
RSS uses `ps -o rss=` on the replay renderer identified by CDP TracingStartedInBrowser frame metadata, plus hided at replay completion: {json.dumps(rss)}.
The whole Chrome tree is not gate ③'s denominator; these are endpoint RSS samples, not a ten-minute memory-growth claim.
Operator topology before: {json.dumps(read('operator-before.json'))}.
Operator topology after: {json.dumps(read('operator-after.json'))}.
Any concurrent operator topology drift is retained; this run only reads that socket.

## Reproduction and local evidence

Build with `bash macos/scripts/build_dev_app.sh` and the spike build command in README, then run `S0_RUN_DIR="$PWD/agents/runs/web-shell-pivot-s0/<fresh-run>" bash spikes/web-shell/measure/run-s0.sh`.
Run directory: `{run}`.
Echo distributions and hop timestamps: `echo-web-1..3.json`, `echo-swift-1..3.json`, `hop-summary.json`, `logs/hided-1..3.log`.
Replay: `capture.jsonl`, `snapshot-stats.json`, `chrome-trace.json`, `frames.json`, `frames-summary.json`.
RSS: `rss-tab.json`; native identity/capture: `swift-windows-1..3.json`, `swift-1..3.png`, `swift-echo-1..3.png`; cleanup: `cleanup-processes.txt`.
Captures replace terminal bytes with same-length x filler; replay therefore tests synthetic byte volume/timing, not preservation of private content or original ANSI semantics.
All run artifacts stay local and are not committed.

## ① Human IME procedure and unresolved items

Run a fresh isolated fixture, open the Chrome live pane, and use a real Korean input method.
Automation does not decide any of these four checks.

1. Candidate window follows the composing cursor: `ime-01-candidate-follows-cursor.png`.
2. Backspace during composition does not leak DEL: `ime-02-backspace-no-del.png`.
3. Two or more adjacent Hangul syllables do not overwrite the next cell: `ime-03-adjacent-hangul.png`.
4. ASCII letters echo immediately: `ime-04-ascii-immediate.png`.

① remains PENDING_HUMAN; S1 is not started.
Foreground IME, physical display/compositor latency and release-build parity are not established by these background measurements.
'''
    (run/'REPORT.md').write_text(report)
    print(report)

if __name__ == '__main__':
    main()
