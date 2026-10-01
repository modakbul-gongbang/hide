# hcoord

hcoord coordinates Herdr agents and writes their portable parent relationship to the child pane.
Its ledger and outbox stay under `${HCOORD_HOME:-~/.hcoord}`, and the pane-token contract is documented in [docs/pane-tokens.md](docs/pane-tokens.md).

## Install

The packaged hide app carries hcoord and reconciles `~/.hcoord/bin/hcoord` plus its user LaunchAgent whenever the app opens.
It uses the Electron runtime already inside the app, so this path does not require a system Node installation.

To install hcoord as a standalone Herdr plugin on a machine with Node.js 22 or newer:

```sh
herdr plugin install modakbul-gongbang/hide/plugins/hcoord
```

The plugin build installs its dependencies and compiles the CLI.
Its one-shot startup hook runs `hcoord daemon ensure` and exits after ensuring the login-owned service.
If Node or npm is unavailable, the build fails before Herdr registers the plugin.

To work from this checkout:

```sh
herdr plugin link ./plugins/hcoord
pnpm --dir plugins/hcoord build
plugins/hcoord/bin/hcoord status
```

To install the fixed command used by an HQ over SSH on a remote machine:

```sh
pnpm --dir plugins/hcoord install:remote
~/.hcoord/bin/hcoord status
```

Set `HCOORD_HOME` for an isolated installation.
On macOS its LaunchAgent label is derived from that directory, so it does not replace the default `com.hcoord.daemon` service.

## Daemon lifecycle

```sh
~/.hcoord/bin/hcoord daemon start
~/.hcoord/bin/hcoord daemon status --json
~/.hcoord/bin/hcoord daemon stop
```

`start` clears a manual-stop marker.
`stop` records that choice, and a later hide launch or plugin startup leaves the daemon stopped until `start` is run.
The LaunchAgent starts at login and restarts an unexpected exit, while the ledger and outbox survive executable upgrades.

Every write command saves a letter in `${HCOORD_HOME:-~/.hcoord}/outbox` first.
A session whose sandbox cannot write there (a Codex `workspace-write` session without that directory in `writable_roots`) gets `permission_denied` naming the blocked path and the next action, and nothing is sent; running the same command again after the fix is safe.
An unexpected exception keeps its errno code and message in the `hcoord.command_failed` event on stderr.
A watch check an observer never closes is sent to the observer, reminded once and escalated to the human once, then left alone.
A watch whose target and observer have both been unreachable for an hour is stopped by the daemon (`watch.orphaned` event) and can be restarted or reassigned by a human.

After deleting hide.app, remove the default login service explicitly:

```sh
launchctl bootout "gui/$(id -u)/com.hcoord.daemon" 2>/dev/null || true
rm -f "$HOME/Library/LaunchAgents/com.hcoord.daemon.plist"
```

## Delegate from a Herdr pane

Inside a pane with a reported agent session, `here` identifies that execution and registers it first when needed:

```sh
~/.hcoord/bin/hcoord agent spawn \
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
The remote machine must have `~/.hcoord/bin/hcoord`, the source repository, and a testable Herdr server; hcoord stores no SSH credentials and the HQ always initiates the connection.
Runs to one machine share a multiplexed SSH connection (a socket under `/tmp/hcoord-ssh-<uid>/`, closed after 60 idle seconds), so a flaky SSH agent is asked to sign far less often.
A collection that keeps failing the same way logs one `hcoord.collect_failed` line per ten minutes with a `repeated` count, and `hcoord.collect_recovered` when it works again.
