# Pet Asset Guide

How the pet chooses its image, and how to add or replace art safely.

## Where assets live

All art lives in `assets/pet-theme/<theme-id>/assets/`, and
`assets/pet-theme/<theme-id>/theme.json` maps a pose onto a file.
`macos/scripts/build_dev_app.sh` copies the whole `assets/pet-theme` tree into
`HerdrIDE.app/Contents/Resources/pet-theme`, so editing the source folder and
rebuilding the bundle is the entire deployment step.

Nothing in the shell hardcodes a filename. `PetTheme.swift` resolves every
pose through the manifest, and a pose the manifest does not cover is a load
error, not a blank pet. The manifest rules are in
[theme-contract.md](theme-contract.md).

Assets ship as a **png + webp pair with the same basename**. The native
renderer reads whichever the manifest names; the pair is kept because the
webp variants of the six status poses carry the animation frames while their
png counterparts are single stills.

## Asset slots

The bundled `default` theme is the campfire character.

| Pose | File | When it shows |
| --- | --- | --- |
| idle | `idle-fire.webp` | nothing urgent, under 8s |
| roam | `walk-sheet.png` | 8s idle, free roaming |
| working | `working-fire.webp` | working, reserved rung of the ladder |
| carrying | `carrying-sheet.png` | exactly one working pane |
| juggling | `juggling-sheet.png` | two or more working panes |
| notification | `attention-fire.webp` | unseen question or approval |
| error | `error-fire.webp` | unseen error |
| disconnected | `disconnected-fire.webp` | herdr unreachable |
| yawning | `yawning-fire.png` | 60s idle, first 4s |
| dozing | `dozing-fire.png` | next 4s |
| collapsing | `collapsing-fire.png` | next 4s |
| sleeping | `sleeping-z-fire.png` | until woken |
| waking | `waking-sheet.png` | activity interrupts sleep |

Which pose a given agent state produces is decided in
`herdr_core::pet::pose`; see [status-model.md](status-model.md).

Adding a new state image means adding the file pair and one line in
`theme.json`.

The blue-slime set (`idle.png`, `working.png`, `attention-0..3.png`,
`error.png`, `disconnected.png`, `sleeping.png`, `idle.svg`) is an alternate
character that the default theme does not reference. It has no animation
frames and does not cover the motion or sleep-sequence poses, so a theme
built on it would need new art for those before it could load.

## Animated slots: two mechanisms, both native

The renderer plays frames itself; there is no CSS and no webview.

**Animated webp.** The six status poses are multi-frame webp with embedded
per-frame delays. macOS ImageIO decodes them directly
(`CGImageSourceGetCount` plus `kCGImagePropertyWebPDelayTime`), so they need
no conversion and declare nothing in the manifest.

**Sprite sheets.** The motion poses are horizontal sheets (2048x512, four
512x512 frames) declared as `{"frames": 4, "durationMs": 800}`.
`PetAnimationLoader` slices them with `CGImage.cropping`.

Frames advance on a `.common`-mode run loop timer. The pet window is
deliberately never key, and unlike a webview's throttled JS timers, an
AppKit run loop timer in `.common` mode keeps firing while the window is
unfocused.

Conventions for a new sheet:

- Frames in one horizontal row, equal width, character on a shared ground
  line. A sheet whose width does not divide evenly by its frame count is
  rejected at load rather than rendered skewed.
- Keep one facing convention across the sheet; the current walk sheet faces
  left natively.

## Generating new art with ima2

The current set was produced with `ima2`, and consistency comes from
reference images:

- New pose of the existing character:
  `ima2 edit <existing-asset>.png -p "<change only the flame ...>"` - keeps
  logs, stones, and palette identical.
- New action (e.g. walk frames):
  `ima2 gen "<prompt>" --ref assets/pet-theme/default/assets/idle-fire.png`
  and ask for a sprite sheet ("exactly N frames in one horizontal row, same
  size and ground line").
- Generated backgrounds are often *painted* white or checkerboard, not true
  alpha. Strip them with an edge flood-fill (light, low-saturation pixels
  only) rather than a global colour key, so the character's own light pixels
  survive.

`docs/asset-prompts.md` holds the prompts the current set was generated from.

## Adding a second theme

The loader already namespaces by theme id; `theme.json` is the only contract.
A new theme is a sibling directory under `assets/pet-theme/` that supplies
art for all thirteen poses. This release bundles one theme, and the theme id
the core reports is fixed at `default`.
