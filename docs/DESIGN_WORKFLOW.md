# Design workflow

This document owns how a design change moves from an idea to shipped code: the authorities a change has to agree with, the scratch-to-PR flow, the sheet transplant procedure for parallel screen work, and how to add a token, a System part, or a Component.
[UI_BEHAVIOR.md](UI_BEHAVIOR.md) owns what the shipped UI does; this document owns how you get there.
Read [docs/README.md](README.md) first for which documents are current.

## Authorities

Three files each own one kind of truth, and none of them restates another's values.

- **Visual authority** is the Pen library, [design/hide-ui.lib.pen](../design/hide-ui.lib.pen).
  Its `System /` sheets are shadcn parts drawn 1:1 with the dev gallery: same name, same variants, one sheet per part with `Light` and `Dark` frames and one node per state.
  Its `Component /` sheets are hide composites built on top of `System /` masters (menus, rows, panels) that do not exist in shadcn.
  Only `System /` and `Component /` top-level sheets belong in this file; screen proposals, audits, and scratch never do.
- **Numeric authority** is [design/tokens.json](../design/tokens.json): shadcn-named tokens (`--background`, `--foreground`, `--card`, `--popover`, `--primary`/`--primary-foreground`, `--secondary`, `--muted`/`--muted-foreground`, `--accent`, `--destructive`, `--border`, `--input`, `--ring`, `--sidebar-*`, and the four `--accent-choice-*` picks) each carrying a Dark value and a Light value, plus aliases.
  `node scripts/gen-tokens.mjs` writes `web/src/tokens.css` (Tailwind v4 `@theme`, `:root`/`.light`, `.dark`) and `web/src/generated/accents.ts`.
  `node scripts/gen-pen.mjs` writes the same values into `design/hide-ui.lib.pen` as Pen variables on a `Mode` (Light/Dark) theme axis.
  `tokens.json` is web-only: it no longer generates or updates `HideTheme.swift`, which is frozen for the remaining Swift-shell coexistence period (macos/AGENTS.md).
- **Code authority** is `web/src/components/ui` for shadcn parts (one file per part, matching the gallery and the Pen `System /` sheets) and `web/src/components` for hide composites such as `entry-menu.tsx` and `settings-rows.tsx`, matching `Component /` sheets.

Screen designs are committed separately from the library, in [design/hide-screens.pen](../design/hide-screens.pen), as `Screen / <Area>` sheets (each with Light and Dark frames), importing the library rather than redrawing its masters.

## Dev gallery

The dev-only route `/gallery` (`web/src/gallery/`) renders every `System /` state as the real shadcn component, driven by `web/src/gallery/manifest.ts`, which lists each part and the exact state names its Pen sheet draws.
It is excluded from the production build `hided` serves.
`node scripts/check-pen-gallery.mjs` refuses a manifest and a library that name different parts or different states for either theme frame; see Checks below.

## The flow: scratch to one PR

1. **Scratch.** Create an ignored, library-linked scratch document with `node scripts/design-scratch.mjs <task-slug>`.
   It requires the pinned Pen CLI version and an existing Pen login, links this checkout's `design/hide-ui.lib.pen` read-only, refuses to overwrite an existing scratch, and refuses a symlinked scratch directory.
   The command prints the scratch path (`agents/runs/<task-slug>/design/scratch.pen`) and the exact edit command.
2. **Edit.** Use an independent Pen CLI headless session per file: `pen interactive --in <scratch-path> --out <scratch-path>`.
   Never use the shared desktop MCP or `--app desktop` for agent editing; an explicit MCP `filePath` did not reliably isolate the active desktop document in verification.
   Inside the CLI, read `read_skill()` and its schema/execute guides, then confirm `get_app_state()` and `list_libraries()` before editing.
   `list_libraries()` returns each imported library's ID; reference a component as `<id>:<component-id>` and a variable as `$<id>:--token-name`, discovering the ID fresh in each scratch rather than reusing another task's alias.
   Only one writer edits a shared library document at a time.
   Save with `save()` and exit with `exit()` before human review; before resuming CLI edits of a document the user opened, the user saves and closes it in the desktop app.
   Never resolve a conflicting design edit by line-merging the `.pen` JSON; keep both versions and reapply the approved change through Pen.
   Library changes become visible in an importing document only after it is closed and reopened; per-instance overrides survive that reopen.
   Each worktree reads its own committed library revision, not another checkout's mutable library path.
