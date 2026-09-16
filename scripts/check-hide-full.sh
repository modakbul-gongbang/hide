#!/usr/bin/env bash
# One fail-closed entrypoint that runs everything CI requires plus the local
# gates CI cannot run. It is a superset of `verify` and `design-contract`, so a
# green run here predicts both; the reverse is not true.
#
# Keep this list equal to `.github/workflows/pr.yml` and `design-contract.yml`.
# `scripts/check-hide-design-enforcement.mjs` below fails when the workflow and
# the checker binding drift apart.
set -euo pipefail
export LC_ALL=en_US.UTF-8
export LC_CTYPE=en_US.UTF-8
export LANG=en_US.UTF-8
cd "$(dirname "$0")/.."
# The log lands beside the build output it describes, ignored and removed with
# the worktree.
mkdir -p target
exec > >(tee target/hide-full.log) 2>&1

# verify / rust and swift lanes
bash scripts/verify-cargo.sh lint
bash scripts/verify-cargo.sh test
bash scripts/verify-swift.sh test
bash scripts/check-right-panel-sections.sh
bash scripts/check-shortcut-contract.sh

# verify / repository invariants lane
python3 -m unittest discover -s scripts/tests -p 'test_*.py'
bash scripts/check-harness-ignore-anchor.sh
bash scripts/check-agent-asset-committed.sh
bash scripts/check-capability-readers-off-lock.sh
bash scripts/check-terminal-row-cache.sh
bash scripts/check-packaged-resource-access.sh
bash scripts/check-no-workstation-identity.sh
bash scripts/check-git-worktree-presentation.sh
bash scripts/check-git-worktree-states.sh
bash scripts/check-worktree-base-policy.sh
bash scripts/check-worktree-catalog-presentation.sh
bash scripts/check-worktree-removal-boundary.sh
zsh scripts/check-herdr-pin-single-source.sh
zsh scripts/check-herdr-contract.sh --schema-only

# design-contract workflow
node scripts/check-design-contract.mjs
node --test scripts/tests/design-controls.test.mjs

# local only: DESIGN.md lint needs the network, and the enforcement checker
# asserts the workflow still binds the commands above.
node scripts/check-hide-design.mjs
node scripts/check-hide-design-enforcement.mjs
