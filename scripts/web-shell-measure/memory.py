#!/usr/bin/env python3
"""Resident memory of the measured processes at one instant.

usage: memory.py <phase> <chrome-pid> <hided-pid> <cdp-port>
-> {phase, chrome_tree_rss_kb, chrome_processes, chrome_renderer_rss_kb, processes, hided_rss_kb, page: {js_heap_used_bytes, live_terminals, pane_views}}

Chrome's renderer and GPU processes are children of the browser process, so
the tree sum is what the tab costs; the page's own JS heap comes from
`performance.memory`, which Chrome exposes.
"""
import json
import subprocess
import sys
import urllib.request


def children(pid):
    out = subprocess.run(["pgrep", "-P", str(pid)], capture_output=True, text=True).stdout.split()
    found = [int(p) for p in out]
    for child in list(found):
        found.extend(children(child))
    return found


def rss_kb(pid):
    out = subprocess.run(["ps", "-o", "rss=", "-p", str(pid)], capture_output=True, text=True).stdout.strip()
    return int(out) if out else 0


def chrome_type(pid):
    """The `--type=` a Chrome child was started with; the browser process has none."""
    out = subprocess.run(["ps", "-o", "command=", "-p", str(pid)], capture_output=True, text=True).stdout
    for word in out.split():
        if word.startswith("--type="):
            return word[len("--type="):]
    return "browser"


def page_metrics(port):
    with urllib.request.urlopen(f"http://127.0.0.1:{port}/json/list", timeout=5) as response:
        pages = json.load(response)
    page = next((p for p in pages if p.get("type") == "page"), None)
    if page is None:
        return None
    expression = (
        "JSON.stringify({js_heap_used_bytes: performance.memory ? performance.memory.usedJSHeapSize : null,"
        " live_terminals: window.__hideProbe ? window.__hideProbe.liveTerminals().length : null,"
        " pane_views: document.querySelectorAll('[data-pane-view]').length})"
    )
    script = f"""
    const p = await connectPage({port!r});
    process.stdout.write(await p.evaluate({json.dumps(expression)}));
    p.close();
    """
    here = __file__.rsplit("/", 1)[0]
    out = subprocess.run(
        ["node", "--input-type=module", "-e", f"import('{here}/cdp.mjs').then(async ({{connectPage}}) => {{{script}}})"],
        capture_output=True,
        text=True,
        timeout=20,
    )
    return json.loads(out.stdout) if out.stdout.strip() else {"error": out.stderr.strip()[:300]}


def main():
    phase, chrome_pid, hided_pid, port = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
    tree = [chrome_pid] + children(chrome_pid)
    processes = [{"pid": p, "type": chrome_type(p), "rss_kb": rss_kb(p)} for p in tree]
    doc = {
        "phase": phase,
        "chrome_tree_rss_kb": sum(entry["rss_kb"] for entry in processes),
        "chrome_processes": len(tree),
        # The page's own renderer is the tab's cost; the rest is the browser,
        # GPU, network and utility processes any tab shares.
        "chrome_renderer_rss_kb": [entry["rss_kb"] for entry in processes if entry["type"] == "renderer"],
        "processes": processes,
        "hided_rss_kb": rss_kb(hided_pid),
        "page": page_metrics(port),
    }
    print(json.dumps(doc))


if __name__ == "__main__":
    main()
