# Pet Asset Guide

How the pet chooses its image, and how to add or replace art safely.

## Where assets live

All art lives in `themes/default/assets/`.
The Vite build copies the whole `themes/` tree into `dist/themes/`, so editing the source folder and rebuilding is the entire deployment step.
The frontend resolves every image through one helper in `web/app.js`:

```js
const asset = (name, extension = 'png') => `/themes/default/assets/${name}.${extension}`;
```

Every asset ships as a **png + webp pair with the same basename**.
`petImage()` renders a `<picture>` that prefers webp and falls back to png, and the png also serves the `prefers-reduced-motion` branch.
If you add art, always add both files.

## Asset slots

| Slot | File basename | When it shows |
| --- | --- | --- |
| idle / done | `idle-fire` | nothing urgent |
| working | `working-fire-v5` + `working-ingot` (orbiting) | agents working |
| attention | `attention-fire` | unseen question/approval |
| error | `error-fire` | unseen error |
| disconnected | `disconnected-fire` | herdr unreachable |
| yawning | `yawning-fire` | 60s idle, first 4s |
| dozing | `dozing-fire` | next 4s |
| collapsing | `collapsing-fire` | next 4s |
| sleeping | `sleeping-z-fire` | until woken |
| sleepy-character idle | `sleeping-fire` | "Sleepy campfire" character setting |
| walk cycle | `walk-sheet` (runtime) + `walk-1..4` (unused at runtime, kept as sheet source frames) | while roaming |

Status → basename mapping lives in two small tables in `web/app.js`:
`campfireAssets` (status states) and `sleepPhaseAssets` (sleep phases).
Adding a new state image means adding the file pair and one line in the right table.

## Animated slots: sprite sheets, not GIFs

The walk animation is a **horizontal sprite sheet** (`walk-sheet.png`, 4 frames of 512x512, character facing left natively) driven by CSS:

```css
.walk-sprite { background-size: 400% 100%; }
html.walking .walk-sprite { animation: walk-cycle .8s steps(4) infinite; }
```

Use this pattern for any future frame animation.
Do not drive frames from a JS timer: the render webview is unfocused, so its timers are throttled to ~100ms+ and the animation stutters (this was learned the hard way).
CSS `steps()` runs regardless of timer throttling.
APNG/GIF would also work for simple loops, but a sprite sheet keeps per-frame control (direction flip, pause) and both png+webp variants.

Conventions for a new sheet:

- Frames in one horizontal row, equal width, character on a shared ground line.
- Character faces **left** in the source art is fine; the CSS flips with `scaleX(-1)` for the other direction - keep one convention and set the flip rule accordingly (current sheet: faces left natively, flipped when walking right).
- `background-size: (N*100)% 100%` and keyframe end `background-position-x: (N*100)/(N-1) ... ` - for 4 frames the magic end value is `133.3334%`.

## Generating new art with ima2

The current set was produced with `ima2`, and consistency comes from reference images:

- New pose of the existing character: `ima2 edit <existing-asset>.png -p "<change only the flame ...>"` - keeps logs/stones/palette identical.
- New action (e.g. walk frames): `ima2 gen "<prompt>" --ref themes/default/assets/idle-fire.png` and ask for a sprite sheet ("exactly N frames in one horizontal row, same size and ground line").
- Generated backgrounds are often *painted* white/checkerboard, not true alpha.
  Strip them with an edge flood-fill (light, low-saturation pixels only) rather than a global color key, so the character's own light pixels survive.

## Character switching in settings

The settings window already has a `character` select (`orb` = Campfire, `sleepy`, `signal`) routed through `petAsset()`.
To add a selectable character set: add a new option there, and branch the basename tables on `state.settings.character` - the asset helper and png/webp pairing stay unchanged.
A future "theme pack" would generalize the hardcoded `default` in `asset()` to a setting, since the directory layout already namespaces by theme.
