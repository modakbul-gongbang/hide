#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
slug="hide-agent-tree-and-worktree-panel"
record_root="$(git -C "$repo_root" worktree list --porcelain | awk '/^worktree / { print substr($0, 10) }' | while IFS= read -r candidate; do
  state="$candidate/agents/runs/$slug/state.json"
  [[ -f "$state" ]] || continue
  if python3 -c 'import json, sys; raise SystemExit(0 if json.load(open(sys.argv[1])).get("worktree", {}).get("path") == sys.argv[2] else 1)' "$state" "$repo_root"
  then
    printf '%s\n' "$candidate"
    break
  fi
done)"
[[ -n "$record_root" ]] || { echo "AC25: record root not found" >&2; exit 1; }

run_dir="$record_root/agents/runs/$slug"
fixture_home="$run_dir/live-fixture/home"
fixture_repo="$run_dir/live-fixture/repository/main"
app="$repo_root/macos/build/assembled/$slug.app"
binary="$app/Contents/MacOS/HerdrMacOS"
[[ -x "$binary" ]] || { echo "AC25: assembled app is missing: $binary" >&2; exit 1; }
[[ -d "$fixture_repo/.git" ]] || { echo "AC25: disposable fixture is missing: $fixture_repo" >&2; exit 1; }

stamp="$(date +%Y%m%dT%H%M%S)"
evidence="$run_dir/verification/performance-harness-$stamp"
server_dir="$evidence/server"
mkdir -p "$server_dir"
short_root="$(mktemp -d /tmp/hide-git-idle.XXXXXX)"
ln -s "$server_dir" "$short_root/run"
socket_path="$short_root/run/herdr.sock"
server_pid=""
app_pid=""
workspace_id=""

cleanup() {
  set +e
  if [[ -n "$app_pid" ]] && kill -0 "$app_pid" 2>/dev/null; then
    kill -TERM "$app_pid" 2>/dev/null
    wait "$app_pid" 2>/dev/null
  fi
  if [[ -n "$workspace_id" ]]; then
    HOME="$fixture_home" HERDR_SOCKET_PATH="$socket_path" herdr workspace close "$workspace_id" \
      >"$server_dir/workspace-close.json" 2>&1
  fi
  if [[ -S "$socket_path" ]]; then
    HOME="$fixture_home" HERDR_SOCKET_PATH="$socket_path" herdr server stop \
      >"$server_dir/server-stop.txt" 2>&1
  fi
  if [[ -n "$server_pid" ]]; then
    wait "$server_pid" 2>/dev/null
  fi
  unlink "$short_root/run" 2>/dev/null
  rmdir "$short_root" 2>/dev/null
}
trap cleanup EXIT INT TERM

HOME="$fixture_home" HERDR_SOCKET_PATH="$socket_path" herdr server \
  >"$server_dir/server.log" 2>&1 &
server_pid=$!
for _ in $(seq 1 40); do
  [[ -S "$socket_path" ]] && break
  sleep 0.25
done
[[ -S "$socket_path" ]] || { echo "AC25: isolated server did not start" >&2; exit 1; }

HOME="$fixture_home" HERDR_SOCKET_PATH="$socket_path" \
  herdr workspace create --cwd "$fixture_repo" --label idle-measurement --no-focus \
  >"$server_dir/workspace-create.json"
workspace_id="$(python3 - "$server_dir/workspace-create.json" <<'PY'
import json, sys
print(json.load(open(sys.argv[1]))["result"]["workspace"]["workspace_id"])
PY
)"

