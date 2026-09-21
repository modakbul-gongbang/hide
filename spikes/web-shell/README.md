# Web shell Stage-0 spike

Measurement-only, outside the product workspace and CI gates.

## Layout

- `hided-spike/`: independent Cargo project linking the existing core, with one loopback WebSocket.
- `web-spike/`: Vite, React and xterm.js; live terminal and explicit 120-second replay.
- `measure/`: the shared isolation, echo, RSS, frame and reporting scripts.

## Build and measure

Run from this worktree, with the pinned Herdr already cached and Chrome installed:

```sh
bash macos/scripts/build_dev_app.sh
. scripts/toolchain-env.sh
cargo build --locked --manifest-path spikes/web-shell/hided-spike/Cargo.toml
pnpm --dir spikes/web-shell/web-spike build
S0_RUN_DIR="$PWD/agents/runs/web-shell-pivot-s0/<fresh-run>" bash spikes/web-shell/measure/run-s0.sh
```

The orchestrator requires a built worktree dev bundle, checks free ports, starts the private server and stops all owned process groups on exit.
Never source its isolation environment back into the operator's pane.
Private socket names derive from the run directory; private XDG, HOME, fixture and browser profile live inside that directory.
The operator socket is read only for before/after topology counts.
The terminal fixture is cleared between each of three alternating 50-sample web and Swift trials.
The native candidate runs in the background with private state; exact-window screenshots are saved before and after echo.
The run then captures and replays 120 seconds, samples RSS, and writes `REPORT.md` plus `hop-summary.json`.
The renderer PID comes from the replay's CDP frame metadata, excluding Chrome's spare renderer from the tab denominator.
Use a fresh run directory for every attempt and preserve failed attempts for diagnosis.

## Timing boundaries

Both drivers record timestamps before and after the same `herdr pane send-text` command with the same LF-terminated marker.
By Observer decision, gate ② uses CLI return as t0 for both shells; pre-spawn origin and CLI cost remain in raw hops and REPORT.
The return timestamp is an acknowledged handoff proxy; negative completion differences, if present, are retained without clamping.
The daemon stamps notification, snapshot request, owner start/end and WS send; structured send-completion logs correlate by request timestamp.
The browser stamps message entry and xterm's write callback, resolving one armed marker from the parsed buffer without polling.
Swift uses the next completed draw trace from the exact candidate PID after input, a software proxy without marker identity.
The report discloses that limitation and records load, build mode and every trial without excluding outliers.
Replay must cover 120000ms of rAF intervals and complete inside the Performance trace; short recordings remain INCOMPLETE.

## Verification

```sh
python3 -m unittest discover -s spikes/web-shell/measure -p 'test_*.py'
node scripts/check-design-contract.mjs
```

Tests exercise full-window scoring, long frame units, and child cleanup after owner death.
Run artifacts, browser profiles and traces remain local under `agents/runs/`; commit only spike source.
IME is a manual Chrome check and always remains PENDING_HUMAN until a person performs its four checks.
