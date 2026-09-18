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
- The published label is the session-level `$task` token, backed by `PersistedDisplayState.task`. A second `pane.report_metadata` under the same source carries `name`, `progress` and `expected_reply`, each cleared with `null` when absent, so the two reports stay under the 16-token limit and the pane under 32.
- The session name comes from the session record (`hide-session` parses Claude's `ai-title`; Codex takes the first human turn) and is written once per name: `agent.rename` when Herdr's `[a-z][a-z0-9_-]{0,31}` rule accepts it, the `name` token otherwise, and `tab.rename` when the tab holds one agent and its label is Herdr's or ours. Ownership lives in `PersistedDisplayState.plugin_name`, `plugin_name_as_agent` and `plugin_tab_label`; an operator's name or label is never overwritten (`name_owned`, `tab_label_owned`).
- A refused rename is one `agent_rename_failed`/`tab_rename_failed` log line and no retry; `session_title_missing` is logged once per Claude pane without an `ai-title`.
- The v3 provider contract returns `task`, `task_changed`, `progress`, `expected_reply`, and `attention` in that order, and a false `task_changed` keeps the persisted task byte-for-byte; `expected_reply` is asked for as a 40-character imperative sentence and cut at `MAX_EXPECTED_REPLY_CHARS`.
- A missing task state uses the first three and last eight Human turns with an omission marker, while later turn-start requests use only the new Human-turn delta and the prior task.
- `refresh-active-pane-task` discards the rolling task for the focused pane and re-derives it from the initial session view.
- The watcher clears the v1 `$summary` token once per pane at startup; the user's `~/.config/herdr/config.toml` is not managed by this plugin and must use `$task` after installation.

`CLAUDE.md` beside this file is a symlink to this file so both supported runtimes load the same nested instructions.
