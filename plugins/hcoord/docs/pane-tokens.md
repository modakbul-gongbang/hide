# hcoord pane lineage tokens

hcoord is the only writer of the portable parent relationship it creates.
Any Herdr client can read the relationship from `herdr agent list` without running hcoord or reading its ledger.

## Token contract

| Token | Value | Writer | When present | Lifetime |
| --- | --- | --- | --- | --- |
| `parent_pane` | The parent's exact Herdr pane id on the parent machine | hcoord, with source `hcoord` | Every child registered or spawned with a parent | It remains after `hcoord agent end` and disappears with the child pane |
| `parent_machine` | The parent's stable operating-system machine id | hcoord, with source `hcoord` | Only when parent and child use different Herdr servers | It has the same lifetime as `parent_pane` |

A same-machine relationship omits `parent_machine`.
Writing the same relationship again must converge on the same values.
If hcoord cannot write the tokens, it leaves the running agent and pane intact and reports the retry action rather than inventing another parent source.

## After a Herdr server restart

Herdr does not restore pane tokens when its server restarts; it restores panes, their ids and each agent's session reference, so the child keeps its pane and its session while both tokens are gone.
The hcoord ledger keeps the relationship, and the running daemon writes it back: every 5 s for this machine's Herdr and every 60 s for a saved remote machine, it reads each Herdr server that holds a registered child with one `herdr pane list` and writes the tokens only to a child whose pane still hosts its recorded session and lacks them.
This does not depend on which started first: after a reboot the daemon can start minutes before Herdr, and the tokens return within one interval of Herdr answering.
A child Herdr reports without a session is told apart only by its terminal, which a restart replaces, so it is not linked again: its pane now runs something else.
A server that does not answer is logged once to the daemon's stderr and adds nothing to the ledger; a write Herdr refuses is logged and tried again five minutes later.
A `lineage.reconciled` ledger event records each pass that wrote a token.

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
