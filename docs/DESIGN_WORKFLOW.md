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
  `tokens.json` is web-only: it generates `web/src/tokens.css` and nothing else.
- **Code authority** is `web/src/components/ui` for shadcn parts (one file per part, matching the gallery and the Pen `System /` sheets) and `web/src/components` for hide composites such as `entry-menu.tsx` and `settings-rows.tsx`, matching `Component /` sheets.

Screen designs are committed separately from the library, in [design/hide-screens.pen](../design/hide-screens.pen), as `Screen / <Area>` sheets (each with Light and Dark frames), importing the library rather than redrawing its masters.

## Dev gallery

The dev-only route `/gallery` (`web/src/gallery/`) renders every `System /` state as the real shadcn component, driven by `web/src/gallery/manifest.ts`, which lists each part and the exact state names its Pen sheet draws.
It is excluded from the production build `hided` serves.
`node scripts/check-pen-gallery.mjs` refuses a manifest and a library that name different parts or different states for either theme frame; see Checks below.

The gallery also renders production screens on synthetic scenes, one document per scene because a scene seeds the app's own stores.
`/gallery?scene=projects-sidebar` is the shell's real `Sidebar` and `createActions` over `web/src/gallery/sceneData.ts`, a snapshot with the content `Screen / Projects Sidebar` draws; its folds go through the real actions and the scene answers them as the core would.
Its query takes `theme` (`light`, `dark`), `width` (the sidebar's CSS width, seeded as the core's `ui_state.sidebar_width`), `scale` (the interface text scale, which reaches only what lies outside the sidebar, since the sidebar keeps its sizes at every scale) and `content` (`reference`, or `long` for long Korean titles).
Every name, path and count in a scene is invented example data, so a capture of it is safe to publish.

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
   Keep the chosen design as a reference bundle (see Reference bundle and review run below) before anything else edits it, marked `user` when the operator chose it and `delegated` when it is a proposal made under delegated authority.
5. **Library or screen file.** Promote only the approved reusable components or tokens into `design/hide-ui.lib.pen`, or the approved screen into `design/hide-screens.pen`, with one writer handling the shared update.
   Preserve master IDs when editing so existing instances keep their identity: a component state is a `ref` of its reusable master with descendant overrides, never a redrawn copy.
   Run `node scripts/gen-pen.mjs` after editing the library; it refreshes mapped variables and the Foundations sheet without moving authored sheet positions.
6. **Code.** Implement the change in `web/src/components/ui` or `web/src/components`, reaching every color, spacing, and radius value through a token.
7. **Compare in both themes.** For a `System /` part, compare the gallery's rendering against the Pen export, Light and Dark.
   For a `Component /` or a screen, compare an actual app capture against the Pen export, Light and Dark, because Pen cannot reproduce real app state or Radix behavior.
   For a screen with a review target, `node scripts/design-review.mjs review` does this in one run against the reference bundle.
   Pen has no code export and does not model Radix interaction; the comparison is a human visual judgment, not an automated pixel diff.
   Keep every capture under `agents/runs/<slug>/`; never commit a screenshot, scratch file, or comparison image (see AGENTS.md, Evidence Belongs Outside The Repository).
8. **One PR.** Land the token/library/screen change and the code change together.
   The PR body follows `.github/pull_request_template.md`: one before/after image of the actual screen above the fold, captioned with where to look, and the one visual judgment the reviewer has to make; the rule results as automated facts in Evidence.
   The review run's other comparison images and state captures go in Evidence's folded block (uploaded, never committed); `prd_ship.js screenshots` puts every image it uploads under Summary, so pick the one that stays there and move the rest before publishing.
   A failed rule, an INCOMPLETE item, or a required check that was not run stays visible in Evidence, never only in a folded block.

## Reference bundle and review run

