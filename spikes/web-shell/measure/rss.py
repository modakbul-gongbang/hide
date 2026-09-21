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
    items = sys.argv[1:]
    if items[0] == "--trace":
        # CDP frame metadata identifies the page, unlike ps which also finds
        # Chrome's spare renderer. Refuse ambiguity instead of picking a PID.
        trace = json.load(open(items[1]))
        renderers = {frame['processId'] for event in trace['traceEvents']
                     if event['name'] == 'TracingStartedInBrowser'
                     for frame in event['args']['data']['frames']
                     if frame.get('isOutermostMainFrame') and ':5173/' in frame.get('url', '')
                     and 'mode=replay' in frame['url']}
        if len(renderers) != 1:
            raise SystemExit(f"expected one traced replay renderer, got {renderers}")
        items = ["tab_renderer=" + str(renderers.pop())] + items[2:]
    parts = {}
    total = 0
    for item in items:
        name, pid = item.split("=", 1)
        value = rss_kb(pid)
        parts[name] = {"pid": int(pid), "rss_kb": value, "rss_mb": round(value / 1024, 2)}
        total += value
    print(json.dumps({"method": "ps -o rss=", "parts": parts, "sum_kb": total, "sum_mb": round(total / 1024, 2)}, indent=2))


if __name__ == "__main__":
    main()
