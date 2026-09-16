# Agent Notes

This plugin is a member of the Hide Cargo workspace.
The repository `AGENTS.md` owns the working rules, the harness namespace, the evidence policy, and the required gates; this file adds only what is specific to the plugin.

## Project notes

- The running watcher is Herdr's own plugin clone, not this checkout, and its startup wrapper does not respawn it: [docs/deployment.md](docs/deployment.md).
- The plugin owns the label prompt, schema, and parser in `src/context_label.rs`; provider process, availability, retries, and duplicate suppression belong to `hide-ai/` and are not reimplemented here.
- The watcher is one event loop over `events.subscribe`, the wake socket, and elapsed display boundaries; it does not run periodic Herdr checks or spawn the Herdr CLI.
- `pane.agent_status_changed` is a pane-scoped contract filter, so the watcher expands it for the pane ids in its bootstrap list and rebuilds the subscription when panes are created or closed.
- Hooks and refresh write their existing marker files before sending one line to `watcher.sock`; a missing watcher leaves the marker for the next bootstrap.
- Build output is the workspace's `target/`; the scripts under `scripts/` resolve the binary two levels above the plugin directory.

`CLAUDE.md` beside this file is a symlink to this file so both supported runtimes load the same nested instructions.
