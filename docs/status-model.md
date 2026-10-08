# Status Model

How Herdr's raw agent state becomes a group in the sidebar, a pet pose, and a badge row.

The state of an agent is five axes, not one word.
What it needs from the operator, whether it is running, whether it has reported a completion, whether the operator has looked at it, and whose work it is are independent, and mixing them into one string is what made the same agent read differently in different views.

- Demand: question, approval, error, none.
- Activity: working, stopped, unknown.
- Completion: reported, not reported.
- Read: read, unread.
- Ownership: operator, delegated.

`herdr-core/src/agent_state/axes.rs` is the single owner of all five.
`agent_state/turn.rs` derives the group, request verb, close and rest gates from them; `sidebar.rs` assembles the snapshot rows.
It also derives everything a view draws from them - the group, the mark, whether the row is emphasized, the status word, the descendant badge, and whether closing the pane needs a confirmation or a fresh status check - so no surface decides any of it a second time.

Ownership is not stored anywhere.
It is read back off the row: a row whose lineage depth is greater than zero is delegated, and every other row, including an orphan whose parent is gone, is the operator's.
`ownership_of` is the only function that makes that judgement, and `apply_lineage` reapplies every derived value once the lineage is known.

## agent_status provenance and background waits

Herdr owns the raw `agent_status`; Hide reads it through `agent.list`/`agent.get`, converts it at `herdr-core/src/wire.rs` and derives its axes in `agent_state/axes.rs`.
For Claude Code and Codex, Herdr's [official agent reference](https://herdr.dev/docs/agents/) describes terminal-screen inference as the lifecycle source, rather than lifecycle state reported by their session hooks.
A native session reference or display-only hook token does not prove that a tool is running or that its command completed.
Other integrations can have full lifecycle reporting, so this screen-inference statement is specific to those two agents, not every agent Herdr supports.

Hide's current mapping calls raw `done` a reported completion and `idle` a ready stopped pane, as specified below.
That is a presentation signal supplied by Herdr, not a verified task outcome, successful test, exited background command or durable delivery receipt.
The current upstream documentation also distinguishes semantic state waits from arbitrary command completion; its display/attention descriptions must not replace Hide's pinned wire mapping without checking the bundled schema and adapter.
Use a command's own exit/result or an explicit coordination receipt when a workflow needs completion evidence, rather than treating a quiet pane or Done group as that evidence.

