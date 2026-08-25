# Theme Asset Contract

`themes/<theme-id>/theme.json` is the source of truth for state-to-asset mapping.

The manifest must contain `schemaVersion`, `name`, `version`, `viewBox`, and `states`.

The eight required state keys are `idle`, `working`, `attention-0`, `attention-1`, `attention-2`, `attention-3`, `error`, and `disconnected`.

`sleeping` is optional.

PNG assets must be square, at least 1024px on each side, contain a real alpha channel, and keep the character boundary at least 15% away from every edge.

SVG assets may be used by a theme and may expose `#eyes-js`, `#body-js`, and `#shadow-js` for cursor tracking and transform animation.

The implementation applies motion in code, so state files are static PNG or SVG assets rather than sprite sheets or animated formats.

The supplied default theme follows the guide in `docs/asset-prompts.md` and uses one consistent blue slime character across all nine supplied states.
