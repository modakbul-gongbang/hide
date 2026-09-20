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

Five events are registered: `SessionStart`, `UserPromptSubmit`, `SubagentStart`, `SubagentStop`, and `Stop`.
`SessionEnd` is not registered by either, so the `Stop` sweep is what closes a turn out.

Every entry carries `--runtime claude-code|codex` and `--source hide-subagents@<version>` inside its command.
The runtime argument selects that runtime's stdout envelope; the version-3 marker makes an installation without `UserPromptSubmit` outdated so Settings offers to refresh the stored commands.
That marker is the whole basis for judging what is installed: the source name proves the entry is Hide's, and the version after the `@` separates a current hook from an outdated one.
Nothing parses the rest of the command, and the helper does not pass the marker on: it is an install marker, not the metadata source (see below).

## What the hook returns and reports back

On `SessionStart`, the helper writes one runtime JSON envelope whose `hookSpecificOutput.additionalContext` combines the existing one-line worktree-purpose instruction with the bounded Project Memory capsule when Memory is enabled.
On `UserPromptSubmit`, it parses at most 256 KiB of runtime input, resolves the same durable Project identity as the app, and performs a read-only local lookup against the materialized active projection.
The prompt text, up to two recent human topics, and current checkout metadata are search inputs only; the original prompt is never replaced.
At most three whole Memory items and 600 estimated tokens are returned in the same `additionalContext` envelope, excluding items already provided by `SessionStart` for that session.
Claude Code and Codex currently accept the same envelope, but the installed runtime argument keeps that protocol choice explicit.
`SubagentStart`, `SubagentStop`, and `Stop` write nothing to stdout, preserving their existing silent behavior.
This stdout is advisory context for the agent and is independent of the best-effort metadata report described below.

The prompt path performs no provider or embedding call, transcript scan, child-process launch, or database write.
Missing, locked, corrupt, stale, over-limit, unresolved-Project, and over-deadline stores return no Memory context and still exit zero.
The local deadline is 100 ms and candidate, item, and token counts are hard bounded.
Current Claude Code and Codex `UserPromptSubmit` input and output shapes are fixed by sanitized fixtures in `hide-agent-hooks/tests/fixtures/`; a runtime whose installed shape is unsupported is diagnosed as update-required without disabling the other runtime or Sessions browsing.

The helper is stateless, as a hook script must be.
The count lives in `~/.hide/agent-hooks/panes/`, keyed by `$HERDR_PANE_ID`, and is republished after every event through the `pane.report_metadata` socket method, which Herdr defines as display-only pane metadata.
The request goes through `hide-herdr-client`, the same client the context-label plugin reports with, to the socket in `HERDR_SOCKET_PATH` or Herdr's default `~/.config/herdr/herdr.sock`, with a two-second timeout so a Herdr that does not answer costs the agent's turn that long and no more.
The metadata source is `hide-subagents`, the marker name without its version: Herdr limits a source to ASCII letters, digits, `:`, `.`, `_` and `-`, and refuses the marker's `@` with `invalid_metadata_source`.
The core reads the tokens back out of the pane tokens its ordinary snapshot already carries, so no subscription or poll exists for any of this.

The first release shelled out to `herdr pane report-metadata` with the pane id after the options and the versioned marker as the source.
The pinned CLI refuses both, the helper swallowed the exit status, and every pane on the machine read as `session_predates_install` with advice to restart that changed nothing.
Two things keep that from recurring: `report::tests` sends one report at a fake socket and asserts the exact request, and a report that fails is written down (next section) rather than dropped.

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

The helper always exits zero and drains its bounded standard input after writing applicable context.
A hook that fails must never be what breaks the operator's agent.

Exiting zero is not the same as saying nothing.
The outcome of every report is recorded in `~/.hide/agent-hooks/last-report-failure.json`: a failure writes the pane, the event, the socket and Herdr's answer, and the next success removes the file, so it describes the hook's current state rather than its history.
`Diagnosis` reads it back as `last_report_failure`, `doctor` prints it as a `Last report failed:` line, and the Settings group shows it as an error note above the restart advice, because with a refused report on record a restart is not the fix.
The Settings screen learns of it because the coordinator re-reads the diagnosis once a second while the Settings agents tab is on screen (`settings_observed`, the same flag the Background AI group sets), and reads nothing while it is not.