A design change is judged against the design that was chosen, not against whatever the Pen file says by the time the code lands: a later edit to Pen, or the same wrong change made to Pen and code together, would otherwise hide the drift.
`scripts/design-review.mjs` keeps that choice and checks the production screen against it.
A target in `design/review-targets.json` names what one run covers: the committed Pen file and its `Screen /` sheet, the Pen nodes it pairs with the screen and each node's width, theme, text scale, content and state, the gallery scene, the states the scene can be put in, the conditions to measure, the default layout rules, and the questions left to a person.
A target exists for `projects-sidebar`; add one when a change touches another screen, not before.

### Keep the chosen design: `baseline`

```sh
node scripts/design-review.mjs baseline <slug> --target projects-sidebar --approval user|delegated --reference "agents/prd/<slug>/prd.md D-08" [--name <name>] [--from <file.pen>] [--rules <rules.json>]
```

- It copies the target's Pen file (or `--from`, such as a scratch proposal), every library it imports and every image it uses into `agents/runs/<slug>/design/baseline/<name>/`, rewriting the references so the bundle opens in Pen from any directory or machine (two different files that would share one name in the bundle are refused), and exports the target's nodes from that copy, so the images prove the copy is whole.
- `manifest.json` records when, from which file and commit, with which Pen version, each frame's conditions and image, the layout rules, and a hash of every file.
- `--approval` separates the operator's choice from a delegated proposal, and `--reference` points at the decision in the PRD instead of restating it.
- A bundle is never overwritten: a rerun with the same name is refused, and a failed export publishes nothing.
- `node scripts/design-review.mjs show <bundle>` prints the approval, frames and rules and checks every hash, with neither Pen nor a browser, so an implementor on a machine without Pen still reads the numbers and opens the PNGs.

### Check the screen against it: `review`

```sh
node scripts/design-review.mjs review <slug> --baseline <bundle> [--theme light,dark] [--width 292,240] [--content reference,long] [--scale 1,1.25] [--state rest,hover-parent,...] [--no-pen]
```

One run, in this order, with each step's real result in `agents/runs/<slug>/design/review/<run>/report.md` and `report.json`:

1. The static contract: `node scripts/check-design-contract.mjs`, as CI runs it.
2. The current Pen: the bundle's nodes exported from the target's Pen file in this checkout, so a proposal bundle is set against what the checkout now carries.
3. The production screen: the command starts its own Vite dev server and headless Chromium, opens the target's gallery scene under each selected condition, and measures the bundle's rules there through `web/e2e/sidebar-geometry.mjs`, the same geometry the e2e specs assert on.
   The rules are the bundle's, never values read back from the code under review: `childIndentPx` (a child's status mark and title start that many pixels per level right of its list's roots), `rootsAligned`, `stableUnderHover` and `stableUnderFocus` (no row moves or resizes while any one row is hovered or keyboard-focused), `noOverlap` (no part of a row overlaps another or runs past the row, no text spills), `sharedColumns` (one column for every time and badge end, one for every chevron) and `noSidewaysOverflow`.
4. Comparisons: for each bundle frame, the reference, the current Pen and the actual screen side by side, captured under that frame's width, theme, text scale, content and state (a target without `scales` in its conditions is measured at scale 1, as `projects-sidebar` is, because the sidebar does not follow the scale) and labelled with them; an image whose width is not the frame's is reported as a different condition, never scaled to look alike.
5. State sheets: every selected condition and state no Pen frame draws (a narrow width, a larger text size, long titles, hover, focus, a folded parent, a closed checkout) as captures of the actual screen only, labelled as such.

The exit status is 0 for PASS, 1 for FAIL (the static contract or a rule failed) and 3 for INCOMPLETE: a missing Pen or Pen login, a node Pen did not export, a bundle file that no longer matches its hash, or a browser step that did not finish.
An INCOMPLETE report names the cause and the command to rerun on a machine that can render; it is never read as a pass.
A PASS covers only the rules; the comparisons and the report's judgment list are for a person.
The run owns its dev server, browser and Pen session and closes them on every exit, including an interrupt at any step, which ends the whole run; a process it cannot close is reported, and it touches no other app, pane or server.

