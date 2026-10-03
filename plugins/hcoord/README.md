# hcoord

hcoord coordinates Herdr agents and writes their portable parent relationship to the child pane.
It is a part of hide, installed and run only by hide; there is no separate install.
Its ledger and outbox stay under `${HCOORD_HOME:-~/.hide/hcoord}`, and the pane-token contract is documented in [docs/pane-tokens.md](docs/pane-tokens.md).

## Where hide puts it

The packaged hide app carries hcoord, and every launch or device connection reconciles it through hide's kit (`hide-kit/src/hcoord.rs`):
the compiled CLI is copied to `~/.hide/kit/hcoord/`, the command is `~/.hide/hcoord/bin/hcoord`, and `~/.local/bin/hcoord` links to it when that name is free or already hide's, so `hcoord` on `PATH` is hide's; a relocated hcoord (`HCOORD_HOME`) never takes that link.
On this Mac it runs on the Electron runtime inside the app, so it needs no system Node; on a device it runs on a Node 22.12 or newer that the device already has.

A machine that still has hcoord in the old `~/.hcoord` is moved once by the kit: it stops the old daemon, renames the folder to `~/.hide/hcoord` whole (the ledger, letters and a manual stop go with it), drops only the socket, the lock of the dead daemon and the temporary files, and starts the daemon again under the same `com.hcoord.daemon` label.
If the move fails, the old folder and the old daemon keep running and Settings shows the reason on the hcoord row; the next launch or Reinstall tries again.
`hcoord home adopt --json` is that step, run by the kit; it moves only `~/.hcoord` of the `HOME` it runs with and takes no path, refuses when `HCOORD_HOME` is set or when the new home already exists, and never merges two homes.

To work from this checkout, build and call the compiled CLI directly:

```sh
pnpm --dir plugins/hcoord build
node plugins/hcoord/dist/hcoord/cli.js status
```

Set `HCOORD_HOME` for an isolated installation.
On macOS the plain `com.hcoord.daemon` label belongs only to the account's default home (`~/.hide/hcoord` under the home in the user database, not `$HOME`); any other home gets a label derived from its directory, so an isolated install never replaces the account's service.
launchd domains are per account, not per HOME, so a test that only moves HOME still gets a label of its own.

A Codex session in `workspace-write` needs `~/.hide/hcoord` in `writable_roots` of `~/.codex/config.toml` to send letters; hide does not edit that file.

## Daemon lifecycle

```sh
hcoord daemon start
hcoord daemon status --json
hcoord daemon stop
hcoord daemon uninstall
```

`start` clears a manual-stop marker.
`stop` records that choice, and a later hide launch leaves the daemon stopped until `start` is run.
`stop` leaves its LaunchAgent loaded so a later `start` can kickstart it.
`uninstall` stops and unloads this home's exact LaunchAgent, confirms that launchd has released it, and removes only its plist.
It works even when the daemon is already stopped, preserves the ledger and outbox, and keeps a manual-stop marker so a kit pass does not undo the removal.
If launchctl cannot confirm removal, the command fails and leaves the plist and home available for recovery.
The LaunchAgent starts at login and restarts an unexpected exit, while the ledger and outbox survive executable upgrades.

Every write command saves a letter in `${HCOORD_HOME:-~/.hide/hcoord}/outbox` first.
A session whose sandbox cannot write there (a Codex `workspace-write` session without that directory in `writable_roots`) gets `permission_denied` naming the blocked path and the next action, and nothing is sent; running the same command again after the fix is safe.
An unexpected exception keeps its errno code and message in the `hcoord.command_failed` event on stderr.
A watch check an observer never closes is sent to the observer, reminded once and escalated to the human once, then left alone.
A watch whose target and observer have both been unreachable for an hour is stopped by the daemon (`watch.orphaned` event) and can be restarted or reassigned by a human.

Before deleting hide.app, remove its login service through the installed CLI:

```sh
hcoord daemon uninstall --json
```

For an isolated installation, pass the same `HOME` and `HCOORD_HOME` used to start it to `daemon uninstall` before removing that home.
This removes the per-home hashed label and leaves the account's default service untouched.
If hide.app was already deleted, run `node plugins/hcoord/dist/hcoord/cli.js daemon uninstall --json` from a built checkout with those same home variables.

## Delegate from a Herdr pane

Inside a pane with a reported agent session, `here` identifies that execution and registers it first when needed:

```sh
hcoord agent spawn \
  --parent here \
  --name worker \
  --intent issue-123 \
  --repo /path/to/repository \
  --branch issue-123 \
  --kind claude \
  -- --model opus "Fix issue 123"
```

`--kind` selects the agent Herdr starts (`codex` by default), and the arguments after `--` go to that executable itself, so they never repeat its name.
Claude takes its flags and then the prompt as its first message; Codex takes its flags and at most one task as the last argument, which hcoord submits as the first turn once Codex is ready.
`hcoord agent spawn --help` prints the full usage.
The arguments are checked before anything is created, so a refused spawn leaves no worktree, workspace, pane, or parent registration behind.
Without `HERDR_PANE_ID`, or when Herdr reports no session-bearing agent in that pane, the command explains the refusal and creates nothing.
Right after its own start, confirmed or timed out, hcoord records the child's terminal and session when Herdr reports them within 2 s, and names the child again when Herdr reports it without its name, so one spawn writes the lineage.
Retry a failed lineage write with the same intent so hcoord repairs the existing child instead of starting another one.
A retry restores a name Herdr dropped only when the recorded session matches, because a pane keeps its terminal while agents are restarted in it; otherwise it refuses and names the `herdr agent rename` a person runs after checking the pane.
`--resume-start` replaces the recorded start of a child that is gone.

Remote registration, worktree spawn, and outbox collection use the machine names saved by Herdr.
The remote machine must be a hide device (hide installs `~/.hide/hcoord/bin/hcoord` there), with the source repository and a testable Herdr server; hcoord stores no SSH credentials and the HQ always initiates the connection.
Runs to one machine share a multiplexed SSH connection (a socket under `/tmp/hcoord-ssh-<uid>/`, closed after 60 idle seconds), so a flaky SSH agent is asked to sign far less often.
A collection that keeps failing the same way logs one `hcoord.collect_failed` line per ten minutes with a `repeated` count, and `hcoord.collect_recovered` when it works again.
