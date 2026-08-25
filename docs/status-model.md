# Status Model

How herdr's raw agent state becomes a pet pose and a badge row.
The rules here are a contract with what the user already sees in the herdr sidebar; the dashboard must not disagree with it.

## herdr token contract: `_new` means unseen

herdr reports attention state as suffixed string tokens, not booleans:

- `status_question_new: "?"` - a question the user has **not** looked at yet.
- `status_question: "?"` - the same question, already acknowledged. herdr also drops `agent_status` back to `idle` for it.

The same `_new` / plain split applies to approval and error tokens.

Only unseen (`_new`) tokens may be promoted to attention or error.
Matching on the `status_question` prefix alone pulls acknowledged items back into the waiting list, which is exactly the bug the user caught by comparing the dashboard against the herdr sidebar.
Legacy boolean form (`status_question: true`) still means unseen and is accepted.

Covered by tests in `crates/herdr-core/src/herdr.rs` (acknowledged `?` -> idle, unseen `?` -> attention, done -> done), enforced by `INV-herdr-unseen-token`.

## Pet pose priority

An unseen error or question/approval takes precedence over ordinary work so a `!` or `?` is never hidden by a background task.
The compatibility `top_status` field still exposes the five existing states to the dashboard and badge code.
The behavior layer adds the clawd-style priority used for pose selection:

```
error > notification > sweeping > attention > carrying/juggling > working > thinking > idle/roam > sleeping
```

The current Herdr data has no separate sweeping or thinking token, so those slots remain reserved and fall through to the existing status.
One working pane maps to `carrying`, two or more to `juggling`.
After eight idle seconds the pet can roam; after sixty idle seconds it runs `yawning -> dozing -> collapsing -> sleeping`.
Any pointer activity produces `waking` before returning to the normal priority.
Urgent error/attention and the badge contract always win over these delight states.

## Badges

All badges sit in a single row at the top right, in this order:

| Order | Color | Meaning |
| --- | --- | --- |
| 1 | blue | agents currently working |
| 2 | green | finished but not yet confirmed by the user |
| 3 | yellow | unseen question or approval |
| 4 | red | error |

A count of zero hides that badge.
Do Not Disturb hides all of them.

Green is deliberately separate from yellow: finished-unconfirmed is "look when you have a moment", unseen question is "act now".
Acknowledged items disappear from the list entirely.

## Ambient signals (subagents, background tasks)

A pane's card can also show small optional badges for active subagents and
background tasks, sourced from a Herdr server's optional `ambient` snapshot
field. See [docs/ambient-signals.md](ambient-signals.md) for the client
behavior, the privacy boundary, and the scope boundary between this repo
(client only) and the Herdr server (not in this repo).
