---
version: alpha
name: Hide-native-shell
essence: |
  A dark, compact macOS shell for coding agents: one near-black surface ladder (background, sidebar, panel, elevated, balloon), hairline dividers instead of shadows, Inter with the ss03 stylistic set at small sizes, neutral controls, and color reserved for semantic state marks. Every value the shell draws is a HideTheme token, and this document is what those tokens mean.
description: |
  The design contract for Hide's native surfaces: sidebar, checkout cards, tab strip, pane headers, right panel, status bar, Search, New Agent, Settings, file search and editor overlays. Raycast's launcher was the visual reference for the surface ladder and the typography; that analysis is no longer carried here, and these tokens govern the application.

colors:
  background: "#101112"
  sidebar: "#171819"
  panel: "#1D1F21"
  elevated: "#27292C"
  balloon: "#34373B"
  divider: "#34363A"
  primary: "#F4F4F6"
  secondary: "#A4A5A8"
  muted: "#92959A"
  accent: "#D3D3D4"
  file-icon-neutral: "#9C9C9D"
  file-icon-document: "#D3D3D4"

typography:
  micro:
    fontFamily: Inter
    fontSize: 9px
    fontWeight: 400
    lineHeight: 1.4
    fontFeature: '"ss03"'
  caption:
    fontFamily: Inter
    fontSize: 10px
    fontWeight: 400
    lineHeight: 1.4
    fontFeature: '"ss03"'
  body:
    fontFamily: Inter
    fontSize: 11px
    fontWeight: 400
    lineHeight: 1.4
    fontFeature: '"ss03"'
  subhead:
    fontFamily: Inter
    fontSize: 12px
    fontWeight: 400
    lineHeight: 1.4
    fontFeature: '"ss03"'
  title:
    fontFamily: Inter
    fontSize: 13px
    fontWeight: 400
    lineHeight: 1.4
    fontFeature: '"ss03"'
  headline:
    fontFamily: Inter
    fontSize: 17px
    fontWeight: 400
    lineHeight: 1.4
    fontFeature: '"ss03"'
  display:
    fontFamily: Inter
    fontSize: 30px
    fontWeight: 400
    lineHeight: 1.4
    fontFeature: '"ss03"'

rounded:
  radiusExtraSmall: 4px
  radiusSmall: 6px
  radiusMedium: 8px
  radiusLarge: 10px
  radiusExtraLarge: 16px

spacing:
  spacingNone: 0px
  spacingXXS: 2px
  spacingXS: 4px
  spacingSM: 8px
  spacingMD: 12px
  spacingLG: 16px
  spacingXL: 24px
  spacingXXL: 32px
  spacingXXXL: 40px

components:
  shell-hairline:
    backgroundColor: "{colors.divider}"
  shell-metadata:
    textColor: "{colors.muted}"
  shell-supporting-copy:
    textColor: "{colors.secondary}"
  shell-file-icon-neutral:
    textColor: "{colors.file-icon-neutral}"
  shell-file-icon-document:
    textColor: "{colors.file-icon-document}"
  shell-sidebar:
    backgroundColor: "{colors.sidebar}"
    textColor: "{colors.primary}"
    typography: "{typography.body}"
    rounded: "{rounded.radiusSmall}"
  shell-panel:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.primary}"
  shell-keycap:
    backgroundColor: "{colors.elevated}"
    textColor: "{colors.primary}"
    typography: "{typography.micro}"
    rounded: "{rounded.radiusSmall}"
    height: 18px
    padding: "{spacing.spacingXS}"
  shell-tooltip:
    backgroundColor: "{colors.balloon}"
    textColor: "{colors.primary}"
    typography: "{typography.subhead}"
    rounded: "{rounded.radiusMedium}"
  shell-primary-action:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.background}"
    typography: "{typography.body}"
    rounded: "{rounded.radiusSmall}"
---

## Design library and exploration

[hide-ui.lib.pen](design/hide-ui.lib.pen) is the shared visual design-system library, not a catalog of every product screen.
It contains Foundations, primitive controls, and agreed reusable components with their state examples.
This document owns their meaning and behavior; `design/tokens.json` owns the numeric token values, `scripts/gen-tokens.mjs` writes `web/src/tokens.css` and keeps `HideTheme.swift` in sync, and `scripts/pen-token-map.json` mirrors those values into the library.
The library does not generate Swift controls automatically: an approved system change updates the library, token or component implementation, and this contract together.

Keep task-specific screen exploration in `agents/runs/<slug>/design/scratch.pen` inside the task's worktree.
That namespace is already ignored; scratch and screenshots are not merge deliverables.
Create it from the task's checkout with `node scripts/design-scratch.mjs <task-slug>`.
The command checks the verified Pen CLI version, uses headless `--library` to link this checkout's `design/hide-ui.lib.pen`, and prints the absolute scratch path and edit/review commands.
It requires an existing Pen login, does not run a model prompt, refuses existing scratches and symlinked output directories, and publishes no scratch when the import fails.
Do not automatically upgrade the CLI or silently fall back to a copied library when creation fails; resolve the reported cause first.
The script owns the tested version pin; changing it requires repeating import, variable/component rendering, update/reopen and two-worktree isolation checks.

Agents edit with `pen interactive --in <scratch-path> --out <scratch-path>` in an independent headless session per file.
Do not use the shared desktop MCP or `--app desktop` for agent editing: a supplied MCP path did not reliably select that document in verification.
Inside the CLI, read `read_skill()` and its schema/execute guides, then confirm `get_app_state()` and `list_libraries()` before editing.
The latter returns each imported library's ID and status; a component is referenced as `<id>:<component-id>` and a variable as `$<id>:--token-name`.
Discover the ID in each scratch rather than reusing an alias from another task.
For an existing document needing a library, use CLI `import_library({path: ...})`; do not handwrite ordinary-file imports or redraw shared masters.
The CLI 0.3.8 component guide still says cross-file references are unsupported; the library-qualified references described here were verified and take precedence for this workflow.

Human review is a file handoff, not simultaneous editing.
The agent saves with `save()` and exits with `exit()`, then the user opens the printed scratch path in Pen and gives feedback or edits it.
Before the agent resumes, the user saves and closes that document; the agent starts a new headless session to load those changes.
Do not keep a stale desktop editor open and later save over CLI changes.
Library changes become visible when the importing document is closed and reopened, while per-instance overrides remain.
Another worktree's library change first has to arrive through Git in this worktree; opening the scratch does not update Git or follow another checkout's library.

Show alternatives in that task's scratch document, let the user choose and revise them, and obtain approval before implementing the selected screen.
Record the approved behavior and any proposed system additions in the committed PRD, so the decision survives scratch cleanup.
Capture the approved design under the run directory before implementation, then review the actual app against it.
Promote only approved reusable components or tokens into the library, with one writer handling the shared update.
Current product composition lives in code and the maintained behavioral contracts, not in long-lived screen boards.

`node scripts/gen-pen.mjs` refreshes mapped variables and Foundations while preserving authored sheet positions.
`node scripts/check-design-contract.mjs` rejects token drift, stale Foundations, and library sheets outside `System /` or `Component /`.
The former `design/hide.pen` screen collection is recoverable from Git history and is no longer an active design input.

## Native Git and lineage tokens

The [agent workflow contract](design/agent-workflow-review.md) and the reusable component sheets in `design/hide-ui.lib.pen` define parent-centric delegation, shared Agent identity, pane focus, and Explorer Git decorations.
The former Review candidates, Final, R2, R3, the family-only Overview comparison, and the task-forest Overview that the worktree-grouped list replaced are historical and are not implementation references.
`My Work` is a session-local default, and `All` restores delegated rows without changing status groups, pane focus, or read state.

`HideTheme.lineageIndent` is one column per descendant level, uniform at every depth.
It is a measurement rather than a chosen spacing: the distance from a row's status mark to its agent badge, so a child's mark sits centered under its parent's badge and the tree reads as columns.
It replaced a step that shrank after two levels, which kept deep trees narrow at the cost of the marks lining up with nothing.
`lineageChevronWidth` reserves 16pt on every project agent row for the disclosure control, and that column is where the connector lives: the trunk drops from the control that opens the branch, so a branch and the thing that shows or hides it are one column rather than two.
The disclosure occupies the first identity line rather than the center of a multiline row, and the descending rail starts below its glyph.
Continuing ancestor rails remain in that ancestor parent column through deeper descendants.
`lineageElbowY` places the turn at the row's status mark, a fixed offset from the row's top rather than a fraction of its height, so a row that grows a qualifier line does not slide the connector off the mark.
The 16pt column is the row's role column and it draws one glyph: `chevron.right` or `chevron.down` when the row has descendants to fold or open, `arrow.turn.down.right` when the row is a child drawn away from its parent, and nothing otherwise.
The chevron toggles the fold in the project tree; on a raised Needs You or Done row, and in the Agents view, it only says the descendants exist, because those rows never unfold.
The return glyph is the way back: its tooltip and accessibility help name the parent (`Return to parent <name>`) and clicking it selects the parent's pane, the same action the pane header's Return control performs.
It appears on a child listed under its own worktree, where the tree has rebased it to depth zero, and on a child listed flat in the Agents view; under its parent the tree line already says whose it is.
An orphan keeps the `↳ from an agent Hide can't see` line beneath its row, because there is no parent to return to.
Descendants start folded, and the fold persists per pane id for as long as the pane exists.
`HideTheme.gitRowFontSize` (11pt) and `HideTheme.gitDetailFontSize` (10pt) are the History board's row and detail sizes; the Overview draws its group headers on the shared scale instead.
Checkout titles use `HideTheme.Typography.subhead` and `checkoutRowHeight` (36pt), with primary text contrast even when no terminal is attached.
The one-line checkout row reserves fixed columns for its kind glyph and right-edge chevron, with the title and last-commit age between them, so disclosure and agent changes do not move its identity.
The kind glyph is the pull-request lifecycle octicon when current GitHub data has a pull request, then branch, home for the primary checkout, commit for detached HEAD, or folder for a plain folder; `primary` and `detached` are not separate badges.
Open, draft, merged and closed pull requests keep their lifecycle shapes and colors, stale GitHub data mutes only the octicon, an unavailable GitHub lookup falls back to the branch glyph, and a missing folder colors its branch glyph with `danger` and omits the age.
`missing` and `temporary` remain the only checkout-row badges.
The sidebar hierarchy is Project > Workspace > Agents; a workspace corresponds to one checkout path, including a plain folder.
Workspaces without a branch use their actual folder name, including missing paths.
Detached checkouts use the commit glyph and retain the commit and path in their tooltip.
Workspaces with nested agent rows toggle disclosure across the whole row, while the always-reserved right-edge slot contains the expansion arrow only when disclosure applies.
Workspaces without nested agent rows open when clicked and leave that slot empty.
Workspace disclosure persists across launches and hides only the nested agent rows, preserving selection, running panes, and raised attention rows.
Project-view number shortcuts skip agents hidden by workspace disclosure.
`agentMarkWidth` (12pt) and `checkoutIconWidth` (14pt) define the status and branch columns.
`compactAgentLeadingInset` derives the root agent status center from the Workspace branch center, accounting for the lineage chevron gutter.
Compact agent rows use `spacingXS` (4pt) between the status, provider icon, and title.
An agent row's title is the core's `identity_label` at both densities: the rolling task, or the workspace label when no task exists; a Herdr agent name remains a control identifier and never becomes display copy.
A prominent row keeps its project context as a qualifier on a third line, `Typography.micro` in `muted`, because beside the sentence it took the width the sentence needed.
The second line is chosen by the core from the row's group and drawn as given, so no view decides it twice ([status model](docs/status-model.md#the-second-line)):

