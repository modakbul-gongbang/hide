# Hide brand

## Core idea

hide is a multi-agent IDE that helps one person keep several coding efforts in view without watching every terminal.
It preserves the context of each task and brings forward the questions, approvals, and results that need a person's attention.

**Brand line:** Many agents. One clear view.
**Korean line:** 여러 에이전트의 흐름은 한눈에, 집중은 필요한 곳에만.

The product should make three things easy:

1. See which work is moving and where it belongs.
2. Notice a change that needs a decision or a review.
3. Return to that work without rebuilding its context.

These are product promises, not claims that hide completes or verifies an agent's work on the operator's behalf.
The actual attention groups and read rules are owned by [the status model](status-model.md).

## Mark

The approved mark is the owl face in [hide-mark.png](../design/brand/hide-mark.png).
Its two eyes suggest parallel work, while the connected lime shape brings them into one field of view.
The broad ivory face and open blue-gray space keep the expression calm and legible.
It is a symbol of awareness, not a character that speaks for the agents or a live status indicator.

Use the lowercase `hide` wordmark in text beside the mark when the product name is needed.
The mark itself contains no lettering.

| Role | Reference color | Use |
| --- | --- | --- |
| Quiet field | `#B7C9CC` | The mark's pale blue-gray background |
| Face | `#F4F4F6` | The owl's broad ivory silhouette |
| Shared view | `#B9FF66` | The connected eye shape and restrained brand emphasis |
| Focus | `#101112` | The two eyes and dark text on the light field |

These values describe the artwork; they do not replace the web shell's functional tokens or status colors.
Keep the image's proportions, lower-right crop, eye spacing, and full square background.
Do not add glow, gradients, expressions, accessories, or a different color to signal agent state.
At small sizes, use the full mark rather than extracting either eye.

The macOS app icon at `desktop/resources/hide-icon-1024.png` is the mark inside a rounded 896-pixel tile with a 64-pixel transparent inset on a 1024-pixel canvas.
`desktop/resources/hide.icns` packages that icon for the app.
The mobile web icons in `web/public/m/` are size derivatives of the full square mark.

## Third-party agent marks

The first-run agent choice, the Agents tab and Hide AI's agent selects and Add menu are the only places that draw other vendors' marks (`web/src/components/agent-mark.tsx`), each on a fixed light plate (`--logo-plate`) so a dark-filled mark stays legible in both themes.
`web/src/assets/agents/manifest.json` is the only list: a bundled file names its source URL, licence, any modification and the date it was taken, and an agent without an official source mark is a monogram with the reason written beside it.
`scripts/check-agent-logos.mjs` (run by `check-design-contract`) fails a bundled file the manifest does not list, an adapter with no entry, and an entry without its source and licence.
Marks are shown as the vendor published them: never recoloured, redrawn or approximated (design principle 10), and a mark whose source cannot be confirmed becomes a monogram rather than a guess.
A mark is bundled in the format the vendor publishes, an SVG or, where it publishes none, its PNG (`gemini-cli.png`), named for its adapter id so `web/src/agentLogos.ts` maps it without a list of its own.
An SVG that recolours itself under `prefers-color-scheme` takes the scheme from the app's theme inside an `<img>`, not from the plate behind it, so a dark-theme variant turns light on the light plate; where the vendor offers a fixed-colour variant that one is bundled instead, with the reason in its manifest `note` (Pi).
Showing a mark names the product it belongs to and implies no endorsement; trademark questions go to the vendor's own brand page named in the manifest.

## Voice and visual rhythm

Be calm, direct, and specific about what changed and what the operator can do.
Show summaries and relationships before raw activity, and let actionable changes carry the emphasis.
Keep layouts spacious and stable; use one clear accent at a time instead of decorating every working agent.
Avoid surveillance language, noisy urgency, and promises that an agent's output is correct before a person reviews it.

The logo signals this approach, while the interface continues to use its established design tokens and semantic status rules.