3. **Pen toolchain limits to design around**, not to invent workarounds for:
   - Pen is not CSS. Unsupported alignment, margin, and percentage properties must not be invented.
   - Width and height use numeric literals; variable references have rendered at zero in this toolchain.
   - Variable-bound node opacity also renders invisible in this toolchain; the state bindings in `pen-token-map.json` materialize their numeric values during generation and checking.
   - The canvas substitutes JetBrains Mono for unavailable SF Mono and plain Inter because `ss03` does not travel on a token; these substitutions are not a pixel-comparison acceptance gate.
   - A cross-library `ref`'s own internal `$--token` fills and strokes resolve to the imported library's own default (Light) value, regardless of any `theme: {Mode: 'Dark'}` tag anywhere in the importing document.
     A bare local `$--token` on a node the importing document authors itself, and an explicit property override placed at the ref site using the importing document's own local `$--token`, both resolve correctly per theme.
     A descendant-override key on a cross-library ref also needs the alias prefix (`hideui:btn-lb`, not `btn-lb`), not only the ref's own target.
     `design/hide-screens.pen` carries its own local copy of every token, read live through `pen-tokens.mjs`, and `scripts/pen-screens.mjs`'s `themedOverrides()` restates a ref's colors by walking the ref's actual master in `design/hide-ui.lib.pen` and copying every `$--token` name it finds, at any depth, into a local override of the same name; a hand-typed color recipe is only for a real per-variant design decision (Button and Badge import `BUTTON_VARIANTS`/`BADGE_VARIANTS` from `pen-system.mjs` for exactly that reason), never for a master's own default.
     `scripts/check-hide-screens.mjs` refuses a ref or a descendant-override key whose colorable properties are not restated this way.
4. **Human approval.** Show alternatives in the task's scratch document, let the user choose and revise them, and get approval before implementing.
   Record the approved behavior and any proposed system addition in the PRD so the decision survives scratch cleanup; a scratch is not a merge deliverable.
5. **Library or screen file.** Promote only the approved reusable components or tokens into `design/hide-ui.lib.pen`, or the approved screen into `design/hide-screens.pen`, with one writer handling the shared update.
   Preserve master IDs when editing so existing instances keep their identity: a component state is a `ref` of its reusable master with descendant overrides, never a redrawn copy.
   Run `node scripts/gen-pen.mjs` after editing the library; it refreshes mapped variables and the Foundations sheet without moving authored sheet positions.
6. **Code.** Implement the change in `web/src/components/ui` or `web/src/components`, reaching every color, spacing, and radius value through a token.
7. **Compare in both themes.** For a `System /` part, compare the gallery's rendering against the Pen export, Light and Dark.
   For a `Component /` or a screen, compare an actual app capture against the Pen export, Light and Dark, because Pen cannot reproduce real app state or Radix behavior.
   Pen has no code export and does not model Radix interaction; the comparison is a human visual judgment, not an automated pixel diff.
   Keep every capture under `agents/runs/<slug>/`; never commit a screenshot, scratch file, or comparison image (see AGENTS.md, Evidence Belongs Outside The Repository).
8. **One PR.** Land the token/library/screen change and the code change together.

## The screen transplant procedure

`design/hide-screens.pen` is one shared file, so parallel PRDs working on different `Screen / <Area>` sheets do not line-merge its JSON.
Instead, a branch transplants only its own sheets into main's copy of the file, node by node:

```sh
node scripts/pen-transplant.mjs --from <branch-file> --into <main-file> --sheet <sheet-id> [--sheet <sheet-id> ...]
```

