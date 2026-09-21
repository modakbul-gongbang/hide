#!/usr/bin/env python3
"""Bounded process group, supervised by the run owner even after SIGKILL.

Usage: owned.py OWNER_PID PID_FILE COMMAND [ARG ...]
The wrapper must be stopped before its pid file is removed or reused.
"""
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

MAX_SECONDS = 900
owner = int(sys.argv[1])
pid_file = Path(sys.argv[2])
child = None
stopping = False

def stop(_signum=None, _frame=None):
    global stopping
    stopping = True

signal.signal(signal.SIGTERM, stop)
signal.signal(signal.SIGINT, stop)
started = time.monotonic()
try:
    child = subprocess.Popen(sys.argv[3:], start_new_session=True)
    pid_file.write_text(str(child.pid))
    while child.poll() is None and not stopping:
        try:
            os.kill(owner, 0)
        except ProcessLookupError:
            break
        if time.monotonic() - started > MAX_SECONDS:
            print("owned process exceeded 900s budget", file=sys.stderr)
            break
        time.sleep(0.1)
finally:
    if child is not None:
        try:
            os.killpg(child.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            child.wait(timeout=3)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait()
        # Descendants can survive their group leader's exit.
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
sys.exit(child.returncode if child.returncode is not None and child.returncode >= 0 else 1)
