#!/usr/bin/env bash
# Shell resources are read through `PackagedResourceBundle.app`, never through
# SwiftPM's generated `Bundle.module`.
#
# The generated accessor for an executable target looks in two places: beside
# the executable's `Bundle.main.bundleURL` and at the absolute path of the
# resource bundle in the build tree that produced the binary. The installed
# app keeps the bundle under `Contents/Resources`, which is neither, so every
# `Bundle.module` read succeeded only through the build-tree fallback. On
# 2026-09-11 the worktree that built the installed app was removed and the app
# died at launch inside `HideTheme.inter`, before its first window.
#
# `PackagedResourceBundle.app` looks in `Contents/Resources` first and reports
# a missing bundle as a diagnostic; this gate keeps every read on it.
set -euo pipefail

cd "$(dirname "$0")/.."

if matches=$(grep -rn 'Bundle\.module' macos/Sources/HerdrMacOS); then
    printf 'read shell resources through PackagedResourceBundle.app, not Bundle.module:\n%s\n' "$matches" >&2
    exit 1
fi