## Judging what is installed

`hide_agent_hooks::diagnosis` resolves one reason, in a fixed order, and the first match wins:

1. `config_unreadable` - the file could not be read or parsed, so nothing was installed into it.
2. `remote_host` - the pane is on another machine. Hide does not write to another machine's file system.
3. `hooks_not_installed` - the runtime is here and carries no hook of Hide's.
4. `session_predates_install` - the hook is installed and this pane carries none of Hide's tokens, so the session was already running when it was installed. Restarting the agent instruments it. A pane whose reports Herdr refuses lands here too; `last_report_failure` is what tells the two apart.
5. `hook_outdated` - the session is reporting through an older hook than this Hide writes.
6. `unknown` - genuinely unknown, and said to be.

Both the pane's own mark and the Settings diagnosis read that one function, and every projection carries the reason's stable code alongside its sentence so no surface has to recognise its own operator-facing text.

## Installing

Hide installs once, on first run, and then leaves the operator's configuration alone.
The claim is a marker file in `~/.hide/agent-hooks/`, created exclusively so two launches racing each other still install once.
It deliberately does not live in the rendered UI state, which the shell echoes back and could reset, and it does not depend on any hook file: an operator who removes a hook has removed it, and the next launch does not quietly put it back.
Only a runtime that is present and carries no hook of Hide's is installed into; an outdated hook is left for the Settings diagnosis to ask about, and a configuration file that could not be read is not written to on a guess.

Every later install is the `install_agent_hooks` event, sent from the Settings diagnosis after the operator said yes.
The request is a set and the write rewrites the same hook group either way, so approving twice is one install.
The write itself runs on the coordinator thread, outside every lock, and the diagnosis is read back from the file afterwards so the screen shows what the file now says rather than what was asked for.

The helper ships beside the app's own executable, in `Contents/MacOS/`, under the name `hide_agent_hooks::HELPER_BINARY_NAME`.
`hide_agent_hooks::helper_for` is the only thing that resolves it, and it checks the layout rather than the file name: the executable's parent must be `MacOS`, its parent `Contents`, and its parent must end in `.app`.
Anything else is refused as `HelperNotBundled`, and a bundle that shipped without the helper is refused as `HelperMissing`.
A refused install writes nothing and reports why; an install the operator pressed for also raises `agent_hooks.install_refused` so the press is answered on screen.

The refusal exists because a hook command outlives the process that wrote it.
It is a path stored in the operator's own configuration file and run by every future session of that agent, so the only path worth writing is one that survives a rebuild.
On 2026-09-10 a development build resolved the helper beside its own executable under `target/debug/deps`, which Cargo deletes on the next build, and wrote that path into both `~/.claude/settings.json` and `~/.codex/hooks.json`.
Every Claude and Codex session on the machine then failed four hooks per turn with `No such file or directory` until the entries were taken out by hand.
Resolving "beside the executable" was the defect; the bundle layout is the whole answer, and `a_cargo_build_directory_is_refused_instead_of_written_into_a_hook` is the test that keeps it.

Removal does not need the helper, and must not: it reads the configuration file and takes out the entries carrying Hide's marker.
So a runtime whose helper has gone missing is offered a removal as well as a reinstall, which is how the operator clears entries that name a binary that is no longer there.
The other read failures are not offered a removal, because Hide could not parse the file and does not know what removing would touch.

`hide-agent-hooks doctor [--json]` prints the same judgement in a terminal, because a broken hook shows on screen only as an uninstrumented mark and the output of that command is the evidence.
Install and remove are deliberately not CLI subcommands: writing to the operator's configuration is the app's decision, taken with their approval, not something a stray command line performs.

## Testing

The operator's real `~/.claude/settings.json` and `~/.codex/hooks.json` are never a test target.
Every test in this crate builds its own `HOME` fixture and asserts against that.
