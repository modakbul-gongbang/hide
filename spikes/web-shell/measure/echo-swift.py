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
from datetime import datetime


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
        text=True,
    )
    time.sleep(0.5)
    samples = []
    try:
        for i in range(repeats):
            marker = f"{chr(97 + (i % 26))}{i % 10}"
            t0 = time.time()
            sent = subprocess.run(
                [herdr_bin, "pane", "send-text", pane_id, marker],
                check=False,
                capture_output=True,
                text=True,
            )
            if sent.returncode != 0:
                raise SystemExit(f"send-text failed: {sent.stderr}")
            deadline = time.time() + 3
            matched = None
            while time.time() < deadline:
                line = proc.stdout.readline()
                if not line:
                    break
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
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=2)
        except subprocess.TimeoutExpired:
            proc.kill()
    json.dump(
        {
            "method": "herdr pane send-text t0 -> Swift TerminalLatency receive_to_draw log timestamp t1",
            "samples": samples,
        },
        sys.stdout,
        indent=2,
    )
    sys.stdout.write("\n")


if __name__ == "__main__":
    main()
