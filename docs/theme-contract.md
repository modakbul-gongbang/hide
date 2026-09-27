# Theme Asset Contract

`assets/pet-theme/<theme-id>/theme.json` is the source of truth for the
pose-to-asset mapping. There is no pet window in the current app, and no
asset loader reads this manifest today; the pet window is tracked as
[backlog issue #184](https://github.com/modakbul-gongbang/hide/issues/184).
This file documents the contract a future pet reads.
`herdr-core/src/pet.rs` (`herdr_core::pet::pose`) is the core-side owner of
the pose a loader would draw; it computes the pose without drawing anything.

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

Nothing currently packages `assets/pet-theme` into an app bundle or reads it
at runtime; the artwork stays in the repository checkout only, waiting on the
Electron pet feature tracked in issue #184.
A future loader that finds no theme at all should report it on stderr and
draw an explicit "Pet art unavailable" state in the pet window - never an
empty window.

## Adding art

Add the manifest-referenced asset under `assets/`, then add or repoint its entry in `states`.
Use the bundled campfire artwork as the style reference and inspect `theme.json` for current filenames rather than maintaining a second slot table.
Keep a shared ground line and facing direction across sprite frames, and verify real alpha rather than a painted checkerboard background.
A PNG/WebP pair is not required by this contract.
A second theme needs all required poses; `default` is the bundled theme, so adding a directory alone does not add a theme selector.
