#!/usr/bin/env bash
set -euo pipefail

macos_root="$(cd "$(dirname "$0")/.." && pwd)"
repo_root="$(cd "$macos_root/.." && pwd)"

git -C "$repo_root" diff --quiet HEAD -- \
    macos/Sources/HerdrMacOS/AgentBadge.swift \
    macos/Sources/HerdrMacOS/OperationalModels.swift \
    macos/Sources/HerdrMacOS/PaneShortcutSettings.swift \
    macos/Sources/HerdrMacOS/RemoteTerminalHost.swift \
    macos/Sources/HerdrMacOS/ShellView.swift \
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

while IFS= read -r header; do
    case "$header" in
        *"struct ShellView: View"*|*"struct HideMainView: View"*|*"struct HideToolbar: View"*) ;;
        *)
            printf 'forbidden HideUI hunk: %s\n' "$header" >&2
            exit 1
            ;;
    esac
done < <(git -C "$repo_root" diff --unified=0 HEAD -- macos/Sources/HerdrMacOS/HideUI.swift | /usr/bin/grep '^@@' || true)

printf 'workbench ownership boundary verified\n'
