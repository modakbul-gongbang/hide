# Status Model

How Herdr's raw agent state becomes a group in the sidebar, a pet pose, and a badge row.

The state of an agent is three axes, not one word.
What it needs from the operator, whether it is running, and whether the operator has looked at it are independent, and mixing them into one string is what made the same agent read differently in different views.

- Demand: question, approval, error, none.
- Activity: working, stopped, unknown.
- Read: read, unread.

`herdr-core/src/sidebar.rs` is the single owner of all three.
It also derives everything a view draws from them - the group, the mark, whether the row is emphasized, the status word, and whether closing the pane needs a confirmation - so no surface decides any of it a second time.

## Hide owns the read axis, at pane level

Herdr's seen is tab-scoped.
Its own documentation is explicit: focusing a tab, or targeting it with pane focus or agent focus, marks every pane in that tab seen.
Three finished agents side by side in one tab therefore cleared together on a single click, which is the bug this model exists to fix.

So Hide keeps its own record instead.
A pane is read when it has held Hide's keyboard focus since its last state change, and a state change is Herdr's `state_change_seq` rising **or** the derived demand and activity pair changing.
The pair matters because Herdr's sequence does not always rise when only plugin tokens change: with a pane's lifecycle held at `idle`, clearing its idle token so only a question token remained left the sequence where it was.

The record is `pane_read_records` in the persisted UI state, keyed by pane id, so it survives a restart.
A record whose pane the server stops reporting is dropped on the same pass, scoped to the namespace that pass owns, so a local sync never drops a remote pane's record.
A corrupt store loads as an empty record, which reads as everything unread, and says so in a diagnostic; it is never silently treated as read.

Nothing reads Herdr's `done` versus `idle` split, or a token's `_new` suffix, to decide the read axis.
This is enforced by `INV-herdr-unseen-token`.

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
| Done | No demand, stopped, and unread |
| Working | Running |
| Seen | Everything else: read demands, read completions, unknown |

A blocked pane stays in Needs You whether or not it has been read.
The approval prompt is still on screen waiting, so it leaves the group when the prompt is answered, not when it is looked at.

Done is deliberately separate from Needs You: finished-unseen is "look when you have a moment", an unread demand is "act now".

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
| No demand, stopped, unread | `✓` | Green | Done; completion awaiting the operator's review |
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

## Ambient signals (subagents, background tasks)

A record may carry optional `ambient` counts: `subagents_active`, `background_running`, and `background_failed`.
`sidebar.rs::parse_ambient` treats absent or null data as no signal and missing keys as zero; present counts must be nonnegative integers fitting `u32`.
Unknown keys are discarded, so task names, prompts, commands, output, and paths never enter this count projection.
A malformed ambient object excludes that agent with a diagnostic while other valid records continue to project.
`pet.rs::ambient_totals` sums with saturation and returns no counts while disconnected; `PetBadgeRow` renders positive counts only.
This client does not own upstream transcript scanning, authorization, or server restart policy.

Regression owners are `ambient_counts_parse_and_unknown_keys_never_survive`, `a_malformed_ambient_record_excludes_only_that_agent`, and `ambient_counts_sum_across_panes_and_go_quiet_while_disconnected`.

## GitHub status in the Workspace row

The PR icon is independent of agent status and Workspace disclosure.
It appears for a known pull request, an in-progress lookup, or a GitHub lookup failure; a successful lookup with no matching PR leaves it absent.
Clicking opens details without selecting a pane, marking agents read, or folding the Workspace.
The popover shows the PR number, title, Open/Draft/Merged/Closed state, head and base branches, and CI rollup.
Its refresh action reloads one repository; its external action opens the PR URL through the existing external browser route.

The first appearance of a local Git project requests its GitHub status once for the runtime session.
Repeated appearances are no-ops; explicit refresh advances that repository's generation.
The existing Git section trigger, selected Overview project and sidebar requests share `GithubReader`, its per-project cache, 15-second subprocess timeout, one active worker and coalesced pending requests.
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
