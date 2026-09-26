# Status Model

How Herdr's raw agent state becomes a group in the sidebar, a pet pose, and a badge row.

The state of an agent is five axes, not one word.
What it needs from the operator, whether it is running, whether it has reported a completion, whether the operator has looked at it, and whose work it is are independent, and mixing them into one string is what made the same agent read differently in different views.

- Demand: question, approval, error, none.
- Activity: working, stopped, unknown.
- Completion: reported, not reported.
- Read: read, unread.
- Ownership: operator, delegated.

`herdr-core/src/sidebar.rs` is the single owner of all five.
It also derives everything a view draws from them - the group, the mark, whether the row is emphasized, the status word, the descendant badge, and whether closing the pane needs a confirmation or a fresh status check - so no surface decides any of it a second time.

Ownership is not stored anywhere.
It is read back off the row: a row whose lineage depth is greater than zero is delegated, and every other row, including an orphan whose parent is gone, is the operator's.
`ownership_of` is the only function that makes that judgement, and `apply_lineage` reapplies every derived value once the lineage is known.

## Hide owns the read axis, at pane level

Herdr's seen is tab-scoped.
Its own documentation is explicit: focusing a tab, or targeting it with pane focus or agent focus, marks every pane in that tab seen.
Three finished agents side by side in one tab therefore cleared together on a single click, which is the bug this model exists to fix.

So Hide keeps its own record instead.
A pane is read when it has held Hide's keyboard focus since its last state change, and within one answering Herdr connection a state change is Herdr's `state_change_seq` moving **or** the derived demand and activity pair changing.
The pair matters because Herdr's sequence does not always rise when only plugin tokens change: with a pane's lifecycle held at `idle`, clearing its idle token so only a question token remained left the sequence where it was.
`state_change_seq` is process-local, so the first projection after a connection bootstrap reconciles a saved record only when the sequence moved backwards and the agent session id, demand and activity still match before adopting the new sequence.
A sequence that moved forward is new work completed while Hide was disconnected and remains unread.
A different known agent session or a different demand or activity also remains unread; a matching restored agent remains read when a restarted server reset the sequence.
An older saved record with no session identity is migrated on that same backwards-sequence and matching-state proof, then persists the detected identity for later restarts.
The reconciliation stays pending for a saved pane until agent detection catches up, because restored pane topology can arrive before the restored agent list.

The record is `pane_read_records` in the persisted UI state, keyed by pane id, so it survives a restart.
A record is dropped only when the authoritative pane layout stops reporting that pane, scoped to the namespace that pass owns, so a temporarily incomplete agent list and a local sync can never drop a restored or remote pane's record.
A corrupt store loads as an empty record, which reads as everything unread, and says so in a diagnostic; it is never silently treated as read.

Nothing reads Herdr's `done` versus `idle` split, or a token's `_new` suffix, to decide the read axis.
This is enforced by `INV-herdr-unseen-token`.

## Completion is separate from stopped

An agent can be stopped because it has completed a turn or because a newly opened pane is ready for its first instruction.
Only `agent_status: done` or a `status_done` token reports a completion.
An idle lifecycle or `status_idle` token reports a ready stopped pane and does not put it in Done, even though a missing read record still makes its read axis unread.
The completion fact is part of the pane read fingerprint, so a ready-to-completed transition becomes unread even when Herdr's process-local sequence does not move.
Herdr's done/idle distinction supplies completion evidence only; Hide's own pane record remains the sole read authority.

## A descendant's change turns its ancestors unread

The fingerprint carries one more element: the outstanding demands and reported completions of the row's live descendants, each keyed by the descendant's pane (`PaneReadRecord::descendant_signals`).
A descendant entering a question, approval or error, moving between those three, or finishing a turn adds a signal the record does not hold, and every ancestor row becomes unread through the same comparison its own state change would use; there is no second store, timer or event.
A signal that goes away does not turn anything on: a question being answered, a finished child starting new work, or a child pane closing is trimmed from the record on the next projection, so the same descendant is news again the next time it asks or finishes.
Reading the ancestor records the signals it shows at that moment and nothing more.

A descendant's signal never puts an ancestor in Needs You: a child's question leaves a working root in Working.
That is the boundary: a child is asked for its status through its parent, and a parent that has heard from a child is drawn bright until the operator looks at it.
The one way a descendant moves its root between groups is the waiting state below, and it moves the root into Working, never into an attention group.