| Group | Sentence |
| --- | --- |
| Needs You, unread reported completion (Done) | `expected_reply`, else `progress`, caption regular in `primary` |
| Working | `progress`, caption regular in `secondary` |
| Seen, unknown | none - the row is one line |

A row with no sentence draws the status word instead, `Typography.caption` medium in the mark's color, so a row is never left with an empty second line; beside a sentence the word is not drawn, because the mark and the group heading already say it and the two together took a third of the row.
A delegated row's sentence is `muted`, like the rest of it.
A row whose descendants are folded, and every raised row, wears the descendant badge on its first line, before the elapsed time: a `badgeHeight` capsule filled with `elevated`, `spacingXS` horizontal padding, one cell per non-zero state in the order error, approval, question, working, done.
Each cell is the row mark of that state (`×` `!` `?` `●` `✓`, caption bold monospaced in the state's semantic color) followed by the count in `Typography.micro` monospaced `secondary`, cells `spacingXS` apart and mark and count `spacingXXS` apart.
The counts are the core's `descendant_counts` and sum every live descendant; a row with none, or with only ready descendants, wears no badge.
Opening the fold removes the badge, because the opened rows carry their own marks.
Under an opened parent, a child running in another worktree names it on the qualifier line with the `GitIcon.worktree` glyph and the worktree name in `Typography.micro` `muted`; a child in the parent's own worktree has no qualifier, and the former floating worktree chip beneath the row is gone.
The sentence is one line with tail truncation, and the full text is the row tooltip and the accessibility label, which reads name, agent kind, status word, sentence in that order even when the word left the screen.
There is no third gray for the sentence: a `muted` step between `secondary` and `primary` was tried and the two grays did not separate in the rendered row.
Only a collapsed Workspace can gain a second line.
That line starts with the representative agent's status mark and provider badge, adds `+N` for additional agents, then shows one tail-truncated sentence chosen by the core in this order: purpose token, branch description, representative agent title, pull-request title.
When no agent exists, the mark and provider summary are absent and the sentence keeps the same leading alignment; when neither an agent nor a sentence exists, the row stays one line.
An expanded Workspace omits the summary and sentence because each nested agent row already carries its own state.
The full second-line sentence remains in the checkout tooltip and accessibility label.
The right-edge disclosure and fixed semantic status colors follow [the shared status contract](docs/status-model.md#shared-agent-and-workspace-status-contract).
`agentWorking` (`#61A6FF`) is the fixed blue semantic status token; workspace chrome and user accent choices do not recolor it.
The accents Settings offers are the `AccentChoice` tokens (`--color-accent-choice-lime`, `-sky`, `-violet`, `-amber`); the chosen one is stored as its hex in `ui_state.accent_hex` and takes the place of `accent`.
The checkout tooltip starts with `#N · state · title` when a pull request is known and appends `Last known <age>` when stale, then carries the agent count, detached commit and path when applicable.
Pull-request numbers, file counts, ahead and behind counts, review decisions and CI do not appear on the sidebar surface.
Clicking the lifecycle octicon or choosing `Open PR #N` from the checkout context menu opens that pull request in GitHub; the rest of the row retains its disclosure or selection action.
GitHub refresh, unavailable detail and stale detail live only in Overview's GitHub popover, and the former sidebar pull-request popover has no retained control.
PR lifecycle is a semantic-color exception to monochrome chrome: Open `#3FB950`, Merged `#A371F7`, Closed `#F85149`, and Draft `#9198A1`.
`HideTheme.PullRequest` owns this GitHub-style dark palette and the 14pt glyph size, shared with the branch icon, inside the existing 24pt control.
Official MIT-licensed Octicons distinguish open, merged, closed, and draft by shape as well as color; the vector PDF resources and license ship in the bundle.
The sidebar glyph, Overview popover header, and State badge use the same lifecycle color, including during hover and selection.
Review decisions and CI retain their own status meanings.
`HideIconButton` supports template image content with an explicit semantic color while keeping the shared hit area, interaction treatment, tooltip, and accessibility behavior.
Reference: [GitHub Primer state labels](https://primer.github.io/design/components/state-label/).
The primary branch mismatch keeps its migration action as a warning icon beside the role badge.
The context menu groups creation, branch configuration, path access, and guarded deletion with native separators.
New worktree uses stacked Branch name, Create from, Start with, and optional one-line Purpose fields, followed by Cancel and Create worktree.
Purpose shows a `current / 40` count, warns after 40 characters, stops at 80, and is absent for a remote Herdr older than 0.9.1.
The checkout row and Overview header context menus both offer `Set purpose…`, whose one-field sheet keeps the current text while a retryable save error remains visible.
Saving an empty purpose clears it; display falls back through branch description, representative agent title and pull-request title.
`formControlHeight` is 36pt; compact settings retain `settingsFieldHeight` at 24pt.
`HideFormPicker` owns stacked menu selection with an explicit selected label, and `HideSettingsField` owns form text inputs through the shared input surface.
Inside a settings row the same picker drops its stacked label and takes the `settingsControlWidth` (200pt) right-hand column, because the row already carries the label on the left; its accessibility label stays the picker's own.
Terminal tabs use the stable Herdr tab name; the pane header alone carries the focused pane title and its agent status/provider marks.
Work tabs begin at the shared `tabPreferredWidth` (180pt) and shrink equally as the strip fills, preserving titles and shortcut keycaps through the inclusive `tabTitleMinimumWidth` (104pt) boundary.
Only the active tab draws the existing 24pt toolbar close control at every density; selecting an inactive tab reveals that control in the same trailing position.
Below 104pt tabs become icon tabs and keep identity, live state, notice and dirty marks, using a filled document glyph for preview files.
An inactive icon tab uses the leading `tabIconIdentityWidth` (40pt), while the active tab adds the close control for a 64pt total width.
If those compact widths cannot all fit, the strip reserves `tabOverflowControlWidth` (28pt) for an ordered `…` menu and shows one contiguous canonical range containing the active tab.
Selecting a hidden tab moves that visible range without reordering it, widening restores compact, title and preferred states in reverse, and drag destinations use the same per-tab widths and starts as rendering.
The strip never scrolls horizontally or scrolls the active tab into view.
With the left sidebar visible, the first tab background starts flush at the content edge with no outer leading inset; the tab's internal identity spacing and one-point dividers remain unchanged.
With the left sidebar hidden, the strip retains `trafficLightInset` (69pt) before its first control.
The tooltip and accessibility label retain the full stable tab name, focused-pane context and state at every density; file and diff tabs retain their file names.
The sidebar runtime version stays on one line with middle truncation; its tooltip carries the complete value.
`worktreeDialogWidth` is 440pt for the consequence-first deletion confirmation.
`gitSectionIcon` uses `externaldrive.badge.checkmark`, and `gitPullRequestIcon` uses `arrow.triangle.pull`; status uses existing semantic colors and every icon has a tooltip.
`HideTheme.GitIcon` names refresh (`arrow.clockwise`), merged (`checkmark.circle`), unmerged (`circle`), dirty (`circle.fill`), clean (`checkmark`), merged PR (`arrow.triangle.merge`), closed PR (`xmark.circle`), unavailable (`exclamationmark.circle`), and absent PR (`minus.circle`).

## Pane header lineage and ownership

The pane header keeps one 28pt identity row.
Its title is `title · sentence` on one line, or `title · word` when the row has no sentence: the identity in caption semibold `primary`, ` · ` in `muted`, the status word in caption medium in the mark's color when the row shows it, and the sentence in caption regular with the row's emphasis color, tail-truncated to no less than `Layout.paneHeaderSentenceMinWidth` (120pt).
When the header is narrower than that, `ViewThatFits` drops the sentence first and the word second, so the identity is what survives a three-way split.
A shell operation string (`forking…`, `reopening…`) takes the sentence's slot while it runs.
The header and the sidebar row call a pane by the same name, and the header's accessibility label carries the status word in the same order as the row's.
A pane with children gains a second 24pt row for the child chips, and that row exists only when there are children; a pane with none stays at 28pt.
This is a user decision between four candidates, not a default: compressing the marks onto the breadcrumb row, relying on the sidebar alone, and a bottom status bar were all rejected, because the chip has to carry the child's name where the operator is already looking.

An authoritative parent becomes one compact Return control in the first row, with icon-only fallback before current identity or actions are truncated.
The second row names the first direct child with the shared status mark, 16pt provider badge, body type and 24pt hit area; the remaining children fold into the adjacent `+N` relationship control.
Long names truncate at the tail inside the bounded chip, with their full identity in the shared tooltip.
The direct-children sheet uses a bounded scrolling list, and each title may wrap to two lines.
Pending navigation disables child changes; Retry remains attached to the inspected target that failed.
The relationship sheet inspects on row selection and navigates only through its explicit Open action.
A relationship Open or parent Return publishes one request-scoped pending state from the existing control and the retained canvas.
The same target cannot dispatch again while that request is pending.
Target retirement before dispatch and core-owned refusal, timeout, or remote-control failure keep the current pane geometry and tab topology, show the scoped reason, offer Retry when the outcome is retryable, and offer Dismiss to clear only the notice.
Retry creates a new request ID only after the prior request has settled.
The shell never derives success from an old focused layout or optimistic remote selection, never attributes an unrelated global error to the control, and never sends a second focus event as rollback.
A canvas notice preserves pending and failed feedback after successful navigation removes the source header or sheet from view.
A root with no parent carries no Return control, following the existing rule that a control with nothing to do is not drawn.

The header wash marks the pane Hide is showing, while the outer primary hairline marks the terminal that owns the native keyboard responder.
Zoom or Restore stays at the right edge, secondary fork, port, and sibling actions live in overflow, and Close remains separate.

Ownership is drawn as emphasis, not as a new color or container.
The operator's own rows are bright; delegated rows are subdued, using the existing emphasized/subdued treatment that Needs You and Done already use.
Nothing new is introduced for it: a delegated row is simply never emphasized, because it can only be Working or Seen.
A child's question or completion reaches the operator through its ancestors: the ancestor row turns unread and its descendant badge changes, and the ancestor's own group does not move.

The uninstrumented mark is drawn in exactly three places, and only on panes where an agent was detected: the pane header's 28pt identity row, the sidebar agent row, and the Overview worktree row's agent line.
It is a mark plus an accessible name, never a color alone, and its tooltip carries the whole sentence.
The subagent count sits beside it as a badge; a count Hide cannot read is drawn as unknown and never as a zero, because a zero claims the agent is working alone.

An Overview agent row reuses the same agent identity and state presentation as the sidebar and relationship sheet.
A missing row means the current live projection has no agent there; an uninstrumented mark means Hide cannot see in-process children and never means zero.

## In-Product Components

This section is the native shell contract.
It applies to the sidebar, checkout cards, tab strip, terminal and browser headers, right panel, status bar, empty and unavailable states, Search, New Agent, Settings, file search, and editor overlays.
The direction is compact Orca chrome expressed through the existing Hide components, with neutral controls and semantic state marks.
Pet windows, the menu bar dashboard, and native context menus retain their existing appearance.
The dashboard's active preservation tokens remain in the token definition file and its shared rows retain their existing system font.

### Surfaces and text

Frontmatter names below map directly to the same property on `HideTheme`.
Depth comes from the four surfaces and a hairline, without drop shadows.

| Token | Use |
| --- | --- |
| `{colors.background}` | Terminal surround, empty checkout and main canvas |
| `{colors.sidebar}` | Sidebar and navigation base |
| `{colors.panel}` | Pane headers, right panel, status bar and sheet containers |
| `{colors.elevated}` | Selected rows, compact controls, keycaps and input surfaces |
| `{colors.balloon}` | Tooltip surface, one step above elevated |
| `{colors.divider}` | One-point hairline and neutral focus outlines |
| `{colors.primary}` | Primary labels |
| `{colors.secondary}` | Supporting labels and inactive controls |
| `{colors.muted}` | Metadata and unheld search shortcut |
| `{colors.accent}` | Neutral primary action and control tint |

Semantic danger, warning, and success keep their existing state meanings.
Project and provider illustrations retain their category colors.
A selected agent retains the core's state mark and a panel fill; text remains readable on that fill.
No new state or copy is derived in the renderer.

### Typography scale

The bundled Inter variable font uses stylistic set ss03 for chrome.
Keycaps and numeric metadata use the system monospaced face.
Font scale continues to multiply these sizes; terminal and editor content retain their separate content-size tokens.
The font is bundled under its OFL license and does not require installation on the machine.

| Token | Size | Use |
| --- | --- | --- |
| `{typography.micro}` | 9px | Keycaps, small marks and numeric metadata |
| `{typography.caption}` | 10px | Supporting labels and badges |
| `{typography.body}` | 11px | Rows and control labels |
| `{typography.subhead}` | 12px | Tooltip text and explanatory text |
| `{typography.title}` | 13px | Section emphasis |
| `{typography.headline}` | 17px | Sheet headings and empty-state titles |
| `{typography.display}` | 30px | Large empty-state symbol or title |

### Spacing and radius

Frontmatter spacing and radius names map directly to `HideTheme`.
Fixed content geometry remains in its named Layout tokens rather than changing with text emphasis.

| Spacing token | Role |
| --- | --- |
| `{spacing.spacingNone}` | Flush structural stacks |
| `{spacing.spacingXXS}` | Tight label stacks |
| `{spacing.spacingXS}` | Keycap horizontal padding and small gaps |
| `{spacing.spacingSM}` | Control gaps and tooltip horizontal inset |
| `{spacing.spacingMD}` | Compact group padding |
| `{spacing.spacingLG}` | Sheet and panel content inset |
| `{spacing.spacingXL}` | Larger section and empty-state spacing |
| `{spacing.spacingXXL}` | Search empty state |
| `{spacing.spacingXXXL}` | Main empty-state surround |

| Radius token | Role |
| --- | --- |
| `{rounded.radiusExtraSmall}` | Micro shapes |
| `{rounded.radiusSmall}` | Keycaps, inline rows and buttons |
| `{rounded.radiusMedium}` | Tooltips, small cards and controls |
| `{rounded.radiusLarge}` | Search input and larger cards |
| `{rounded.radiusExtraLarge}` | Container vocabulary |

### Shared control family

The shell owns control appearance through shared styles while retaining native Button, Toggle and TextField behavior.
Sheet and popover presentation, text editing, IME, scroll physics and ProgressView animation remain platform-owned.
The native controls are not replaced with gesture-only drawings.

| Component | Appearance and geometry | State contract |
| --- | --- | --- |
| `HideTextButtonStyle` | Quiet, standard and prominent appearances; compact 24pt / body 11, regular 36pt / title 13; radius 6 | Standard uses elevated fill and divider; quiet has no resting container; prominent uses the current accent; destructive role uses danger; disabled prominent actions use the elevated surface and muted label; hover, pressed and focus remain visible |
| `HideChoiceGroup` tabs | Subhead 12; single-line labels, 4pt horizontal padding and 8pt gaps; transparent base; selected primary label and 2pt bottom indicator | Selection never adds a pill to section tabs; hover and keyboard focus remain distinct from selection |
| `HideChoiceGroup` segmented | Contained choices on sidebar, 2pt inset, divider border, radius 6, elevated selected choice | Tree/List changes only inspection mode; selected choice and group label are accessible |
| `HideSearchField` | Shared input surface, 36pt height, radius 6, 8pt gap, magnifier and 24pt clear action | Existing `HideSearchKeyboard` is the only focus owner; a local focus observation drives the neutral outline; native IME and search keyboard behavior remain intact |
| `HideCheckboxStyle` | 16pt mark inside a compact hit area; neutral checked fill and check mark | Toggle owns checked state and accessibility; unchecked, checked, disabled, hover and focus are distinguishable |

`HideInputSurface` owns text-input typography, horizontal inset, elevated fill, neutral border, focused outline and disabled appearance at compact 24pt or regular 36pt minimum height.
`HideMultilineEditor` owns native multiline editing, focus tracking, the shared input surface, and its accessibility label; callers provide only the bound text, label, and token-backed minimum height.
It is a presentation modifier and does not install focus, submit, selection or keyboard handlers.
Search retains `HideSearchKeyboard` as its only focus owner; address and form inputs retain their existing native editing bindings.
`HideMenuChipLabel` owns compact menu-trigger typography, chevron, surface and border; the native Menu retains activation and selected menu-item semantics.
`HideEmptyState` owns the shell's empty/unavailable heading, decorative icon, explanation, wrapping and accessibility grouping using real caller-provided content.
Its optional semantic emphasis colors the heading and icon for warnings and failures while keeping the explanation readable.
Standalone Pet/dashboard content and operating-system menu/alert presentation retain their explicit platform exceptions.
The protocol mismatch state uses that native alert exception rather than introducing a shell component.
Its copy identifies the older side from the two protocol numbers instead of asking the operator to infer it.
An older running Herdr uses `Restart Herdr when your work is safe` with `Open Restart Guide`; a newer running Herdr uses `Hide needs an update` with `Open Hide Releases`; an unknown comparison uses `Hide and Herdr aren’t compatible` without a potentially wrong update link.
Every state also offers `Copy Diagnostics` and `OK`, says that no workspace or agent was created, and none of its actions stops, replaces, reinstalls, or mutates the running Herdr server.
The alert is presented only from the core's typed `protocol_mismatch` readiness state, so raw CLI JSON never becomes its copy.

`HideTheme.Control` owns compactHeight 24, regularHeight 36, checkboxSize 16 and tabIndicatorHeight 2.
All controls use the existing spacing, corner and surface tokens; hover uses subtleFill, pressed uses secondary opacity, and disabled uses disabled opacity.
A disabled control cannot activate, and destructive meaning comes from the Button role rather than its text.
Pending operations keep their existing explicit progress labels and disabled actions; shared styles do not invent pending or error state.
Hover and focus observations are local to the affected control and never dispatch core events or publish shell state.
The duplicate toolbar and destructive button styles are retired into `HideTextButtonStyle`.
Settings tabs, sidebar mode choices and right-panel sections use `HideChoiceGroup`; the central work-tab strip preserves drag/close/MRU behavior and uses shared interaction feedback for its selection action.
The sidebar opts into equal-width choices and supplies option-specific command tooltips; equal width covers each choice's background and hit area, not only its layout slot.
Cmd+K, file search and Overview reuse `HideSearchField`, including its clear action and the same keyboard selection behavior.
Main-shell worktree review and settings Boolean controls use `HideCheckboxStyle`; a checkbox inside an operating-system Menu retains native menu semantics.
History, sidebar and tab rows retain their domain layout but reuse `HideInteractiveButtonStyle` for hover, pressed, focus and disabled feedback.
Shell actions use shared text/icon styles, including destructive roles and editor conflict recovery.
The old unreferenced checkout-summary renderer is retired; Overview remains the active project context composition.

The stat strip, group headers and agent rows remain owned by Overview instead of becoming general-purpose domain components.

### Recent navigation in the native shell

Control+Tab and Control+Shift+Tab cycle all unified surfaces in recent-use order, across every project, checkout and device the session holds.
This includes terminal, Browser plugin, file/editor, and diff tabs, and committing a row from another project moves the focused project with it.
The overlay is named “Recent Panels”: a single Control+Tab returns to the actually previous surface, including a file view, and repeated chords toggle between the last two surfaces.
Holding Control while pressing Tab again walks older visits rather than tab-strip or agent-list order.
Option+Tab and Option+Shift+Tab cycle projects globally and restore each project's last used surface.
Hold the chord's modifier to preview, release it to commit, or press Escape to keep the original selection.
Menu actions commit immediately.
Window > Reopen Closed Tab uses Shift+Command+T and is disabled when the session-local recent-close stack is empty or a restore is already running.
The keyboard chord and Window menu item restore regardless of whether a terminal, file editor, or search field owns focus.
Restoration is one action with no confirmation: an in-flight pane uses the existing pane-header progress suffix, while a restore without a target pane uses the tab strip's compact warning line.
Missing cwd, unavailable prior conversation, pruned Browser pane, missing file, and retryable failure states use the same warning color and inline notice vocabulary as existing pane operations, without adding a banner, card, or modal alert.
A definitive Herdr close refusal removes its reserved reopen entry, while an unconfirmed transport or acknowledgement result keeps the entry and explains inline that Hide could not determine whether the item closed.
Pane and tab mutations use the same target-scoped activity suffix and failure notice, so a delayed operation stays attached to the surface it affects and does not become a global alert.
An unknown activity state stops destructive close and offers the existing read-only `Check status` action inline; it is separate from the one-time work-interruption confirmation.
The close confirmation keeps `Keep open` as the cancel/default action and leaves `Stop work and close` as the explicit destructive choice.
Option+1 through Option+9 select sidebar agents; Command+1 through Command+9 retain direct strip selection.
Agent number hints follow the command registry: reveal only during an exact Option hold, ignoring Caps Lock, and clear on release or a suppressing sheet.
Numbered agent shortcuts are handled before native text interpretation, so terminal and editor responders cannot consume the Option chord.
With no other project or tab available, navigation keeps the current selection without a modal.
Selecting an empty project shows its existing empty state; closing an empty strip or reselecting a checkout whose terminal is starting requires no acknowledgement.
Automatic MRU pruning and concurrent selection recovery use structured diagnostics without a modal.
Workspace and device registration changes and connection-test requests use their existing list or status presentation.
Invalid runtime identities, unavailable devices, and failed operations remain visible, and destructive decisions retain their confirmations.

The project identity is `CoreWorkspaceSnapshot.id`, scoped by device, following the sidebar's Project > Workspace > Agents hierarchy.
Its checkouts are workspaces in that hierarchy, so two checkouts of one repository share a project cycle; panel history is one order over every project, and a project's own last surface is that order narrowed to it.
Herdr workspaces contributing tabs to those checkouts do not create separate Hide projects.
The existing core catalog determines grouping; navigation does not infer it from display labels or directory names.

Both switchers use the same themed overlay and registry-derived keycaps, with at most nine rows around the highlight.
Project rows show the last surface and checkout; panel rows show their project and checkout, collapsed to the checkout alone when both carry the same name, and their surface type.
Both lists give remote rows a separate trailing `Remote · <device label>` badge, leaving the agent mark, title, checkout and dirty indicator intact.
The badge uses `HideTheme.recentLocationMaxWidth` and middle truncation; its tooltip and accessibility help retain the complete host identity.
`RecentLocationBadge` supplies this identity to `HideBadge`'s bounded variant; the shared badge remains the only owner of its typography, padding, background and border.
Local rows have no extra badge, and a device absent from the registration map is identified by its actual remote ID rather than being presented as local.
Device names are resolved once with each navigation projection, not while cycling held-key highlights.
Recent Panels uses the same focused-pane agent brand mark as the tab strip, including Claude Code and Codex; file, diff and unassociated terminal surfaces keep their type icons.
A panel row whose tab holds exactly one agent pane is titled by that agent's identity with its status mark before the brand mark, derived in the core as the strip entry's `agent_identity`; a tab with no agent or several keeps the Herdr tab label, and the Herdr label itself is never changed for this.
History is session-local and retains only existing projects and surfaces.
A deleted highlight moves to the next surviving entry without reordering the held cycle and records the reconciliation in structured trace.
If none survives, cancel with a structured recovery trace and keep the core's current selection.
Empty projects show “No open tabs”; a project without an available checkout keeps the current selection and records the recovery.

### Device picker

The bottom-sidebar device trigger uses the compact menu-chip component with a laptop or server icon and the selected device name.
It opens a Hide-themed popover, not a native system Menu.
The approved A composition uses one flat two-line list: the actual device name above, Local or Remote, connection state and available agent count below.
Remote unavailable rows say `Not connected` and do not present a stale count as current.
The selected device has the selected wash and a checkmark; keyboard focus is separate and does not change device until activation.
Up and Down move focus, Return or a click selects, and Escape dismisses without a selection change.
Empty lists say `No devices available`, long names truncate in their title line, and the complete identity remains in the row's accessibility label.
The list uses the existing tooltip width and relationship-list height cap, with scrolling beyond the cap.
The reusable device-row and remote-location state sheets are maintained in the design library; the approved scope is recorded in `agents/prd/remote-interface-parity/prd.md`.
The web shell draws the same trigger and list at the bottom of its sidebar from the shared tokens (`web/src/DevicePicker.tsx`), with line icons standing in for the SF Symbols.

### Search keyboard navigation

Command+K opens agent/workspace search and Command+P opens file search with the same focused query field and first-result selection behavior.
An agent result is titled by the identity every other surface uses and subtitled by the row's second line, falling to the status word when the state chose no sentence; the pane id leaves the printed row but still matches the query and is read by accessibility, so a result can be found by title, sentence, or id.
Up and Down move the selection in display order, stopping at either end, while typing continues in the query field.
Return executes the highlighted result through the existing agent, checkout, or file-opening action; Escape closes the sheet.
The selected row uses the existing accent emphasis fill and scrolls into view.
Filtering preserves a surviving selection by identity; a retired selection moves to the first remaining result.
Empty results have no selection, and arrows or Return require no modal acknowledgement.
A stale result is checked against the live result set before execution.
File search never executes results from a previous query or checkout while its asynchronous index is updating.

### Keycaps, hint chips, and tooltips

`HideKeycap.swift` owns every shortcut glyph.
An 18-point high keycap uses `{typography.micro}`, medium monospaced weight, `{colors.elevated}`, `{rounded.radiusSmall}`, and a one-point `{colors.divider}` border.
Its horizontal inset is `{spacing.spacingXS}`.
Search keeps its keycap visible in `{colors.muted}` and emphasizes it with `{colors.primary}` during an exact Command hold.
Tab and agent number keycaps reserve their inline space so holding a modifier does not move labels.

`HideBalloon.swift` owns tooltip and floating-hint modes.
Hint mode renders the same keycap.
Tooltip mode uses `{typography.subhead}`, `{colors.balloon}`, `{rounded.radiusMedium}`, the same hairline, horizontal `{spacing.spacingSM}`, and vertical `{spacing.spacingXS}`.
Tooltip width is limited to 360 points.
There is no native tooltip layered underneath it.
Do not use native `.help()` tooltips in the main shell; use the shared command tooltip.
The excluded Pet view retains its native tooltip.

Both take a command resolved from the menu registry, effective pane binding, direct selection number, or chordless label.
A chorded tooltip reads label followed by the registry chord in parentheses; chordless controls show only the label.
The identical formatter supplies the control's accessibility help.

`HideOverlay.swift` gathers control anchors into the content root.
The overlay does not take pointer events or add layout space.
A balloon sits four points above its control, flips below when needed, and stays eight points inside the window horizontally.
Tooltip hover delay is 400 milliseconds; exact modifier holds reveal hints after 150 milliseconds.
Release, app deactivation, and opening a sheet clear hints.
Pane focus, active tab, tab order, zoom state, and disappearing anchors update the exposure set.
Pointer exit, mouse down, scroll, key down, resign-key, and anchor removal dismiss tooltips.
Fades last 120 milliseconds, or zero with Reduce Motion enabled.
The event monitor observes and returns key events.

### Icon buttons and badges

`HideIconButton.swift` owns icon-only actions in the sidebar command bar, tab strip, pane headers, and browser toolbar.
Callers provide the symbol, help text, action, optional selection state, and a role; they do not add size, padding, foreground, background, or button-style overrides.
`standard` uses `HideTheme.IconButton.standardSize` (32×32pt) with an elevated resting surface.
`toolbar` uses `HideTheme.IconButton.toolbarSize` (24×24pt) with a transparent resting surface, fitting the 28pt pane header and 32pt tab strip.
Both use `radiusMedium`; icon typography is `body` for standard and `caption` for toolbar, independently of the hit area.
Hover raises foreground contrast and adds `Opacity.subtleFill`; selection uses `Opacity.selectedFill` and the accessibility selected trait.
Press uses `Opacity.secondary`; disabled uses `Opacity.disabled`, suppresses hover emphasis, and delegates activation blocking to the native Button.
Hover state stays local to each button; repeated identical hover events publish no state changes, and no runtime dispatch or new timer is added.
The shared command tooltip retains pane/tab targets, and the accessibility label defaults to help unless a more specific name is supplied.

```swift
HideIconButton(
    systemImage: "plus",
    help: "New Tab",
    variant: .toolbar,
    command: .menu(.newTab),
    action: model.addTab
)
```

Text buttons, menu triggers, title-bearing navigation rows, and the agent lineage renderer retain their own components and semantics.
Workspace disclosure uses its entire 36pt row, so its chevron is an indicator rather than an icon button.
`HideBadge.swift` owns compact labels, with `HideTheme.badgeHeight` (16pt); agent provider artwork remains in the existing agent badge.
A state keeps its symbol and semantic color when read, with reduced emphasis instead of a new word.

### Sheets, overlays, and abnormal states

Search, New Agent, Settings, and file search each host the same tooltip overlay.
Sheets use panel containers, headline titles, body or caption supporting text, and the same spacing scale.
The Settings sheet is `settingsSheetSize` wide and grows with the presenting window from that height to `settingsSheetMaxHeight`, keeping `settingsSheetWindowInset` clear above and below, so a tab taller than the smallest size is read without a scroll on an ordinary window; the Settings scene, which is its own window, keeps the smallest size.
The selected provider card uses an elevated fill and stronger neutral border; its status remains readable.
Disabled Start and Add controls retain their existing enablement conditions and use disabled emphasis.

### Projects and checkout context

Projects use the native sidebar list, existing Search (Command+K), and persisted project/workspace disclosure.
Projects and their checkouts sort by the latest authoritative agent activity timestamp or Git commit timestamp, descending; server state-change sequence breaks timestamp ties, and stable IDs break remaining ties.
Missing activity remains absent and sorts after known activity; no UI interaction or local clock invents recency.
An active pane without a wall timestamp can only contribute its server sequence, not a fabricated date.
Activity orders projects inside one device group and never across two, and the remote session list follows that same order rather than its own alphabetical one.
Each project row's trailing detail carries the recency the order was decided by, after the count it already showed: `2 agents · 3m`.
That time is one token in the elapsed form the agent rows already use, with the first minute written `now` rather than counted in seconds, and a project the core reported no activity for shows its count alone rather than a claimed recency.
It is recomputed from the core's timestamp on each snapshot, so it ages while the app is open without a timer of its own.
The project name takes the row's width first; the trailing detail truncates in a narrow sidebar rather than pushing the name out.
Raised Needs You and Done groups retain their status ordering above Projects.

A registered project can be pinned from its row menu, the same `Pin` / `Unpin` item in the trailing `⋯` menu and the row's right-click menu, which otherwise carry the same items.
Pinned projects are drawn once, under a `Pinned N` section header that sits between the raised groups and `Projects · Recent activity`, and only while at least one project is pinned; the activity header stays and counts the unpinned projects.
Inside `Pinned` the order is the tree's own, device first and then latest activity, so a local pin always precedes a remote one and pins never repeat per device.
The section header is the existing sidebar section label with no pin glyph, the row is the ordinary project row with its disclosure, selection and trailing detail, and nothing changes in Search.
The pin lives on the project's registration (`WorkspaceRegistration.pinned`), so it survives a relaunch, an older state file reads as unpinned, an unregistered folder row offers neither `Pin` nor `Remove project…`, and removing the registration takes the pin with it.
A pinned project is exempt from the device's `Inactive projects` fold whatever its activity; its own stale worktrees still fold behind its `Inactive N` row.
The remote navigation context, chosen from the sidebar's device selector, carries no pins because its wire carries none.

Inactive work is folded without changing that activity order.
A project's merged, closed, or seven-day inactive secondary checkouts move behind one trailing `Inactive N` disclosure, while its primary and every checkout with live work, local changes, unpushed commits, or current focus remain visible.
When every checkout in a project is inactive, the project moves behind the trailing `Inactive projects N` disclosure for its device.
Both rows use the existing sidebar interaction feedback and chevron language, default closed, and remember expansion independently at the project-path and device levels.
Opening a fold restores the original project or checkout rows, including their existing elapsed time and pull-request state, so the reason stays encoded in the row rather than repeated as explanatory copy.
Search continues to index the complete project tree; choosing a folded result brings the focused row back into the active list without opening either fold.
The project tree uses one token-based indentation ladder: top-level projects and the device inactive disclosure start at the root inset, their checkout or archived-project children advance one level, and checkout rows revealed by an inactive disclosure advance once more.
Selection begins just before the selected checkout's own content edge instead of spanning back to the top-level edge, so its background preserves the child relationship.
Each completed project block leaves the same project-level vertical gap before the next sibling or top-level inactive disclosure; rows inside a block retain their compact spacing.

The right panel order is Overview, Explorer, History, then Sessions.
The compact section selector uses text labels on one line without a competing checkout title.
Saved Git section selections migrate to Overview; other selections survive, and new state starts on Overview.
Overview is project-scoped and is one list: the current Project's worktrees as groups, each holding the agents working in it, under a strip of derived project facts.
It has no mode, no graph, no inspector and no summary rows; what a group or a row has to say is on its own line, and what an operator would look up sits in a tooltip or an existing popover.

The top block carries the project name, `Project · N workspaces · M inactive` (the inactive count only when there is one), the refresh icon button, and the stat strip.
The strip's first row is always drawn for a Git project: `N GB on disk`, which opens the existing disk popover, and `⑂ N open PRs`, which opens the existing GitHub popover; zero open pull requests is a measured value and is drawn.
The second row holds only cells with something to act on, `main ↓N behind origin` and `N merged to clean up`, and is absent when neither applies; behind is read only, in the warning color, because Hide does not fetch, and cleanup opens the review sheet.
Every cell follows one glyph language: a number when it is known, `…` in muted while it is being read, `?` in the warning color when it cannot be, and absence when it does not apply; the reason is in the tooltip and the popover, never on the surface.
A plain folder project has only its size in the strip and only its size on its group header.
GitHub summarizes active branches from the existing bounded, per-branch PR selection, not an invented repository-wide PR total.
The popover states the lookup window and preserves loading, no recent PRs, authentication, unavailable and stale results.

A group header is a minimum 52pt comparison card with its identity on the first line and its badge strip below.
Its first line is the disclosure chevron, the same checkout kind glyph as the sidebar, the semibold branch, and a muted one-line purpose; when no purpose exists, the pull-request title is the fallback, and when neither exists only the branch remains.
The full purpose is the header tooltip when the visible copy truncates.
Its second line is a `HideBadge` strip in the fixed order pull request, checks, files, behind, ahead and allocated size.
The strip stays on one line when it fits; at a narrow width it wraps without truncating the pull-request lifecycle or review-decision word or dropping a later badge.
The pull-request badge includes the bundled octicon, number and visible lifecycle or review-decision word; merged and closed outrank draft, draft outranks review decisions, and open is the final fallback.
Checks are `✓ Checks`, `✗ Checks` or `… Checks`, and the badge is absent when the pull request has no checks.
Files are `Clean`, `N files`, `… files`, `? files` or `missing` with the matching muted, warning or danger treatment.
Behind is shown only above zero as `↓N behind <base>` in warning; ahead is shown only above zero and only without a pull request as `↑N ahead`.
A plain folder project keeps only its allocated-size badge.
The pull-request and checks badges open the existing GitHub popover, `N files` opens that checkout on History, and `Clean` does not activate; none of these badges opens GitHub directly.
The chevron folds the group's rows and shares the sidebar's collapsed set.
Groups sit in one fixed order, the primary checkout first, then linked worktrees oldest first by the time they were added, then a `› Inactive N` fold that shares the sidebar's fold state; an unfolded inactive checkout is a header line with no rows.
No agent state and no search reorders a group.

An agent row is mark · badge · title · optional detail · `↗`, at the pane header's 28pt height, and the whole row is the button: a click shows that pane, and the `↗` says so.
A delegated child is indented under its parent with `↳`; a child delegated into another worktree stands in its own group with the caption `↳ from <parent> · <parent branch>` under it.
An empty group has one row, `No agent · Start agent…`, whose menu is the same `Terminal only / Claude / Codex` choice the header's `New agent here ▸` offers.
The header's context menu is `New agent here ▸`, `New worktree…`, `Set purpose…`, `Set as base branch`, then `Open pull request #N` when there is one and `Open in History`, then `Copy Path` and `Open in ▸`, then the destructive `Delete worktree…` behind the sidebar's gate and wording.
An agent row's context menu is `Open pane`, `Reveal in sidebar`, `Copy pane id` and the destructive `Close pane…`, with the same confirmation the pane header uses.
Search matches an agent's title, its state sentence and a branch; it keeps the matching rows with their group header, and no match reads `No matching agents or workspaces` with a `Clear search` action.

Loading, local-only, no-workspace, disconnected and unreadable states are distinct and each is drawn in its smallest form.
While the live agent projection is unavailable a caption above the search says so and the last known rows stay clickable; a value Hide cannot read is `?` and never a zero.
Only a row click, the header click, the `N files` chip and the menus' explicit actions change pane focus, checkout focus, the panel section or read state; scrolling, folding, searching and refreshing never do.
The pinned Herdr contract offers lifetime-scoped display metadata and agent lineage but no persistent commit-authoring relation, so Hide neither adds Git trailers nor renames branches.
Retired or moved panes leave the list on the next topology projection.
Remote Overview explicitly reports that local Git context is unavailable.
Explorer contains file navigation only, with no checkout summary above it.
History retains its diff navigation; Git/PR context remains in Overview and Workspace controls.
Sessions is Project-scoped and opens as a mixed latest-first Claude Code and Codex list with provider badges, `All / Codex / Claude Code` filtering, and search.
Its `Sessions / Memory` mode is restored per Project through core-owned UI state.
Rows are read views in the narrow panel; one click opens a read-only session or Memory detail in the checkout's existing replaceable editor preview, and beginning a Memory edit promotes that preview to keep-open.
The row owns its actual provider, first request, checkout, time, availability, content, source count, and lifecycle copy; unavailable sources dim only their own row and expose Retry plus Copy source location.

Memory Off is a disclosure state, not an empty management screen.
It explains provider transmission and subscription use, provider retention and deletion limits, local derived-data retention, and known-secret exclusion before the single primary `Turn on Memory` action.
An outdated hook opens an explicit confirmation that names the local Claude Code and Codex configuration ownership and preservation boundary before Hide updates its marked entries.
Memory On shows a flat searchable list, `Memory on`, active count, source count, and update time without exposing categories, embeddings, indexes, or merge internals.
Actionable failures use the smallest local notice with exactly one matching action: update hooks, open Settings, sign in, retry, review a conflict, or review capacity.
Turning Memory off is immediate and retains its count; deleting all derived Project Memory requires a destructive confirmation that distinguishes it from raw provider sessions.

Memory detail shows the full item, learned time, revision history, provided-session count, and navigable provider source sessions.
Edit promotes the preview and has explicit Save and Cancel; Forget is reversible in the app session, while confirmed Project deletion is not.
Conflicting candidates remain excluded from injection and offer `Keep existing`, `Replace with new`, and `Forget both` in detail.
After learning, a compact `Learned N memories · Undo` notice does not cover the working surface.

A provided prompt shows `✦ Memory attached N` only when N is greater than zero, and uses attached or provided rather than used or applied.
Activating it changes panel visibility, section, Memory mode, and the exact `This turn` filter in one core event; `Show all` clears that filter.
SessionStart provenance is `Project Memory ready · N` only when N is greater than zero.
At 320pt, 344pt, and 400pt widths, mode, provider, count, and action controls remain legible; long snippets tail-truncate with the full value in tooltip and accessibility text.
VoiceOver reads session rows as provider, first request, checkout, time, availability and Memory rows as content, source count, status.
VoiceOver reads a group header as its branch, purpose and each badge in words, such as `prd/hide-orchestrator, review the checkout flow, pull request 107 changes requested, checks failing, 12 changed files, 2.4 GB`, and a checkout row as its name, lifecycle, commit age and second-line sentence in that order; every tooltip is also the control's accessibility help.

Allocated on disk sums main, linked worktree folders and the actual shared Git directory once.
Nested roots belong to the longest matching root; hard links share one inode allocation, and descendant symlinks are not followed.
An incomplete component has no total; the UI separates the confirmed subtotal from unavailable target measurements.
Allocated blocks are not a promise of reclaimable space, particularly for APFS clones.

Cleanup opens a review sheet with separate Available and Excluded groups, exact branch and folder, allocated size or failure, and target-specific exclusion reasons.
Nothing is preselected and Remove is disabled until a user explicitly checks an eligible folder.
Main/current, dirty/untracked, live-pane use, locked, nested, detached, unknown and not-confirmed-merged targets are excluded.
Only clean, unused linked worktrees merged into local main can be removed, without force; branches and history remain.
Ordinary merges are proven by Git ancestry.
When squash merge leaves the branch commits outside that ancestry, the exact GitHub pull request head commit must equal the reviewed worktree HEAD and its merge commit must already be an ancestor of local main; a matching branch name, closed pull request, stale lookup, or remote-only merge is not enough.
An inaccessible cwd from a pane whose old worktree folder is already gone does not exclude unrelated current worktrees, while an absolute stale cwd still excludes any current worktree whose path contains it.
Removal is one `git worktree remove`; every build cache a checkout owns lives inside it, so nothing outside the folder is deleted or left behind.
Confirm rechecks current Git and Herdr state before each target and refuses changed state with Review again recovery.
Completion lists individual removed/refused outcomes; repeating the same completed intent does not repeat removal.
Review and cancel perform no filesystem mutations.
This file deletion flow is separate from registration removal.

Overview draws on the shared scale alone: the project title uses the 17pt headline token, stat values are 12pt semibold mono with a 10pt caption beside them, group headers are 11pt semibold with a 9pt mono detail line, and rows use the pane header's 28pt height and the sidebar's agent mark, badge and lineage widths.
Actual agent lifecycle colors retain their existing semantic meaning; the warning color marks a behind count and an unreadable value, and no color is introduced for the list.
Only the shown pane's row receives the elevated surface.
Menu actions reuse `HideTextButtonStyle` so system appearance cannot introduce a competing light button surface.
GitHub and disk popovers use the same dark panel surface as existing PR details and show pending refresh alongside any retained result.
The cleanup sheet uses the existing 440pt worktree dialog width and a 560pt height with a scrolling list.
Colors, typography, spacing, corners, status marks and tooltip/accessibility help come from the shared shell system.

`Remove project…` removes only Hide's registration and never deletes files, worktrees, sessions or Herdr workspaces; it is offered only for registered projects, from the same row menus as the pin.
A project Herdr has no pane in is confirmed with the registration-only copy (`Hide will remove only its registration. …`, `Remove registration`).
A project with panes is not refused: the confirmation reads the core's counts (`Closes 3 panes (2 running agents). The folder, repository, and worktrees stay on disk.`, destructive `Close 3 panes and remove`, `Cancel`), the parenthetical is omitted at zero running agents and the nouns follow their counts.
On confirmation the core sends `pane.close` for every pane in the project's checkouts from a worker outside the runtime mutex and waits for Herdr's snapshot to confirm they are gone, the same handshake worktree deletion uses; only that confirmation removes the registration and its row.
A timeout or refusal leaves the project registered with the reason in the error banner, and a repeated `Remove project…` continues from the panes that remain; a repeat while the close is still running starts nothing.
Removing the project that holds the focused checkout moves focus and the pane selection to the next project, the way a Herdr restart does, so no sync reports the closed pane as unavailable.
A registration id is keyed by its folder, so adding that folder back while the close is still running is refused with the reason in the banner (`workspace.remove_in_flight`), removing a project whose first pane is still being opened is refused the same way (`workspace.create_in_flight`), and an add that lands anyway cancels the removal and says so (`workspace.remove_cancelled`) rather than losing the project it just opened a pane in.
A completed removal disappears from the core snapshot and a repeated request is a quiet no-op.
Save failures remain caller-visible; normal no-op results never become alerts.

The project sidebar requests GitHub data once when a local Git project appears; repeated appearances reuse the same result.
Only the selected Overview project's GitHub cell popover owns explicit refresh and status detail; the sidebar has neither a GitHub popover nor a refresh menu action.
All triggers share the existing bounded background reader, authentication and cache.
Explorer and History do not independently start GitHub queries.
Loading, missing authentication, query failure and stale results remain explicit; an absent or unrecognized CI result never renders as passing.
History is a compact navigation list; activating a row opens a read-only diff as a central editor tab instead of dividing the panel vertically.
Diff tabs use the editor's monospaced content scale, fixed old and new line-number columns, semantic added and removed tints, and horizontal scrolling for long lines.
Their scroll canvas fills the editor viewport, with short diffs anchored at the top left and long diffs growing beyond it for scrolling.
The web Workspace shows Explorer and History as two independent tools; see Web Workspace.
History rows retain the Seti file mark, a separate status letter and available line counts, with the full path, rename origin and comparison group in their tooltip and accessibility name.
Diff tabs use a distinct type mark and keep the same preview and Keep Open behavior as file tabs.
Text file tabs use a fixed line-number ruler and preserve source whitespace through non-wrapping horizontal scrolling.
The ruler clips all drawing to its own bounds, and text loaded into an initially empty editor retains the editor's monospaced content font.
Syntax selection comes from the core's filename-aware language result, including extensionless configuration files and JSON-family extensions.
Loading and failed tree states, empty sidebar and checkout, missing pane projection, waiting pane size, browser connecting or disconnected, and editor conflict or stale banners use the same tokens as normal state.
Remote and browser idle, loading, ready, stale, unavailable, and failed phases preserve their existing labels and semantic status colors.
Existing controls retain their accessibility contracts; Overview adds named stat, group, row, search, start-agent and cleanup targets.

### Enforcement

Add a named token before using a new visual value.
`node scripts/check-design-contract.mjs` is the entrypoint CI runs, and it runs all three checkers below.
`check-hide-theme-literals.mjs` rejects inline styling, native tooltips, and shortcut glyph literals outside token definitions and Pet files.
`check-hide-components.mjs` prevents duplicated component ownership and checks every migrated tooltip file.
`check-design-controls.mjs` counts control usage against `scripts/design-control-policy.json`.
The shell test parses this document's frontmatter and typography table against actual token values.

### Design consistency and control ownership

`HideTheme` owns visual tokens; the shared component owning a control owns its appearance and interaction states.
A screen chooses the component's supported role or variant and supplies data and actions.
It must not introduce a parallel button, picker, disclosure or checkbox style merely to match one screen.
Use existing `HideTextButtonStyle`, `HideIconButton`, `HideFormPicker`, `HideBadge`, `HideKeycap` and `HideBalloon` where their contracts fit.
A missing component or variant is a design decision: describe its role and states here before a separately authorized UI implementation, then update the owner and all affected consumers together.
A token name alone is not approval to add a new visual treatment.

Every interactive component's contract specifies its label and accessible name, supported sizes/roles, default, hovered, pressed, keyboard-focused, selected and disabled states where applicable.
Pending actions must show pending feedback and preserve the existing retry/duplicate-action contract.
Relationship navigation uses the core's request-specific pane-focus outcome as that contract's authority.
The adopted `Component / Relationship action states` and its referenced Ready, Pending, Target unavailable, and Open failed states use existing semantic and surface tokens without opacity overrides, so the disabled Open label, warning icon, long Korean or English reason, and Retry action remain legible.
Status indicators retain text or a symbol alongside color; actual product state supplies their values.
Keyboard activation, selection, IME handling and focus semantics remain part of the control contract when its appearance changes.
Focus and hover are local presentation state and must not publish core snapshots or trigger Git/disk work.
Geometry, color, typography and spacing are selected through existing tokens and supported variants rather than downstream overrides of a shared component's appearance.

The machine-readable control policy is `scripts/design-control-policy.json`.
Its exact paths identify approved owners, existing legacy uses and platform exceptions, with a reason and count for each detected construct.
The policy records native control invocations inside shared owners, the Pet dashboard empty-state exception and native menu controls.
Remaining input invocations are owned wrappers or the address editing boundary, each with the shared input surface.
Overview's stock segmented Picker, cleanup's stock checkbox appearance and obsolete toolbar/destructive styles have no retained allowance.
TextField, SecureField and TextEditor invocations are counted as well: new input controls belong in a documented shared owner, while enumerated existing fields remain legacy uses.
An allowance permits a specific native behavior boundary; it does not permit a caller to invent another appearance.
A new occurrence, an unlisted style implementation, or a new source file using these constructs fails the check, including in nested directories.
When an occurrence is removed, reduce its allowance in the same reviewed change so old exceptions cannot silently become spare capacity.
Do not regenerate or increase allowances just to make CI pass.
A new exception requires its owning reason and design decision in this guide, plus the explicit policy diff.
Existing platform menu/sheet/popover presentation, scroll behavior, SF Symbols and `ProgressView` retain native behavior; the checker does not prohibit their use or claim to restyle them.
The Pet design exception remains in the existing token/component checks; the control inventory still bounds the currently enumerated uses rather than exempting every new file with a similar name.

`node scripts/check-design-contract.mjs` runs the token, component ownership and control-policy checks together.
`--staged` reads ordinary staged source and checker files into a temporary directory, checks that exact content and removes the temporary copy without changing the index or working tree.
These are static source checks, not a Swift compiler or an aesthetic evaluator.
The control inventory deliberately does not count every Button invocation because buttons can inherit an approved root style.
It does not resolve inherited styles, AppKit controls, protocol aliases or arbitrary custom drawing; component ownership and token checks provide complementary bounds.
They catch the listed syntax and counted drift; aliases, an equally sized replacement inside an allowed legacy file, and visually poor compositions made from valid tokens are not proven correct by a passing result.
The Git hook uses the same repository checks for every contributor without changing global configuration.
CI and the opt-in local pre-commit hook run the same checker; activation and failure recovery are owned by CONTRIBUTING.md.

### Native component catalog and visual review procedure

A native component catalog is planned and is not currently a shipped screen.
When separately authorized, it should render the actual shared components with explicitly labelled sample data and local state, without dispatching project, pane or filesystem actions.
Its coverage should include buttons, section tabs and mode choices, search fields, checkbox/disclosure controls, workspace rows, status labels and empty/pending/error states.
It must reuse product components rather than draw a second approximation of them.
Review actual hover, focus, selection and disabled feedback alongside English, Korean, mixed-script labels and long unbroken identifiers.
Use the approved Overview structure as the product composition reference; sample PR counts and activity labels never enter runtime data.

For a visual change, first show the component states and the affected product screen in the exact identified native candidate at 320, 344 and 400pt panel widths where supported.
Follow docs/PERFORMANCE_TESTING.md for isolated state and app/process coordination; preserve the installed app and operator panes.
Record the build, actual widths, interactions, screenshots and unverified states under `agents/runs/<slug>/`.
Obtain human judgment on a new visual baseline before treating it as approved; a passing hook, CI result or image diff cannot supply that judgment.
If layout structure remains unresolved, present distinct candidates before implementation under design principle 11.
Do not refresh an expected screenshot merely because a new build differs; explain the intended design change and review it.
Shared controls and their policy checks are implemented; a native component catalog and screenshot-comparison service are not yet available.
Visual baseline approval remains a separate human review.

## File document toolbar and Markdown

The central file surface uses one document toolbar, preserving the existing tab strip and Explorer.
The current folder and filename give context; Find uses AppKit's native find bar, Wrap changes the source text container, and reveal actions target Explorer and Finder.
Controls use HideIconButton and the shared tooltip/accessibility renderer.
Unsaved drafts and the existing read-only/conflict notices remain visible in either mode.
Diff tabs retain their existing viewer.

The core names each open file's kind (`document_kind`: text, markdown, image, pdf, binary) and the overlay picks the adapter from it; the toolbar is the same bar in every kind, with the controls a kind cannot use taken away rather than left dead.
A PDF, recognised by its `%PDF-` signature whatever its name, shows in PDFKit's view: continuous vertical pages fitted to the width, text selectable, nothing editable.
Its toolbar keeps the breadcrumb and the two reveals, shows Find disabled with the tooltip `Find is unavailable for PDF`, and hides Wrap, the Markdown mode group and Unsaved, which a PDF can never earn.
A PDF that PDFKit cannot decode, cannot be read, or is password-protected shows the `PDF unavailable` empty state with the reason under the same toolbar.
An image hides Wrap as well; a file that is not UTF-8 shows the `Preview only` empty state with `This file type cannot be shown as text.` and keeps Wrap disabled beside a disabled Find.
The size and read-only reasons are unchanged.
Task-local design review covers each changed document kind, including the PDF failure when that surface changes.

Markdown files alone show the centered Live/Source HideChoiceGroup, and Live is the default.
Both are editors over the same draft: Live draws the formatting in place and hides the markup on every line the caret is not on, the way Obsidian's Live Preview does; Source is the monospaced editor with its line-number ruler and Wrap toggle.
The core owns mode and source wrapping per open file tab; another tab has independent choices, returning to a tab restores them, and close/reopen or app restart starts Live with source wrapping off.
These choices share the existing ephemeral editor-tab lifecycle and are not added to persisted UI state.
Autosave captures its file identity when scheduled so a subsequent tab selection cannot redirect the write.
Closing a file tab carries that tab's matching pending save in the same close intent, and the tab remains open with a visible error if the exact path and contents cannot be saved.
The native editor retains only its latest unacknowledged draft while older core snapshots arrive, preventing a snapshot echo from moving the caret or replacing newer input.
The syntax highlighter and text view use the same scaled monospaced font; unchanged view updates do not restart highlighting or reset its typography.
Core acknowledgement, switching file identity, and explicitly reloading a disk conflict settle that presentation buffer.

The Live view is one editable text view whose storage is the source, so draft, autosave, Find, selection and copy read the same text Source would (`MarkdownLiveEditor.swift`).
Foundation's Markdown parser, asked for source positions, supplies block and inline structure as ranges over that source (`MarkdownLiveSource.swift`); the characters of a block no run covers are its markup.
Formatting is attributes over those ranges, and markup is hidden by the layout manager generating no glyph for it rather than by removing it: the text never changes, only what is drawn.
The line holding the caret, and every line a selection crosses, shows its source; a fenced code block is one unit, so a caret anywhere inside it shows both fences.
Markup hides again the moment the caret leaves, with no animation.
Headings 1 through 6 take `HideTheme.Editor.headingFontSizes` at semibold with their hashes hidden; bold, italic and strikethrough hide their delimiters, italic as a skew because the bundled Inter has no italic face; inline code and fenced blocks use the editor's monospaced font over the panel fill, a fenced block filling its full measure; unordered markers draw as a bullet and ordered markers keep their digits, both with a hanging indent; a quote indents behind a `quoteRuleWidth` bar and hides its `>`; a link shows its text in the accent color with the brackets and URL hidden; a `---` line draws as a rule.
Tables, images, HTML, footnotes and task lists are not drawn: they stay monospaced source, editable in place, and a parse the view cannot use leaves the whole document monospaced with the reason in the notice bar while typing continues.
An empty Markdown file is an empty Live editor with the caret in it.
The body is Inter at `documentFontSize` with `documentLineSpacing`, wrapped in a `documentWidth` measure the text view keeps centred in the pane; Korean uses the font's native fallback and word wrapping, and the text-scale chords apply in both modes.
Links open on Command-click only, through the existing owner: HTTP(S) in the external browser, a relative file inside the current symlink-resolved checkout as an Explorer reveal, and anything else as a caller-visible notice; a plain click places the caret.
Raw HTML is inert literal text, never a browser execution surface, and no image resource is read or fetched.
A document over 256 KB opens in Source with the Live option disabled and the notice `Live preview is off for files over 256 KB` under the toolbar, because Live re-parses the whole document after each edit.
That re-parse runs off the main thread, one at a time, with a burst of keystrokes coalescing into at most one more; attributes are re-applied only over the region whose plan changed, and hidden markup is recomputed from the selection alone, so a caret move touches its old and new lines and nothing else.
No re-parse lands while an IME composition is marked, so Korean input composes uninterrupted.
In both modes a Markdown document answers the list keys with orca's rules carried over to source text (`MarkdownListEditing.swift`): Enter after an item's text starts the next item with the same marker or the next number, Enter or Backspace on an empty item removes its marker and leaves the list, Tab and Shift-Tab move an item one level (two spaces) with a numbered item counting in the column it joins, the column an item left renumbers from 1, and a lone `1. ` line stays as typed on Enter because it may be text; none of this runs while a composition is marked, and a task box is not an item.
The text that results is ordinary Markdown, with nothing hidden or special in it.

`MarkdownLiveSourceTests` asks the plan for values: which ranges each construct styles, which characters it hides, and which the caret reveals.
`MarkdownLiveEditorTests` drives the real AppKit view: hidden glyphs off the caret line and revealed on it, a fenced block as one unit, typing into a formatted line landing at the caret, Korean and English wrapping in the measure, Command-click against plain click, the raw fallback with its notice, the 256 KB Source fallback, and the list keys reaching both views but not a composition.
`MarkdownListEditingTests` asks the list rules for the edit each key produces, as text with the caret marked.
`ConversationMarkdownTests` covers the conversation ledger's separate rendered Markdown, which keeps tables as textual cells and images as labelled descriptions.
Native editor tests check complete typed and autosaved content, final lines without a newline, and the first glyph remaining outside the line-number ruler across wrap changes and window widths.
They exercise real AppKit layout and the core file-save boundary, and reproduced missing/reordered characters, a nonterminating EOF draw, and covered leading glyphs before the fixes.
These few user-outcome tests retain no mock call graph or exact view hierarchy contract.
The core's file-view lifecycle test checks independent tabs, repeated-intent convergence and reopen defaults.
Native screenshots remain necessary to approve toolbar spacing, font fallback and narrow-window behavior.

## Editor preview tab

A single click on an Explorer file or a History row opens it in the checkout's one preview tab, VS Code's model: the strip draws the title in the theme's italic variant, and the next single click replaces the tab in the same slot instead of adding one.
The core owns the flag (`EditorTabSnapshot.preview`, and the strip entry beside it), decides replacement and promotion, and the shell only says what the click meant: `file_open` and `changes_select` carry `preview`, and one `file_keep_open` event promotes.
Promotion happens in the same slot, on four triggers: a double-click on the Explorer row, a double-click on the tab title, the first edit, and File > Keep Open (`⇧⌘K`, declared in `ShellMenuCommand`); a drag to a new slot promotes as well.
A dirty tab is never replaced: the core promotes it where it sits and opens the new preview beside it.
Every other entry point - Cmd+P, Reopen Closed Tab, a Markdown or terminal link, a file the Explorer just created - opens an ordinary tab, and a single click on a file that already has a tab focuses it without touching the slot.
A replaced preview tab is not a close: its document, Markdown mode and wrap state are dropped and nothing enters Recent Closed; closing the tab yourself records it as any file tab.
Editor tabs stay ephemeral, so the flag is never persisted.
This one-slot model is the native shell's; the web Workspace keeps one preview per View area and remembers it with its layout (see [Web Workspace](#web-workspace)).

The italic variant is `HideTheme.Typography.previewSlant`, an oblique of the bundled Inter face applied through the font matrix, because that face carries no italic axis; it is reached only through `hideFont(italic:)`.
The tooltip and the accessibility label read `name · Preview` while the tab is one and drop the suffix on promotion (`EditorTabTitlePresentation`); the tab's colors, close button, keycap and its Recent Panels row are the ordinary tab's.
Task-local design review covers the preview, promoted and dirty-kept states; Pen substitutes the family's own italic, so its angle must be reviewed in the native app.
The core's rules are fixed by `runtime/tests/editor_preview.rs`, the shell's by `EditorPreviewTabPresentationTests`.

## Explorer file management

The local tree's context menu is the native `NSMenu`, in VS Code's order: New File, New Folder, a separator, then on a file row Open with Default App, Open in Browser Pane and a separator, then Reveal in Finder, Copy Path, Copy Relative Path, a separator, Rename, a separator, Delete.
A folder row has no open items, because its open is Reveal in Finder; the empty area below the rows stands for the root and offers only the two creations; a remote tree is read-only and offers only the two copies.
`WorkspaceOutlineMenuPresentation` decides the item set, so the menu a click gets is a value a test can ask for.

Open with Default App hands the file to macOS through the existing external opener, and a refusal is the notice `macOS could not open <path>: <reason>`.
Open in Browser Pane stays in the menu whether or not it can act; when it cannot, the item is disabled and its tooltip carries the one reason, in the order the operator can act on it: `Remote files open on their device`, `Opening…`, `Node.js is not on PATH`, `Not connected to Herdr`, `No focused pane to open beside`.
The menu does not auto-enable, so the disabled state is the presentation's decision and not AppKit's.
The outcome of an open is a notice: the host's own sentence when it refused, `No running chromux profile. Launch one with chromux launch <name>.` when nothing is running, `Browser pane did not open in time` after thirty seconds.
`Component / Explorer file menu` in the design library draws the file menu enabled and with the item disabled.
`docs/BROWSER_PANES.md` owns how the pane is opened and which profile is chosen.

Delete has two entry points, the menu item and `⌘⌫` while the tree holds the keyboard, and both end in the same confirmation: an alert titled `Move 'name' to Trash?`, a folder told that everything in it goes too, and both told the item can be restored from Finder, with Cancel as the default and Move to Trash as the destructive button.
Nothing reaches the core without that alert; Cancel and Esc send nothing.
The `⌘⌫` chord is declared in `ShellMenuCommand` with the tree as its scope so the collision checks see it and a pane command cannot be rebound onto it, and the application menu builds no item for it, because the same chord anywhere else must reach the terminal untouched.
The item goes to the macOS Trash through the core's `path_trash` event, never to a permanent delete; the selection moves to the next sibling, else the previous one, else the parent, decided by the tree and carried in the event so the cursor lands in the same frame as the removal.
The event also carries the item's inode as read when the prompt opened, and the core refuses an item that was replaced at that path while the modal was open, so what leaves is what the modal named.
A failed move keeps the item and puts the reason on the row under it, the way a refused name is shown.

New File, New Folder and Rename take the name in the row itself: a draft row is inserted at the top of the target folder, or the item's own label becomes the field.
Enter sends, Esc and any other loss of focus cancel, and an unchanged rename closes the field without asking anything.
An empty name, a name with `/`, and a name already in that folder are refused before the round trip; the field stays and the reason is one row directly under it, in the danger color with no icon.
The field draws on the elevated surface so it reads as an input among labels; nothing else about the row changes.

The filesystem change is the core's: one event carries the request, the core refuses paths outside the focused checkout and any overwrite, runs the exclusive call off the runtime mutex, and settles one `explorer_operation` slot.
The tree reads a finished slot to re-read only the folders it touched, keeping every loaded folder and its expansion, and a failed slot to place the reason under the row the change started from.
The selection moves to the new or moved item because the core sets `selected_path`; expanded folders and open file tabs inside a renamed folder follow it.

A drag moves one item inside the tree.
Dropping on a folder puts the item inside it, on a file puts it beside that file, and on the empty area puts it at the root; the receiving folder row is what highlights.
The same parent, the item itself and a folder inside the item show no drop indicator and accept nothing.
The pasteboard type is private to the tree, so Finder never reads the drag as a file and nothing is copied out.

The existing 22pt native row reserves a fixed 12pt Git status slot at the trailing edge.
The outline sizes each indented cell within the effective document viewport before laying out its contents; neither general column resizing nor expansion-driven outline resizing can override that width.
Seti file artwork and disclosure keep their existing columns, and the filename truncates before the Git slot instead of moving it.
Modified, Added, Untracked, Renamed, and Conflict render as `M`, `A`, `U`, `R`, and `!` with semantic color and a matching status name in tooltip and accessibility help.
A folder with any changed descendant renders `●`; the mark describes derived folder state and never relabels the folder as a modified file.
Deleted descendants still mark an existing ancestor folder but never create a file row that no longer exists.
Clean and unavailable decoration both reserve the slot, while loading and failure are distinguished by the panel notice above the still-usable tree.
The decoration is not a control and cannot intercept file open, disclosure, inline editing, drag, keyboard navigation, or the native context menu.

Git state comes from the root-scoped History projection.
Rename keeps both previous and current relative paths, conflict remains an independent status, and folder state is derived from the complete changed set rather than only loaded outline children.
Explorer visibility reuses the History reader's bounded two-second refresh outside the runtime mutex.
Switching Workspaces replaces the decoration root, and no per-row, hover, selection, or scroll path starts Git.

## Web Workspace

The web shell's Workspace screen follows the approved S6 proposal (candidate A), and its View areas follow the approved drag-review boards of PRD S7 (`agents/prd/workspace-views-layout/prd.md`); the native shell is unchanged until its own stage.
Its toolbar reads left to right: the path back (`Main / Project / Workspace`, with the device named when it is not this Mac), the layout switch, then the two tool toggles.
The layout switch is three adjacent icons in one segmented group, Agents only, Agents and Views, and Views only, with the chosen one on `{colors.elevated}` and the others muted; the same three names appear in its menu and as palette commands, and each icon's tooltip is its accessible name.
Its library master is `Component / Layout switch` in `design/hide-ui.lib.pen`.
Explorer and History are toggles that open and close independently, drawn pressed while shown; with both shown they share the tool column, Explorer above History, and each has its own close.
A right-click or the menu key on the toolbar offers the layouts, each tool, Copy Workspace path and Open Project Overview.

Agents and Views sit side by side, each region with its own tabs, and a boundary between them drags with a guide line and lands once on release.
Neither region narrows below `--size-workspace-area-min` (`HideTheme.Layout.workspaceAreaMinWidth`) at the supported 1024-wide window: the tool column gives way first, down to `--size-panel-min`, and a narrower window follows the rules under Narrow windows.
An empty Agent area offers New tab.
Changing the layout only changes space: it closes no tab, document or pane, and choosing Views only never makes a split; a split comes only from a split command or a drop on an edge.

An Agent tab carries its provider's mark (Claude, Codex) or a neutral terminal mark for any other kind, and never replaces Herdr's tab name; its tooltip and accessible name carry the kind, the full name and the agent's state.
A View tab carries the file-type mark, and a diff tab the comparison mark, so a kind is never told by color alone.
Its title is one line cut at the tail within `--size-tab-preferred`, and its tooltip and accessible name carry the kind, the full path, and `Preview` or `Unavailable` while the view is one, so a long Korean or English path stays readable.
A dirty view shows `●` in `--color-warning` after its title.
Closing a view is always called Close view, distinct from moving a file to the Trash and from closing a pane or tab.

### View areas

The Views region holds one or more View areas, each with its own tab bar above its own view, split left and right or up and down as often as the limits allow.
A split divides one area in two along one axis, and either half can split again along either axis, so every arrangement of side-by-side and stacked areas is a tree of halves.
One area is active: the next file opens there, the palette and the keyboard act on it, and its active view's tab alone carries the accent indicator (`--size-tab-indicator` in `{colors.accent}`).
Every other area still shows its own active view's tab on `{colors.background}` with a primary title and no indicator, so the operator sees what each area holds and which one is in charge.
An area whose tabs outrun its width scrolls its own strip so the shown view's tab stays in sight whenever the shown view changes or the area is resized.
Clicking a tab or a view, or moving focus to another area from the palette, makes that area active.
Between two areas is a divider `--size-resize-handle` wide in `{colors.divider}`, turning `{colors.accent}` on hover and keyboard focus; it drags with a guide line and lands once on release, as the boundary between Agents and Views does, so a drag never resizes a document or a terminal on every pointer move.
A focused divider moves with the arrow keys along its axis, one step and one change per press; the size of the step is the implementation's.
No area narrows below `--size-workspace-area-min` or gets shorter than `--size-view-area-min-height` (144), a divider stops where either neighbour would, and each side of a split keeps between 15 and 85 percent of it.
An area whose last view leaves disappears, and its neighbour takes the space.
When the last view of the whole Views region closes, one empty area stays and says that no file or diff is open, with Show Explorer when the Explorer is hidden and Open file (`⌘P`), and the layout does not change on its own.

A Workspace holds at most 6 View areas, no area sits more than 3 splits deep, and at most 64 views are open at once.
A split past the area or depth limit is refused with its reason, an open past 64 says `This Workspace has 64 views open. Close a view to open another.`, and in both cases every open view stays as it was.
Opening a file that is already shown moves to its view, so it is never refused.
The limits and the minimum sizes are fixed; they bound what one Workspace can ask of the page and of the core.

### Opening, preview and Open to the side

A single click on an Explorer file or a History row opens it in the active area's preview view, whose title is italic, and the next single click replaces that preview in place, so browsing leaves one tab per area rather than a trail.
Each area has at most one preview, and a click never touches another area or a pinned view.
A double-click on the row or the tab, Keep open, or the first edit pins the preview where it is.
Because the first edit pins, a document that is dirty, saving, or whose save failed is never a preview, in any area that shows it.
Opening a file that is already shown moves to its view instead of adding a tab, and when several views show it the one used last is chosen.
Opening a file from Agents only switches to Agents and Views and focuses the active View area.
Diffs are placed by the same rules.

Open to the side, from the Explorer's file menu, a History row's menu or the palette, is the only way to show one file twice.
It puts a pinned second view in the area beside the active one, trying right, then left, then below, then above, or in a new area on the right when there is only one area; when that area already shows the file, its view is focused instead, and with no view open at all it opens in the empty area.
From the only area, Open to the side is a split, so it is offered only where Split right would be and otherwise stays listed, disabled, with its reason, in the Explorer's menu, History's menu and the palette alike.
Both views show one document: an edit in either appears in the other at once, and each keeps its own scroll position, cursor and selection.
Korean input composes in either view, and neither view's composition or echo breaks the other's.
Closing one of them leaves the document, its text and its unsaved state in the other; closing the last one goes through the save and conflict protection every document close has, and a save whose outcome is unknown is never shown as saved.
Dragging a tab moves its view and never copies it.

### Dragging a view

A View tab dragged past `--size-tab-drag-activation` lifts a floating copy on `{colors.elevated}` with a hairline `{colors.divider}` border that follows the pointer, while the tab keeps its place and nothing on screen resizes.
Over a tab bar, a thin insertion line (`--size-tab-indicator` in `{colors.accent}`) marks where the tab will land: in its own bar the drop reorders, and in another area's bar it moves the view there with no split and no copy; when that area already shows the same document, the moved view lands at the line and that area's view of the document gives way, so no area holds one document twice.
Over the left, right, top or bottom edge of an area's content, the half of that area the drop would create is washed at `--opacity-selected-fill` inside a hairline `{colors.accent}` boundary, with one short label such as `Split right`, and the drop creates that area and moves the view into it.
Only one destination is highlighted at a time, and moving to another edge or bar replaces it.
Where the view cannot go, because it is the only view of the area whose edge it is over, the area cannot be halved at its minimum size, the Workspace is at its area or depth limit, or the target is not a View area, no overlay appears and the pointer shows the forbidden cursor.
Escape, a release outside a valid target or outside the window, and a target that disappeared or became ineligible before the release keep the original order and layout; only a valid drop changes the layout, once.
Nothing is resized, reattached or saved while a drag is in progress, and the drag itself is never stored.
The half-area preview, the one-winner rule and the eligibility rules are fixed; how close to an edge the pointer has to be is the implementation's.
A drag never moves a view to another Workspace, never turns an Agent tab into a view, and never splits a terminal.

### The View tab menu

A right-click on a View tab, or the menu key while the tab has focus, opens its menu with these items in this order: Keep open, Split right, Split left, Split up, Split down, Move right, Move left, Move up, Move down, Copy path, Reveal in Explorer, Close view.
Keep open appears only on a preview view.
A Move item appears only toward an area that exists in that direction, and moves the view into it without a split.
A Split item the Workspace cannot make stays listed, disabled, with its reason under it, the way every web menu draws an item its target cannot use.
The labels say where the view goes, never Move to Group.
Close view closes the view and never the file on disk; the menu has no file deletion and never closes a pane or a tab.
Opening the menu moves no focus and changes nothing, and Escape closes it.
Every item is also a palette command for the active view, and the palette adds Open to the side, focus to the next or previous area, and resizing the active area, so every split, move, close and resize can be done from the keyboard.
A palette command that cannot run now is drawn muted with its whole reason under its title, and picking it does nothing.
No new shortcut is assigned: the existing tab close and move chords act on the active view.
The item labels above are fixed; the reason wording is the implementation's.

### View states

A view whose file is being read shows `Opening…`.
A restored view whose device or root is not ready says what it waits for, such as `Waiting for <device> to connect`, and reads its file by itself once that is ready.
A view whose file cannot be read shows why, with Close view and Retry, and its tab title is struck through in `{colors.muted}`.
Each state belongs to its view alone, so one missing file never blanks another view or area.
After a restart the app reopens the last Workspace it was on as it was left: its areas and their sizes, each area's tabs in order with its preview, pinned views and active view, the active area, the layout and the tools; unsaved text returns from the browser's drafts, and Herdr's current tabs and panes are used as they are.
A first run, or a last Workspace that no longer exists, starts on Main.
A layout file that cannot be read is kept aside and the app starts on Main, where the operator picks a Workspace and continues with a new layout.

### Narrow windows

When the window cannot give the working regions their minimum beside the tool column at `--size-panel-min`, Explorer and History open as a temporary overlay above the working regions instead of a column; Escape or a click outside closes it and returns focus to the toggle that opened it.
When Agents and Views cannot both have their minimum width side by side, the region used last fills the width and an explicit switch leads to the other.
When the View areas cannot all have their minimum, only the active area shows, with an area switcher to the others.
Widening the window brings back the chosen layout, the area sizes and the tool column, because none of these narrow arrangements is stored or sent to the core.
The breakpoints follow from the minimum-size tokens; the switch and switcher wording is the implementation's.

### Library masters

The View area masters in `design/hide-ui.lib.pen` are `Component / View tab`, `Component / View insertion line`, `Component / View split overlay`, `Component / View tab menu` and `Component / View area message`.
Their sheets carry the states as refs: the View tab sheet draws preview, pinned, hover, active in the active area, active in another area, dirty, unavailable, diff, a long Korean title and the floating drag copy; the View placement sheet draws a reorder, a move into another area, a right and a down split, and an ineligible target; the View tab menu sheet draws a preview's menu and a pinned view's menu with a disabled Split and its reason; the View area states sheet draws the empty Views and each view's opening, waiting and unavailable states.
The canvas draws the unavailable strike as a hairline over the sample title and the minimum area height as its literal 144; the product strikes the title through and reads `--size-view-area-min-height`.

### Agent panes and the Agents explorer

Several View areas leave the Agent side as S6 drew it: the layout switch, the independent tool toggles, and the child chips below behave the same with one area or six.

A pane whose agent delegated work shows every direct child on one row under its header, each chip a status mark, the provider mark and a title capped at `--size-pane-child-chip-max`; the row scrolls sideways instead of growing, and a pane with no children has no row.
Its library masters are `Component / Pane child chip` and `Component / Pane child row`; the native pane header keeps its first child and `+N` until the native shell changes.
A chip opens the existing child at once; while that move is in flight the chip shows `…` and repeats of it are ignored, and a failure shows the core's reason under the header with Retry, when the core says it can be retried, and Dismiss.
A child pane has a compact Return mark in its identity row, named with the parent in its tooltip and accessible name.
The pane menu, from its `⋯` or a right-click on the header, lists the parent, the other siblings and the children as explicit Open items, then Copy pane name and Close pane; opening it moves no focus and marks nothing read.

The Agents explorer groups every current agent, this machine's and each connected device's, under Needs You, Done, Working and Seen and leaves an empty group out; a device's row names its device before the agent kind, and a device that is not connected lists nothing it only last reported.
A delegated row is indented and muted, and a row with live descendants carries a `↳N` badge whose tooltip counts them by state.

## Terminal image attachment boundary

Dropping local file URLs into a visible terminal focuses that receiving pane and starts one attachment intent through the existing ordered writer.
Command-V and Control-V capture PNG or TIFF clipboard images at the same ingress; ordinary text and keys keep their existing terminal behavior.
The core reserves the original pane and connection generation before asynchronous image preparation, file validation or transfer, so later focus changes cannot redirect the attachment.
The shell normalizes clipboard images into private PNG files off the input thread, while the core owns transfer state, held input, retry and cancellation.
Local files keep their original paths after validation; remote attachments are uploaded through authenticated SFTP and insert only the resulting remote paths.
When the terminal advertises bracketed paste, each path has its own paste frame; the complete drop is enqueued once in file order.
Otherwise the paths are inserted as shell-quoted text with a trailing space.
Spaces, Korean and apostrophes survive; shell expansion characters are escaped, and paths containing control characters, symlinks, directories and special files are refused with an actionable notice.
Nothing performs automatic Enter or replaces existing prompt text.
One transfer is admitted at a time, with at most eight files, 20 MiB per file and 40 MiB total; clipboard decoding is also capped at 16 megapixels.
The original terminal's later input is held up to 64 KiB, then explicitly refused rather than growing the queue or silently submitting an incomplete prompt.
Only a successful transfer to the original live generation releases the attachment followed by that held input.
A compact notice above the affected terminal shows preparation, upload or failure and offers Retry where safe, or explicit cancellation that discards held input.
An original pane that closes or reconnects invalidates the intent; neither files nor old input are forwarded into a replacement session.
Private staging uses owned 0700 directories and 0600 files with a 128-file and 256 MiB cap.
Files older than 24 hours are pruned on the next attachment attempt, not by a resident background cleaner; successful local clipboard references therefore remain readable after the paste.
Failed or cancelled transfers clean up only their own created files where the destination remains reachable, with deferred cleanup recorded diagnostically.
It inserts paths even when a provider cannot decode the referenced file; provider validation remains visible in its own composer.
Hide provides no thumbnail shelf, attachment membership or synchronized removal; subsequent editing and submission remain native provider operations.
Provider-native attachment behavior remains owned by the provider; pasting a path is not proof of image acceptance.
The former shelf was removed because it could not synchronize native attachment deletion or clear confirmed submissions reliably.
Reintroducing this surface requires a supported provider contract for stable attachment identity, idempotent add/remove, native draft changes and accepted submission events.
Both surfaces must reflect the same attachment membership, and the shelf must clear only after confirmed submission, preserving items on failure.
PTY writes, key events and terminal viewport state cannot substitute for that contract.

## Project Home

Project Home uses the shared tab choice group, badges, agent identity marks, settings field, icon buttons and command tooltip.
Tasks is the session default, with ad hoc requests above four Git-derived columns; Agents reuses each card in three canonical lifecycle columns.
Needs You uses the warning halo and an error uses danger, without moving the card out of its Git column.
Only the current delivery fact and linked issue appear in the footer.
Merged and Seen columns start collapsed.
The board scrolls horizontally below its column minimum and titles wrap to two lines; branch labels truncate at the tail with full shared tooltip and accessibility help.
`HideTheme.Home` owns the 288-point column, 148-point collapsed column, 6-point halo and 12-point child indent, mapped by `scripts/pen-token-map.json` into the library.
