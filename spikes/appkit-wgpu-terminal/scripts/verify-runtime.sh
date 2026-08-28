#!/bin/zsh
set -euo pipefail

script_dir=${0:A:h}
spike_root=${script_dir:h}
manifest="$spike_root/fixtures/runtime-manifest.json"

command -v jq >/dev/null || { print -u2 -- "stage=fixture.read cause=jq-not-found"; exit 1; }

bundle_relative=$(jq -er '.bundle_relative_path' "$manifest")
executable_relative=$(jq -er '.executable_relative_path' "$manifest")
probe_count=$(jq -er '.probe_count' "$manifest")
autoclose_ms=$(jq -er '.autoclose_ms' "$manifest")
report_relative=$(jq -er '.artifacts.runtime_report' "$manifest")
log_relative=$(jq -er '.artifacts.runtime_log' "$manifest")
ax_relative=$(jq -er '.artifacts.ax_tree' "$manifest")
metrics_relative=$(jq -er '.artifacts.metrics' "$manifest")
split_relative=$(jq -er '.screenshots.split' "$manifest")
zoom_relative=$(jq -er '.screenshots.zoom' "$manifest")

bundle_path="$spike_root/$bundle_relative"
executable_path="$spike_root/$executable_relative"
report_path="$spike_root/$report_relative"
log_path="$spike_root/$log_relative"
ax_path="$spike_root/$ax_relative"
metrics_path="$spike_root/$metrics_relative"
split_path="$spike_root/$split_relative"
zoom_path="$spike_root/$zoom_relative"

mkdir -p "${report_path:h}" "${split_path:h}"

existing_pids=$(pgrep -f "$executable_path" || true)
if [[ -n "$existing_pids" ]]; then
  print -u2 -- "stage=instance.preflight cause=existing-instance pids=$existing_pids"
  exit 1
fi

"$script_dir/build-app.sh"

"$executable_path" --report "$report_path" --probe-count "$probe_count" --autoclose-ms "$autoclose_ms" >"$log_path" 2>&1 &
app_pid=$!

cleanup() {
  if kill -0 "$app_pid" 2>/dev/null; then
    kill -TERM "$app_pid" 2>/dev/null || true
    wait "$app_pid" 2>/dev/null || true
  fi
}
trap cleanup EXIT

for _ in {1..200}; do
  if [[ -s "$report_path" ]] && jq -e '.window_id > 0 and .input_samples >= 32' "$report_path" >/dev/null 2>&1; then
    break
  fi
  if ! kill -0 "$app_pid" 2>/dev/null; then
    print -u2 -- "stage=runtime.wait cause=app-exited-before-report"
    tail -n 80 "$log_path" >&2
    exit 1
  fi
  sleep 0.05
done

jq -e '.window_id > 0 and .input_samples >= 32' "$report_path" >/dev/null
instance_count=$(pgrep -f "$executable_path" | wc -l | tr -d ' ')
[[ "$instance_count" == "1" ]] || { print -u2 -- "stage=instance.runtime expected=1 actual=$instance_count"; exit 1; }

window_id=$(jq -er '.window_id' "$report_path")
/usr/sbin/screencapture -x -l "$window_id" "$split_path"
/usr/bin/osascript "$script_dir/inspect-ax.applescript" "$app_pid" >"$ax_path"

split_state=$(jq -er '.zoom_state' "$report_path")
/usr/bin/osascript "$script_dir/send-zoom.applescript" "$app_pid"
for _ in {1..80}; do
  [[ $(jq -r '.zoom_state' "$report_path") == "Pane zoomed" ]] && break
  sleep 0.05
done
zoom_state=$(jq -er '.zoom_state' "$report_path")
[[ "$zoom_state" == "Pane zoomed" ]] || { print -u2 -- "stage=shortcut.zoom cause=state-not-updated actual=$zoom_state"; exit 1; }
/usr/sbin/screencapture -x -l "$window_id" "$zoom_path"

/usr/bin/osascript "$script_dir/send-zoom.applescript" "$app_pid"
for _ in {1..80}; do
  [[ $(jq -r '.zoom_state' "$report_path") == "Split layout" ]] && break
  sleep 0.05
done
restored_state=$(jq -er '.zoom_state' "$report_path")
[[ "$restored_state" == "Split layout" ]] || { print -u2 -- "stage=shortcut.restore cause=state-not-updated actual=$restored_state"; exit 1; }

sleep 2
cpu_samples=()
rss_samples=()
for _ in {1..5}; do
  process_sample=$(ps -p "$app_pid" -o %cpu=,rss= | xargs)
  [[ -n "$process_sample" ]] || { print -u2 -- "stage=metrics.sample cause=process-missing"; exit 1; }
  cpu_samples+=("${process_sample%% *}")
  rss_samples+=("${process_sample##* }")
  sleep 0.5
done

idle_cpu=$(printf '%s\n' "${cpu_samples[@]}" | awk '{sum += $1} END {if (NR == 0) exit 1; printf "%.3f", sum / NR}')
rss_kb=$(printf '%s\n' "${rss_samples[@]}" | sort -nr | head -1)
rss_mb=$(awk -v rss="$rss_kb" 'BEGIN {printf "%.3f", rss / 1024}')

jq -n \
  --arg schema "herdr.native-spike.metrics.v1" \
  --arg bundle "$bundle_path" \
  --arg split_state "$split_state" \
  --arg zoom_state "$zoom_state" \
  --arg restored_state "$restored_state" \
  --argjson instance_count "$instance_count" \
  --argjson idle_cpu_percent "$idle_cpu" \
  --argjson browser_closed_rss_mb "$rss_mb" \
  --argjson first_usable_ms "$(jq '.first_usable_ms' "$report_path")" \
  --argjson input_to_present_p95_ms "$(jq '.input_to_present_p95_ms' "$report_path")" \
  --argjson input_samples "$(jq '.input_samples' "$report_path")" \
  --argjson physical_width "$(jq '.physical_width' "$report_path")" \
  --argjson physical_height "$(jq '.physical_height' "$report_path")" \
  --argjson scale_factor "$(jq '.scale_factor' "$report_path")" \
  --arg adapter "$(jq -r '.adapter' "$report_path")" \
  '{schema: $schema, bundle: $bundle, exactly_one_instance: ($instance_count == 1), instance_count: $instance_count, first_usable_ms: $first_usable_ms, input_samples: $input_samples, input_to_present_p95_ms: $input_to_present_p95_ms, idle_cpu_percent: $idle_cpu_percent, browser_closed_rss_mb: $browser_closed_rss_mb, physical_size: [$physical_width, $physical_height], scale_factor: $scale_factor, adapter: $adapter, zoom_sequence: [$split_state, $zoom_state, $restored_state]}' >"$metrics_path"

wait "$app_pid"
trap - EXIT

remaining=$(pgrep -f "$executable_path" || true)
[[ -z "$remaining" ]] || { print -u2 -- "stage=instance.cleanup cause=remaining-process pids=$remaining"; exit 1; }

print -r -- "bundle=$bundle_path"
print -r -- "report=$report_path"
print -r -- "metrics=$metrics_path"
print -r -- "ax=$ax_path"
print -r -- "split_screenshot=$split_path"
print -r -- "zoom_screenshot=$zoom_path"