Regression owners: `a_descendants_demand_or_completion_turns_every_ancestor_unread_and_nothing_else_does`, `an_ancestors_group_comes_from_its_own_axes_and_a_child_never_makes_it_needs_you`.

## A quiet root waiting on its children

A lineage root that is quiet itself - no demand of its own, not blocked, stopped whether idle or done - while at least one live descendant is working or holds a question, approval or error is waiting on its children.
It has not finished: the work it started is still running under it, so it sits in Working, and it reaches Done (when its own completion is unread) or Seen only once it and every descendant are quiet.
A merely ready or finished descendant is quiet, and one whose activity Herdr reports as unknown does not make its root wait, because a waiting state the projection cannot vouch for is not drawn.
A descendant's question keeps the root waiting in Working, carries `?1` on its badge and turns it unread through the descendant signals above; it never moves the root to Needs You.

Mark precedence on a root is its own demand, then its own work, then waiting on children, then idle or done.
The flag is only ever set on a row with no demand of its own that is not working, so the precedence is the order of the checks in `agent_group_for`, not a second rule.
Only a lineage root waits: a delegated middle row keeps its own mark, because its group is already Working or Seen by delegation and its parent's badge already counts the grandchild.

`apply_lineage` decides it on the same pass that sums `descendant_counts`, and publishes it as the additive `waiting_on_descendants` flag beside `group: working`; the row's mark stays the hollow ring `○`, its status word is `Waiting`, and it is not emphasized.
No new group value reaches the wire, so a decoder that does not know the flag, such as the frozen Swift shell, draws an ordinary Working row.
The web row draws the ring in the working color from the flag, and the pet's Working badge and the Workspace representative count the row in Working because both read `group_of`, which reads the flag.

Regression owners: `a_quiet_root_waits_on_busy_descendants_in_working_until_every_one_is_quiet`, `a_root_waiting_on_its_children_counts_as_working_not_done`, the Swift `aRootWaitingOnItsChildrenDecodesAsAnOrdinaryWorkingRow`, and the web `agentRow.test.ts`.

## Herdr token contract: both forms mean the same demand

Herdr reports attention state as suffixed string tokens, not booleans:

- `status_question_new: "?"` - a question, in the form Herdr uses before it considers the tab seen.
- `status_question: "?"` - the same question after Herdr considers it acknowledged. Herdr also drops `agent_status` back to `idle` for it.

The same pair applies to approval and error tokens, and the legacy boolean form (`status_question: true`) is still accepted.

Both forms map to the same demand.
The suffix is Herdr's answer to a question Hide no longer asks it, so reading the suffix as unread would put the tab-scoped verdict back in charge of a pane-level decision.

## The four groups

| Group | Membership |
| --- | --- |
| Needs You | An unread demand - question, approval or error - or a pane Herdr reports as blocked right now |
| Done | No demand, stopped, completion reported, and unread |
| Working | Running, or a quiet root waiting on a busy descendant |
| Seen | Everything else: ready idle, read demands, read completions, unknown |

A blocked pane stays in Needs You whether or not it has been read.
The approval prompt is still on screen waiting, so it leaves the group when the prompt is answered, not when it is looked at.

Done is deliberately separate from Needs You: finished-unseen is "look when you have a moment", an unread demand is "act now".

## Close protection is separate from read state

`requires_close_confirmation` and `requires_close_status_check` are core-derived values carried by both the sidebar agent row and its pane projection.
The confirmation value is true for Working activity, an unresolved demand, or a blocked pane; a stopped unread completion does not create a work-interruption prompt.
The status-check value is true only for Unknown activity with no demand and no block.
It prevents a destructive local or remote close from guessing that an unobserved agent is idle, and the caller-visible remedy is to refresh status before closing.
The shell presents that remedy as the existing read-only `Check status` action and keeps the destructive close confirmation separate from it.
An unknown row still belongs to Seen for sidebar grouping, so close safety never changes the read or ownership axes.

Needs You and Done are the operator's own groups, so only the operator's own rows enter them.
A delegated row can be Working or Seen and nothing else: its question, approval, error or completion is its parent's problem, and answering it is what delegation means.
The row keeps its own demand, mark and status word, so the parent's badge can still say what its child is asking for; what changes is only which group the row sits in and whether it is drawn bright.
Done is therefore scoped to the lineage root: a delegated child that finishes leaves a dimmed Seen row, and the completion the operator acts on is the root's.

