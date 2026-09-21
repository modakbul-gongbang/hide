#!/usr/bin/env python3
"""The one pane id of the private fixture from `herdr api snapshot` on stdin."""
import json
import sys

doc = json.load(sys.stdin)
snap = (doc.get("result") or doc).get("snapshot") or (doc.get("result") or doc)
panes = snap.get("panes") or []
if len(panes) != 1:
    raise SystemExit(f"expected exactly one fixture pane, found {len(panes)}")
pane = panes[0]
print(pane.get("pane_id") or pane.get("id"))