Only the selected conditions are measured, so a change names what it touched (`--theme dark --width 240 --content long`), and a bundle frame is always captured under its own conditions so its comparison exists.

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

The Overview of every project is `Screen / Main`, named after its web file and screen kind.
It draws the agent and project sidebar beside the Overview, the sidebar's Overview row marked: its title with Add project and `새 이슈` as the primary action, its facts line with the open issues, and the `Tasks · Agents · Projects` tabs with the waiting count on Agents, whose view is the Project Overview's lanes or lineage over every project.
Its Tasks board holds every project's issues in `백로그 · 진행 중 · 리뷰 · 완료`, each card an issue with its project beside its id, `시작` on a backlog card under the pointer, the worktrees with no issue folded into one line at the foot of 진행 중, and 완료 folded to one line per project with its count.
Below it, the Dependencies mode draws an arrow that crosses projects, with the blocker named by its repository on the lock line.
Its web files are `web/src/App.tsx`, `web/src/sidebar.tsx`, `web/src/MainScreen.tsx`, `web/src/TaskBoards.tsx`, and `web/src/projectBoard.ts`.

Project Overview is `Screen / Project Overview`.
Each frame carries its header: the title row with the path back, New agent as the quiet action and `새 이슈` as the primary one, the facts line of worktrees, disk, behind and merged with the view's mode control at its right end, and the tiles `Agents · Issues · Sessions`, the chosen one outlined.
Its first frame is Agents › 체크아웃 as every entry opens it: a lane per checkout with its head (glyph and branch, purpose, issue and PR chips, `↑N ↓N`, files), main pinned on top with a lane selected, delegation lines down across lanes in the Observer's column, the operator's turn in a yellow node, a merged lane dimmed with `정리`, and the two fold lines.
Under it is Agents › 계보: the `Observer · Implementor · 하위 에이전트` columns, a row per lineage with the asking one first, each node's checkout, issue and PR chips, and the two fold lines.
Beside them are Issues › Board (PRD overview-lenses-issues), the facts line carrying the filter beside the mode control and the four columns `백로그 · 진행 중 · 리뷰 · 완료` of issue cards only (백로그 with `+` and a card offering `시작` under the pointer, the operator's turn in the warning border, the checkout and PR chips, `이슈 없는 워크트리 N` and `이슈 없는 PR N` at the foot of 진행 중 and 리뷰, 완료 folded to one line per issue with the pull request that closed it), Issues › Card states (each stage's buttons in the id line's slot, a Local issue's edit, the operator's turn, blocked, a failed source read, the card whose panel is open, and the id's preview), and Issues › Dependencies (a chain of cards with stage words and arrows, blocked cards dimmed, unrelated issues below).
The issue panel frames stand beside the board in the width it leaves: a GitHub issue in progress (properties, 이 이슈로 한 일, the Markdown body and comments), a Local issue, a Local issue edited in place, and a GitHub issue whose read failed with `재시도`.
The issue card and panel are drawn on local tokens and library refs in this sheet, not as `Component /` masters, because each is one screen's part (`web/src/TaskBoards.tsx`, `web/src/IssuePanel.tsx`) rather than a composite under `web/src/components`.
Its web files are `web/src/ProjectOverview.tsx`, `web/src/OverviewLenses.tsx`, `web/src/overviewLens.ts`, `web/src/IssuesView.tsx`, `web/src/TaskBoards.tsx`, `web/src/IssuePanel.tsx` and `web/src/projectBoard.ts`; a card's agent rows are `web/src/components/agent-row.tsx`.

Workspace is `Screen / Workspace`.
It draws the side panel (`Component / Side panel`) open at the Workspace's full height over the agent column, then the panel closed with two views still open.
The toolbar spans only the agent column and has no tool toggles; the panel's first row holds each area's tabs and New tab, then the tool-column toggle, Expand, Pin and the panel toggle, which the toolbar carries with the open-view count while the panel is closed; its second row holds the document header and the Explorer and History tool tabs.
Its web files are `web/src/WorkspaceScreen.tsx`, `web/src/TabBar.tsx`, `web/src/ViewAreas.tsx`, `web/src/Tools.tsx`, and `web/src/ExplorerTree.tsx`.