## Where a parent comes from

Ownership, the tree, the breadcrumb and the descendant badge all start from one fact per agent: the pane it was spawned from.
Herdr records no lineage, so that fact has one source, read once in `wire.rs::lineage_parent` and nowhere else, so every row and every view sees the same parent: the pane token `parent_pane`, whose value is the parent's pane id, written by whoever created the pane through `pane.report_metadata`.

Hide's own fork writes it under the source `hide` after `pane.split` and `agent.start`.
sasu's dispatch writes it under its own source after `agent.start`, the only start that can carry its role marker; any orchestrator can write the same token by hand, and a child whose creator wrote nothing is a root.

An empty token is a cleared declaration, not a parent named by an empty string.
The token is display-only in Herdr's own terms and dies with the pane, so a closed child leaves no edge behind, and a parent that has gone makes the child an orphan root.

Regression owner: `a_parent_declared_as_a_pane_token_is_the_lineage`.

## The descendant badge

A row with descendants reports them on its first line, before the elapsed time: one mark and count per state, error, then approval, question, working and done, with zero states left out and the marks the rows themselves use.
The counts are `descendant_counts` on the row, derived on the lineage pass over every live descendant rather than the direct children only, so a grandchild's question reaches the root as `?1`.
A descendant that is merely ready adds nothing, and one whose activity Herdr reports as unknown is left off the badge and written to the diagnostic log (`lineage.unknown_descendants`), because a count the projection cannot vouch for is not drawn.
A closed pane leaves the list and therefore the badge on the next projection.

The badge is drawn while the row's descendants are folded away and leaves when they are opened, since the opened rows carry their own marks; a raised row in Needs You or Done never unfolds and always wears it.
Descendants are folded by default: `expanded_agent_pane_ids` in the persisted UI state names the panes the operator opened, it lives as long as the pane id does, and an older store's collapsed set is ignored rather than migrated, so the first launch after the change starts every parent folded.
The web shell folds a parent by this one set wherever it draws the parent, in Agents and under its checkout in Projects, and counts the badge over every live descendant of the parent's device, so a descendant in another checkout is counted on the badge and still drawn as a root in its own checkout.

Regression owners: `the_descendant_badge_sums_every_live_descendant_and_skips_ready_and_unknown_ones`, `lineage_expansion_persists_without_attention_opening_it_and_prunes_on_disappearance`, `the_snapshot_carries_no_stall_notice_and_ownership_is_operator_or_delegated`, and the Swift `DelegatedRowPresentationTests`.

Order within the whole list is one function, `sort_agents`: group order first, then most recent activity descending, then snapshot order.
The label plugin's `sort_rank` token is not read.
The Projects view raises Needs You and Done above the project tree.
Raised agents also remain in their checkout tree, so a Workspace summary always has agent rows to reveal and an attention transition never leaves a child without its parent.
Both appearances share one direct-select shortcut, assigned to the first visible occurrence.
Collapsing a parent hides descendants in the tree while raised attention rows remain reachable.
The Agents view draws all four groups with their boundaries visible and omits empty ones.

## Shared agent and Workspace status contract

The sidebar hierarchy is Project > Workspace > Agents.
A Workspace is one checkout path, including a plain folder; internal `WorkspaceSnapshot` names the project and `CheckoutSnapshot` names this Workspace.
A branch is the Workspace title when available; otherwise its real folder name is the title.
`primary`, `detached`, `missing`, temporary state, and uncommitted changes describe the checkout, not an agent lifecycle.
They remain separate badges or Git indicators and never select the agent status color.

### One meaning across surfaces

Agent rows, focused-agent tab marks, and Workspace summaries use the same status mark and semantic color mapping.
Working uses a fixed blue status token, independent of the user's accent color; Done uses green, so running and completed work remain distinct.
The symbol and accessible text accompany color, so color alone never carries the distinction.
Every surface draws the mark in one box of one size: `●` and `○` as a filled dot and a ring of the same diameter, because the two glyphs render at different sizes in every face, and every other mark as its glyph in that box (`web/src/components/status-mark.tsx`).

