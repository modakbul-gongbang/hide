# Web shell Stage-0 spike

Measurement-only. Not a workspace member and not a CI gate.

## Layout

- `hided-spike/` independent Cargo project. Links `herdr-core` by path. One loopback WebSocket.
- `web-spike/` Vite + React + xterm.js 6 + addon-webgl. Live and replay modes.
- `measure/` isolation harness and the shared echo/RSS/frame drivers.

## Isolation

Every command that talks to Herdr is run from a subshell that sources `measure/isolated-env.sh`.
That script unsets `HERDR_PANE_ID`, `HERDR_TAB_ID`, `HERDR_WORKSPACE_ID`, and `HERDR_ENV`, and points `HERDR_SOCKET_PATH` at `/tmp/h-s0.sock`.
`hided-spike` refuses to start when the variable is unset or when it resolves to the operator socket.

```sh
# private server
bash spikes/web-shell/measure/start-server.sh
# hided (from a subshell with the isolated env)
source spikes/web-shell/measure/isolated-env.sh
. scripts/toolchain-env.sh
cargo run --manifest-path spikes/web-shell/hided-spike/Cargo.toml
# web
cd spikes/web-shell/web-spike && pnpm dev
# stop only the private server
bash spikes/web-shell/measure/stop-server.sh
```

## Rerun the numbers

```sh
bash spikes/web-shell/measure/run-s0.sh
```

Writes under `agents/runs/web-shell-pivot-s0/`: `REPORT.md`, `capture.jsonl`, `echo-*.json`, `rss-*.json`, `frames.json`, `chrome-trace.json`.
