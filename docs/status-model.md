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
The Projects view raises Needs You and Done above the project tree and does not repeat those rows inside it; the Agents view draws all four groups with their boundaries visible and omits empty ones.

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

The pet's badge row counts the same four groups the sidebar draws, in the same order:

| Order | Color | Group |
| --- | --- | --- |
| 1 | yellow | Needs You |
| 2 | green | Done |
| 3 | blue | Working |

A count of zero hides that badge.
Do Not Disturb hides all of them.

The pet's "act now" number is the whole Needs You count and its done number is the whole Done count, so a badge can never disagree with the section it stands for.
`herdr-core/src/pet.rs` counts the groups the projection already decided rather than reading tokens or axes a second time.
The pet dashboard's count tiles read the same four groups, plus the rows whose server stopped answering.

## Ambient signals (subagents, background tasks)

A pane's card can also show small optional badges for active subagents and
background tasks, sourced from a Herdr server's optional `ambient` snapshot
field. See [docs/ambient-signals.md](ambient-signals.md) for the client
behavior, the privacy boundary, and the scope boundary between this repo
(client only) and the Herdr server (not in this repo).
