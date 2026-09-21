#!/usr/bin/env python3
"""Sum RSS of named PIDs. Values are kilobytes from ps."""
import json
import subprocess
import sys


def rss_kb(pid: str) -> int:
    out = subprocess.check_output(["ps", "-p", pid, "-o", "rss="], text=True).strip()
    if not out:
        raise SystemExit(f"rss: pid {pid} not found")
    return int(out)


def main():
    if len(sys.argv) < 2:
        raise SystemExit("usage: rss.py name=pid [name=pid ...]")
    parts = {}
    total = 0
    for item in sys.argv[1:]:
        name, pid = item.split("=", 1)
        value = rss_kb(pid)
        parts[name] = {"pid": int(pid), "rss_kb": value, "rss_mb": round(value / 1024, 2)}
        total += value
    print(json.dumps({"method": "ps -o rss=", "parts": parts, "sum_kb": total, "sum_mb": round(total / 1024, 2)}, indent=2))


if __name__ == "__main__":
    main()
