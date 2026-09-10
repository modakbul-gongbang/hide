# Deployment

## The running watcher is not this checkout

Herdr installs the plugin as its own git clone of the Hide repository, and the plugin directory inside that clone is what actually runs:

```text
~/.config/herdr/plugins/github/hide-<hash>/plugins/agent-context-labels/
```

Building this checkout changes nothing about the live sidebar.
Confirm which binary is running before concluding a fix did or did not work:

```sh
ps -eo pid,etime,command | grep 'hide-agent-context-labels watch' | grep -v grep
```

The path in that output is the answer.

## Getting a change into the running watcher

The supported route is to push and let Herdr reinstall the plugin, which reruns the build script from the workspace root.

For a local trial before pushing, overwrite the installed binary.
The plugin is a workspace member, so the binary lives under the repository's `target/`, two levels above the plugin directory:

```sh
P=~/.config/herdr/plugins/github/hide-<hash>
cargo build --release --locked -p agent-context-labels
cp target/release/hide-agent-context-labels "$P/target/release/hide-agent-context-labels"
```

This is temporary. A plugin rebuild or reinstall discards it, so a trial that works this way is not yet delivered.

## Restarting the watcher

The startup wrapper launches the watcher once and waits on the Herdr server; it does not respawn a watcher that exits.
Killing the watcher leaves no watcher running, and the sidebar silently stops updating.

Restart it by hand, or restart the Herdr server to get the wrapper to run the normal startup path:

```sh
kill <watcher-pid>
nohup /bin/sh "$P/plugins/agent-context-labels/scripts/start-watcher.sh" >/tmp/watcher.log 2>&1 &
```

The startup script needs no credential.
It extends `PATH` with the usual install locations so the watcher finds the `codex` binary when the Herdr server was started outside a login shell; the account is whichever one `codex login` left on this machine.

## State lives outside both checkouts

```text
~/.local/state/hide.agent-context-labels/
├── events.jsonl        # append-only log; the first place to look
├── display-state.json  # per-pane summary, verdict, analyzed turn per phase
├── hook-state.json     # pending native-hook interaction state
└── settings.json       # automatic-summaries toggle
```

`events.jsonl` is the diagnostic record.
Counting its events by day and by `detail` is what identifies a provider problem:

```sh
grep '<pane-id>' ~/.local/state/hide.agent-context-labels/events.jsonl | tail -30
grep 'ai_provider_availability\|ai.daily_rollup' ~/.local/state/hide.agent-context-labels/events.jsonl | tail -5
```

`ai_provider_availability` is written at every watcher start and names each provider's state (`codex=ready;claude=unsupported`).
`ai.daily_rollup` is written once per UTC day by the provider layer and counts outcomes per provider, which replaces the request counter the plugin used to keep in `usage.json`.

A directory left under the previous id, `~/.local/state/herdr-agent-context-labels/`, is moved to the new path once when the upgraded watcher first starts; no other command moves it.
If a directory already exists under the new id, nothing is moved and `state_migrated` reports `kept_both`; remove whichever one is stale by hand.

## Recovering a move that ran under a running old-id watcher

If the new watcher's move happened while the previous-id watcher was still running, that watcher recreates its directory on its next write and the state is split across both.
Stop the old watcher first, then reconcile:

- `display-state.json`: the running watcher rewrites its whole state on every change, so the newer file is complete; keep it.
- `events.jsonl`: concatenate the moved history and the lines the old watcher appended afterwards, in that order.
- `settings.json` and `hook-state.json`: keep the moved copies unless the old watcher wrote a newer one.

Leave the reconciled files under the id of the watcher that will run next, and delete the other directory.

## Sidebar colors are the user's, not the plugin's

The plugin publishes `status_*` tokens; `~/.config/herdr/config.toml` decides how they are painted.
A status change that depends on being visually distinct needs a matching config edit, followed by:

```sh
herdr config check
herdr server reload-config
```

`status_working` and `status_done` render the same `●` symbol, so they must be given different colors.
