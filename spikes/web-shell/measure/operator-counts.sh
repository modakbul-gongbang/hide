#!/usr/bin/env bash
# Read-only topology of the operator Herdr server. Does not attach, focus,
# close, or send input.
set -euo pipefail
operator_socket="${HOME}/.config/herdr/herdr.sock"
out="${1:-}"
if [[ -z "$out" ]]; then
  printf 'usage: operator-counts.sh <output-json>\n' >&2
  exit 2
fi
# Explicit socket; do not inherit an isolated env.
env -u HERDR_SESSION -u HERDR_CONFIG_PATH \
  HERDR_SOCKET_PATH="$operator_socket" \
  herdr api snapshot > "${out}.raw"
python3 - "$out" <<'PY'
import json, sys
out = sys.argv[1]
raw = json.loads(open(out + ".raw").read())
snap = (raw.get("result") or raw).get("snapshot") or (raw.get("result") or raw)
payload = {
    "source": "operator herdr api snapshot (read-only)",
    "workspaces": len(snap.get("workspaces") or []),
    "tabs": len(snap.get("tabs") or []),
    "panes": len(snap.get("panes") or []),
    "focused_pane_id": snap.get("focused_pane_id"),
    "version": snap.get("version"),
}
json.dump(payload, open(out, "w"), indent=2)
print(json.dumps(payload))
PY
rm -f "${out}.raw"