Project Sessions is `Screen / Project Sessions`.
It draws the Project Overview on its Sessions tab: the Overview's header over the provider-filtered session list with search, and the read-only detail pane.
Its web files are `web/src/ProjectOverview.tsx` and `web/src/ProjectSessions.tsx`.

Settings is `Screen / Settings`.
It draws the seven tabs (General, Appearance, Agents, Issues, Devices, Performance, Shortcuts) and the Group/Row layout a tab renders, shown on the Appearance tab.
Its web files are `web/src/SettingsSheet.tsx` and `web/src/settings.ts`.

Palette is `Screen / Palette`.
It draws the sidebar's Search icon with its `Search ⌘K` hint, the ⌘K palette with its grouped two-line results and its no-match and nothing-to-search states, and the ⌘P file palette on the same shell.
Its web files are `web/src/Palette.tsx`, `web/src/search.ts`, and `web/src/components/sidebar-header.tsx`.

Dialogs and Sheets is `Screen / Dialogs and Sheets`.
It draws every Dialog and AlertDialog surface the shell opens: New worktree, Delete worktree, Remove project, Purpose, Unsaved drafts, Add a project, Keyboard shortcuts, New issue (with `만들고 바로 시작` unchecked and checked), and Start from an issue; the committed sheet still draws the removed sidebar New workspace panel in Add a project's place until the screen is redrawn.
Its web files are `web/src/WorkspaceDialogs.tsx`, `web/src/AddProjectDialog.tsx`, `web/src/DraftRecovery.tsx`, `web/src/ShortcutSheet.tsx`, and `web/src/IssueDialogs.tsx`.

Menus and Overlays is `Screen / Menus and Overlays`.
It draws the sidebar row menu, the Explorer context menu, the device picker, and the Explorer git-status notice, each anchored in its real screen context.
Its web files are `web/src/entry-menu.tsx` and `web/src/DevicePicker.tsx`.

Projects Sidebar is `Screen / Projects Sidebar`.
It draws the sidebar's Projects tab as the scope picker: above it the fixed Overview row with the house glyph and the project count, then the `Projects | Agents` strip ending in Add project and Search; in the list, pinned and activity-ordered projects with the row of the scope on screen selected, a Git project’s first Overview child as a checkout-row master instance with a layout-dashboard glyph and empty trailing slots, checkout rows with their kind glyph, age and agent line, an opened checkout’s agent rows, and both inactive folds.
The Overview child owns selection on Overview; a checkout row opens and unfolds, then folds on activation while already selected and unfolded.
Beside each theme's sidebar it draws a pull-request row under the pointer with its card (`Component / PR hover card`) opened to the right, the state the row's tooltip has become.
Its web files are `web/src/sidebar.tsx`, `web/src/components/sidebar-header.tsx`, `web/src/projects.ts` and `web/src/components/pr-card.tsx`.

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
The node test suites `scripts/tests/pen-gallery.test.mjs`, `scripts/tests/pen-transplant.test.mjs`, `scripts/tests/design-scratch.test.mjs`, `scripts/tests/design-review.test.mjs`, and `scripts/tests/hide-screens.test.mjs` exercise the gallery comparison, the transplant script, the scratch creator, the reference bundle and the review rules, and the screen-list checker respectively against fixtures; they do not prove Pen rendering or browser layout, which stay a local `design-review.mjs review` run with the real Pen CLI and Chromium.

These are static source checks, not a rendering engine or an aesthetic evaluator: they catch drift between the files that are supposed to agree, not whether a composition looks right.
Visual approval is always a human judgment made from a gallery-or-app capture beside the Pen export, recorded under `agents/runs/<slug>/` and never as a committed image; the review run's rules make layout drift a failure, not an approval.
