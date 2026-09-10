# Agent Notes

This plugin is a member of the Hide Cargo workspace.
The repository `AGENTS.md` owns the working rules, the harness namespace, the evidence policy, and the required gates; this file adds only what is specific to the plugin.

## Project notes

- The running watcher is Herdr's own plugin clone, not this checkout, and its startup wrapper does not respawn it: [docs/deployment.md](docs/deployment.md).
- The plugin owns the label prompt, schema, and parser in `src/context_label.rs`; provider process, availability, retries, and duplicate suppression belong to `hide-ai/` and are not reimplemented here.
- Build output is the workspace's `target/`; the scripts under `scripts/` resolve the binary two levels above the plugin directory.

`CLAUDE.md` beside this file is a symlink to this file so both supported runtimes load the same nested instructions.
