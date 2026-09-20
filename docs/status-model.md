# Status Model

How Herdr's raw agent state becomes a group in the sidebar, a pet pose, and a badge row.

The state of an agent is five axes, not one word.
What it needs from the operator, whether it is running, whether it has reported a completion, whether the operator has looked at it, and whose work it is are independent, and mixing them into one string is what made the same agent read differently in different views.

- Demand: question, approval, error, none.
- Activity: working, stopped, unknown.
- Completion: reported, not reported.
- Read: read, unread.
- Ownership: operator, delegated, escalated.

`herdr-core/src/sidebar.rs` is the single owner of all five.
It also derives everything a view draws from them - the group, the mark, whether the row is emphasized, the status word, and whether closing the pane needs a confirmation or a fresh status check - so no surface decides any of it a second time.

Ownership is not stored anywhere.
It is read back off the row: a row whose lineage depth is greater than zero is delegated, and a row whose stall level is `hard` is escalated regardless of depth.
`ownership_of` is the only function that makes that judgement, and `rederive_ownership` is what reapplies every derived value after the lineage or a stall clock moves.

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
| Needs You | An unread demand - question, approval or error - a pane Herdr reports as blocked right now, or a lineage root whose descendant has been stalled past the hard threshold |
| Done | No demand, stopped, completion reported, and unread |
| Working | Running |
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

Ownership, the tree, the breadcrumb and the stall clock all start from one fact per agent: the pane it was spawned from.
Herdr records no lineage, so that fact has one source, read once in `wire.rs::lineage_parent` and nowhere else, so every row and every view sees the same parent: the pane token `parent_pane`, whose value is the parent's pane id, written by whoever created the pane through `pane.report_metadata`.

Hide's own fork writes it under the source `hide` after `pane.split` and `agent.start`.
sasu's dispatch writes it under its own source after `agent.start`, the only start that can carry its role marker; any orchestrator can write the same token by hand, and a child whose creator wrote nothing is a root.

An empty token is a cleared declaration, not a parent named by an empty string.
The token is display-only in Herdr's own terms and dies with the pane, so a closed child leaves no edge behind, and a parent that has gone makes the child an orphan root.

Regression owner: `a_parent_declared_as_a_pane_token_is_the_lineage`.

## The stall clock

Delegation is only safe if work that stops being anybody's problem comes back.
Each delegated row that is waiting on somebody, or running with nothing to show for it, carries a clock.
A finished child is not stuck, a released pane has no session to be stuck in, an unknown activity gives nothing to measure, and a remote pane is uninstrumented by decision; none of those are timed.

The clock measures one uninterrupted wait, so any move in the agent's own state - Herdr's sequence, its demand, or its activity - starts it over.
While the server is away every clock holds its reading rather than counting, because that gap is Hide's blindness and not the agent being stuck.

At five minutes the lineage root carries a notice naming the descendant and what it is waiting on, and its group does not change.
At fifteen the child stops being drawn as somebody else's work and the root enters Needs You.
The notice lands on the root rather than climbing one level per threshold: at depth three, one level at a time would keep the operator waiting forty-five minutes for news of something stuck for fifteen.
When two descendants have waited exactly as long, the notice names the one asking for the most.

A stalled session is by definition one that reports nothing new, so nothing new arrives to trigger a publish.
The `agent.list` tick that already runs once a second asks `stall_publish_due` instead, which is a pure in-memory comparison; no timer of Hide's own exists for this.

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

| Agent condition | Mark | Color | Text and behavior |
| --- | --- | --- | --- |
| Question | `?` | Yellow | Question; Needs You while unread or blocked |
| Approval | `!` | Yellow | Approval; a blocked pane stays Needs You even after being read |
| Error | `×` | Red | Error; Needs You while unread or blocked |
| No demand, stopped, completion reported, unread | `✓` | Green | Done; completion awaiting the operator's review |
| No demand, stopped, no completion | `○` | Gray | Idle; a newly opened agent is ready for its first instruction |
| No demand, working | `●` | Blue | Working |
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
Raised Needs You and Done rows remain available, while number shortcuts skip hidden tree rows.

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
| Working | `progress` |
| Seen (read demands, read completions, idle, unknown) | none |

`status_word_visible` is true only when the group wanted a sentence and the tokens carried none: the status word stands in for it, so an emphasized or working row never has an empty second line and a plugin that is absent or has not labelled the pane yet reads as before, title and status word.
Beside a sentence the word is never drawn; the mark and the group heading already say it.
A delegated row follows the same table for its own group, which for a child is Working or Seen, so a delegated child that has stopped shows only its title.
The pane header is one line: `title · sentence`, or `title · word` for a row with no sentence, with the sentence dropped first and the word second when the header is narrow, and a shell operation string (`forking…`, `reopening…`) taking the sentence's slot while it runs.
The accessibility label of a row and of a header always carries the status word, in the order title, agent kind, status word, sentence, so a row whose word left the screen is still read out with it.

### Search and Recent Panels

The ⌘K sheet's agent row is titled by the identity and subtitled by the second line above; when the state chose no sentence it falls to the status word because the rolling `task` is already the title.
The pane id is no longer printed on the row but still matches the query and is read by accessibility.
A tab holding exactly one agent pane carries that agent's identity and mark into its Recent Panels row (`StripTabSnapshot.agent_identity`), derived in the core on every status, lineage, or strip rebuild pass; a tab with none or several keeps its Herdr label.
Neither the core nor the plugin renames the Herdr tab for this; the Recent Panels label is projection only.
