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
