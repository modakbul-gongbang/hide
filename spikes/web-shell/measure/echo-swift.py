#!/usr/bin/env python3
"""Shared echo definition for the Swift shell.

t0 = herdr pane send-text
t1 = timestamp of the next completed TerminalLatency receive_to_draw on the
isolated app PID (log stream ndjson).
"""
import json
import os
import subprocess
import sys
import time
import selectors
from datetime import datetime
from pathlib import Path


def parse_ts(stamp: str) -> float:
    stamp = stamp.replace("Z", "+00:00")
    if len(stamp) >= 5 and stamp[-5] in "+-" and stamp[-3] != ":":
        stamp = stamp[:-2] + ":" + stamp[-2:]
    return datetime.fromisoformat(stamp).timestamp()


def main():
    pane_id = os.environ["S0_PANE_ID"]
    app_pid = os.environ["S0_SWIFT_PID"]
    herdr_bin = os.environ.get("HERDR_BIN", "herdr")
    repeats = int(os.environ.get("S0_ECHO_REPEATS", "50"))
    proc = subprocess.Popen(
        [
            sys.executable, str(Path(__file__).with_name("owned.py")),
            str(os.getpid()), str(Path(os.environ["S0_PRIVATE"]) / "swift-log.pid"),
            "/usr/bin/log",
            "stream",
            "--process",
            app_pid,
            "--level",
            "debug",
            "--style",
            "ndjson",
            "--predicate",
            'subsystem == "me.grab.hide" AND category == "TerminalLatency"',
        ],
        stdout=subprocess.PIPE,
        bufsize=0,
    )
    time.sleep(0.5)
    pending_lines = []
    partial = b""
    samples = []
    hops = []
    load = subprocess.check_output(["uptime"], text=True).strip()
    selector = selectors.DefaultSelector()
    selector.register(proc.stdout, selectors.EVENT_READ)
    try:
        for i in range(repeats):
            marker = f"s{i:04d}\n"
            t0 = time.time()
            sent = subprocess.run(
                [herdr_bin, "pane", "send-text", pane_id, marker],
                check=False,
                capture_output=True,
                text=True,
                timeout=3,
            )
            cli_return = time.time()
            if sent.returncode != 0:
                raise SystemExit(f"send-text failed: {sent.stderr}")
            deadline = time.time() + 3
            matched = None
            while time.time() < deadline:
                if not pending_lines:
                    if not selector.select(timeout=max(0, deadline - time.time())):
                        break
                    block = os.read(proc.stdout.fileno(), 65536)
                    if not block:
                        break
                    split = (partial + block).split(b"\n")
                    pending_lines.extend(split[:-1])
                    partial = split[-1]
                    if not pending_lines:
                        continue
                line = pending_lines.pop(0)
                try:
                    event = json.loads(line)
                except json.JSONDecodeError:
                    continue
                message = event.get("eventMessage", "")
                if "receive_to_draw" not in message or "outcome=completed" not in message:
                    continue
                t1 = parse_ts(event["timestamp"])
                if t1 >= t0:
                    matched = (t1 - t0) * 1000
                    break
            if matched is None:
                raise SystemExit(f"no receive_to_draw after marker {marker}")
            samples.append(matched)
            hops.append({"sample": i, "t0_ms": t0 * 1000, "cli_return_ms": cli_return * 1000, "draw_ms": t1 * 1000})
            time.sleep(0.08)
    finally:
        selector.close()
        proc.terminate()
        try:
            proc.wait(timeout=2)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
    json.dump(
        {
            "method": "herdr pane send-text t0 -> Swift TerminalLatency receive_to_draw log timestamp t1",
            "samples": samples,
            "hops": hops,
            "load": load,
        },
        sys.stdout,
        indent=2,
    )
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
