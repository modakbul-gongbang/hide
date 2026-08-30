# hide design reference

hide is a dark-only macOS workspace navigator for people moving between repositories, checkouts, panes, and agents.
Its visual language is quiet and operational: one accent for focus, state-specific color for attention, compact rows, and a clear consequence before a destructive action.

## Reference

The interaction and visual direction is informed by Raycast's public design principles: fast, simple, and delightful, plus its keyboard-first list, form, and action-panel patterns.

- Raycast design principles: https://www.raycast.com/blog/a-fresh-look-and-feel
- Raycast UI patterns: https://developers.raycast.com/api-reference/user-interface
- Raycast open-source reference repository: https://github.com/raycast/extensions

The implementation in this repository is original SwiftUI code.
No Raycast source code, logo, or proprietary asset is bundled.

## License note

The Raycast Extensions reference repository is published under the MIT License.
This note records the source and license context for the reference only; it does not change hide's license.
See the repository's license at https://github.com/raycast/extensions/blob/main/LICENSE.

## hide tokens

The single token baseline lives in `macos/Sources/HerdrMacOS/HideUI.swift` under `HideTheme`.
The product is dark-only, with obsidian surfaces, restrained separators, a lime focus accent, and status colors reserved for operational meaning.
Dialogs state the consequence before confirmation, and the most frequent actions - checkout and agent navigation - are one click from the sidebar.
