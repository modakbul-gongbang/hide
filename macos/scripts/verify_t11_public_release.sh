#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
remote_scan="$repo_root/docs/verification/evidence/hide-rebrand/t11-public-remote-scan-20260831.md"
release_record="$repo_root/docs/verification/evidence/hide-rebrand/t11-public-release-20260831.md"

test -s "$remote_scan"
test -s "$release_record"

/usr/bin/grep -Fq '| macOS or Linux local home paths | 0 |' "$remote_scan"
/usr/bin/grep -Fq '| Known local account identifiers | 0 |' "$remote_scan"
/usr/bin/grep -Fq '| Personal email address | 0 |' "$remote_scan"
/usr/bin/grep -Fq '| Credential-shaped token values | 0 |' "$remote_scan"
/usr/bin/grep -Fq 'Final tag: `v0.1.9`' "$release_record"
/usr/bin/grep -Fq 'hide-v0.1.9-macos-arm64.zip: OK' "$release_record"
/usr/bin/grep -Fq 'The release is still a draft and has not been published.' "$release_record"

printf '%s\n' 'T11 public release evidence: OK'