| Agent condition | Mark | Color | Text and behavior |
| --- | --- | --- | --- |
| Question | `?` | Yellow | Question; Needs You while unread or blocked |
| Approval | `!` | Yellow | Approval; a blocked pane stays Needs You even after being read |
| Error | `×` | Red | Error; Needs You while unread or blocked |
| No demand, stopped, completion reported, unread | `✓` | Green | Done; completion awaiting the operator's review |
| No demand, stopped, no completion | `○` | Gray | Idle; a newly opened agent is ready for its first instruction |
| No demand, working | `●` | Blue | Working |
| Root with no demand, stopped, a live descendant working or asking | `○` | Blue ring | Waiting; Working group until every descendant is quiet |
| No demand, stopped, read | `○` | Gray | Idle; a read completion is not another unread Done |
| No demand, unknown activity | `~` | Gray | Unknown; never silently labeled Idle |
| Owning server unavailable | `⊘` | Gray | Disconnected; current agent activity is unavailable |

Read questions, approvals, and errors retain their symbol and hue with reduced emphasis.
Reading is acknowledgment, not evidence that a demand was resolved.
A read, non-blocked demand belongs to Seen unless the core places its running activity in Working; the status mark still describes the demand.
Disconnected presentation overrides the retained mark and text on every affected surface without modifying demand, activity, read records, or the last known group.
Unavailable-server tooltips describe the connection problem rather than presenting retained counts as current work.

### Workspace aggregation

`sidebar.rs` owns Workspace aggregation from the canonical agent projection after pane-level read state is applied.
Each Workspace counts unique agent pane IDs physically owned by its tabs, independent of sidebar visibility, raised rows, parent collapse, or Workspace collapse.
The existing status synchronization indexes pane ownership once and visits each canonical agent once; it adds no timer, I/O, or per-frame work and publishes only changed summaries.
A descendant running in another checkout contributes to that checkout, even if its lineage row appears beneath a parent elsewhere.
An agent repeated in a raised section and the project tree counts once.
Plain terminal panes without agents do not create an Idle agent status.
A Workspace with no known agents draws no summary chip.
A populated Workspace places its representative status and provider icon in a trailing chip, followed by `+N` for the remaining agents (omitted for a single agent).
The chip uses the canonical physical agent count, even when agents are also raised above the tree.
An agent whose lineage starts in another checkout also appears as a local root in the Workspace that physically owns it, so that Workspace can reveal the agents its chip counts.
Rendering and shortcut numbering share this ownership-aware tree projection.

The representative follows the same group order as the sidebar: Needs You > Done > Working > Seen.
Within a group, Error precedes Approval, then Question; otherwise Unknown precedes ordinary Idle within Seen, so missing information is not hidden by an idle sibling.
Equivalent candidates keep canonical agent order.
A read error in Seen cannot outrank an unread question in Needs You.
The Workspace draws the representative agent's exact mark, color, and emphasis through the shared presentation.

The tooltip lists positive counts in group order: Needs You, Done, Working, Seen.
Unknown is reported as a subset of Seen, never added again to the total.
When the owning server disconnects, a Workspace with retained agents displays Disconnected and suppresses the stale activity breakdown.
Connection recovery resumes the current canonical projection; it does not mark agents read.

### Disclosure and selection

The disclosure chevron sits at the right edge of a Workspace row.
The left edge holds the checkout-kind icon, title, and checkout badges.
The representative status moves into the trailing agent summary chip immediately before the chevron.
A root agent status mark aligns with the Workspace branch icon; compact agent rows use a 4pt gap between status, provider icon, and title.
A Workspace with nested agent rows uses the entire row, including its name and empty space, as the disclosure hit area.
Its right-edge chevron is a non-interactive indicator within that same button, not a second small control.
A Workspace without nested agent rows shows no chevron and clicking its row opens the Workspace.
Select an agent to focus its pane; expanding or collapsing a populated Workspace only changes the tree.
Collapsing does not change the selected pane, tab, agent read state, running processes, or aggregated status.
`collapsed_checkout_ids` persists across launches.
The web shell starts every checkout closed instead and keeps the ones the operator opened in `expanded_checkout_ids`; a `ui_state_update` without that set, as the Swift shell sends, leaves it unchanged, and each shell ignores the other's set.
Raised Needs You and Done rows remain available, while number shortcuts skip hidden tree rows.
In the web shell the fold controls of a project, a checkout and a parent agent all sit on the right of their row in slots kept at rest; a folded control is always shown and an unfolded one appears under the pointer, with keyboard focus in the row, while the row's menu is open, or on an input with no hover.
The body of each row navigates (a project to its Overview, a checkout to its Workspace, an agent to its pane) and a fold never does: folding changes no screen, pane, tab, read state, group or process.

