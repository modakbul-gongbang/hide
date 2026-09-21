#!/usr/bin/env python3
"""Resident memory of the measured processes at one instant.

usage: memory.py <phase> <chrome-pid> <hided-pid> <cdp-port>
-> {phase, chrome_tree_rss_kb, chrome_processes, hided_rss_kb, page: {js_heap_used_bytes, live_terminals}}

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
    doc = {
        "phase": phase,
        "chrome_tree_rss_kb": sum(rss_kb(p) for p in tree),
        "chrome_processes": len(tree),
        "hided_rss_kb": rss_kb(hided_pid),
        "page": page_metrics(port),
    }
    print(json.dumps(doc))


if __name__ == "__main__":
    main()
