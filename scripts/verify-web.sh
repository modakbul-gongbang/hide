#!/usr/bin/env bash
# The web and desktop entrypoint for verification, as plain argv.
#
# Usage: verify-web.sh [install [--ignore-scripts] | PACKAGE ACTION [args...]]
# PACKAGE: web, desktop, hcoord. ACTION: typecheck, lint, test, build, e2e,
# playwright-install, package (desktop only), test:e2e (hcoord only).
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

# Explicit CI modes retain step boundaries and their own failure outcomes.
if (( $# > 0 )); then
    if [[ "$1" == install ]]; then
        shift
        if (( $# > 1 )) || { (( $# == 1 )) && [[ "$1" != --ignore-scripts ]]; }; then
            printf 'usage: %s install [--ignore-scripts]\n' "$0" >&2
            exit 2
        fi
        exec pnpm install --frozen-lockfile "$@"
    fi
    (( $# >= 2 && $# <= 130 )) || { printf 'expected PACKAGE ACTION [args...]\n' >&2; exit 2; }
    package=$1
    action=$2
    shift 2
    case "$package" in
        web|desktop) directory=$package ;;
        hcoord) directory=plugins/hcoord ;;
        *) printf 'unknown verification package: %s\n' "$package" >&2; exit 2 ;;
    esac
    case "$action" in
        typecheck|lint|test|build) exec pnpm --dir "$directory" "$action" "$@" ;;
        e2e)
            [[ "$package" != hcoord ]] || { printf 'use hcoord test:e2e\n' >&2; exit 2; }
            if [[ "$package" == desktop ]]; then
                pnpm --dir "$directory" build
            fi
            exec pnpm --dir "$directory" exec playwright test "$@"
            ;;
        playwright-install)
            [[ "$package" != hcoord && $# == 0 ]] || { printf 'invalid playwright-install arguments\n' >&2; exit 2; }
            exec pnpm --dir "$directory" exec playwright install chromium
            ;;
        package)
            [[ "$package" == desktop && $# == 0 ]] || { printf 'package requires desktop and no arguments\n' >&2; exit 2; }
            exec pnpm --dir desktop package
            ;;
        test:e2e)
            [[ "$package" == hcoord && $# == 0 ]] || { printf 'test:e2e requires hcoord and no arguments\n' >&2; exit 2; }
            exec pnpm --dir "$directory" test:e2e
            ;;
        *) printf 'unknown verification action: %s\n' "$action" >&2; exit 2 ;;
    esac
fi

pnpm install --frozen-lockfile

pnpm --dir plugins/hcoord typecheck
pnpm --dir plugins/hcoord build
pnpm --dir plugins/hcoord test
pnpm --dir plugins/hcoord test:e2e

for package in web desktop; do
    pnpm --dir "$package" typecheck
    pnpm --dir "$package" lint
    pnpm --dir "$package" test
    pnpm --dir "$package" build
done