### Verification ownership

Core status tests own the Done mark, representative priority, unique-pane counts, cross-checkout ownership, and unchanged read semantics.
Core close tests own the separation between work confirmation and unknown-status blocking, including local and remote close refusal.
Swift presentation tests own fixed semantic colors and the shared disconnected override.
Native verification covers mixed states, Done-to-Idle acknowledgment, right-side disclosure, unchanged terminal selection on collapse, empty and missing workspaces, and disconnect/recovery.
Run evidence belongs under `agents/runs/`, never in `docs/`.

## Pet pose priority

An error or an unread demand takes precedence over ordinary work, so a `!` or `?` is never hidden by a background task.
The behavior layer adds the clawd-style priority used for pose selection (`herdr_core::pet::expanded_state`):

```
error > notification > sweeping > attention > carrying/juggling > working > thinking > idle/roam > sleeping
```

The current Herdr data has no separate sweeping or thinking token, so those slots remain reserved and fall through to the existing status.
One working pane maps to `carrying`, two or more to `juggling`.
After eight idle seconds the pet can roam; after sixty idle seconds it runs `yawning -> dozing -> collapsing -> sleeping`.
Any pointer activity produces `waking` before returning to the normal priority.
The pose ladder is the one place an error is counted apart from the rest of Needs You, because the ladder puts it a rung higher; no count on any surface is drawn from that split.

A lost Herdr connection outranks all of it: `herdr_core::pet::pose` reports `disconnected`, because a server that stopped answering cannot say anything true about agent state.
The last valid agent list is retained so counts do not blink to empty, but every retained agent is counted as disconnected - a stale yellow "act now" badge for a dead server is the failure this prevents.

## Badges

The pet's badge row counts three of the sidebar's groups, in the same order; Seen has no badge:

| Order | Color | Group |
| --- | --- | --- |
| 1 | yellow | Needs You |
| 2 | green | Done |
| 3 | blue | Working |

A count of zero hides that badge.

The pet's "act now" number is the whole Needs You count and its done number is the whole Done count, so a badge can never disagree with the section it stands for.
`herdr-core/src/pet.rs` counts the groups the projection already decided rather than reading tokens or axes a second time.
The pet dashboard's count tiles read the same four groups, plus the rows whose server stopped answering.

## The subagent badge

The badge row carries one more count after the three groups: the in-process subagents Hide's hook reports as working, in purple.
`pet.rs::subagents_active` sums the `working` hook token over the agents on an answering server, with saturation, and returns zero while disconnected; a pane whose agent has gone is not counted even if its token lingers, and an instrumented pane whose count is unknown adds nothing rather than a zero.
It is the one count the hook can vouch for; Herdr's own wire carries no ambient counts, and nothing here scans transcripts or output.

Regression owner: `subagent_counts_sum_the_hook_tokens_of_listed_agents_and_go_quiet_while_disconnected`.

## Uninstrumented is not an unknown activity

A pane whose agent Hide cannot see into is a different answer from a pane whose activity Herdr reports as `unknown`, and the two are drawn differently on purpose.

- Activity `unknown` is Herdr saying it does not know what the process is doing. It is one of the three activity values and it groups like any other.
- Uninstrumented is Hide saying it cannot tell what that session has spawned. It is not an activity, it never changes a group, and it is drawn as its own mark beside the agent.

The reason is resolved once, by `hide_agent_hooks::diagnosis::instrumentation`, in a fixed order: config unreadable, remote host, hooks not installed, session predates install, hook outdated, unknown.
The first match wins and nothing falls through to an empty value or an invented cause.
Every projection carries the reason's stable code alongside its sentence, so no surface has to recognise its own operator-facing text.

The mark appears in three places, and only on panes where an agent was detected: the pane header's 28pt identity row, the sidebar row, and the Overview worktree row's agent line.
That third position exists because an empty agent line has to distinguish "nobody is working here" from "Hide cannot see into this worktree".
A count Hide cannot read is reported as unknown and never as zero, because a zero is a claim that the agent is working alone.

## GitHub status in the Workspace row