- `--from` is the branch's `design/hide-screens.pen` (or another `.pen` file holding the authored sheets); `--into` is the target file to write, ordinarily main's current `design/hide-screens.pen` after a rebase.
- `--sheet` names one or more top-level sheet IDs to move; the script refuses a sheet ID that does not exist in `--from`.
- The script replaces (or adds) exactly those sheets in `--into` by node identity and leaves every other sheet in `--into` untouched, so two branches that touched different `Screen /` areas both land cleanly.
- Run `node scripts/gen-pen.mjs` and `node scripts/check-design-contract.mjs` against the result before committing; a transplant that leaves stale Foundations or an unrecognized sheet name fails the same checks a normal edit would.
- Never resolve a `hide-screens.pen` conflict by hand-merging JSON; re-run the transplant with the correct `--sheet` list instead.

## Web screen list

`design/hide-screens.pen` draws every area the web shell shows today, one `Screen / <Area>` sheet per area, imported from `design/hide-ui.lib.pen` the same way a `Component /` sheet is drawn from `System /` masters.
Each sheet carries a `Light` and a `Dark` frame and uses realistic content, including Korean labels and a long path, to show real wrapping and truncation rather than an abstract state.
`scripts/check-hide-screens.mjs` enforces the shape (`Screen / ` naming, both theme frames, every reference resolving against the library, every cross-library color restated locally, and local variables matching `design/tokens.json`) and `scripts/gen-screens.mjs` regenerates the file from `scripts/pen-screens.mjs`.

Main is `Screen / Main`.
It draws the agent and project sidebar beside the Projects list, grouped by device.
Its web files are `web/src/App.tsx`, `web/src/sidebar.tsx`, and `web/src/MainScreen.tsx`.

Project Overview is `Screen / Project Overview`.
It draws a project's Tasks board under its header: the ad hoc strip, the four Git columns with Merged folded, and a needs-you card in the warning halo.
Its web files are `web/src/ProjectOverview.tsx` and `web/src/projectBoard.ts`; its agent rows are `web/src/components/agent-row.tsx`.

Workspace is `Screen / Workspace`.
It draws the tab strip, the layout switch, and the split content of a terminal beside the Explorer.
Its web files are `web/src/WorkspaceScreen.tsx`, `web/src/TabBar.tsx`, `web/src/ViewAreas.tsx`, and `web/src/ExplorerTree.tsx`.

Project Sessions is `Screen / Project Sessions`.
It draws the provider-filtered session list with search, and the read-only detail pane.
Its web file is `web/src/SessionsScreen.tsx`.

Settings is `Screen / Settings`.
It draws the five tabs (General, Appearance, Agents, Devices, Shortcuts) and the Group/Row layout a tab renders, shown on the Appearance tab.
Its web files are `web/src/SettingsSheet.tsx` and `web/src/settings.ts`.

Palette is `Screen / Palette`.
It draws the sidebar Search field, the ⌘K palette with its grouped two-line results and its no-match and nothing-to-search states, and the ⌘P file palette on the same shell.
Its web files are `web/src/Palette.tsx`, `web/src/search.ts`, and `web/src/components/search-field.tsx`.

Dialogs and Sheets is `Screen / Dialogs and Sheets`.
It draws every Dialog and AlertDialog surface the shell opens: New worktree, Delete worktree, Remove project, Purpose, Unsaved drafts, New workspace, and Keyboard shortcuts.
Its web files are `web/src/WorkspaceDialogs.tsx`, `web/src/NewWorkspace.tsx`, `web/src/DraftRecovery.tsx`, and `web/src/ShortcutSheet.tsx`.

Menus and Overlays is `Screen / Menus and Overlays`.
It draws the sidebar row menu, the Explorer context menu, the device picker, and the Explorer git-status notice, each anchored in its real screen context.
Its web files are `web/src/entry-menu.tsx` and `web/src/DevicePicker.tsx`.

## How to add a token