write_state() {
  local mode="$1" visible="$2" path="$3"
  cat >"$path" <<EOF
{
  "schema_version": 1,
  "left_sidebar_visible": true,
  "right_panel_visible": $visible,
  "right_panel_section": "git",
  "expanded_paths": [],
  "collapsed_workspace_ids": [],
  "project_base_branches": {},
  "collapsed_agent_pane_ids": [],
  "selected_path": null,
  "selected_pane_id": null,
  "shortcut_bindings": {},
  "pet_visible": false,
  "pet_origin": null,
  "pet_shortcut": null,
  "focused_device_id": null,
  "focused_checkout_id": null,
  "workspace_registrations": [],
  "device_registrations": [],
  "accent_hex": "#7AA2F7",
  "font_size": 13.0,
  "pane_text_scales": {},
  "editor_text_scale": 1.0,
  "pane_read_records": {},
  "pane_terminal_sizes": {}
}
EOF
  printf 'mode=%s\nstate_path=%s\n' "$mode" "$path"
}

measure_mode() {
  local mode="$1" visible="$2"
  local mode_dir="$evidence/$mode"
  local state_path="$mode_dir/ui-state.json"
  mkdir -p "$mode_dir/samples"
  write_state "$mode" "$visible" "$state_path" >"$mode_dir/context.txt"
  uptime >>"$mode_dir/context.txt"
  printf 'started_at=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >>"$mode_dir/context.txt"

  HOME="$fixture_home" HERDR_SOCKET_PATH="$socket_path" \
    "$binary" --state-path "$state_path" >"$mode_dir/app.log" 2>&1 &
  app_pid=$!
  printf 'pid=%s\n' "$app_pid" >>"$mode_dir/context.txt"
  sleep 6
  kill -0 "$app_pid" 2>/dev/null || { echo "AC25: app exited in $mode mode" >&2; return 1; }

  : >"$mode_dir/children-20s.txt"
  local second child_ids
  for second in $(seq 1 20); do
    printf 'second=%s time=%s\n' "$second" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
      >>"$mode_dir/children-20s.txt"
    child_ids="$(pgrep -P "$app_pid" | tr '\n' ',' | sed 's/,$//' || true)"
    if [[ -n "$child_ids" ]]; then
      ps -o pid=,ppid=,comm=,args= -p "$child_ids" >>"$mode_dir/children-20s.txt" 2>/dev/null || true
    else
      printf '(none)\n' >>"$mode_dir/children-20s.txt"
    fi
    sleep 1
  done

  local attempt=0 valid=0 sample_path
  while [[ "$valid" -lt 5 && "$attempt" -lt 12 ]]; do
    attempt=$((attempt + 1))
    uptime >"$mode_dir/samples/load-$attempt.txt"
    sample_path="$mode_dir/samples/sample-$attempt.txt"
    /usr/bin/sample "$app_pid" 3 -file "$sample_path" >/dev/null 2>&1 || true
    if rg -q 'Main Thread|com\.apple\.main-thread' "$sample_path" && rg -q 'HerdrMacOS_main' "$sample_path"; then
      valid=$((valid + 1))
    fi
  done
  printf 'valid_samples=%s\nsample_attempts=%s\n' "$valid" "$attempt" >>"$mode_dir/context.txt"
  printf 'finished_at=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >>"$mode_dir/context.txt"
  uptime >>"$mode_dir/context.txt"

  kill -TERM "$app_pid"
  wait "$app_pid" 2>/dev/null || true
  app_pid=""
  [[ "$valid" -eq 5 ]] || { echo "AC25: fewer than five symbolicated samples in $mode mode" >&2; return 1; }
}

measure_mode shown true
measure_mode hidden false

python3 - "$evidence" <<'PY'
import json
import re
import sys
from pathlib import Path

root = Path(sys.argv[1]).resolve()

def load_value(text):
    match = re.search(r"load averages?:\s*([0-9.]+)", text)
    if not match:
        raise SystemExit("AC25: load average was not recorded")
    return float(match.group(1))

