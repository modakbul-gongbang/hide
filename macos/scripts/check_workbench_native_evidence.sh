#!/usr/bin/env bash

set -euo pipefail

if [[ $# -ne 6 ]]; then
    echo "usage: $0 <native-run-dir> <dev-app-bundle> <app-pid> <server-pid> <minimum-fixture-files> <expected-branch>" >&2
    exit 2
fi

native_run_dir="$1"
dev_app_bundle="$2"
expected_app_pid="$3"
expected_server_pid="$4"
minimum_fixture_files="$5"
expected_branch="$6"
repository_root="$(cd "$(dirname "$0")/../.." && pwd)"
fixture_root="$native_run_dir/workspace"

case "$native_run_dir" in
    agents/runs/*/native) ;;
    *)
        echo "native evidence must stay under agents/runs/<slug>/native" >&2
        exit 1
        ;;
esac

[[ "$(git -C "$repository_root" branch --show-current)" == "$expected_branch" ]]
codesign --verify --deep --strict "$dev_app_bundle"
[[ -s "$dev_app_bundle/Contents/Resources/THIRD_PARTY_NOTICES/seti-ui-MIT.txt" ]]
[[ "$(git -C "$fixture_root" branch --show-current)" == "verification-workbench" ]]

fixture_count="$(find "$fixture_root" -type f -not -path '*/.git/*' | wc -l | tr -d ' ')"
screenshot_count="$(find "$native_run_dir" -maxdepth 1 -type f -name '*.png' -size +0c | wc -l | tr -d ' ')"
[[ "$fixture_count" -ge "$minimum_fixture_files" ]]
[[ "$screenshot_count" -ge 9 ]]

required_evidence=(
    "$native_run_dir/app-sample.txt"
    "$native_run_dir/herdr-server-sample.txt"
    "$native_run_dir/dev-slot-handoff-prequit-process.txt"
    "$native_run_dir/dev-slot-handoff-postquit-windows.json"
    "$native_run_dir/installed-slot-handoff-stat.txt"
)
for evidence_file in "${required_evidence[@]}"; do
    [[ -s "$evidence_file" ]]
done

rg -q "Process:         HerdrMacOS \\[$expected_app_pid\\]" "$native_run_dir/app-sample.txt"
rg -q "Process:         herdr \\[$expected_server_pid\\]" "$native_run_dir/herdr-server-sample.txt"
rg -q "$expected_app_pid.*macos/build/assembled/hide.app.*--verification-ui-fixture.*$native_run_dir/workspace" \
    "$native_run_dir/dev-slot-handoff-prequit-process.txt"
rg -q "APP_NOT_FOUND" "$native_run_dir/dev-slot-handoff-postquit-windows.json"

"$repository_root/macos/scripts/check_workbench_ownership.sh"
git -C "$repository_root" diff --check
[[ -z "$(git -C "$repository_root" status --porcelain=v1 --untracked-files=all -- agents)" ]]

echo "workbench native evidence verified: fixture_files=$fixture_count screenshots=$screenshot_count"