1. Add the entry to `design/tokens.json`, with both a Dark (`value`) and a Light (`light`) value for a color token, or use `type: "alias"` to point at another token.
2. Run `node scripts/gen-tokens.mjs` to regenerate `web/src/tokens.css` and `web/src/generated/accents.ts`.
3. Run `node scripts/gen-pen.mjs` to carry the same value into the Pen library's `Mode` variables and Foundations sheet.
4. Reference the token from Tailwind classes or CSS custom properties in `web/src` (`check-web-tokens.mjs` refuses a literal hex, `rgb()`/`hsl()` or `px` value anywhere in a source, including a CodeMirror theme object or another stylesheet, and Tailwind's own default spacing/text scales, since those are also unchosen literals).
5. A visual case the token system does not cover is a proposed addition, reviewed and approved before use, never settled with a one-off value.

## How to add a System part

A `System /` sheet is a shadcn part redrawn with hide tokens, name and variant 1:1 with shadcn's own naming.

1. Start from the shadcn source for the part and copy its structure onto hide's tokens rather than inventing a new visual language.
2. Add or extend the part's section in `web/src/gallery/manifest.ts`: the section name matches the sheet name (without the `System / ` prefix) and the state list matches exactly what the sheet's `Light` and `Dark` frames draw.
3. Draw the `System / <Name>` sheet in `design/hide-ui.lib.pen` with `Light` and `Dark` frames, each holding one node per state named identically to the manifest list.
4. Implement (or confirm) the part in `web/src/components/ui/<name>.tsx`.
5. Run `node scripts/check-pen-gallery.mjs` to confirm the sheet and the manifest agree, and `node scripts/check-design-contract.mjs` before delivery.

## How to add a Component

A `Component /` sheet is a hide composite assembled from `System /` masters (never redrawn from scratch).

1. Confirm the composite is actually needed in the current web screens; do not add a Component for a screen that has no PRD or committed plan (see D-20 in `agents/prd/web-design-system-reset/prd.md` for the classification this reset used for existing Components).
2. Compose it in the scratch document from `System /` masters as refs with descendant overrides, so it inherits token updates automatically.
3. Implement it as a hide composite in `web/src/components/<name>.tsx`, built from the same `web/src/components/ui` parts the sheet composed.
4. `Component /` sheets are not covered by the gallery; compare them against an actual app capture in both themes instead (see step 7 of the flow above).

## Checks

`node scripts/check-design-contract.mjs` is the entrypoint `design-contract.yml` runs, and it runs:

- `check-pen.mjs` - refuses a Pen library that is not what the token generator would produce: a stale token value, a stale Foundations sheet, a top-level sheet whose name carries neither the `System /` nor the `Component /` prefix, or an id that names more than one node.
  A node written whole inside a ref's `descendants` counts: its key already addresses the node, so an `id` of its own makes Pen's loader report duplicate ids.
- `check-pen-gallery.mjs` - refuses a `System /` sheet and `web/src/gallery/manifest.ts` that name different parts, or a state drawn on one side and not listed on the other, for either theme frame.
- `check-web-tokens.mjs` - refuses a web source file that reaches a color, size, or radius through a literal instead of a token: a hex, `rgb()`/`hsl()` or `px` literal anywhere in the file, an inline color style, a Tailwind arbitrary `[...]` value, or one of Tailwind's own default spacing/text scale classes.
- `check-hide-screens.mjs` - refuses a `design/hide-screens.pen` top-level node not named `Screen / `, an id that names more than one node, a `Screen /` sheet missing a `Light` or a `Dark` frame, a local `$--variable` the file does not define, a ref or descendant-override key whose import alias or target id does not resolve against the imported library, a cross-library ref that leaves one of the imported master's own colors un-restated locally, or a local variable block that differs from what `design/tokens.json` generates.

`--staged` reads the exact staged content of the design inputs and checker files into a temporary directory and checks that, without touching the index or working tree; the tracked `.githooks/pre-commit` runs it and is opt-in (`git -c core.hooksPath=.githooks commit`).
The node test suites `scripts/tests/pen-gallery.test.mjs`, `scripts/tests/pen-transplant.test.mjs`, `scripts/tests/design-scratch.test.mjs`, and `scripts/tests/hide-screens.test.mjs` exercise the gallery comparison, the transplant script, the scratch creator, and the screen-list checker respectively against fixtures; they do not prove Pen rendering, which stays a local step with the real CLI.

These are static source checks, not a rendering engine or an aesthetic evaluator: they catch drift between the files that are supposed to agree, not whether a composition looks right.
Visual approval is always a human judgment made from a gallery-or-app capture beside the Pen export, recorded under `agents/runs/<slug>/` and never as a committed image.