def sample_result(path):
    text = path.read_text(errors="replace")
    main = re.search(r"^\s*(\d+) Thread_.*(?:Main Thread|com\.apple\.main-thread)", text, re.M)
    valid = bool(main and "HerdrMacOS_main" in text and "Call graph:" in text)
    total = int(main.group(1)) if main else 0
    wait = 0
    if valid:
        lines = text.splitlines()
        start = next(i for i, line in enumerate(lines) if re.search(r"Thread_.*(?:Main Thread|com\.apple\.main-thread)", line))
        end = len(lines)
        for i in range(start + 1, len(lines)):
            if re.match(r"^\s*\d+ Thread_", lines[i]):
                end = i
                break
        main_lines = lines[start:end]
        for i, line in enumerate(main_lines):
            if "herdr_core_snapshot" not in line:
                continue
            count = re.match(r"^[^0-9]*(\d+)\s", line)
            descendants = "\n".join(main_lines[i:i + 30])
            if re.search(r"Mutex.*lock|mutex_lock|__ulock_wait", descendants, re.I):
                wait = max(wait, int(count.group(1)) if count else 0)
    return {
        "path": str(path.resolve()),
        "valid": valid,
        "main_samples": total,
        "mutex_wait_samples": wait,
        "mutex_wait_percent": round((wait / total * 100.0) if total else 0.0, 4),
    }

analysis = {"protocol": {"idle_seconds": 20, "sample_seconds": 3, "sample_windows": 5}, "modes": {}}
for mode in ("shown", "hidden"):
    mode_root = root / mode
    loads = [load_value((mode_root / "context.txt").read_text())]
    loads += [load_value(p.read_text()) for p in sorted((mode_root / "samples").glob("load-*.txt"))]
    children = (mode_root / "children-20s.txt").read_text().splitlines()
    seconds = {int(m.group(1)) for line in children if (m := re.match(r"second=(\d+) ", line))}
    if seconds != set(range(1, 21)):
        raise SystemExit(f"AC25: {mode} lacks all twenty child observations")
    forbidden = []
    for line in children:
        parts = line.strip().split(maxsplit=3)
        if len(parts) >= 3 and parts[0].isdigit() and Path(parts[2]).name in {"git", "du"}:
            forbidden.append(line)
    samples = [sample_result(p) for p in sorted((mode_root / "samples").glob("sample-*.txt"))]
    valid = [sample for sample in samples if sample["valid"]][:5]
    mean = sum(sample["mutex_wait_percent"] for sample in valid) / len(valid) if valid else None
    analysis["modes"][mode] = {
        "maximum_load_1m": max(loads),
        "load_under_14": max(loads) < 14,
        "new_git_or_du_observations": len(forbidden),
        "forbidden_process_lines": forbidden,
        "symbolication_failures": len(samples) - len([s for s in samples if s["valid"]]),
        "valid_sample_windows": len(valid),
        "mean_mutex_wait_percent": round(mean, 4) if mean is not None else None,
        "samples": samples,
    }

shown = analysis["modes"]["shown"]["mean_mutex_wait_percent"]
hidden = analysis["modes"]["hidden"]["mean_mutex_wait_percent"]
analysis["difference_percentage_points"] = round(shown - hidden, 4) if shown is not None and hidden is not None else None
analysis["verdict"] = "pass" if all((
    analysis["modes"]["shown"]["load_under_14"],
    analysis["modes"]["hidden"]["load_under_14"],
    analysis["modes"]["shown"]["new_git_or_du_observations"] == 0,
    analysis["modes"]["hidden"]["new_git_or_du_observations"] == 0,
    analysis["modes"]["shown"]["valid_sample_windows"] == 5,
    analysis["modes"]["hidden"]["valid_sample_windows"] == 5,
    analysis["difference_percentage_points"] <= 0.5,
)) else "fail"
(root / "analysis.json").write_text(json.dumps(analysis, indent=2) + "\n")
print(json.dumps(analysis, indent=2))
if analysis["verdict"] != "pass":
    raise SystemExit("AC25: measured bounds failed")
PY

printf '%s\n' "$evidence" >"$run_dir/verification/performance-harness-latest.txt"
echo "AC25: PASS - $evidence"
