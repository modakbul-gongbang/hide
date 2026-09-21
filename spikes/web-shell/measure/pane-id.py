#!/usr/bin/env python3
"""Print the first pane id from a herdr api snapshot on stdin."""
import json
import sys

raw = json.load(sys.stdin)
snap = (raw.get("result") or raw).get("snapshot") or (raw.get("result") or raw)
panes = snap.get("panes") or []
if not panes:
    raise SystemExit("pane-id: no panes in snapshot")
pane = panes[0]
pane_id = pane.get("id") or pane.get("pane_id")
if not pane_id:
    raise SystemExit(f"pane-id: pane object has no id: {pane.keys()}")
print(pane_id)
