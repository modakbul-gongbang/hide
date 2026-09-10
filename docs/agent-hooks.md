# Agent Hooks

How Hide learns what an agent has spawned inside its own process, and what it writes to the operator's machine to learn it.

Herdr reports panes.
An agent that spawns a subagent in-process creates no pane, so nothing about it reaches Herdr's wire, and a pane running six subagents is indistinguishable from one working alone.
This is the only part of the tree Hide cannot read from the session snapshot, and it is why anything is written to the operator's configuration at all.

## What is written, and where

`hide-agent-hooks/` is the only code path in the product that writes a file the operator owns outside the app's own state.

| Runtime | File |
| --- | --- |
| Claude Code | `~/.claude/settings.json` |
| Codex | `~/.codex/hooks.json` |

One entry is appended per registered event, and nothing else in the file is touched.
The write is atomic - a temporary file and a rename - and `serde_json`'s `preserve_order` is enabled for this crate so appending one hook does not rewrite the operator's whole file in alphabetical order.
Entries belonging to other tools are counted before and after, and a regression test asserts they survive.

Four events are registered, because those are the four both runtimes declare here: `SessionStart`, `SubagentStart`, `SubagentStop`, and `Stop`.
`SessionEnd` is not registered by either, so the `Stop` sweep is what closes a turn out.

Every entry carries `--source hide-subagents@<version>` inside its command.
That marker is the whole basis for judging what is installed: the source name proves the entry is Hide's, and the version after the `@` separates a current hook from an outdated one.
Nothing parses the rest of the command.

## What the hook reports back

The helper is stateless, as a hook script must be.
The count lives in `~/.hide/agent-hooks/panes/`, keyed by `$HERDR_PANE_ID`, and is republished after every event through `herdr pane report-metadata`, which Herdr defines as display-only pane metadata.
The core reads it back out of the pane tokens its ordinary snapshot already carries, so no new request, subscription or timer exists for any of this.

| Token | Meaning |
| --- | --- |
| `hide_hooks` | The hook version this session reported with. Its presence is the proof that the session is instrumented at all |
| `hide_sub_working` | Subagents currently running |
| `hide_sub_done` | Subagents this session has finished |

`hide_sub_blocked` is defined and never written: neither shipped adapter can observe a blocked subagent from the four available events, so the count stays unknown rather than being drawn as a zero.
A token that is not a count is dropped rather than coerced.

`SessionStart` resets the pane, so a new session in a reused pane never inherits the last one's numbers.
`Stop` sweeps `working` to zero, because the turn is over and a `SubagentStop` that never arrived cannot leave a count behind.
A pane Herdr has stopped listing has its record swept on the next session bootstrap.

The helper always exits zero and drains its standard input.
A hook that fails must never be what breaks the operator's agent.

## Judging what is installed

`hide_agent_hooks::diagnosis` resolves one reason, in a fixed order, and the first match wins:

1. `config_unreadable` - the file could not be read or parsed, so nothing was installed into it.
2. `remote_host` - the pane is on another machine. Hide does not write to another machine's file system.
3. `hooks_not_installed` - the runtime is here and carries no hook of Hide's.
4. `session_predates_install` - the hook is installed and this pane carries none of Hide's tokens, so the session was already running when it was installed. Restarting the agent instruments it.
5. `hook_outdated` - the session is reporting through an older hook than this Hide writes.
6. `unknown` - genuinely unknown, and said to be.

Both the pane's own mark and the Settings diagnosis read that one function, and every projection carries the reason's stable code alongside its sentence so no surface has to recognise its own operator-facing text.

## Installing

Hide installs once, on first run, and then leaves the operator's configuration alone.
It never silently reattaches on later launches; an operator who removes the hook has removed it.

Every later install is the `install_agent_hooks` event, sent from the Settings diagnosis after the operator said yes.
The request is a set and the write rewrites the same hook group either way, so approving twice is one install.
The write itself runs on the coordinator thread, outside every lock, and the diagnosis is read back from the file afterwards so the screen shows what the file now says rather than what was asked for.

The helper ships beside the app's own executable, in `Contents/MacOS/`, under the name `hide_agent_hooks::HELPER_BINARY_NAME`.
A build that did not bundle it fails the install with the path it looked at rather than writing a hook that cannot run.

`hide-agent-hooks doctor [--json]` prints the same judgement in a terminal, because a broken hook shows on screen only as an uninstrumented mark and the output of that command is the evidence.
Install and remove are deliberately not CLI subcommands: writing to the operator's configuration is the app's decision, taken with their approval, not something a stray command line performs.

## Testing

The operator's real `~/.claude/settings.json` and `~/.codex/hooks.json` are never a test target.
Every test in this crate builds its own `HOME` fixture and asserts against that.
