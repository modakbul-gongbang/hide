#!/usr/bin/env bash
set -euo pipefail

macos_root="$(cd "$(dirname "$0")/.." && pwd)"
repo_root="$(cd "$macos_root/.." && pwd)"

git -C "$repo_root" diff --quiet HEAD -- \
    macos/Sources/HerdrMacOS/AgentBadge.swift \
    macos/Sources/HerdrMacOS/OperationalModels.swift \
    macos/Sources/HerdrMacOS/PaneShortcutSettings.swift \
    macos/Sources/HerdrMacOS/TerminalHost.swift \
    macos/Sources/HerdrMacOS/ImeTerminalView.swift \
    macos/Sources/HerdrMacOS/PetAnimation.swift \
    macos/Sources/HerdrMacOS/PetHotkey.swift \
    macos/Sources/HerdrMacOS/PetMenuBar.swift \
    macos/Sources/HerdrMacOS/PetSettings.swift \
    macos/Sources/HerdrMacOS/PetTheme.swift \
    macos/Sources/HerdrMacOS/PetURLCommand.swift \
    macos/Sources/HerdrMacOS/PetView.swift \
    macos/Sources/HerdrMacOS/PetWindow.swift

if git -C "$repo_root" ls-files --error-unmatch macos/Sources/HerdrMacOS/HideUI.swift >/dev/null 2>&1 \
    && git -C "$repo_root" grep -qF -- 'struct HideSidebar: View' macos/Sources/HerdrMacOS/HideUI.swift; then
    printf 'HideSidebar implementation still lives in HideUI.swift\n' >&2
    exit 1
fi

test -f "$macos_root/Sources/HerdrMacOS/HideSidebar.swift"
test -f "$macos_root/Sources/HerdrMacOS/HideTerminalSurface.swift"
test -f "$macos_root/Sources/HerdrMacOS/ShellRootView.swift"
test -f "$macos_root/Sources/HerdrMacOS/WorktreeCreationSheet.swift"

if git -C "$repo_root" ls-files --error-unmatch macos/Sources/HerdrMacOS/HideUI.swift >/dev/null 2>&1; then
    printf 'obsolete HideUI.swift remains tracked\n' >&2
    exit 1
fi

printf 'workbench ownership boundary verified\n'
