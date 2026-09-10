# Agent Context Labels for Herdr

[![Herdr](https://img.shields.io/badge/Herdr-%E2%89%A5%200.8.0-6c7086)](https://herdr.dev)

Compact task summaries and attention signals for Codex and Claude Code panes in [Herdr](https://herdr.dev).

The plugin keeps the Agents sidebar useful when several coding agents are running at once.
Each supported pane gets a short task label, lifecycle status, agent kind, and the time since its last state change.

```text
○  api-client       codex
   Retry policy cleanup       14s

?  docs             claude
   Choose installation path    2m
```

<p align="center">
  <a href="#install">install</a> ·
  <a href="#sidebar-layout">sidebar layout</a> ·
  <a href="#status-symbols">status symbols</a> ·
  <a href="#actions">actions</a> ·
  <a href="#privacy">privacy</a> ·
  <a href="#development">development</a>
</p>

## What it does

- Adds a maximum-30-character task summary to recognized Codex and Claude Code panes.
- Shows question, approval, error, working, unseen completion, idle, and unknown states as compact symbols.
- Holds one steady symbol per state; working is told apart from an unseen completion by color, not by a blink.
- Shows compact elapsed time such as `12s`, `4m`, `2h`, or `3d`.
- Keeps completion semantics aligned with Herdr's native `working`, `done`, `idle`, `blocked`, and `unknown` lifecycle states.
- Uses native agent hooks for high-confidence interaction signals and the user's own logged-in Codex CLI, through Hide's `hide-ai` provider layer, for task summaries and plain-text question detection.
- Publishes `sort_rank` and `activity` tokens and installs an `agent.view.set` sort on watcher start, so the sidebar orders panes by who is blocking whom: work that finished unread comes first (error, then question/approval, then a plain completion), then work still running, then everything already seen. Ties inside every group break on the `activity` clock, so the most recently active pane leads.
- Runs one headless watcher, so no dedicated watcher pane is required.

The data flow is intentionally small:

```text
Herdr agent lifecycle ────────────────┐
Claude/Codex hooks ── attention ──────┼─> pane metadata tokens ─> Agents sidebar
local session JSONL ─> redact ─> hide-ai ─> codex app-server ─┘
```

Native hook state wins over semantic question detection, and both win over the ordinary Herdr lifecycle display.

## Status symbols

| Symbol | Meaning |
| --- | --- |
| `?` | The agent needs an answer from the user. |
| `!` | The agent is waiting for approval. |
| `×` | The agent stopped with an error. |
| `●` | The agent is working. |
| `●` | Background work finished and Herdr reports it as unseen. Shares a symbol with working, so give `$status_working` and `$status_done` different colors. |
| `‖` | The user interrupted the last turn; clears when the pane works again. |
| `○` | The pane is idle and has been seen. |
| `~` | Herdr cannot classify the current lifecycle state confidently. |

Herdr owns the underlying lifecycle verdict.
In particular, `done` means background work finished before the tab was viewed, while `idle` means the settled pane has been seen.

### Unread and seen

A pane is **unread** when its state changed while you were not looking at it, and becomes **seen** the moment you focus it. That is the whole rule: focus since the last state change.

This distinction drives both the color and the order, so the three interaction symbols are published as two tokens each:

| Unread token | Seen token | Shown for |
| --- | --- | --- |
| `$status_question_new` | `$status_question` | `?` |
| `$status_approval_new` | `$status_approval` | `!` |
| `$status_error_new` | `$status_error` | `×` |

Color the `_new` variants to stand out and the plain ones to recede — that is why one `?` can be bright and another grey. The remaining states (`working`, `done`, `interrupted`, `idle`, `stale`) have a single token each.

### Where each symbol comes from

Not every symbol has the same source, which matters if you skip the optional [agent hooks](#agent-hooks):

| Symbol | Source | Without hook wiring |
| --- | --- | --- |
| `●` `○` `~` `‖` | Herdr's lifecycle, read from the screen | works |
| `!` | Herdr's `blocked` lifecycle, or the hook seeing a permission request | works; the hook only makes it arrive sooner |
| `?` | The hook seeing a question tool, or the model reading plain prose | works, but every `?` is an inference rather than a confirmed fact |
| `×` | The `StopFailure` hook only | **never appears** |

## Requirements

- Herdr 0.8.0 or newer.
- macOS or Linux.
- Codex, Claude Code, or both.
- A stable Rust toolchain with Cargo for the current source-based installation.
- A Codex CLI that is logged in (`codex login`), for generated summaries and plain-text question detection.

There is no API key and no environment variable.
Summaries are produced by `codex app-server`, the same local process the Codex CLI itself uses, under the account already logged in on this machine.
Lifecycle symbols continue to work without Codex.
When no provider can answer (Codex is not installed, not logged in, or over its usage limit), the watcher keeps existing summaries, records `analysis_provider_unavailable` with the provider state, and asks again ten minutes later.

Claude Code is registered as a provider but reports `unsupported`: the installed Claude Code and the pinned Herdr API expose no request-response contract that returns a structured answer without entering the user's own conversation.
Nothing is scraped from a terminal to work around that, and the state is visible in the log rather than silently skipped.

## Install

This document is the canonical reference for installing and configuring the plugin.
[INSTALL.md](INSTALL.md) is a Korean quick start that points back here for anything it does not cover; where the two ever disagree, this file is right.

Install directly from GitHub.
The plugin is a directory of the Hide repository, so the install path names that directory:

```bash
herdr plugin install modakbul-gongbang/hide/plugins/agent-context-labels
```

Install Herdr's agent integrations for every agent runtime you use:

```bash
herdr integration install claude
herdr integration install codex
```

Log the Codex CLI in once, in any shell:

```bash
codex login
```

The watcher finds `codex` on the usual install paths (`~/.local/bin`, `/opt/homebrew/bin`, `/usr/local/bin`) even when the Herdr server was started outside a login shell.

The plugin startup hook runs when a Herdr server starts after restoring its session.
If the plugin was installed or linked while a server was already running, the watcher starts on the next Herdr server start because linking and enabling a plugin do not rerun startup hooks.

Confirm the registration:

```bash
herdr plugin list --plugin hide.agent-context-labels
herdr plugin action list --plugin hide.agent-context-labels
herdr integration status
```

## Sidebar layout

The plugin publishes metadata tokens, while the final row layout belongs to the user's Herdr configuration.
Add this to `~/.config/herdr/config.toml`:

```toml
[ui]
agent_panel_sort = "priority"

[ui.sidebar.agents]
rows = [
  [
    { token = "$status_error_new", fg = "#f38ba8", bold = true },
    { token = "$status_error", fg = "#6c7086", bold = true },
    { token = "$status_question_new", fg = "#f9e2af", bold = true },
    { token = "$status_question", fg = "#6c7086", bold = true },
    { token = "$status_approval_new", fg = "#fab387", bold = true },
    { token = "$status_approval", fg = "#6c7086", bold = true },
    { token = "$status_done", fg = "#a6e3a1", bold = true },
    { token = "$status_working", fg = "#89b4fa", bold = true },
    { token = "$status_interrupted", fg = "#cba6f7", bold = true },
    { token = "$status_idle", fg = "#a6adc8", bold = true },
    { token = "$status_stale", fg = "#6c7086", bold = true },
    "workspace",
    { token = "$agent_codex", fg = "#89b4fa", bold = true },
    { token = "$agent_claude", fg = "#fab387", bold = true },
    { token = "$elapsed", fg = "#6c7086", dim = true },
  ],
  [
    { token = "$summary", fg = "#74c7ec", bold = true },
  ],
]
```

Every status token the plugin can publish is listed above. A token you leave out is not an error, but the state it represents then renders without color, so the unread states are the ones you least want to omit.

Two rules the colors have to respect:

- `$status_working` and `$status_done` both render `●`, so they must differ. Above, working is blue and a finished-but-unread pane is green.
- The `_new` variants should read as louder than their seen counterparts, since that contrast is what tells you which panes are still waiting on you.

The palette fits a dark Catppuccin-style theme and is yours to change; the plugin never sets a color.

Validate and reload the configuration:

```bash
herdr config check
herdr server reload-config
```

### Sort order

The watcher installs its own sidebar sort on startup, which ranks panes by who is blocking whom:

1. Finished but unread, in the order `error`, `question`, `approval`, `semantic_question`, `done`.
2. Still working, whether or not you have seen it.
3. Everything already seen, with no state ranking at all.

Every group breaks ties on the `activity` clock, so the most recently active pane leads. Group 3 carries no state ranking on purpose: once you have seen a pane, recency is the only thing left that orders it.

To change the ranking, create `sort-order.json` in the plugin's config directory:

```bash
mkdir -p ~/.config/herdr/plugins/config/hide.agent-context-labels
cat > ~/.config/herdr/plugins/config/hide.agent-context-labels/sort-order.json <<'EOF'
{"order": ["error", "question", "approval", "semantic_question", "done", "working", "interrupted", "idle", "stale"]}
EOF
```

The list above is the built-in default, so writing it verbatim changes nothing. Notes:

- **This file replaces the built-in order outright.** If it exists, the default no longer applies, and a plugin upgrade that reorders the default will not reach you until you update or delete this file.
- Naming only some states puts those first and appends the rest in their default relative order. Unknown names are ignored, and invalid JSON leaves the default in place rather than failing the watcher.
- The file is read once per process, so a change takes effect on the next watcher start.
- It reorders states within the unread group; it cannot move a pane between the three groups, because unread, working, and seen are decided by the pane, not by this file.

## Agent hooks

Herdr's built-in Claude and Codex integrations provide the lifecycle states consumed by this plugin.
For precise `?`, `!`, and `×` classification, register the bundled `scripts/agent-hook.sh` as an additional hook in the agent runtime.

Use the plugin's absolute root path from `herdr plugin list --plugin hide.agent-context-labels --json` and invoke:

```text
/bin/sh '<plugin-root>/scripts/agent-hook.sh'
```

Register that command for these events without replacing existing hooks:

| Event | Purpose |
| --- | --- |
| `PreToolUse` | Detect `AskUserQuestion` and `request_user_input`. |
| `PermissionRequest` | Distinguish approval from a direct question. |
| `PostToolUse` / `PostToolUseFailure` | Clear the matching pending tool state. |
| `StopFailure` | Report a stopped error. |
| `UserPromptSubmit` / `SessionStart` | Clear attention when the user or a new session resumes work. |

Tool-call IDs are matched before a completion clears pending attention, so an unrelated parallel tool cannot clear the wrong question or approval.

The wiring is optional, but it is not cosmetic. Without it, summaries and every Herdr lifecycle symbol still work and the model still identifies direct plain-text questions, so `?` and `!` both keep appearing. What you lose is precision on two of them and one symbol entirely:

- `×` never appears. An error verdict has no source other than the `StopFailure` hook, so a turn that ended in failure is indistinguishable from one that ended normally.
- Every `?` is the model's reading of prose rather than a confirmed question tool, which also ranks it below a hook-confirmed question in the sidebar order.
- `!` still arrives from Herdr's `blocked` lifecycle, just later — the hook sees a permission request before the dialog reaches the screen.

## Actions

The plugin registers three explicit actions and does not force any keybinding:

```bash
herdr plugin action invoke refresh-active-pane-summary --plugin hide.agent-context-labels
herdr plugin action invoke enable-automatic-summaries --plugin hide.agent-context-labels
herdr plugin action invoke disable-automatic-summaries --plugin hide.agent-context-labels
```

Enable and disable are separate idempotent operations.
Repeating either action leaves the same setting, which avoids an accidental double dispatch flipping the setting back.

An optional refresh keybinding looks like this:

```toml
[[keys.command]]
key = "prefix+r"
type = "plugin_action"
command = "hide.agent-context-labels.refresh-active-pane-summary"
description = "refresh active pane summary"
```

## Summary generation

Analysis is keyed to the conversation turn, identified by the user's own last message.
A turn buys at most two provider requests: one when the user's message is the newest thing in the transcript, which names the task while the agent works, and one when the agent stops, which decides whether the pane is waiting on a reply.
Nothing the agent emits in between triggers a request, and the attention verdict for a turn is drawn once and then held, so a pane's symbol cannot change on its own while the user is not looking at it.

Keying on the turn rather than on the transcript is deliberate.
An earlier version hashed the sanitized session window and re-asked whenever that hash moved; because the window grows with every token an agent emits, a busy pane asked again on nearly every poll, and one day the request budget was spent by midday.

Slow provider calls run outside the status loop, so lifecycle and elapsed-time updates continue while a request is in flight.

Requests go through Hide's `hide-ai` provider layer (`hide-ai/` in the same repository).
This plugin owns the prompt, the output schema, and the parsing of the answer (`src/context_label.rs`); `hide-ai` owns the provider process, availability, timeouts, retries, and duplicate suppression.
The Codex model is `gpt-5.6-luna`, requested through `codex app-server` with an ephemeral read-only thread, the plugin's own base instructions in place of the coding-agent harness, every optional Codex feature disabled, and the output schema attached.
The answer is validated against the schema before this plugin sees it.
The accepted provider output is exactly one JSON object with these fields:

```json
{"expected_reply":"","summary":"Retry policy cleanup","attention":"none"}
```

`summary` is normalized to one line and at most 30 characters, and the provider is instructed to write it in Korean.
`attention` accepts only `question` or `none` because approval and error states come from native hooks rather than model inference.

A request that fails for a reason that may clear on its own is retried by the provider layer with exponential backoff, at most four times; an answer the schema refuses is retried at most twice.
After that the turn is abandoned (`analysis_abandoned`) and never asked again until the user takes the next turn.
A usage limit, a missing login, or a provider that is not connected parks every pane for ten minutes instead: the turn is kept and asked again once the wait passes.
When more than one provider is connected, the configured priority is `codex`, then `claude`; a fallback happens only on a provider-side outage, login, usage-limit, or unsupported state, and is written to the log as `ai.fallback`.
A request that timed out is never re-run on another provider, because whether it completed is unknown.
Automatic summaries are enabled by default and the chosen setting survives watcher restarts.

## Privacy

The plugin reads the local JSONL session reported by Herdr for each supported pane.
The context spans the last two user turns, with an upper bound of 4,000 characters.
Two turns rather than one, because whether a closing message is a fresh question or a wrap-up of one already answered is often only visible in the preceding exchange.

Before anything is sent to the provider, the plugin:

- keeps only user and assistant prose;
- removes fenced code blocks;
- masks secret-looking values;
- masks email addresses;
- masks common absolute filesystem paths.

Raw prompts, raw model output, and credentials are not written to plugin logs.
Operational logs contain structured event names, pane IDs, agent kinds, request ids, provider names, stable failure classes, context length, and fingerprints rather than conversation text.
The Codex thread is ephemeral, so `~/.codex/sessions` receives nothing from these requests.

Local runtime data is stored under:

```text
~/.local/state/hide.agent-context-labels/
```

A state directory left by the plugin's previous id, `herdr-agent-context-labels`, is moved here once on the next start; if both exist, neither is touched and `state_migrated` reports `kept_both`.

The important files are:

| File | Contents |
| --- | --- |
| `display-state.json` | Last summaries, semantic attention, per-phase analyzed turns, and lifecycle timestamps. |
| `hook-state.json` | Pending native-hook interaction state. |
| `settings.json` | Automatic-summary preference. |
| `events.jsonl` | Structured operational events and failure classes. |

The structured event log is restricted to the current user, and the remaining state files contain no credentials or raw conversation text.
Logs retain at most 30 days, three files, and 30 MiB per file rotation cycle.

## Troubleshooting

Check that Herdr recognizes the agents and that the plugin has published tokens:

```bash
herdr agent list
```

Inspect plugin command history and local structured events:

```bash
herdr plugin log list --plugin hide.agent-context-labels
tail -n 50 ~/.local/state/hide.agent-context-labels/events.jsonl
```

Verify one sanitized provider call without changing pane metadata.
The binary is built by the workspace, so it lives under the repository's `target/`:

```bash
../../target/release/hide-agent-context-labels verify-provider --provider codex
```

It prints the provider's availability, then the verdict for a fixed two-line transcript, and exits non-zero when the provider cannot answer.
Every command, this one included, first moves a state directory left under the previous plugin id to the current id.
To verify a development build without touching the live watcher's state, give it its own home and point Codex at the real one: `HOME=$(mktemp -d) CODEX_HOME=~/.codex ../../target/release/hide-agent-context-labels verify-provider --provider codex`.
`--provider claude` prints `claude=unsupported` and exits non-zero; that is the documented state, not a fault.

Classify an arbitrary transcript to see what the model would decide, without touching any pane:

```bash
printf 'user: run the build\nassistant: The build finished. Shall I deploy?' \
  | ../../target/release/hide-agent-context-labels analyze-stdin
```

Common event codes include `ai_provider_availability`, `raw_session_unavailable`, `analysis_provider_unavailable`, `analysis_abandoned`, and the provider layer's `ai.attempt`, `ai.request.finished`, `ai.fallback`, and `ai.daily_rollup`.

| Symptom | Cause and fix |
| --- | --- |
| `ai_provider_availability` shows `codex=needs_login` | Run `codex login` and restart the watcher, or wait for the ten-minute recheck. |
| `ai_provider_availability` shows `codex=not_installed` | Put `codex` on one of the paths the startup script searches. |
| `raw_session_unavailable` | Herdr's reported session and the file on disk disagree. Send the pane one message to refresh it. |
| `analysis_provider_unavailable` with `usage_limited` | The Codex account is over its usage window. Every pane waits for the reset the provider reported, or ten minutes. |
| `analysis_abandoned` | The provider layer exhausted its retries for that turn; the detail carries the failure class. The next turn starts fresh, and the refresh action re-asks now. |
| A summary looks stale | Summaries refresh once per turn, so a pane mid-turn keeps the label it was given at the start. Use the refresh action to re-ask immediately. |
| Two watchers seem to run | They cannot; a file lock guarantees one. A `watcher_already_running` log line is normal. |

## Development

The plugin is a member of the Hide Cargo workspace and is built from the repository root; its scripts run the binary from the workspace's `target/release`.
The official Herdr plugin flow runs manifest build commands for GitHub installs, but `plugin link` does not build a local checkout.
Build first, then link the plugin directory:

```bash
git clone https://github.com/modakbul-gongbang/hide.git
cd hide
cargo build --release --locked -p agent-context-labels
herdr plugin link "$PWD/plugins/agent-context-labels"
```

Run the complete local checks from the repository root:

```bash
cargo fmt --all --check
cargo test --locked -p hide-ai -p agent-context-labels
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --release --locked -p agent-context-labels
```

The runtime smoke test must run inside a Herdr-managed pane:

```bash
./scripts/verify-runtime.sh
```

## References

- [Herdr plugin documentation](https://herdr.dev/docs/plugins/)
- [herdr-reviewr](https://github.com/persiyanov/herdr-reviewr), whose README is a useful example of a clear install-first Herdr plugin guide

## License

[MIT](LICENSE)
