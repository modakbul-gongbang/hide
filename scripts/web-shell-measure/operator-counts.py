#!/usr/bin/env python3
"""Read-only topology counts of the operator's Herdr, before and after a run."""
import json
import subprocess
import sys

bin_path, socket, out = sys.argv[1:4]
env = {"PATH": "/usr/bin:/bin", "HOME": __import__("os").environ["HOME"], "HERDR_SOCKET_PATH": socket}
raw = subprocess.run([bin_path, "api", "snapshot"], env=env, capture_output=True, text=True, check=True).stdout
doc = json.loads(raw)
snap = (doc.get("result") or doc).get("snapshot") or (doc.get("result") or doc)
counts = {
    "source": "operator herdr api snapshot (read-only)",
    "workspaces": len(snap.get("workspaces") or []),
    "tabs": len(snap.get("tabs") or []),
    "panes": len(snap.get("panes") or []),
    "focused_pane_id": (snap.get("focused") or {}).get("pane_id") or snap.get("focused_pane_id"),
    "version": (snap.get("version") or {}).get("version") if isinstance(snap.get("version"), dict) else snap.get("version"),
}
json.dump(counts, open(out, "w"), indent=2)
print(json.dumps(counts))
