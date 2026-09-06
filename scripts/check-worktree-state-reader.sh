#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

cargo test --manifest-path herdr-core/Cargo.toml worktrees::behavior_tests -- --test-threads=1
cargo test --manifest-path herdr-core/Cargo.toml disk::tests

echo "Worktree rows preserve ancestry, upstream, dirty, detached, nested, missing, disk and measurement state"