[Issue #348](https://github.com/modakbul-gongbang/hide/issues/348) records one actual run in which an agent waiting for a background command was read as `done`.
This is a known screen-inference limitation: a prompt-like quiet screen can cease to look working while background work is still outstanding.
It is one observed case, not a guarantee that every background wait produces `done` or that Hide detects outstanding commands independently.
The corresponding behavior for Monitor and for Codex's background terminal has not been verified by that observation.
The existing [waiting-on-descendants rule](#a-quiet-root-waiting-on-its-children) uses proven lineage and child axes; it is not a general background-command detector.

## Hide owns the read axis, at pane level

Herdr's seen is tab-scoped.
Its own documentation is explicit: focusing a tab, or targeting it with pane focus or agent focus, marks every pane in that tab seen.
Three finished agents side by side in one tab therefore cleared together on a single click, which is the bug this model exists to fix.

So Hide keeps its own record instead.
A pane is read when it has held Hide's keyboard focus since its last state change, and within one answering Herdr connection a state change is Herdr's `state_change_seq` moving **or** the derived demand and activity pair changing.
The pair matters because Herdr's sequence does not move when only the core's own label changes: an analysis that reads a question at the end of a turn arrives after the agent stopped, so the demand appears with the sequence where it was.
`state_change_seq` is process-local, so the first projection after a connection bootstrap reconciles a saved record only when the sequence moved backwards and the agent session id, demand and activity still match before adopting the new sequence.
A sequence that moved forward is new work completed while Hide was disconnected and remains unread.
A different known agent session or a different demand or activity also remains unread; a matching restored agent remains read when a restarted server reset the sequence.
An older saved record with no session identity is migrated on that same backwards-sequence and matching-state proof, then persists the detected identity for later restarts.
The reconciliation stays pending for a saved pane until agent detection catches up, because restored pane topology can arrive before the restored agent list.

The record is `pane_read_records` in the persisted UI state, keyed by pane id, so it survives a restart.
A record is dropped only when the authoritative pane layout stops reporting that pane, scoped to the namespace that pass owns, so a temporarily incomplete agent list and a local sync can never drop a restored or remote pane's record.
A corrupt store loads as an empty record, which reads as everything unread, and says so in a diagnostic; it is never silently treated as read.

Nothing reads Herdr's `done` versus `idle` split to decide the read axis.
This is enforced by `INV-herdr-unseen-token`.

## Completion is separate from stopped

An agent can be stopped because it has completed a turn or because a newly opened pane is ready for its first instruction.
Only `agent_status: done` reports a completion.
An `idle` lifecycle reports a ready stopped pane and does not put it in Done, even though a missing read record still makes its read axis unread.
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
Only a lineage root waits: an ordinary delegated middle row keeps its own mark and Working or Seen group, and its parent's badge already counts the grandchild.
An active escalation uses the six-cause exception below.

`apply_lineage` decides it on the same pass that sums `descendant_counts`, and publishes it as the additive `waiting_on_descendants` flag beside `group: working`; the row's mark stays the hollow ring `○`, its `status_code` is `waiting` (the word `Waiting`), and it is not emphasized.
No new group value reaches the wire, so a decoder that does not know the flag draws an ordinary Working row.
The web row draws the ring in the working color from the flag, and the pet's Working badge and the Workspace representative count the row in Working because both read `group_of`, which reads the flag.

Regression owners: `a_quiet_root_waits_on_busy_descendants_in_working_until_every_one_is_quiet`, `a_root_waiting_on_its_children_counts_as_working_not_done`, and the web `agentRow.test.ts`.

## Where each axis comes from

Activity and completion come only from Herdr's `agent_status`: `working` is Working, `idle` and `done` are stopped, `done` alone reports a completion, and any other value reads as unknown rather than idle.
No pane token is read for either.
Demand has these sources:

- A question is the core's label verdict on the agent's last message (`label.question`, see Task identity below).
  It exists only while the label is proven for the pane's current session, and it ends when the agent starts working again or a turn ran between two looks at a stopped agent, so a question never outlives the turn that asked it.
- A native unanswered Claude `AskUserQuestion` or Codex `request_user_input` is a question even with summaries off.
  The current-session/current-state read carries the optional `user_turn` row fact, with `kind: question|plan_approval` and optional `content: {text, choices, truncated}`.
  Text is capped at 8 KiB, choices at eight and each choice at 256 UTF-8 bytes; cuts preserve character boundaries and set `truncated`.
  A correlated native result, a later human turn or an abort clears the pending question.
  Missing content remains absent, and a failed, incomplete or older-state read publishes no structured fact; the independent letter hold stays unknown rather than assuming an answer.
  Native calls are bounded to eight per turn with 256-byte identities; exceeding the bound fails the read instead of dropping a pending question.
- Approval is Herdr's `blocked` lifecycle, whether or not the operator has read it.
- Approval is also a plan waiting for the operator's approval that Herdr reads as an ordinary stop, Codex's "Implement this plan?" (PRD codex-plan-approval-hold).
  The core reads it from the session file, not the screen: the label worker's session read folds the agent's turn records into a turn tracker (`hide-session/src/turns.rs`), and a plan-mode turn that proposed a plan and finished, with no later turn or person's message, waits.
  The answer holds only for the Herdr `state_change_seq` the read was asked under and the session it proved, it is laid on the row's facts (`RowFacts.awaiting_operator`, never on the wire) and resolved by `agent_state::agent_blocked`, and it is not shown while Herdr says the agent works.
  A wait no read has settled for the current state is not shown: the row shows what Herdr says, and the letter doorbell holds instead (`docs/delivery.md`, A menu Herdr reads as a stop).
  It needs no Hide AI: agent summaries off, the read still runs.
  "No, stay in Plan mode" writes nothing to the session, so after that answer the row stays in Needs You until Codex's next turn starts.
  The row's `blocked` value carries it, so it is held exactly like Herdr's `blocked` below, including close confirmation and agent sleep, and the Enter that approves it still counts as the operator's submit, since Codex writes the next turn's message from it.

Error is still a demand value on the wire and in the vocabulary, but nothing produces it since the hand-installed hook path was retired (PRD labels-in-hided D-06); a question outranks an approval.
Whether the operator has read a demand is Hide's own record, never Herdr's tab-scoped seen state.

## The four groups

| Group | Membership |
| --- | --- |
| Needs You | An unread demand - question, approval or error - or a pane Herdr reports as blocked, or whose native question or plan waits for the operator, right now |
| Done | No demand, stopped, completion reported, and unread |
| Working | Running, or a quiet root waiting on a busy descendant |
| Seen | Everything else: ready idle, read demands, read completions, unknown |

A blocked pane, or one whose native question or plan waits for the operator, stays in Needs You whether or not it has been read.
The prompt is still waiting, so it leaves the group when answered rather than when looked at.

Done is deliberately separate from Needs You: finished-unseen is "look when you have a moment", an unread demand is "act now".

## Close protection is separate from read state

`requires_close_confirmation` and `requires_close_status_check` are core-derived values carried by both the sidebar agent row and its pane projection.
The confirmation value is true for Working activity, an unresolved demand, or a blocked pane; a stopped unread completion does not create a work-interruption prompt.
The status-check value is true only for Unknown activity with no demand and no block.
It prevents a destructive local or remote close from guessing that an unobserved agent is idle, and the caller-visible remedy is to refresh status before closing.
The shell presents that remedy as the existing read-only `Check status` action and keeps the destructive close confirmation separate from it.
An unknown row still belongs to Seen for sidebar grouping, so close safety never changes the read or ownership axes.

A row with live descendants also carries `close_descendant_pane_ids`: every live descendant's pane in the order a subtree close takes them, deepest first and each pane after every pane below it, siblings in the lineage's own child order.
`apply_lineage` derives it on the same pass as the tree, and `refresh_agent_lineage` drops the panes of a device that is not connected, so a close never lists what it cannot reach; a row with none omits the field.
A pane or tab close asks about the subtree only when some agent that closes lists a pane outside what closes (PRD close-agent-subtree D-16), and the status check above applies to each listed descendant: an unknown one keeps `모두 닫기` disabled, while `이것만 닫기` stays available.
Closing a parent alone does not close its children: they lose their parent on the next projection and become the operator's roots with their own state.
Regression owners: `each_row_lists_its_live_descendants_deepest_first_and_a_leaf_lists_none`, `a_descendant_on_a_disconnected_device_is_never_listed_for_a_close`, and the tree-close tests in `herdr-core/src/runtime/tests/tree_close.rs`.

Needs You and Done ordinarily belong to operator-owned roots.
A delegated row stays Working or Seen while its parent can handle it; `agent_state/escalation.rs` raises it to Needs You only under the six conditions below.
The row keeps its own demand, mark and status word, so the parent's badge can still say what its child is asking for; what changes is only which group the row sits in and whether it is drawn bright.
Done is therefore scoped to the lineage root: a delegated child that finishes leaves a dimmed Seen row, and the completion the operator acts on is the root's.

## Where a parent comes from

Ownership, the tree, the breadcrumb and the descendant badge all start from one fact per agent: its responsibility parent.
`hide agent spawn --parent here|<self id>` records delegation; omitting `--parent` hands the agent to the operator as an independent root.
The latter has no indentation, from hint, parent line, descendant contribution, ancestor unread propagation, waiting-on-descendants effect or inclusion in the spawner's subtree close.
Its own demand and completion enter Needs You and Done normally, and closing the spawner leaves it running.
The CLI's required `origin` field records the spawner in both modes (null for ordinary roots), but origin never enters the UI or any ownership calculation.
PR and issue panels still find the independent session through its own branch records without a delegated-by line.
Herdr records no lineage, so hided writes the four pane tokens through `pane.report_metadata`: `parent_pane` names the parent's pane id, `parent_machine` names its machine when the parent is remote, and `child_session` and `parent_session` are the original session digests.
The value and lifetime contract is unchanged.
The session tokens are lowercase hexadecimal SHA-256 digests of each original `agent_session.value`; this keeps even a path-valued session inside Herdr's 80-character token limit.
A same-server relationship omits `parent_machine`.
A changed child session clears all four tokens, while an absent session proves neither a valid relationship nor a session change.
Ending registration leaves the tokens until the child pane changes or disappears.
A registration ends by itself when the host's authoritative pane topology stops reporting its pane, as a read record is dropped, and never from an incomplete or stale read ([delivery.md](delivery.md#agent-registration-and-spawning)).
`hide agent list` then reports it `ended` and `disconnected`, and its name is free on that host.
A relationship is accepted only when both panes still report the recorded session digests; without both digests the child is a root until a complete declaration is written.
A changed parent session cannot adopt the previous session's children, and an absent parent agent retains the existing orphan presentation.
A missing or unknown remote machine identity leaves the child a root until its matching device connects.
The operating-system identity is the platform UUID on macOS and the machine-id on Linux; cloned machines need distinct identities before they can safely resolve different parents.
`hide agent register`, delegated `hide agent spawn` and Hide's fork record the relationship in the core's coordination ledger.
The existing one-second agent refresh writes only panes whose tokens differ, with a complete reconciliation on startup or reconnect and an immediate write after spawn.
`wire.rs` and `agent_state/axes.rs` remain the readers of this contract.
A connected device's immutable machine identity comes from its consented helper's connection greeting, outside the runtime mutex; an unavailable identity leaves the parent unresolved and records a diagnostic.
A declaration whose machine cannot be matched stays a root and records one diagnostic for that pane instead of guessing.

A pane outlives the agent it hosted and the tokens outlive the agent with it, so a declaration is only a claim until the sessions prove it: it holds while the child's pane reports the `child_session` and the parent's pane the `parent_session`.
`wire.rs` checks the child as it turns the tokens into a row, and `agent_state::apply_lineage` checks the parent because that is where both rows are known, on this machine or another.
An agent that took over a pane is therefore a root, and a parent pane taken over by another agent adopts none of the old children: no line, no descendant badge, and no orphan hint, since the child was never that agent's.
A pane Herdr reports without a session cannot prove a match, so its relationship does not hold until it reports the recorded session again, and a `parent_pane` without the session tokens is not a relationship at all.
A parent whose pane no longer lists an agent is a different case: the child stays an orphan root with its hint.

An empty token is a cleared declaration, not a parent named by an empty string.
The token is display-only in Herdr's own terms and dies with the pane, so a closed child leaves no edge behind, and a parent that has gone makes the child an orphan root.

Regression owners: `a_parent_declared_as_a_pane_token_is_the_lineage`, `a_child_whose_pane_now_hosts_another_session_declares_no_parent` and the `lineage_sessions` tests in `herdr-core/src/session_sync/tests.rs`, `a_parent_pane_on_another_machine_reused_by_another_agent_adopts_no_local_child`, `a_sleeping_child_stays_under_its_parent_only_while_its_record_can_prove_it`, and `web/e2e/lineage-session.spec.ts`.

## The descendant badge

The sidebar, Sessions and pane header share one badge and direct-child popover.
`direct_child_counts` supplies one mark and count per state, error, approval, question, working and done, with zero states omitted; all-ready children read `↳N`.
Unknown activity adds no invented count and is logged as `lineage.unknown_descendants`.
The badge remains visible whenever there are direct children, including children in another checkout or device; the sidebar never unfolds delegated rows.
Pointer, Enter or Space opens the current direct children with their mark, provider, title, last line, branch or PR, device when different and elapsed time.
Arrow keys select a child, Enter or its arrow opens its pane, and Escape returns focus to the badge.
The last item, All, opens the Overview Agents graph.
A disconnected child's action is disabled with the current connection reason.
Disappearing children leave immediately and an empty popover closes.

The one sidebar raises Needs You and Done above its project tree, at most five and three most recent roots respectively, with older rows behind More.
Those rows also remain under their checkouts, and a shortcut belongs to the first visible occurrence in physical sidebar order.
A checkout starts open; `session_collapsed_checkout_ids` remembers only explicit collapses, independently of older disclosure records.
A collapsed checkout still shows its Needs You rows.
Core `session_folds` puts agentless worktrees behind No agents, excluding the primary, front, dirty, unpushed and already inactive checkouts, and collects agent worktrees and missing folders behind Cleanup at the bottom.
`session_open_folds` remembers explicit openings across startup and device reconnects, including an intermediate empty catalog.
A confirmed project unregistration removes its project key; removing a device removes its project keys and Cleanup key.
Resolving a session removes it from sidebar membership without changing its pane, tab or graph membership.

Regression owners: `web/e2e/sidebar-status.spec.ts`, `web/e2e/projects-sidebar.spec.ts`, `web/e2e/session-panel.spec.ts`, and `runtime::tests::agent_scopes`.

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
| Approval | `!` | Yellow | Approval; a blocked pane, or a plan waiting for approval, stays Needs You even after being read |
| Error | `×` | Red | Error; Needs You while unread or blocked |
| No demand, stopped, completion reported, unread | `✓` | Green | Done; completion awaiting the operator's review |
| No demand, stopped, no completion | `○` | Gray | Idle; a newly opened agent is ready for its first instruction |
| No demand, working | `●` | Blue | Working |
| Root with no demand, stopped, a live descendant working or asking | `○` | Blue ring | Waiting; Working group until every descendant is quiet |
| No demand, stopped, read | `○` | Gray | Idle; a read completion is not another unread Done |
| No demand, unknown activity | `~` | Gray | Unknown; never silently labeled Idle |
| Owning server unavailable | `⊘` | Gray | Disconnected; current agent activity is unavailable |
| Hide ended the agent and holds its conversation | `☾` (moon) | Gray | `sleeping`, `waking` or `sleep_failed` (`Sleeping · resumes when opened`, `Waking…`, `Sleeping · couldn’t resume`); stays in Seen |

Read questions, approvals, and errors retain their symbol and hue with reduced emphasis: a demand on a row the core does not emphasize (read, or a delegated child's) draws its mark and its request line at `--opacity-read-status`, so an unread `?` stands apart from one already looked at (`demandTone` in `web/src/lineage.ts`).
A blocked approval stays in Needs You and keeps full emphasis until it is answered.
Reading is acknowledgment, not evidence that a demand was resolved.
A read, non-blocked demand belongs to Seen unless the core places its running activity in Working; the status mark still describes the demand.
Disconnected presentation overrides the retained mark and text on every affected surface without modifying demand, activity, read records, or the last known group.
Unavailable-server tooltips describe the connection problem rather than presenting retained counts as current work.

A sleeping agent keeps its row, name, place, lineage and badge while Herdr no longer lists it: the core draws the row from its own sleep record (`herdr-core/src/agent_sleep.rs`), because the pinned Herdr forgets an agent and its session reference once its process ends.
Only a seen, stopped, local Claude or Codex agent with no demand and a reported conversation can sleep, so a sleeping row is always in Seen; a delegated child sleeps by the same rule as its parent and keeps its place under it.
A new agent in the pane, whether a wake, the operator or a restore started it, is awake: its `state_change_seq` differs from the one the record kept, and the record is dropped.

### Workspace aggregation

`agent_state/tally.rs` owns Workspace aggregation from the canonical agent projection after pane-level read state is applied.
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

The web shell draws no representative chip; it draws a status badge in its place.
The same pass counts each agent under the mark its own row draws (`RowMark` in `agent_state/tally.rs`, the one decision behind the row's symbol): error, approval, question, working, done, and idle, the hollow ring of a quiet agent the operator has already seen.
A row Herdr reports as unknown draws `~` and is counted in none of them, so the badge claims nothing the projection cannot vouch for.
The counts ride the summary as `marks`, and a project's badge is its checkouts' counts added up.
The badge draws one mark and count per state, worst first (`× ! ? ● ✓ ○`), zero states left out, in the marks and colors the rows use, so it says what opening the rows would show.
Unlike the descendant badge it counts idle agents, because on a folded checkout it is also how the operator sees that agents are there at all.
A checkout's badge stands for its folded agent rows and leaves while they are open; a project's stays, open or folded, because it is the project's own summary (docs/UI_BEHAVIOR.md, Sidebar hierarchy).
Regression owners: `workspace_summary_uses_physical_ownership_priority_and_unique_panes` for the counts, `projects.test.ts` for the project sum, and `projects-sidebar.spec.ts` for where the badges are drawn.

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
The web shell starts every checkout closed instead and keeps the ones the operator opened in `expanded_checkout_ids`, which only `checkout_agents_toggle` (a flip) and `focus_checkout`'s `expanded` change, and the legacy `collapsed_checkout_ids` field a client could send instead is kept in the schema but ignored by the web shell.
Raised Needs You and Done rows remain available, while number shortcuts skip hidden tree rows.
In the web shell the fold controls of a project, a checkout and a parent agent all sit on the right of their row in slots kept at rest; a folded control is always shown and an unfolded one appears under the pointer, with focus inside the row, while the row's menu is open, or on an input with no hover.
The body of each row navigates (a project to its Overview, a checkout to its Workspace, an agent to its pane); a project or checkout row also unfolds its children, or folds them when its scope is already in front and unfolded, and a fold control never navigates: folding from a chevron changes no screen, pane, tab, read state, group or process.

### Verification ownership

Core status tests own the Done mark, representative priority, unique-pane counts, cross-checkout ownership, and unchanged read semantics.
Core close tests own the separation between work confirmation and unknown-status blocking, including local and remote close refusal.
Manual acceptance in the desktop app covers fixed semantic colors and the shared disconnected override, mixed states, Done-to-Idle acknowledgment, right-side disclosure, unchanged terminal selection on collapse, empty and missing workspaces, and disconnect/recovery.
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
`herdr-core/src/agent_state/tally.rs` counts the groups the projection already decided rather than reading tokens or axes a second time.
The pet dashboard's count tiles read the same four groups, plus the rows whose server stopped answering.

## The subagent badge

The badge row carries one more count after the three groups: the in-process subagents Hide's hook reports as working, in purple.
`agent_state/tally.rs::subagents_active` sums the `working` hook token over the agents on an answering server, with saturation, and returns zero while disconnected; a pane whose agent has gone is not counted even if its token lingers, and an instrumented pane whose count is unknown adds nothing rather than a zero.
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

### Not connected, and what fixes it

A Claude Code or Codex pane whose session Hide does not hear also carries a connection (`PaneChildrenSnapshot.connection`), read from the same observation as the mark above (PRD settings-cleanup D-09, D-11, B26 to B31).
It is judged by `sidebar::pane_connection` from `uninstrumented_code`, and by nothing else, for an agent that is awake: a sleeping agent has ended its process, so no hook can speak from its pane, and it carries no connection until it wakes.
Settings counts sessions and never these connections, so a pane that needs a Reopen says so only in its own header.

| Instrumentation | Connection |
| --- | --- |
| instrumented | `connected: true` |
| `session_predates_install`, a Codex and the machine's shared server on, or its daemon still answering | `codex_shared_server` |
| `session_predates_install`, any other | `started_before_hide` |
| `hooks_not_installed`, `config_unreadable`, `hook_outdated` | `setup_needed` (fix it in the agent's row in Settings; Reopen would change nothing) |
| `hooks_switched_off`, `unknown`, an agent with no hook, or an agent asleep | no connection, so no chip |

`can_reopen` is false for `setup_needed` and for a pane on another device, because a Reopen restarts the session through this Mac's Herdr.
The shared server is the machine's last kit read (`KitSnapshot::shares_codex_server`): a read that says the setting is on, or that a daemon still answers with the setting off, is what turns a Codex pane's reason into the shared server, and a later read that says neither turns it back into `started_before_hide` at once (PRD codex-daemon-apply D-07, B9).
After a turn-off that answered `stop_failed`, only a read that says no daemon answers ends the shared server, so a daemon answer Hide could not read keeps the retry on offer (B7).
A kit read happens at launch and on each device connection, on a Reinstall, when a Settings tab showing the kit opens and on this Mac every 5 seconds while it stays open, and in the answer to a turn-off request.
Nothing else polls for a daemon, so one that another app starts later reaches the pane's reason at the next of those reads, not live.

Reopen is one event, `pane_reopen { pane_id }`, and reuses the session-sleep path rather than a second one: the agent is ended the way sleep ends it and started again in the same pane with its own resume arguments, a Codex with `--no-daemon` first (`herdr-core/src/pane_reopen.rs`).
Everything knowable before the agent is touched is refused before it is touched, so a refusal known at that point leaves the pane as it was: an agent that is working or waiting (`agent_busy`), a session Herdr never reported an id for or one with no resume arguments (`session_gone`), a Codex whose capability was never read (`codex_unread`), and a folder that is gone (`start_refused`).
The agent is read again just before it is signalled, because the list the decision used can be a second old: one that has started working or is waiting since is `agent_busy` too, with nothing signalled.
Any other end Herdr or the agent refuses is `end_refused`, and a start Herdr refuses after the end is `start_refused`.
The pinned Herdr keeps an ended agent's name for a moment, and refuses `agent.start` under it as `agent_name_taken` until it lets go; a Reopen, and a wake after a sleep, send the same start again on exactly that refusal for at most ten seconds (`agent_start::start_at_shell_reusing_name`), never before the end has been confirmed, and a name that is still held then is `start_refused`.
Every other start takes that refusal as Herdr's answer, because its name is someone else's.
The core keeps one entry per pane, so a second press while one runs starts nothing; the pane's `connection.reopen` is `{"state":"pending"}` while it runs and `{"state":"failed","reason":<code>}` after a refusal, and the entry goes with the need for it: when the pane connects, when it leaves, or when it shows no chip.
A start that lands publishes nothing more by itself: the session's own hook reaching Hide is what turns the chip off.
A start refused after the end leaves the pane without an agent, so there is no chip to carry the failure: when the pane then has no agent row, the snapshot's `last_error` says once, as `pane_reopen.not_restarted`, that the agent was ended and could not be started again and that its conversation is kept; Herdr's words stay in the diagnostic, and the shell raises the failure as a notice that outlives `last_error` (`web/src/errorNotice.ts`, [UI_BEHAVIOR.md](UI_BEHAVIOR.md) on Reopen).
It is not started a second time, because the wait for the ended agent's name above is the one retry a start has, and a second identical start would learn nothing.
Herdr's words for a refusal go to the diagnostic log (`pane_reopen.answered`), never to the screen.

The Codex shared server is turned off by one more event, `codex_daemon_disable { device_id }` (`local` for this Mac), and only by it: no install pass ever does.
The operator confirms it first, because it also stops the daemon that is running, which disconnects every Codex attached to it (PRD codex-daemon-apply D-04, D-11).
It rides the machine's kit queue as `Scope::codex_daemon_off`, and the pass that carries it runs `codex features disable daemon_auto_start`, reads the setting back, stops the running daemon with `codex app-server daemon stop`, and answers in its report.
`KitSnapshot.codex_daemon_off` is `{"state":"pending"}` from the request until that report, then `{"state":"done"}` or `{"state":"failed","reason":<code>}` with `codex_missing`, `codex_refused`, `timed_out`, `unreachable` or `stop_failed`.
A failure before the setting went off leaves it as it was and stops nothing, while `stop_failed` says the setting is off but the daemon still answers, did not stop, or answered in a way Hide cannot read; the same request again then only stops it.
A plain read of the kit says nothing about the request and keeps the last answer, and a machine whose setting is off with no daemon answering, or whose Codex has no such setting, has nothing to turn off.
A Codex that Hide started runs with `--no-daemon`, is not attached to the daemon, and keeps running.

Regression owners: `runtime::tests::agent_connection` (the connection per reason, Reopen coalescing and refusals, the request's lifecycle), `pane_reopen::tests` (the order of end and start against `FakeHerdr`), `runtime::tests::device_kit` (a device's turn-off over the confirmed connection only, its numbered runs), `hide-kit`'s `the_operators_request_turns_the_shared_daemon_off_once_and_says_so` and the stop tests beside it, and `hide_agent_hooks::codex_daemon::tests` (the stop through the account's own `CODEX_HOME`).

## GitHub status in the Workspace row

The PR icon is independent of agent status and Workspace disclosure.
It appears for a known pull request, an in-progress first lookup, or a GitHub lookup failure; a successful lookup with no matching PR leaves it absent.
A pull request restored from the previous run draws as soon as its checkout row exists.
Clicking opens details without selecting a pane, marking agents read, or folding the Workspace.
The popover shows the PR number, title, Open/Draft/Merged/Closed state, head and base branches, and CI rollup.
Its refresh action reloads one repository; its external action opens the PR URL through the existing external browser route.

The core reads GitHub for every registered local Git project from the moment a window attaches to the daemon (the reader sits in the coordinator's UI-attached block, so a daemon with no window reads nothing), whether or not Overview, the palette or any other screen was opened; a device's project and a plain folder are not read.
A project's status is read once when a window first attaches, again five minutes after its last answer, or sooner when that read failed to get the project's pull requests (`Runtime::reread_stale_github`, with the clock passed in so a test advances it without sleeping; a project whose ask is unanswered is not asked again, so a slow pass is never overtaken by the next), and on an explicit refresh, which advances that repository's generation.
A local agent's session that prints the address of a pull request the last answer does not hold asks for its project as well, so a pull request an agent opens reaches its row when the label worker next reads that session (its turn ending, at the latest) rather than up to five minutes later (`Runtime::read_sighted_pull_requests`).
The project is the one whose pull requests are in that repository, else the printing pane's project when none of its pull requests names another repository, so a repository's first pull request is found and a fork's checkout whose agent opens a pull request upstream is not read for it; an address no read project owns is written to the diagnostic log as `read.sighted_unowned` and nothing more.
The sighting keeps the rule above: a project whose last read worked and has nothing asked since is asked at once and its clock waits for that answer; a project with any ask outstanding (a read in flight, its first read, a refresh, or an answer the clock has not taken yet) is not asked again, and is read once more after that answer lands only if the answer does not hold the pull request and the sighting is still under fifteen minutes old; a project whose last read failed is left to its own retry.
The read is quiet: unlike a refresh from the menu or the popover, it does not set `loading`, so the row keeps what it shows until the answer moves it.
Each address sets off this once while its sighting is under fifteen minutes old, so an address the read still does not return (another repository, or past the newest 200) is not asked for again; a sighting older than that sets off nothing, and an address an answer holds stops being remembered.
At most 64 addresses that a project owns are remembered at once, and one past that is not read and is written as `read.sighted_over_limit`; the label worker keeps at most 32 sightings between two hand-offs, the newest, and a read that finds more writes `read.sighted_capped` with how many it dropped; each owned address it remembers writes `read.sighted` with the projects read now and those waiting for an answer, both empty when every one of them is in failure backoff.
A failed read (the answer's `pull_requests_read` is false, whatever the reason) is asked again after 30 seconds, then 60, 120 and 240, and from the fifth failure in a row every five minutes; the first read that works starts the count over, and another project's clock is untouched: the reader hands back its cached entry for every project in a request, so an answer counts for a project only when that project's generation moved since its last answer.
The previous answer stays drawn while a retry waits, and nothing new is shown for it: the row keeps its muted stale state (design principle 13), and each retry writes one `github` `read.retry` diagnostic with the project, the attempt and the delay.
The failure count lives in the clock (`GithubClock`), not in a second delay, and a project is asked again only after its previous ask answered, so at most 64 projects each cost one read per 30 seconds in the worst case and an outage backs every one of them off to the five-minute pace within four retries.
At most 64 local Git projects are read: the project in front first, then the ones a screen named with `github_request` or `overview_refresh`, then the rest in path order; a count past that is stated in the diagnostic log as `projects.over_limit` each time it changes and the ones left out show no PR icon.
Every project shares `GithubReader`, its per-project cache, one active worker and coalesced pending requests.
Each `gh` command runs on the core's own node (`hide-host/src/gh.rs`), where the operator's GitHub login is, under its 15-second limit; the core sends the command line as a `gh` call, the node refuses any line `hide_node_link::gh::allowed` does not name without running it, and the core parses what it answers.
The last successful answer of every project is kept in `github-snapshot.json` in the state folder (`hide_kit::layout::github_snapshot`, written by `github_store.rs`) and restored when the daemon starts, so the first frame already draws it; the file also keeps what the wire leaves out of a pull request (its head commit, its repository and its created and closed times), because a settled pull request reconnects to its worktree only at that commit.
A restored answer is stale until a read replaces it: `stale` is set and `last_success_at_unix_ms` stays the time of the read that produced it, so the icon is muted and the popover says how old it is.
A file that cannot be read, decoded or is of another schema version is not used and is named in the diagnostic log as `snapshot.discarded`; the next answer replaces it.
`loading` is true only for a project with no answer to show whose first read has not yet answered; a project the reader answers nothing for is not loading either, and a project with a restored or earlier answer is loading only while an explicit refresh of it is in flight.
A project's pull requests are two [`gh pr list`](https://cli.github.com/manual/gh_pr_list) calls asked for at the same time, each under the 15-second limit: the 200 newest pull requests of any state without `statusCheckRollup` (with `headRefOid` and `isCrossRepository`), and the open ones with `number,statusCheckRollup` only.
Asking for the checks of every merged and closed pull request made one read take 11 - 14 seconds, so a settled pull request's checks are not asked for again: the checks read for the same number and head commit while it was open stay in the reader's cache, and without them its checks are `Unknown`, drawn as no mark.
Either call failing fails the project's read, which keeps the last answer and states the failure, so half an answer is never a repository with no pull requests; `pull_requests.ok`, `pull_requests.empty` and `pull_requests.failed` carry the read's `duration_ms`.
Which pull request belongs to a checkout is one rule (`github::belongs_to_checkout`, chosen by `pull_request_for_checkout`), because a branch name is used again for new work: a pull request from a fork never belongs to a local branch of the same name, an open one belongs to the checkout on its head branch, and a merged or closed one only while the checkout's commit (the worktree reader's `head_sha`, or, before that reader has answered, the commit `Repository::head_oid` read from the repository's own files with no process) is exactly the pull request's head commit on that branch; a checkout that was amended, rebased or left behind after the last push therefore loses its merged pull request.
Several candidates resolve open, then merged, then closed, the most recently updated first among equals, and a checkout whose commit is not read yet takes no settled pull request.
The sidebar row, the worktree catalog, the agent rows' pull request chip and the link and hand-off actions all ask that rule, and a catalog read that moves a HEAD decides the connection again.
The PRs view and a worktree's base still list one pull request per head branch (`preferred_per_branch`).
The same generation also reads open issues, their Project Status, and PR closing references for Project Home.
The issue list uses `sort:updated-desc` and reads one sentinel beyond the 200-issue display cap so overflow is based on evidence.
Issue references accept a GitHub issue URL, `owner/repo#N`, or `#N` when the repository is known; unsupported hosts and malformed references are rejected.
A failed component read retains that component's last successful answer, including a successfully empty answer, while a successful PR read still advances if issue reading fails.
Hide stores no new credentials and runs no subprocess or file write under the runtime mutex; the five-minute ask is a comparison on the coordinator's existing wake, and the file is written by its own thread from an owned copy.
The project request event, result status and PR fields travel through revisioned `rest`; presentation reads that snapshot only.

| GitHub checks | UI |
| --- | --- |
| Every reported check succeeded, was neutral, or was skipped | Passing, green |
| Any failure, error, cancellation, timeout, or required action | Failing, red |
| At least one running, queued, waiting, pending, or requested check, with no failure | Running, yellow |
| An explicitly empty check list | No checks, gray |
| Absent check data, an unknown check kind, an unrecognized terminal result, or a merged or closed pull request whose checks were never read while it was open | Unknown, no mark |

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

Overview reuses this cached PR answer, including failure and stale status.
Its active-branch and draft counts describe the selected results, not every PR in the repository's history.
The details state the reader's lookup window and offer the existing PR URL and scoped refresh actions.
A failed or unrequested lookup never becomes a zero count.
The workspace inspector uses the canonical representative agent and disconnected override; inspection itself never acknowledges an agent.

## Task identity

The core publishes one `identity_label` per agent, and every surface calls the agent by it: the sidebar row, the pane header, the ⌘K search row, the ⌃Tab Recent Panels row, the lineage chips, the Overview agent line and the request view.
The provider fallback reads the shared adapter's sidebar label or canonical Herdr kind after common alias normalization, preserving a raw unknown kind and `Agent` for no kind.
`sidebar.rs` owns the ladder (PRD overview-request-view D-13): the label's `goal` (laid on the row as `task`), then the agent's own title for the session (Claude Code's `/rename` over its `ai-title`, Codex's `thread_name`, OpenCode's session title, read by the session adapter), then the provider's name (`Claude`, `Codex`, `OpenCode`, the kind Herdr reports, or `Agent` when it reports none).
The agent's own title rides the same proof as the label: it is laid on the row only while the pane's reference proves the session it was read from.
The Herdr workspace label is never a name: it is whatever the workspace was called when it was opened, and one workspace can hold agents for several checkouts.
The Herdr agent name remains the unique control identifier that Sasu and other orchestrators assign at start, so it never enters the display ladder.
Nothing publishes a session `name`, reads Codex's first human turn as a title, or renames an agent or tab.

The label is made by the core, not by a plugin and not through pane tokens: `herdr-core/src/labels/` reads each Claude, Codex or OpenCode pane's conversation, asks the background AI for the session's goal, one line for the turn and how the turn ended (`context_label.v5`), and keeps the answer per pane (the architecture is in [ARCHITECTURE.md](ARCHITECTURE.md#agent-labels-in-the-core)).
`LabelOverlay::apply` lays that label onto an agent just before the runtime projects it, as `task` (the goal), `expected_reply` (the line when the turn ended on a question), `progress` (the line otherwise) and `question` (a question end on an agent that is not running), and `sidebar.rs::project_agent` reads those four.
With Settings › Hide AI › Features › Agent summaries off nothing of the label is laid: the row is titled by the session's own title or the provider, and has no sentence and no written question (D-11).
A label is shown only for the session it was proven for.
The record keeps the provider's native session owner, proven from transcript metadata, and the Herdr reference it was proven under; the label is applied only while the pane's current provider and session reference equal that owner or that reference (`PaneRecord::proven_for`).
A new session, a reused pane, a provider change and an A to B to A switch therefore show the provider's name and no question until the new session is proven, and returning to A restores A's label.
While a pane has no concrete reference, nothing is shown and the proof is kept for the reference's return.
A read or analysis that lands after the pane's reference moved carries an older generation and is dropped, so a late answer for the old session never reaches a row.
A restart restores a label with no read and no request when the pane's reference still proves it, and an entry the retired plugin left (`display-state.json`) is imported once on this Mac only when it had proven an owner, and is shown by the same rule.
Sleeping rows preserve only labels captured from the proven current projection; older sleep records without provenance fall back to the provider.
This adds no worker, subprocess or I/O under the runtime lock: the worker's reads and analyses run off it and the projection only copies the bounded label strings, with no new notification beyond an actual projected state transition.
Regression owners: in `herdr-core/src/labels/tests.rs`, `another_session_shows_nothing_of_the_last_one_until_it_is_proven`, `an_analysis_that_lands_after_the_session_changed_is_dropped`, `a_restart_restores_the_label_with_no_read_and_no_request`, `an_imported_label_shows_only_for_the_session_it_was_proven_for` and `a_turn_that_ran_between_two_looks_ends_the_question`; the device path is `a_device_panes_label_is_read_through_its_helper` and its disconnect and old-helper siblings; `desktop/e2e/session-labels.spec.ts` proves the session boundary against the running app.
There is no `summary` token and no missing-summary notice: an agent without a task is titled by its provider, and the row says nothing else.
Truncation belongs to each view and does not shorten tooltip or accessibility text.
Projection adds bounded-by-metadata strings per agent to the existing snapshot burst, with no extra event, timer, worker, or subprocess.

### The request view's verb

`agent_state/work.rs` owns the row’s PR and issue association, the live PR ordering and the one row holding each PR’s duty.
`agent_state/turn.rs::verb_of` owns the verb independently of the sidebar group.
`request_view.rs` assembles the block and keeps verb timestamps through the existing ledger.
Each agent row also carries a `request` block (`herdr-core/src/request_view.rs`, PRD overview-request-view): the operator's last request with who sent it, the last reply, the row's pull requests, and one verb the request view groups by.
The verb is computed in the core from the axes above and the row's pull requests, never by the shell, and the first rule that holds wins:
an active menu or plan approval, an unread AI question, or an error is `answer`; a running agent is `working`; then, over the open pull requests whose duty the row holds, failed checks are `fix` and passing, absent or unknown checks are `review`; a turn the label read as `unfinished`, on a row with no working descendants, is `stopped`; running checks are `waiting`; an unread completion, or a pull request settled since the operator's last request and since the operator last opened the row's result (`result_opened_unix_ms` in the verb record), is `result`; a quiet root with working descendants, or a turn the label read as `waiting` (on something other than a pull request), is `waiting`; anything else is `idle`.
Reading an AI question skips only the demand step; another applicable duty, such as failed CI, still wins at its later step.
Only the label gives `stopped` and that `waiting` (D-33): with summaries off, without a provider or after a failed analysis no row stops.
The block also carries the label's `line` and `end` when there is one: the request view shows the line in place of the reply (B18) and both as the expanded row's verdict (B6); `end` is pinned as `label_end` in `contracts/snapshot-wire-enums.json`.
A row's pull requests are its checkout branch's and those its session made: a tool in the session printed the address within thirty seconds of GitHub's `createdAt` (D-31), judged once by the label worker and kept with the session's facts.
An address in a reply or a request is a mention and links nothing.
A pull request on several rows gives its duty to the row on its branch's checkout, else to the row whose session printed it first, so `fix` and `review` appear once.
A settled pull request counts as live only when it settled after the operator's last request (D-43).
The verb's time is kept per pane in `core-state.json` (`request_verbs`), so a restart does not restart the wait.
Opening a finished row in the request view (`overview_open_result`) reads the pane the way a focus does and moves no focus; a demand outlives the read as everywhere else.
Who sent a request is the label worker's verdict (ARCHITECTURE.md, Agent labels in the core); a delegated child's first request is its parent's, by the parent row's title.
Text another program wrote, a title, a label line, a sender's name, a request or a reply, reaches the row without control characters or bidirectional controls, and a title, line or name also without the other Unicode default-ignorable code points (zero-width characters, Hangul fillers; `herdr-core/src/display_text.rs`) and on one line and a sender's name at most 64 characters; a sender whose name reads as the row's word for the operator or an unnamed agent (`나`, `에이전트`, `operator`, compared by its letters and digits after NFKC and case folding) is shown as an unnamed agent.
Regression owners: `herdr-core/src/request_view/tests.rs` for the verb, the pull request links, duty and senders, and `runtime::tests::labels` for the title ladder, the stopped and waiting verbs with the switch on and off, the open-result read and the running-checks re-read.

### The Sessions tool

`agent_state/sessions.rs` maps the verb to one group and one task tag in `row.state.session`.
Answer, Fix, Stopped and Result belong to My turn; Review to Review · Merge; Working and Waiting to In progress; Idle to Resting.
A blocked menu takes Approval before an unread AI question's Answer.
Reading an AI question skips its demand rung and leaves a dimmed question; menu and plan approval remain My turn until answered.
Merge requires every open duty PR on the row to have passing checks and an approved or absent review decision; absent or unknown checks never imply a pass.
Without a label line the row keeps its outline but carries no invented task tag or result sentence.
`agent_scope.sessions` publishes ordered member indices, nonempty groups and counts for checkout, project, device and overall scopes.
Ordinary delegated children remain behind their parent's chip, raised children enter My turn, and Factory workers retain their dedicated surface.
A closed session's recorded, unsettled PR enters Review · Merge only when no live session already carries it.
The existing links reader reads this association off the runtime lock; neither a mention nor a timer creates a new link.

Resolve records `resolved_sessions` in core UI state and publishes the hidden row only after the existing coalesced writer acknowledges that exact save.
Failure keeps it visible with the existing actionable save error; input, activity, session replacement or pane closure invalidates a pending acknowledgement.
New operator input, a received letter or renewed working activity restores the session.
Automatic resolution requires a stopped agent, no demand and every assigned PR merged or closed; settlements before restored input cannot resolve it again.
Only resolutions from the current local date enter Today resolved, folded by default; older resolutions stay in tabs and the graph but leave both lists.
The existing runtime tick advances the local-day projection without adding a timer or per-snapshot clock read.
Closing a pane prunes its resolution and input record.
Regression owners: `runtime::tests::session_state`, `request_view::tests`, and `web/e2e/session-panel.spec.ts`.

### Delegated escalation

`agent_state/escalation.rs` reads the existing doorbell and watch decisions rather than adding delivery clocks.
A child rises when its letter is held by a blocked parent or operator draft, when three bells are exhausted and the parent is again eligible after thirty quiet seconds, when the letter becomes undelivered after sixty minutes, when the child's own pane is Herdr blocked, or when the first watch warning has had no parent response for sixty minutes.
An idle or done parent still owns the child; only a closed parent pane or absent parent agent invokes the existing orphan-root rule.
The parent keeps its group and gets a warning second line naming the first raised child and additional count; the child is a My turn row that opens its own pane.
Menu blocking uses Approval, a letter or question uses Answer, and an unanswered watch uses Stopped.
The first three causes clear on receipt or a successful bell, menu blocking on answer, and watch escalation on response, cancellation or a new child activity episode.
Working activity, a causal reply or child disappearance clears any cause.
Doorbell hold reasons remain diagnostics and never become UI copy.
Phone groups follow these same core values: the first three causes and menu blocking send one Needs You push; undelivered letters and unanswered watches retain their existing human notice without another push.
An ordinary delegated question produces no root push, and clearing an escalation resets the existing effective push state immediately.
Regression owners: `agent_state::escalation::tests`, `runtime::tests::lineage`, and `hided/src/mobile/push.rs`.

### Quiet pane headers

`agent_state/header.rs` publishes quiet identity metadata, the selected PR/CI action, a working-line flag and at most one band per pane.
Connection or sleep availability wins, then the pane's own demand, then the first raised child, then the ordinary task verb.
Approval, Answer, Fix, Review, Merge, Stopped and Result have bands; Working has only a thin blue line, while CI wait, child wait and Idle have none.
Bands carry the core reason, action and stable verb time; extra raised children appear as `+N`.
PR actions select the duty that produced the verb and retain the canonical URL, so equal PR numbers in different repositories cannot redirect the action.
PR reason facts carry checks and review separately from label progress.
The existing input does not supply an approval command or failed check names; the band explicitly says those details are unavailable rather than treating a generated sentence as that evidence.
The quiet identity chip can still show the first linked live PR independently of the duty action.
A failed exit is red with its real exit code, a normal termination is gray, and connection and sleep actions remain in their existing body surfaces.
The band overlays the terminal so state changes never resize its PTY grid.
The identity row retains provider, title, direct-child badge, parent return, PR/CI, Not connected chip and existing controls, dropping the parent text first when narrow.
Pending or failed relationship navigation stays visible at its popover, return control or band with retry where available, and in the retained Agent-area status when the source pane is no longer on screen.
Regression owners: `runtime::tests::session_state`, `web/e2e/sidebar-status.spec.ts`, and the pane/lineage desktop checks.

### The second line

The core chooses the row's second line from the group, and publishes it as `detail` with `status_word_visible`; the shell draws what it is given and decides nothing.
The sentences come from the agent's label line (at most 40 characters): `expected_reply` when the turn ended on a question, the one action the operator is asked for; `progress` otherwise, what the agent is doing or has done. A row carries one or the other, never both.

| Group | Sentence |
| --- | --- |
| Needs You | `expected_reply`, else `progress` |
| Done (unread) | `expected_reply`, else `progress` |
| Working | `progress`; a row with an unresolved demand, `expected_reply`, else `progress` |
| Seen with an unresolved demand (a read demand, a delegated child's request) | `expected_reply`, else `progress` |
| Seen (read completions, idle, unknown) | none |

A request outlives reading: a question, approval or error keeps its sentence until it is resolved, whatever group the row sits in, so a view can keep showing what is being asked after the operator has looked.

`status_word_visible` is true only when the group wanted a sentence and the label carried none (a Seen row never shows the word): the status word stands in for it, so an emphasized or working row never has an empty second line and a pane that has no proven label yet reads as title and status word.
Beside a sentence the word is never drawn; the mark and the group heading already say it.
A delegated row follows the same table for its own group, which for an ordinary child is Working or Seen, so a delegated child that has stopped shows only its title unless it is still asking.

Beside `detail` the row carries `message`: the sentence whole up to the 80-character label cap (`MAX_LABEL_TEXT_CHARS`), absent when the label has none.
It is what the agent last said, and the Overview's node shows it only when the operator rests on the node's line (PRD overview-lenses-tiles-agents D-50, B22); the second line stays the one sentence the table above chooses.

The core publishes the sentence; when a view shows it is that view's presentation.
The web sidebar row keeps a request line in the warning color until the request resolves, at the mark's reduced emphasis once read, shows the sentence of an unread row as a bright line that goes once the row is read, reveals the full sentence up to two lines on the selected or hovered row with the rest in a tooltip, and otherwise draws one line (`web/src/agentRow.ts`, owned by `agentRow.test.ts`; docs/UI_BEHAVIOR.md).
The pane identity row shows the title without a status sentence; actionable state belongs to the core band described above.
A shell operation string (`forking…`, `reopening…`) retains its existing identity-row slot while it runs.
The accessibility label of a row and of a header always carries the status word, in the order title, agent kind, status word, sentence, so a row whose word left the screen is still read out with it.

### Search and Recent Panels

The ⌘K sheet's agent row is titled by the identity and subtitled by its checkout and the second line above; when the state chose no sentence it falls to the status word because the goal is already the title.
The pane id is not printed on the row; only a query holding `:` matches it, by the pane's Herdr id holding the query in its case, and lists those agents first ([UI_BEHAVIOR.md](UI_BEHAVIOR.md)).
A tab holding exactly one agent pane carries that agent's identity and mark into its Recent Panels row (`StripTabSnapshot.agent_identity`), derived in the core on every status, lineage, or strip rebuild pass; a tab with none or several keeps its Herdr label.
The core never renames the Herdr tab for this; the Recent Panels label is projection only.

### Project Home

Project Home is the empty local checkout surface and the Shift-Command-H overlay.
Every entry opens the Agents graph with the checkout in front selected (PRD agents-graph-view D-22), except Recent Panels, which restores the lens and expanded rows as they were left; the request view is the tab beside it.
The Agents view reads the core's four buckets: the operator's turn (Needs You, or Done unread), waiting on children (`waiting_on_descendants`), working, and resting.
The core also supplies graph priorities, fold badges and tile counts; `web/src/agentGraph.ts` places rows and lines, and `web/src/overviewLens.ts` translates the bar and chip labels.
The Issues view is the Tasks board below.
Tasks derives delivery in priority order: merged worktree or merged PR, open PR, then in progress; an open issue no checkout works on is the backlog.
Needs You changes the halo and stable sort priority, never this delivery stage.
Main and non-Git checkouts appear on Tasks only while an agent there works on a linked issue; otherwise their agents are on the Agents view only.
Completed columns start collapsed and disappear only with their underlying pane or worktree.

`runtime/issues.rs` resolves workspace manual overrides, pane-family issue tokens, branch configuration, PR closing references, then the two supported branch prefixes.
`wire.rs` extracts metadata, and the core's own node reads the branch setting with the catalog's other path facts (`Call::PathFacts`) off the runtime lock.
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

## Row presentation and phone notifications

`agent_state/turn.rs::row_state` supplies each row’s attention, title emphasis, line mode, semantic tones, graph bucket and priority, search tone, close state and request timing.
The shell maps semantic tones to its existing tokens and translates status words; a tab carries only the mark tone it needs.
A waiting root intentionally keeps its hollow row glyph with a working tone, and a subdued chip and search result.
`row_tests::read_question_keeps_its_request_line_and_hue_without_operator_attention` and `row_tests::waiting_root_keeps_distinct_row_chip_and_graph_decisions` pin these surface differences.

`agent_state/tally.rs::phone` projects the phone’s groups, roots, places and safe row values from the published snapshot.
`agent_state/turn.rs::push` owns the effective notification state and transition ledger; hided keeps pairing, push encryption, transport and delivery policy.
A read root question clears the server’s effective push state, while the phone’s existing open-page rule keeps its notification while the demand remains.
The phone reads the core’s `holds_notification` value, and `emphasized` controls its compact title.
The projection and push regression tests remain in `hided/src/mobile/{projection,push}.rs`, exercising the same public core functions the transport calls.

### Scope projections

`agent_state/tally/scope.rs` publishes `agent_scope` on the navigator, each device, each project and each checkout.
Physical group totals preserve every reported row; Overview members use the first checkout owner and deduplicate a pane within each project, while checkout marks retain the existing last-owner rule.
Physical row references include an occurrence index so duplicate pane IDs keep their distinct labels and states; Overview and graph membership retain the first source occurrence.
The scope projection reads Factory worker panes from the current summary’s column cards, excludes them from Sessions and the Overview attention count, and keeps them in physical agent lists.
A changed Factory summary refreshes the scope in the same publication; unchanged worker membership reuses the cache.
The retained request projection keeps its previous internal scope semantics.
Sessions separately excludes ordinary delegated children in every scope and publishes its five groups and counts.
Disconnected devices have empty physical totals but retain their last Overview members, matching the existing rail and board behavior.
The scope cache compares owned agent rows, device connection facts and checkout membership and summaries; it restores cached values after a catalog rebuild and recomputes only when those inputs change.
The frozen screen counts are asserted by `agent_state::tally::scope_tests::physical_groups_root_headings_and_requests_keep_the_frozen_screen_values`; ownership, unchanged projection and disconnect retention are asserted by `runtime::tests::agent_scopes`.

`agent_state/tally/lineage.rs` projects the list headings, direct-child membership, folded checkout badges and checkout trees with their two card representatives.
The sidebar uses the connected-device tree; the Issues board uses the project device's tree, preserving the previous scope difference.
A done descendant turns a card yellow only when it is a root relative to that checkout.
Folded checkout lines carry status-priority tiers; the browser keeps its existing locale-aware alphabetical placement inside a tier so Korean and English labels keep their displayed order.
The cross-checkout counts, relative-root highlight and fold transitions are asserted by `runtime::tests::agent_scopes::checkout_trees_and_folded_badges_preserve_cross_checkout_lineage_and_priority`.

`agent_state/work/board.rs` owns the PR list's branch and maker association, issue chip source, ancestor rows, attention ordering, groups and open count.
The maker remains listed after moving to other work, but only the branch's agents can make the PR read as fixing or needing review.
`runtime::tests::agent_scopes::pr_board_keeps_branch_turn_separate_from_its_maker_and_tracks_issue_changes` pins that distinction and GitHub-only invalidation.
`agent_state/work.rs::row_work` supplies the selected PR, additional-PR count and ordered issue keys for expanded and folded request rows.
It preserves live-PR-first selection, checkout-task precedence and issues closed after the request; issue facts and checkout task changes invalidate the scope cache.
The agent palette receives its checkout and closing-issue keys from `tally/relations.rs`.
The device scope also carries raised sections (five Needs You rows and three Done rows before overflow) and each numbered agent's first checkout owner.
`runtime::tests::agent_scopes::raised_sections_keep_five_questions_three_completions_and_first_number_owner` pins those limits.

`agent_state/tally/close.rs` publishes the existing pane, tab, checkout and project close targets' confirmation decision, stop-work rows and outside-descendant lists and counts.
The first unknown target still takes priority over confirmation; a subtree sheet includes every target while counting only descendants outside it.
The shell chooses the current target and translates the published states; runtime close enforcement is unchanged.
A removal dialog whose target has left the catalog keeps its existing result without requesting live close consequences.
`runtime::tests::agent_scopes::close_consequences_keep_unknown_priority_and_outside_descendant_counts` pins these rules and pane-only invalidation.

`agent_state/tally/graph.rs` publishes graph membership, row priority, project attention, cleanup/resting folds and the marks tucked under visible ancestors.
The three fold toggles and project/all selection have sixteen combinations, with identical badge maps shared in the snapshot.
The renderer selects a combination, filters text and draws geometry; a filtered box reads the first remaining row in core priority order.
`runtime::tests::agent_scopes::graph_folds_count_hidden_marks_on_the_nearest_visible_ancestor` pins nested hidden marks and Git-only fold invalidation.
`graph/cross.rs` projects one outgoing chip per other project, ordered by its most urgent child and then latest activity, followed by the incoming parent chip.
The first global pane occurrence wins, same-project delegation stays an indent or line, and distinct device IDs retain context even when their labels match.
Disconnected catalogs retain their chips, matching the graph’s existing last-known membership.
The chip’s count, names and first checkout are projected together; the web resolves navigation and translates the local device label.
`runtime::tests::agent_scopes::graph_cross_project_chips_preserve_order_counts_and_stable_device_context` pins the incoming graph screen values and cache invalidation.

`tally.rs` also owns local and remote device counts and project-removal running-agent totals.
Device scopes publish the current listed rows by source index, preserving duplicate physical rows and local-first order, and the first checkout place for each pane.
The overall scope concatenates connected listings; a disconnected device keeps its catalog places but marks them unavailable to the live sidebar.
The PR board publishes its turn/fixing/blocked counts and the review/draft/finished-agent breakdown, including the existing precedence of a finished agent over draft status.
The scope ownership/disconnect and PR-board tests above assert these values.

The command palette reads `tally/relations.rs` groups and row depths for the selected agent, including its in-checkout ancestors, outside parent and descendants in other checkouts.
It translates their tags and resolves PR/issue labels without rebuilding lineage membership.
The relation fixture in `runtime::tests::agent_scopes` covers both the parent caption and the cross-checkout child group.

`turn.rs::AgentUse` and `tally/cleanup.rs` retain checkout-removal use counts, including the last known busy descendants on disconnected devices.
Their unknown-state and per-ancestor counting rules remain distinct from live close-sheet consequences.
The existing cleanup tests in `runtime::tests::lineage` continue to exercise the public runtime entrypoint.
Delegated-tab placement, active-agent project context, request descendant-question counts and the cleanup row's working indicator also read module-owned answers.
