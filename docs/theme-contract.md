# Theme Asset Contract

`assets/pet-theme/<theme-id>/theme.json` is the source of truth for the
pose-to-asset mapping. The loader is
`macos/Sources/HerdrMacOS/PetTheme.swift`.

## Manifest

```json
{
  "schemaVersion": 2,
  "name": "Campfire",
  "version": "2.0.0",
  "states": {
    "idle": { "asset": "assets/idle-fire.webp" },
    "roam": { "asset": "assets/walk-sheet.png", "frames": 4, "durationMs": 800 }
  }
}
```

`schemaVersion`, `name`, `version`, and `states` are required.
A manifest declaring any other `schemaVersion` is refused rather than read on
a guess.

## Required states

A theme must supply art for **every** pose `herdr-core`'s pet block can
report. A theme that cannot draw one is rejected at load, so a missing pose
is a startup error rather than a blank pet the first time that pose occurs.

The thirteen poses are:

`idle`, `working`, `carrying`, `juggling`, `notification`, `error`,
`disconnected`, `roam`, `waking`, `yawning`, `dozing`, `collapsing`,
`sleeping`.

They correspond one-to-one with `herdr_core::pet::pose`; see
[status-model.md](status-model.md) for which agent state produces which pose.

## Animation: three kinds of asset, one declaration rule

The loader decides how to play a state from the file plus one optional field.

| Kind | Declared as | Played as |
| --- | --- | --- |
| Animated file | `{ "asset": "…​.webp" }` | ImageIO reports more than one frame; each frame's own delay drives the timing |
| Sprite sheet | `{ "asset": "…​.png", "frames": N, "durationMs": M }` | One image cut into `N` equal horizontal frames, `M / N` ms each |
| Static | `{ "asset": "…​.png" }` | The single frame, no timer |

`frames` is what distinguishes a sheet from an animated file, so declare it
only for a sheet. A sheet whose width does not divide evenly by `frames` is
rejected instead of rendering skewed slices.

Animated webp needs no conversion step: macOS ImageIO decodes it directly
(`CGImageSourceGetCount` plus `kCGImagePropertyWebPDelayTime`). The bundled
default theme uses animated webp for the six status poses and 4-frame sheets
for the motion poses.

## Asset rules

Assets are square PNG or webp with a real alpha channel, at least 512px on a
side, with the character boundary at least 15% away from every edge.
Sprite sheet frames sit in one horizontal row, equal width, on a shared
ground line.

Paths in `states` are relative to the theme directory.

## Where themes live at runtime

The installed bundle carries `assets/pet-theme` as
`HerdrIDE.app/Contents/Resources/pet-theme` (copied by
`macos/scripts/build_dev_app.sh`). A build run straight from the checkout has
no bundle, so the loader falls back to the repository copy, and
`--pet-theme-root <dir>` overrides both for verification.

Finding no theme at all is reported on stderr and drawn in the pet window as
an explicit "Pet art unavailable" state - never as an empty window.

## Adding art

Add the file pair under `assets/`, then add or repoint one line in `states`.
`docs/asset-prompts.md` has the generation prompts, and
[pet-assets.md](pet-assets.md) has the slot-by-slot guide.
