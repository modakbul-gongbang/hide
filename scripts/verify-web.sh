#!/usr/bin/env bash
# The web and desktop entrypoint for verification, as plain argv.
#
# Usage: verify-web.sh
#
# Runs what the `hcoord`, `web shell` and `desktop app` CI lanes run short of
# the Playwright flows: hcoord typecheck, unit and e2e tests and build, then
# typecheck, lint, unit tests and the build of both application packages.
# The PRD harness runs each verify command with execvp and no
# shell, so the pnpm workspace install and the per-package steps live here
# rather than in a longer command string in `agents/config.json`:
#
#     "web": "bash scripts/verify-web.sh"
#
# Every artifact lands inside this worktree (`web/dist/`, `desktop/dist/`,
# both packages' `node_modules/`); see docs/BUILD.md.
set -euo pipefail

cd "$(dirname "$0")/.."

. scripts/toolchain-env.sh

pnpm install --frozen-lockfile

pnpm --dir plugins/hcoord typecheck
pnpm --dir plugins/hcoord test
pnpm --dir plugins/hcoord test:e2e
pnpm --dir plugins/hcoord build

for package in web desktop; do
    pnpm --dir "$package" typecheck
    pnpm --dir "$package" lint
    pnpm --dir "$package" test
    pnpm --dir "$package" build
done
