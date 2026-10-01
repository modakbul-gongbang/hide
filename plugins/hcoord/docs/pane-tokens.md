# hcoord pane lineage tokens

hcoord is the only writer of the portable parent relationship it creates.
Any Herdr client can read the relationship from `herdr agent list` without running hcoord or reading its ledger.
A relationship names two panes and holds only while both still host the sessions it was written for, because a pane outlives the agent it hosted.

## Token contract

| Token | Value | Writer | When present | Lifetime |
| --- | --- | --- | --- | --- |
| `parent_pane` | The parent's exact Herdr pane id on the parent machine | hcoord, with source `hcoord` | Every child registered or spawned with a parent | hcoord clears it when the child's pane hosts another session; otherwise it remains after `hcoord agent end` and disappears with the child pane |
| `parent_machine` | The parent's stable operating-system machine id | hcoord, with source `hcoord` | Only when parent and child use different Herdr servers | It has the same lifetime as `parent_pane` |
| `child_session` | The lowercase hex SHA-256 of the child's `agent_session.value` when the relationship was written | hcoord, with source `hcoord` | Whenever `parent_pane` is | It has the same lifetime as `parent_pane` |
| `parent_session` | The same digest of the parent's `agent_session.value` | hcoord, with source `hcoord` | Whenever `parent_pane` is | It has the same lifetime as `parent_pane` |

A same-machine relationship omits `parent_machine`.
Writing the same relationship again must converge on the same values.
A session is written as a digest because Herdr cuts a token value at 80 characters and a session can be a path; a reader derives the same digest from the session each pane reports and compares the two strings.
If hcoord cannot write the tokens, it leaves the running agent and pane intact and reports the retry action rather than inventing another parent source.

## After a Herdr server restart

Herdr does not restore pane tokens when its server restarts; it restores panes, their ids and each agent's session reference, so the child keeps its pane and its session while both tokens are gone.
The hcoord ledger keeps the relationship, and the running daemon writes it back: every 5 s for this machine's Herdr and every 60 s for a saved remote machine, it reads each Herdr server that holds a registered child with one `herdr pane list` and writes the tokens only to a child whose pane still hosts its recorded session and whose tokens are missing or differ from the relationship.
This does not depend on which started first: after a reboot the daemon can start minutes before Herdr, and the tokens return within one interval of Herdr answering.
A child Herdr reports without a session is told apart only by its terminal, which a restart replaces, so it is not linked again: its pane now runs something else.
A server that does not answer is logged once to the daemon's stderr and adds nothing to the ledger; a write Herdr refuses is logged and tried again five minutes later.
A `lineage.reconciled` ledger event records each pass that wrote a token.

## When a pane hosts another agent

Tokens die with the pane, not with the agent, so a different agent started in the pane of an ended child would otherwise inherit its parent, and a parent pane taken over by another agent would adopt the old children.
Two rules close that.
The first belongs to every reader and needs no running hcoord: a reader keeps a relationship only while both panes report the sessions its tokens name (Hide: `wire.rs` for the child, `sidebar::apply_lineage` for the parent).

- A child whose pane reports another session is a root: no parent, no line, no descendant badge on the old parent, and no "from an agent Hide can't see" hint, because it never was that agent's child.
- A parent pane that reports another session adopts none of the children whose `parent_session` names the old one; they are roots.
- A pane that reports no session proves nothing, so the relationship does not hold while that is so: Codex can report `agent_session: null` before its first turn or after a restart. The tokens stay, and the relationship returns as soon as the pane reports the recorded session again.
- A `parent_pane` without both session tokens is not a relationship. Any other writer of `parent_pane` has to write them too.
- A parent whose pane no longer lists an agent at all is the existing orphan case: the child stays a root and says where it came from.
- Relationships across machines follow the same rule, after `parent_machine` has matched a connected device.

The second is the daemon's, applied to its own tokens.
On the reconcile pass that reads a child's route, a child pane that reports a session other than the recorded one is no longer written: the daemon clears all four tokens in one `herdr pane report-metadata --source hcoord --clear-token ...` call and records a `lineage.ended` ledger event naming the child, its parent, the pane and both sessions.
A refused clear is logged and tried again five minutes later, and a pane that reports no session is left alone, because it neither proves the relationship nor ends it.
The daemon does not read the parent's pane for this: the reader above already refuses a stale parent, and the tokens stay until the child's own pane changes.
If the pane later reports the recorded session again, the next pass writes the relationship back.

### Tokens written before sessions were recorded

A `parent_pane` written without the session tokens counts as no relationship until the daemon rewrites it.
The daemon compares the tokens it finds with the full set on every pass, so an old declaration converges within one reconcile interval of the new daemon running: 5 s for this machine's Herdr, 60 s for a saved remote machine.
Hide installs hcoord with the app, so both ship together, and there is no compatibility reading of the old form: a child whose parent was declared by another writer stays a root until that writer adds the tokens.

## Machine id

On macOS the id is `IOPlatformUUID` from `ioreg -rd1 -c IOPlatformExpertDevice`.
On Linux the id is the trimmed content of `/etc/machine-id`.
Hide and hcoord calculate the same value independently, so Hide can resolve a token even when hcoord is not running.

A cloned Linux virtual machine can retain the same `/etc/machine-id` as its source.
Such clones are not distinct identities until the clone receives a new machine id, and duplicate ids can resolve a parent to the wrong registered device.

## Reading and scoping

Without `parent_machine`, a reader resolves `parent_pane` inside the child's Herdr server.
With `parent_machine`, a reader matches that id to a connected device and resolves `parent_pane` inside that device's Herdr server.
If no connected machine matches, the child is a root on its own device until the matching machine reconnects.