The PR icon is independent of agent status and Workspace disclosure.
It appears for a known pull request, an in-progress lookup, or a GitHub lookup failure; a successful lookup with no matching PR leaves it absent.
Clicking opens details without selecting a pane, marking agents read, or folding the Workspace.
The popover shows the PR number, title, Open/Draft/Merged/Closed state, head and base branches, and CI rollup.
Its refresh action reloads one repository; its external action opens the PR URL through the existing external browser route.

The first appearance of a local Git project requests its GitHub status once for the runtime session.
Repeated appearances are no-ops; explicit refresh advances that repository's generation.
The selected Overview project and explicit sidebar requests share `GithubReader`, its per-project cache, 15-second subprocess timeout, one active worker and coalesced pending requests.
The existing [`gh pr list`](https://cli.github.com/manual/gh_pr_list) request additionally asks for `title` and `statusCheckRollup`; it retains the 200-PR limit and existing branch tie-breaking policy.
The same generation also reads open issues, their Project Status, and PR closing references for Project Home.
The issue list uses `sort:updated-desc` and reads one sentinel beyond the 200-issue display cap so overflow is based on evidence.
Issue references accept a GitHub issue URL, `owner/repo#N`, or `#N` when the repository is known; unsupported hosts and malformed references are rejected.
A failed component read retains that component's last successful answer, including a successfully empty answer, while a successful PR read still advances if issue reading fails.
Hide stores no new credentials and adds no polling timer or subprocess under the runtime mutex.
The project request event, result status and PR fields travel through revisioned `rest`; presentation reads that snapshot only.

| GitHub checks | UI |
| --- | --- |
| Every reported check succeeded, was neutral, or was skipped | Passing, green |
| Any failure, error, cancellation, timeout, or required action | Failing, red |
| At least one running, queued, waiting, pending, or requested check, with no failure | Running, yellow |
| An explicitly empty check list | No checks, gray |
| Absent check data, an unknown check kind, or an unrecognized terminal result | Unknown, gray |

Failure takes precedence over pending, then unknown, then passing.
A failed refresh preserves the last known PR and reports the lookup failure and last successful read time.
The visible stale notice qualifies both PR state and CI; old green checks are not presented as a fresh result.
Missing `gh`, logged-out authentication, network errors and rate limits use the reader's existing categorized diagnostics and user-facing reason.

The sidebar tree projection additionally indexes physical pane ownership when forming root rows and direct-select candidates.
This is linear in the visited project panes and agent projection per recomputation, with no queue or asynchronous work; repeated hover values do not trigger it.

### Pull request visual states

PR lifecycle uses the GitHub convention: Open is green, Merged purple, Closed red, and Draft gray.
The sidebar and popover share one color mapping and the matching Octicon; the State text and accessibility label preserve meaning without relying on color.
A merged or closed result takes precedence over an old draft flag.
Glyphs match the 14pt branch icon inside the existing 24pt trailing control, aligned with Workspace disclosure.
CI colors remain separate from PR lifecycle, so Merged does not imply Passing.

### Project Overview summary

Overview reuses this cached per-branch PR answer, including failure and stale status.
Its active-branch and draft counts describe the selected results, not every PR in the repository's history.
The details state the reader's lookup window and offer the existing PR URL and scoped refresh actions.
A failed or unrequested lookup never becomes a zero count.
The workspace inspector uses the canonical representative agent and disconnected override; inspection itself never acknowledges an agent.

## Task identity

The core publishes one `identity_label` per agent, and every surface calls the agent by it: the sidebar row, the pane header, the ⌘K search row, the ⌃Tab Recent Panels row, the lineage chips, and the Overview agent line.
`sidebar.rs` owns the ladder: the plugin's rolling `task`, then the workspace label.
The Herdr agent name remains the unique control identifier that Sasu and other orchestrators assign at start, so it never enters the display ladder.
The plugin publishes no session `name`, does not read Claude's `ai-title` or Codex's first human turn as a separate title, and never renames an agent or tab.
There is no `summary` token and no missing-summary notice: an agent without a task is titled by its workspace, and the row says nothing else.
Truncation belongs to each view and does not shorten tooltip or accessibility text.
Projection adds bounded-by-metadata strings per agent to the existing snapshot burst, with no extra event, timer, worker, or subprocess.

### The second line

The core chooses the row's second line from the group, and publishes it as `detail` with `status_word_visible`; the shell draws what it is given and decides nothing.
The sentences come from the plugin's tokens: `expected_reply` is the one action the operator is asked for, at most 40 characters; `progress` is what the agent is doing or has done.

| Group | Sentence |
| --- | --- |
| Needs You | `expected_reply`, else `progress` |
| Done (unread) | `expected_reply`, else `progress` |
| Working | `progress`; a row with an unresolved demand, `expected_reply`, else `progress` |
| Seen with an unresolved demand (a read demand, a delegated child's request) | `expected_reply`, else `progress` |
| Seen (read completions, idle, unknown) | none |

A request outlives reading: a question, approval or error keeps its sentence until it is resolved, whatever group the row sits in, so a view can keep showing what is being asked after the operator has looked.

`status_word_visible` is true only when the group wanted a sentence and the tokens carried none (a Seen row never shows the word): the status word stands in for it, so an emphasized or working row never has an empty second line and a plugin that is absent or has not labelled the pane yet reads as before, title and status word.
Beside a sentence the word is never drawn; the mark and the group heading already say it.
A delegated row follows the same table for its own group, which for a child is Working or Seen, so a delegated child that has stopped shows only its title unless it is still asking.

The core publishes the sentence; when a view shows it is that view's presentation.
The web sidebar row keeps a request line in the warning color until the request resolves, shows the sentence of an unread row as a bright line that goes once the row is read, reveals the full sentence up to two lines on the selected or hovered row with the rest in a tooltip, and otherwise draws one line (`web/src/agentRow.ts`, owned by `agentRow.test.ts`; docs/UI_BEHAVIOR.md).
The pane header is one line: `title · sentence`, or `title · word` for a row with no sentence, with the sentence dropped first and the word second when the header is narrow, and a shell operation string (`forking…`, `reopening…`) taking the sentence's slot while it runs.
The accessibility label of a row and of a header always carries the status word, in the order title, agent kind, status word, sentence, so a row whose word left the screen is still read out with it.

### Search and Recent Panels

The ⌘K sheet's agent row is titled by the identity and subtitled by the second line above; when the state chose no sentence it falls to the status word because the rolling `task` is already the title.
The pane id is no longer printed on the row but still matches the query and is read by accessibility.
A tab holding exactly one agent pane carries that agent's identity and mark into its Recent Panels row (`StripTabSnapshot.agent_identity`), derived in the core on every status, lineage, or strip rebuild pass; a tab with none or several keeps its Herdr label.
Neither the core nor the plugin renames the Herdr tab for this; the Recent Panels label is projection only.

### Project Home

Project Home is the empty local checkout surface and the Shift-Command-H overlay.
Its session-local choice defaults to Tasks; Agents groups the same card by the canonical root request's group.
Tasks derives delivery in priority order: merged worktree or merged PR, open PR, changed files or ahead commits, then ready.
Needs You changes the halo and stable sort priority, never this delivery stage.
Main and non-Git checkouts appear only in the ad hoc row when they have agents.
Completed columns start collapsed and disappear only with their underlying pane or worktree.

`runtime/issues.rs` resolves workspace manual overrides, pane-family issue tokens, branch configuration, PR closing references, then the two supported branch prefixes.
`wire.rs` extracts metadata and `git_dir.rs` reads the branch setting with the catalog off the runtime lock.
The existing generation-driven GitHub reader fetches PRs, repository identity and open issues together; a 201st sentinel proves overflow and `sort:updated-desc` determines backlog order.
Missing closed or cross-repository linked issues use one bounded read-only GraphQL query, not one process per card.
An enriched issue lookup may retry once without optional Project fields; basic issue facts remain usable and the Project failure stays in the diagnostic and GitHub availability path.
Linked issues take priority within the combined 200-identity project limit.
A failed generation preserves its last successful payload, including a successful empty payload.
The board keeps stale facts and puts their age in the shared issue tooltip; Overview owns the actionable GitHub availability explanation.

Manual issue writes reuse the purpose operation slot and token-first, Git-second writer.
Validation resolves an issue before writing, a failed Git mirror attempts to restore the previous token, and the runtime suppresses only an uncertain mutation until fresh metadata confirms replacement.
A validation failure or confirmed rollback preserves the existing manual override.
No GitHub mutation is allowed by this path.
The existing purpose mirror worker clears a branch issue setting when an observed worktree path is removed; a branch switch, detached HEAD, or unregistered project is not a removed worktree.
While a worktree stays detached, the worker retains its last known branch for cleanup if that path is later removed.
A rejected cleanup enqueue is retained for the next catalog synchronization.
