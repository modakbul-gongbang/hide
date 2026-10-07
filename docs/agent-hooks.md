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

On Windows `~` is the account's profile folder (`%USERPROFILE%`), where both runtimes keep these files.

One entry is appended per registered event, and nothing else in the file is touched.
The write is atomic - a temporary file beside the target, created private to the account (0600 on macOS and Linux, an access list naming only the account on Windows), that replaces the target in one step (`hide_platform::fs::atomic::write_file`) - and `serde_json`'s `preserve_order` is enabled for this crate so appending one hook does not rewrite the operator's whole file in alphabetical order.
The file keeps its mode, a new one is created 0600, and a file that is a symlink stays one: the write lands at the file the link resolves to, so a settings file kept in a dotfiles repository is edited there.
Entries belonging to other tools are counted before and after, and a regression test asserts they survive.

Six events are registered: `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `SubagentStart`, `SubagentStop`, and `Stop`.
`SessionEnd` is not registered by either, so the `Stop` sweep is what closes a turn out.
`PreToolUse` is the one entry with a matcher: it selects only the `Bash` tool in both runtimes (`install::hook_matcher`, the one function the writer and Codex's trust check both read), so a file edit or a search never starts the helper.
An install made before `PreToolUse` existed has the other five entries and reads Outdated until the kit's next pass, which adds the missing entry; the marker stays `hide-subagents@6`.

Every entry carries `--runtime claude-code|codex` and `--source hide-subagents@<version>` inside its command (in Claude Code's Windows entry, inside its `args`).
The runtime argument selects that runtime's stdout envelope; the version-6 marker makes an installation whose command is not guarded against a missing helper outdated, so the next launch or connection replaces it.
That marker is the whole basis for judging what is installed: the source name proves the entry is Hide's, and the version after the `@` separates a current hook from an outdated one.
Nothing else is read from the command but the helper's quoted path, and the helper does not pass the marker on: it is an install marker, not the metadata source (see below).

## What the hook returns and reports back

On `SessionStart`, the helper writes one runtime JSON envelope whose `hookSpecificOutput.additionalContext` combines the worktree-purpose instruction, which also tells the agent to run `hide factory add` to put work into a Factory instead of adding a GitHub label (see [factory.md](factory.md)), with the bounded Project Memory capsule when Memory is enabled.
The same envelope adds Workspace commands only after `hide workspace bootstrap` and `hide workspace info` confirm a renderer-connected Workspace for the caller and report its actual capabilities.
The probe does not need a pane id: the daemon binds a caller inside a Herdr pane to that pane, and any other local caller, such as a tool shell or hook inside Codex's shared app-server daemon or a plain terminal, to the registered checkout holding its cwd (`docs/ARCHITECTURE.md`, the Workspace CLI).
The guidance names that checkout path, so a session can read which Workspace its commands reach.
The helper uses the `hide` CLI beside it in the kit folder, or the CLI on `PATH` when none is there; on a device that is the helper root's current build, whose `hide` reaches this Mac's daemon through the device's return route (PRD device-parity B15).
The hook starts nothing and installs nothing; when the CLI does not answer within its two-second bound the session starts without Workspace guidance and the next one tries again (B26).
A device session gets no Project Memory capsule: the Memory database is this Mac's and is never copied to a device, so the device's `UserPromptSubmit` omits Memory context (B25).
A connected device pane can still receive pending letters through its sibling `hide` CLI and the return route within the same two-second caller budget; [delivery.md](delivery.md#safe-intake-and-manual-fallback) owns that intake and confirmation contract.
An ordinary Claude Code or Codex session started in a connected Herdr pane receives the same conditional guidance as a Hide-managed session when Hide's hook is installed in that runtime's configuration on that machine.
A disconnected pane, a cwd outside every registered checkout, an unavailable renderer, or an unsupported Browser surface receives no Workspace capability claim.
The guidance scopes every command to the caller's checkout Workspace and lists only capabilities returned by the daemon; `hide workspace info` remains the live check and `hide --help` gives the full syntax.
With `browser.open` it also lists the `hide browser` page commands in one line and points at `hide browser help`, their agent guide ([BROWSER_DISPLAYS.md](BROWSER_DISPLAYS.md#agent-page-commands)).
The hook creates an owner-only, finite-lived credential reference and includes its path as a shell environment prefix for the listed commands.
The path is a credential reference and should be handled as private session context, even though it contains no bearer bytes itself.
The credential's bearer bytes and file contents never enter hook stdout, arguments, or the agent context, and each command rechecks the caller's checkout membership and renderer availability.
The prefix is a convenience, not the only way in: a bare `hide` command bootstraps its own one-shot credential, and a caller the daemon cannot place in a pane is bound to the registered checkout holding its working directory (`docs/ARCHITECTURE.md`, hided and the WebSocket boundary).
That covers Codex 0.157 with `daemon_auto_start`, where the tool shell and this hook both run inside the shared `codex app-server` daemon under launchd rather than in the pane: the hook may run with the daemon's environment instead of the pane's, so its Workspace guidance can be missing, and the bare commands still reach the checkout the tool shell runs in.
Every Codex Hide starts passes `--no-daemon`, so a Codex Hide starts runs its hooks in its pane.
The kit no longer turns the daemon off on its own: it only reads whether the machine's Codex has the setting, for that flag, and a machine where an earlier Hide turned it off keeps it off.
`src/codex_daemon.rs` is the one place that changes the setting, through `codex features disable daemon_auto_start`, and only for the operator's own request from a not connected Codex pane's popover (`docs/ARCHITECTURE.md`, The install kit; `docs/status-model.md`, Not connected, and what fixes it).
The kit reads the setting with the capability (`KitReport.codex_daemon_on`), which is what tells a Codex session on the shared server from one that started before the hook.
The SessionStart command hook has an eight-second timeout, including two bounded two-second CLI probes; a failed probe leaves the existing purpose and Memory context intact.
An issued credential remains unclaimed for at most 30 seconds until a CLI receives and acknowledges a Workspace response.
The CLI writes the claimed marker only after the daemon acknowledges that claim, so a caller killed before acknowledgement leaves an unclaimed reference that expires.
A repeated SessionStart in the same attested pane reuses its live persistent reference, including when an earlier hook stopped after claiming but before delivering context.
The SSH bridge tracks every reference it issues and revokes the corresponding daemon token when its file disappears, its unclaimed period expires, its eight-hour lifetime ends, its pane shell exits, or its one-shot caller exits.
A failed bootstrap reply revokes only a newly issued reference; it leaves a reused live reference intact.
Late bridge replies are matched by request ID so one timed-out attestation cannot poison the next bootstrap.
On `UserPromptSubmit`, it parses at most 256 KiB of runtime input, resolves the same durable Project identity as the app, and performs a read-only local lookup against the materialized active projection.
The prompt text, up to two recent human topics, and current checkout metadata are search inputs only; the original prompt is never replaced.
An item is eligible only after a lexical match, a bounded two- or three-character literal match, or meaningful path overlap below the Project root; extraction confidence never stands in for semantic similarity and only breaks ties after relevance.
At most three whole Memory items and 600 estimated tokens are returned in the same `additionalContext` envelope, excluding items already provided by `SessionStart` for that session.
The core records the authoritative SessionStart receipt in the app-owned SQLite store after it observes the injected envelope, including an internal zero-item receipt when the session begins before any Memory exists.
The helper authenticates that receipt with the Project-scoped key in the same SQLite store, binding the runtime, session, hook event, and exact ordered item revisions without creating another persistence surface.
The core accepts it only from provider-owned transcript metadata, including actual Codex developer messages, and verifies the authentication tag before recording or hiding the marker.
That zero-item receipt does not present a misleading `Project Memory ready 0` message; it only lets later prompts distinguish an observed empty start from a projection race.
If the first prompt races that projection, the helper omits Memory for that prompt rather than guessing which items were delivered; the next prompt retries the read-only lookup after the receipt exists, and no sidecar or second store is written.
If multiple observed SessionStart envelopes name different item sets, the exclusion read returns their deterministic union so retries converge instead of selecting an arbitrary receipt.
For a linked worktree, the helper maps the actual cwd relative to that checkout root back into the durable main-worktree namespace before path ranking, so the worktree identity folds while `crates/foo` relevance remains intact.
Claude Code and Codex currently accept the same envelope, but the installed runtime argument keeps that protocol choice explicit.
`SubagentStart`, `SubagentStop`, and `Stop` write nothing to stdout, preserving their existing silent behavior.
This stdout is advisory context for the agent and is independent of the best-effort metadata report described below.

For local and connected device agent delivery, `UserPromptSubmit` reads the submitted prompt from the runtime payload (waiting at most 0.5 seconds for it) to learn whether it is Hide's own bell and which native session it runs in.
The session id goes to `hide inbox --hook --session`, which is how the hook counts as a submission only in its own pane; an unreadable or truncated payload has none, so that hook clears no draft.
The bell turn pulls at most five pending letters and 8 KiB of letter context from the sibling `hide` CLI within one total two-second budget; any other prompt receives only a one-line count of waiting letters.
Only a successful stdout flush permits confirmation, so pre-confirm interruption can repeat an ID and confirmed letters do not repeat.
The doorbell itself carries manual `hide inbox` guidance; a missing or failed hook leaves pending letters available through `hide inbox` and `hide request show`.
This delivery path has its own bounded private diagnostics and does not extend Memory's in-process budget described below.
[delivery.md](delivery.md#safe-intake-and-manual-fallback) owns these intake, failure and confirmation rules.

The Memory lookup itself performs no provider or embedding call, transcript scan, child-process launch, or database write.
Missing, locked, corrupt, stale, over-limit, unresolved-Project, and over-deadline stores return no Memory context and still exit zero.
The caller-visible deadline is 100 ms from process launch, including stdin collection and SQLite work, and candidate, item, and token counts are hard bounded.
The helper gives its in-process work 75 ms so process startup, scheduling, stdout flush, and teardown stay inside that caller-visible limit.
Stdin is read through a nonblocking descriptor until EOF, the size cap, or the absolute deadline, and SQLite receives the same deadline through its progress handler.
Windows has no nonblocking read of a pipe, so there a reader thread collects stdin and the helper stops waiting for it at the same deadline.
Current Claude Code and Codex `UserPromptSubmit` input and output shapes are fixed by sanitized fixtures in `hide-agent-hooks/tests/fixtures/`.
The app probes the installed runtime binaries with a 750 ms bounded version check and currently requires Claude Code 2.1.278 or Codex 0.155.1 for Memory injection.
A runtime below that capability is diagnosed as `Update required` without disabling a supported installed runtime or Sessions browsing.

The helper is stateless, as a hook script must be.
The count lives in `~/.hide/agent-hooks/panes/`, keyed by `$HERDR_PANE_ID`, and is republished after every event through the `pane.report_metadata` socket method, which Herdr defines as display-only pane metadata.
The request goes through `hide-herdr-client`, the same client the core's Herdr connections use, to the socket in `HERDR_SOCKET_PATH` or Herdr's default (`$XDG_CONFIG_HOME/herdr/herdr.sock`, otherwise `~/.config/herdr/herdr.sock`), with a two-second timeout so a Herdr that does not answer costs the agent's turn that long and no more.
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

The helper always exits zero and reads no more than its bounded standard-input prefix before writing applicable context.
A producer that never closes stdin is released at the same absolute Memory deadline.
A hook that fails must never be what breaks the operator's agent.

Exiting zero is not the same as saying nothing.
The outcome of every report is recorded in `~/.hide/agent-hooks/last-report-failure.json`: a failure writes the pane, the event, the socket and Herdr's answer, and the next success removes the file, so it describes the hook's current state rather than its history.
`Diagnosis` reads it back as `last_report_failure`, `doctor` prints it as a `Last report failed:` line, and the Settings group shows it as an error note above the restart advice, because with a refused report on record a restart is not the fix.
The Settings screen learns of it because the coordinator re-reads the diagnosis once a second while the Settings agents tab is on screen (`settings_observed`, the same flag the Hide AI tab sets), and reads nothing while it is not.

## The spawn guard

`PreToolUse` carries the spawn guard (PRD herdr-spawn-guard): a shell call that starts an agent through Herdr is refused before it runs, because Herdr records no parent for such a child, so it would show in the Agents graph with no line to the agent that started it and no watch.
The reason handed back is the filled command to use instead, `hide agent spawn --parent here --name <name> --intent <intent> --kind <kind> --repo <main root> --branch <branch>`, so the agent redoes the call in one step.
What is refused, in a pane of a registered checkout (the call is `herdr`, or `$HERDR_BIN_PATH`, after any `VAR=value` words and the prefix words `time`, `exec`, `command`, `nohup`, `env` and, at the start of a command in a shell line, `if`, `then`, `elif`, `else`, `while`, `until`, `do`, `{` and `!`):

- `herdr agent start <name> --kind <kind> ...`;
- `herdr pane run <pane> <command>...` and `herdr pane send-text <pane> <text>`, when the command's first word is one of Herdr's agent kinds (the 24 the pinned Herdr lists in `herdr agent start --help`, pinned by a unit test; `cursor-agent` is read as `cursor`).

Everything else runs untouched, with no output: other `herdr` calls (`pane split`, `agent list`), `--help`, a start with no `--kind`, a call that does not mention `herdr`, any call outside a Herdr pane or a registered checkout, and a call that names another Herdr: `--session`, `--machine`, or a `HERDR_SOCKET_PATH` or `HERDR_SESSION` assignment that differs from the pane's own, whether inline before the call, through `env`, by an earlier `export`, `declare`, `typeset` or bare assignment in the same call, or by an `unset`, because the `hide agent spawn` it would be sent to starts the child in the pane's own Herdr, and an isolated Herdr server of a verification run is reached that way; a `source` of a script that sets them cannot be read, so that form is still refused, and the exact text is compared, so `HERDR_SOCKET_PATH=$HERDR_SOCKET_PATH` counts as another Herdr.
The guard reads the whole call as a shell would read it, one top-level simple command at a time (`&&`, `||`, `;`, `|`, `&`, newline), with quotes, backslashes, heredoc bodies and comments set aside, so a commit message or a heredoc that quotes `herdr agent start` is not a launch.
A heredoc inside `$(...)` is read as text to its delimiter line, so a quote or a parenthesis in its body does not count, and text that still ends inside a quote, a backtick or a `$(` is a syntax error a shell would not run, so it holds no launch.
A `send-text` or `pane run` whose first word is an agent's name is refused whatever the words after it say, so a plain message that starts with `claude` or `pi` is a known false positive of that rule, and `herdr agent prompt` is the call that sends text to a running agent.
A launch chained behind another command (`cd x && herdr agent start ...`) is refused as a whole and nothing in the chain runs, since the hook sees the tool call and not one command of it; the reason says to run the other commands separately.
A launch inside `bash -c`, `$(...)`, a script or an alias is not found (PRD non-goal).

The registered-checkout test is `hide workspace bootstrap` through the sibling `hide`, the same daemon-owned call `SessionStart` makes, needing no renderer.
Only the daemon's own "this caller is not in a Hide checkout" answers (`checkout_not_registered`, `caller_unavailable`, `pane_not_connected`, `pane_unavailable`, `pane_changed`, `caller_not_in_pane`) mean the call is not Hide's to guide, and it runs silently.
Any other outcome, a `hide` that is missing, a bridge to a device that is gone, an answer that does not come within the guard's 2.5 second budget, an unreadable answer and a reason this build does not know, lets the call run and appends one `daemon.unreachable` line with its cause to the guard's log, throttled to one per ten minutes through the delivery diagnostics' store (cause `guard`), because a refusal Hide cannot explain would be worse than the untracked child.
The line goes to the log only: a hook's standard error reaches nobody the operator or the agent could act on.
A pane of a Herdr server Hide does not attach to, whose working directory is inside a registered checkout, is bound by `hide workspace bootstrap` to that checkout and so reads as registered too; the guard is guidance, not a boundary, and the `hide agent spawn --parent here` it offers cannot parent a child of such a pane (known limitation, PRD D-03 reads the pane and the checkout, not whose Herdr it is).
`--repo` is the repository's main root and `--branch` the caller's current branch, both read from the call's `cwd` through the repository's own files (`hide_project::git`), with no git process; a detached HEAD leaves `<branch>` for the agent to fill.
The guard stays silent inside a Grok, OpenCode or Cursor session (`ForeignOrigin`), where Claude Code's hook runs but its deny handling is unverified.
Claude Code and Codex share one deny envelope, `hookSpecificOutput.permissionDecision = "deny"` with `permissionDecisionReason`; every other path prints nothing and exits 0, and the whole entry runs inside `catch_unwind`, so a defect allows the call rather than refusing an ordinary one; a failed owner handshake in the environment ends the outer hook with 0 as well, since exit 2 from a pre-tool hook would refuse the call.

Each refusal appends one JSON line (pane id, agent kind, shape, runtime; never the command text, and the same file holds the `daemon.unreachable` lines) to `~/.hide/agent-hooks/spawn-guard.log` (private, capped at 256 KiB with one rotation) and echoes it on standard error, where a hook run by hand shows it.
The per-call cost is what an ordinary shell call pays: the payload is read within 0.5 seconds and a byte test for `herdr` over the whole payload runs before anything is parsed or spawned, so a call whose payload does not mention `herdr` (its working directory and transcript path included, which a checkout named `herdr-ide` does, and then the call pays one JSON parse and the lexer as well) costs one process start (numbers in [PERFORMANCE_TESTING.md](PERFORMANCE_TESTING.md#the-spawn-guard-hook-on-a-shell-call)).
`hide-agent-hooks/tests/spawn_guard.rs` runs the helper beside a stand-in `hide` for each outcome above, and `src/spawn_guard.rs` holds the parser's table tests.

## Other agents: skill and guidance hook

On a machine where the kit has never run (no `~/.hide/kit/installed.json`), the default-on agents (Claude Code and Codex) are recorded off in the same pass and the record is marked `awaiting_choice`, so nothing is written to any agent until the operator answers the first-run agent choice.
The hold records both default-on agents off explicitly, whether or not they are installed (operator decision D7): an agent installed later appears in Settings, Agents off and is never turned on by a pass, and the operator switches it on there.
An explicit agent choice in the same pass wins over the hold, and any explicit choice (a scope that names an agent, which the first-run answer always does, naming the unchosen default agents off) clears the mark; an existing install, a record without the field, is never held.
The mark lives in the same record as the choices, so it outlasts a quit between the hold and the answer, and `held_for_onboarding` in the report reads it on every pass.
The core follows it (`ui_state.agent_onboarding`), applies the answer to this Mac and to every device whose own record waits, and sends the saved choice once per run to a device that reports waiting later; one that still waits afterwards is logged, not asked again on every report.

Claude Code and Codex are the agents the kit has always had a hook for.
Hide supports seven agents: Claude Code, Codex, Gemini CLI, Grok, OpenCode, Pi and Cursor, in that order.
Every agent is one row of `hide-kit/src/agents.rs` (`ADAPTERS`), and one switch per agent per machine turns its pieces on and off, in Settings, Agents and in each device's row.
A row carries the agent's program names (`executables`), the folder it reads skills from and the systems the documentation confirms that folder on, whether Hide writes a hook for it, Herdr's integration name for it (none for Gemini CLI), and the official page the row's answers come from (`doc_url`).
A test fails a row with no `https` `doc_url`, and a row with no program, so a claim in the table below always has a page behind it and every agent can be found.

Two pieces are written per agent into the agent's own files, and nothing else; a third, Herdr's integration, is put in through Herdr's own CLI (below):

- The skill stub `hide-browser/SKILL.md` in the folder the agent reads.
  It is a few lines that point at `hide browser help`, so it stays right as the CLI's guide changes.
  Its folder is `~/.agents/skills` for the agents that read it, `~/.claude/skills` for Claude Code (which documents that it does not read the shared folder), and no agent has a folder of its own.
  A shared folder is written while any agent that reads it is on and installed, and it is removed only when none is.
  A file is Hide's only when its marker line, `<!-- hide-skill@<version>: ... -->`, is the first line after the front matter; a file that merely mentions `hide-skill@` is never replaced or removed, and the agent's row says a skill that Hide did not write is already there.
  A stub with Hide's marker over text that is not Hide's was edited by the operator: no pass rewrites it and a switch-off leaves it, the row reads Outdated with that reason, and only Reinstall puts Hide's text back.
  A stub of an older marker version is Hide's own and is replaced by the next pass.
  An agent's own folder (`~/.claude`) is never created for the stub: the hook code reads that folder as the agent's settings being there, so a pass that made it would write a hook on the next pass that it did not write on this one, and a second apply would not be a no-op; the row says the agent has not created its folder yet, and the stub goes in on the pass after it has.
- The guidance hook, for the agents below marked as done.
  It is one `SessionStart` entry, in the agent's own format, whose command is `hide-agent-hooks hook --runtime <agent id> --event SessionStart`.
  `hide-agent-hooks` writes it (`src/guidance.rs`) and nothing else does, under the marker `hide-guidance@1` that proves an entry is Hide's and separates a current one from an older one.
  Its output is the worktree-purpose instruction, including the sentence that points at `hide factory add`, one fixed line that points at `hide browser help`, and the live Workspace guidance when the daemon answers, in the field the agent documents.
  It prints no Memory capsule, because the Memory receipt is read from Claude and Codex transcripts, and it keeps no counters and writes no file.
  A second delivery of the same session therefore changes nothing: the output is a pure function of the daemon's answer, and a test runs the hook twice and compares.

Gemini CLI and Cursor each keep their entry the way their documentation shapes it.
Gemini takes an entry in `~/.gemini/settings.json`.
A removal takes Hide's hook out of whatever group holds it and drops the group only when no hook is left, so another tool's hook that shares a group with Hide's (`{"matcher": "*", "hooks": [Hide's, theirs]}`) stays.
Cursor takes `~/.cursor/hooks.json` (`{"version": 1, "hooks": {"sessionStart": [{"command", "timeout"}]}}`, seconds), a file it shares with the operator's own hooks ([hooks](https://cursor.com/docs/hooks)).
Cursor's documentation requires `version` (a positive integer, `1`), so Hide creates the file with it and adds `"version": 1` to an existing file only when it has none, which keeps the operator's own hooks beside Hide's valid; a `version` the operator wrote is left as it is.
The documentation calls `command` a "script path or command" and does not say whether a shell parses it, so Hide writes the one form that means the same either way: the helper's absolute path and its arguments, with no `if`, `exec` or quoting (a path with a character a shell would read keeps the guarded, quoted form).
A removed helper then fails the hook instead of being skipped, which costs nothing for a fire-and-forget `sessionStart`, and the Settings row reads the gone helper as Failed.
Hide creates the file when it is missing and deletes it again only when nothing but Hide's scaffolding is left.
A settings file Hide created and nothing else is in goes with Hide's hook, as for every agent that shares a settings file; one that holds another key keeps it.
Cursor also loads Claude Code's hooks from `~/.claude/settings.json` (and the project's `.claude/settings*.json`) when "Include Third-Party Plugins, Skills, and Other Configs" is on, which is its default, and merges them with its own at the lowest priority without removing a duplicate ([third-party hooks](https://cursor.com/docs/reference/third-party-hooks)).
So on a machine where Claude Code is on in Hide a Cursor session would also run Hide's instrumented Claude Code hook (the pane counters and reports as `claude-code`, Memory), which is not what a Cursor session is.
Hide keeps the Cursor guidance hook and makes the Claude Code hook stay out: `hide-agent-hooks hook --runtime claude-code` prints nothing and counts nothing when `CURSOR_VERSION` is in its environment, the variable Cursor documents as set for every hook it runs.
Cursor's page on third-party hooks does not say whether those get the variable, so this rests on the documented one; a session where it is missing would run both hooks, which print different fields and count only for `claude-code`.
Grok and OpenCode can run the hooks in `~/.claude/settings.json` too, and Hide writes no hook for either, so Claude Code's hook still speaks there: its pane counters, its Memory and its guidance are what they are anywhere else.
What it does not do inside them is take or confirm letters: a letter is addressed to the pane's own session and is confirmed once that session has seen it, so a hook that runs inside another agent's session would take the letter and confirm it to nobody who reads it (PRD settings-cleanup D-25).
`hide_agent_hooks::runtime::ForeignOrigin` finds such a session by what its agent sets for the processes it starts: `CURSOR_VERSION` for Cursor, `OPENCODE` or `OPENCODE_PID` for OpenCode, and `GROK_HOOK_EVENT` or `GROK_SESSION_ID` for Grok.
`hide-agent-hooks/tests/letter_origin.rs` runs the built helper beside a stand-in `hide` that answers `inbox` with one letter and records its calls, once outside and once inside each of them.
The guidance hook is not written on Windows, because its command is a shell command and Gemini CLI's documentation names no Windows form.

The record `~/.hide/kit/installed.json` keeps the operator's choice per agent (`agents`) and the pieces Hide installed (`hook:<agent>`, `skill:<folder>`, `herdr:<agent>`), and an older build ignores them.
With no choice on record Claude Code and Codex are on, as they have been since their hooks became part of the kit, and every other agent is off.
A piece that was installed and is gone stays gone until Reinstall, and an agent switched off keeps nothing of Hide's and gets nothing back from a later pass.
Whether the agent is installed only decides whether a switch can work: an agent that is not installed on a machine has no switch there, and switching it on is not recorded.
A guidance hook also needs the agent's own settings folder, which Hide does not create: an installed agent that has not made it yet reads that, and its hook goes in on the pass after it has.

### Installed means the program is found

An agent is installed on a machine when one of its programs is found there, the way the operator's terminal would find it; a folder the agent creates does not count, because an editor makes `~/.cursor` without the `cursor-agent` CLI and a CLI that was removed leaves its folder behind (`~/.pi/agent` without `pi`).
The program names are the ones each vendor's install documentation and install script give the command: `claude`, `codex`, `gemini`, `grok`, `opencode`, `pi` and `cursor-agent`.
Cursor's installer now calls its command `agent` and keeps `cursor-agent` as a second name; Hide looks for `cursor-agent` only, because Grok's installer also puts an `agent` on the `PATH`.
The program names live in the kit's adapter table alone; Hide AI keeps no list of them, and an agent it cannot use yet is listed as not installed from what the kit found (`AiRequest.cli_found`).

The search is, in order, the folders the account's login shell puts on its `PATH`, the daemon's own `PATH`, and the usual install folders: `~/.local/bin`, pnpm's global folder (`~/Library/pnpm` on macOS and `~/.local/share/pnpm` on Linux, and the `bin` folder inside it from pnpm 11), `~/.npm-global/bin`, `/opt/homebrew/bin` and `/usr/local/bin` (`hide_platform::programs`, the one search; Hide AI's backends find their CLIs with it too, so the kit's "installed" and Hide AI's "can be asked" cannot disagree about a program only the shell's `PATH` or an install folder reaches).
The login shell is asked because many installers put their own folder on the `PATH` by editing a startup file rather than using one of those folders: Grok's `~/.grok/bin`, OpenCode's `~/.opencode/bin` and Pi's `~/.pi/agent/bin` are written into `~/.zshrc`, `~/.bashrc` or `config.fish`, and a Node CLI installed under nvm lives in nvm's folder.
No daemon sees those folders on its own: an app opened from the Dock gets the system folders and the usual install folders from the desktop host, and a device helper started over an SSH exec channel gets the system folders alone (a non-interactive zsh reads only `~/.zshenv`).
`hide_platform::host::login_shell_path` runs `$SHELL -ilc` with the account's login variables and the home being searched, and reads the `PATH` it prints between two marks, past anything the startup files print; it gives the shell ten seconds, the same as the desktop host's own ask, and ends it and everything it started after that or when Hide quits.
The kit asks, on the kit worker on this Mac and in the helper on a device, and Hide AI's backends ask through the same cached answer on their own worker threads (the Settings reader, the label analyzer and Memory's analysis, never under the runtime lock); the hook diagnosis, Memory's version probe and the read of Codex's daemon setting keep the search without the shell, so nothing else that reads `cli_path` starts a shell.
The answer is kept until one of the shell's startup files changes (zsh's, bash's and sh's in the home and in `~/.config/zsh`, fish's, and the system's in `/etc`), which is how an installer adds a folder, so Settings re-reading the kit every few seconds starts no shell; a file those files read in turn is not watched, and is read again when `hided` or a device's helper next starts.
A shell that does not answer is logged as `platform.login_shell_unread` with its error kind, asked again after a minute, and the search goes on without its folders meanwhile; Windows has no login shell, and its search is the account's own `Path` plus the usual folders.
A program found by the search is also run with it, because a CLI installed as a script starts its interpreter by name (pnpm's `codex` runs `node`).
The Memory version probe does, and so does every child Hide AI starts: a model-list or sign-in probe as much as a request, and the Codex app-server, each given the `PATH` its program was found on (`hide-ai/src/program.rs`, one `Program` type that every backend resolves through and `runner::run` and the Codex session apply).
A CLI named by file, which only a test does, keeps the process's own `PATH` unless the backend's `search_path` is set.

### When the program is gone

An agent that is on and whose program is no longer found (uninstalled, or moved where the search does not reach) keeps everything: the operator's choice stays on record, the stub and the hook Hide wrote stay where they are, and no pass installs, replaces or removes anything for it.
Taking them out on a guess would remove what the operator may want back, and an unused stub or a guarded hook does nothing; this is the kit's rule that only the operator takes a piece away (D-20, D-26).
Its row stays under Installed in Settings with its switch only when the record holds the operator's own choice for it (`AgentReport.chosen`, `KitAgentSnapshot.chosen`); Claude Code and Codex are on by default with no recorded choice, so one of them with no program is listed under Not installed with no switch, and never reads Ready.
A kept row reads `Not on this machine` with the reason that its program is not found, and offers no Reinstall, since Reinstall cannot bring a CLI back; switching it off takes out Hide's pieces as for any agent, and once the program is found again the agent is whole with nothing asked.
Claude Code's and Codex's hook parts follow the kit-part rule instead: they are written while the agent is on and its folder (`~/.claude`, `~/.codex`) is there, found or not, because a CLI the search misses still runs the hook.

### Support table

Hook "done" means Hide writes the guidance hook; "none" rows have no command hook that Hide can write to put text into a session's context, and the row says why and where the documentation says so.
Every row gets the skill stub where the system column says so.

| Agent | Skill folder (systems) | Hook | Herdr integration | Why, and the page that says so |
| --- | --- | --- | --- | --- |
| Claude Code | `~/.claude/skills` (all) | done: six-event hook, a kit part | `claude` | [skills](https://code.claude.com/docs/en/skills) |
| Codex | `~/.agents/skills` (macOS, Linux) | done: six-event hook, a kit part | `codex` | [skills](https://learn.chatgpt.com/docs/build-skills) |
| Gemini CLI | `~/.agents/skills` (macOS, Linux) | done: guidance `SessionStart` | none: the pinned Herdr lists no Gemini CLI target, so its state is read from its screen | [skills](https://geminicli.com/docs/cli/skills/), hooks at geminicli.com/docs/hooks |
| Grok | `~/.agents/skills` (macOS, Linux) | none: `SessionStart` cannot add context, only tool-call events can | `grok` | [skills](https://docs.x.ai/build/features/skills-plugins-marketplaces) |
| OpenCode | `~/.agents/skills` (macOS, Linux) | none: its documentation gives no command hook, only JS plugins (<https://opencode.ai/docs/plugins/>), and the one plugin hook that adds context is `experimental.session.compacting`, which fires at compaction and is marked experimental (<https://opencode.ai/docs/config/>); `instructions` takes a file, glob or URL (<https://opencode.ai/docs/rules/>) and cannot run a command, so it could only carry static text and would mean editing the operator's `opencode.json`. A machine whose OpenCode already runs `~/.claude/settings.json` hooks through a bridge plugin gets Claude Code's hook output without Hide writing anything | `opencode` | [skills](https://opencode.ai/docs/skills/) |
| Pi | `~/.agents/skills` (all) | none: TS extensions, no command hooks | `pi` | [skills](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/skills.md) |
| Cursor | `~/.agents/skills` (macOS, Linux) | done: guidance `sessionStart` in `~/.cursor/hooks.json`, returning `additional_context` ([hooks](https://cursor.com/docs/hooks)); the hooks page does not mention the CLI, and its changelog says the CLI runs session-start hooks (<https://cursor.com/docs/cli/changelog>), so whether the CLI honours `additional_context` is unconfirmed; no Windows shell is named | `cursor` | [skills](https://cursor.com/docs/context/skills) |

Each row also carries what Hide can do for that agent as a list of features (`hide_kit::agents::Feature`, `KitAgentSnapshot.features`), and an agent with any feature missing is Partial, which is what the Agents tab's Partial popover lists:

| Feature | Supported when | Claude Code, Codex | Gemini CLI | Grok, OpenCode, Pi | Cursor |
| --- | --- | --- | --- | --- | --- |
| `skill` | always | yes | yes | yes | yes |
| `guidance` | Hide writes a guidance hook | yes | yes | no | yes |
| `letters`, `memory`, `subagents`, `spawn_guard` | the six-event hook, so `HookSupport::Part` | yes | no | no | no |
| `bell` | the core rings the doorbell for that agent (`AgentAdapter::bell`, tied to `delivery::doorbell::bell_target`) | yes | no | no | no |
| `herdr_integration` | the row has a Herdr target | yes | no | yes | yes |
| `sleep`, `fork`, `start`, `titles` | Hide reads that agent's sessions (`AgentAdapter::session_reader`) | yes | no | no | no |

Claude Code and Codex are the only agents whose sessions Hide reads, so only they get a per-agent session count, and the count is per machine and only of the sessions running now: one number of open panes holding an awake agent, never an accumulation of warnings.
`herdr-core/src/runtime/tests/agent_features.rs` ties each flag to the gate in the core that decides it (`runtime_of`, `sleeps_kind`, `ForkableAgent`, `AGENT_KINDS`, `conversation_agent_kind`, the adapter's `herdr`), so a flag cannot say yes where the core says no.

An agent is a row only when its documentation confirms where it reads skills and the name of the program it installs.
The other thirteen agents earlier builds supported are no longer rows (see Retired agents below).
Hide does not write an agent's `AGENTS.md` or `CLAUDE.md`, its MCP configuration or its model settings, and it runs no installer for an agent.
The one thing it records in an agent's own state is Codex's trust for Hide's own hooks, below.

### Herdr's integration for an agent

Herdr learns an agent's session id, and for some agents its lifecycle, from a hook script or plugin that `herdr integration install <target>` puts into the agent's own configuration.
Lineage, the mailbox identity, labels and sleep all read that session id, so the kit installs the integration with the agent's other pieces, as a third piece of the agent's row, `herdr:<agent>` in the record.
It runs through the machine's own Herdr CLI (`KitTarget::herdr_bin`): the Herdr bundled in the app on this Mac, and the device's own Herdr on a device.
Every call is one owned child with a deadline and a cleared environment but `HOME` and `PATH`, on the kit worker and never under the runtime lock.
The integration is put in only when the agent is on, installed and has made its own folder, since Herdr refuses an agent whose configuration folder is missing.
For Codex the kit also keeps what Herdr's install wrote into `~/.codex/hooks.json`, because Codex asks the operator to review a hook before it runs and Hide records that trust for the entries it installed ([Codex trusts Hide's own hooks](#codex-trusts-hides-own-hooks)); it goes from the record with the `herdr:codex` piece.
Hide takes out only what it put in: an integration that was already in place when Hide first looked is the operator's and stays, whether the agent is switched on or off or the machine leaves Hide, and an older one of the operator's is not replaced.
One of Hide's own that is older is replaced, and one the operator removed by hand stays removed until Reinstall, as for every other piece.
Gemini CLI has no target in the pinned Herdr, so its row carries no integration piece and that is not a failure; a failed install shows on that agent's row alone and the other agents are untouched.

### Which agents ask for a review

Only Codex asks the operator to approve a hook before it runs.
A survey on 2026-10-07 read each agent's official documentation and the executables installed on the maintainer's machine: Claude Code, Grok, OpenCode and Gemini CLI run a hook or plugin from the user folder with no approval screen, and the check a project folder's hook gets is a folder trust, not a hook review.
Pi's user extensions and Cursor's current CLI hooks are read from their documentation and were not run, so they are an inference.
Hide therefore writes no trust for any other agent, and an agent that starts asking for approval of a user-folder hook is a new row for this section and a new trust step beside Codex's, not a change to Codex's.

### Retired agents

Earlier builds supported thirteen more agents: GitHub Copilot CLI, Amp, Factory Droid, Kiro, Qwen Code, Goose, Cline, Kilo Code, Crush, Junie, Augment, Kimi Code and Mistral Vibe.
The kit takes out what it put on a machine for them, once, by ownership: only a piece the record names is looked at, and then only what carries Hide's own marker, the skill stub's marker line and the hook entry's `hide-guidance` source.
A file the operator wrote or edited stays, and a file that does not parse is left as it is and reported on the machine's retirement line; its record entry stays too, so the next pass tries again.
Each piece leaves the record once it is out and an agent with nothing left in the record is not looked at again, so a second pass asks and rescans nothing.
The shared skill stub stays while a supported agent that reads it is on and installed, or was switched on by the operator and has lost its program.
Its record entry is what is retried, so a stub that could not be removed (a folder the account cannot write to) is removed by a later pass even after the retired agents that owned it have left the record.
`hide-agent-hooks` keeps the layouts of the six retired guidance hooks only so this removal can find them, and a hook entry that an earlier build left behind prints nothing and writes nothing when it runs.
This is a transition path: it goes with the release after the one that ships it.

### Codex trusts Hide's own hooks

Codex keeps a content hash for every entry of `~/.codex/hooks.json` in `~/.codex/config.toml` (`hooks.state`) and starts no entry whose hash it does not hold.
A new or changed entry opens a "Hooks need review" screen at the next start, and until the operator approves it Hide hears nothing from that Codex: no session, no letters, no subagents, and no mark on the pane saying so.
That happened on a machine's first install, whenever Hide's hook changed, and whenever another tool put a hook in front of Hide's and moved its position, which is part of the key.

Installing Hide is the operator's agreement to Hide's hooks, so Hide records Codex's trust for them itself (PRD codex-hook-trust D-01).
It does it through Codex's own interface and never by hand: `hide_agent_hooks::codex_trust` starts `codex app-server` over stdio for one short check, reads the entries with `hooks/list` (each entry's key, its hash and its status) and stores `trusted_hash` for the ones that need it with one `config/batchWrite` to `hooks.state`, then reads the list again and counts a write Codex did not keep as a failure.
A listing that shows Codex could not use the file Hide wrote is a failure too, not "nothing to record": codex-cli 0.160.0 reports a `hooks.json` it cannot parse as a `failed to parse hooks config <path>` warning with an empty list, and a broken `config.toml` as an error naming the Codex home; a warning about another tool's hook is not Hide's to fail on.
Hide computes no hash and writes no `config.toml`, so a change in how Codex hashes is followed, and every other setting in that file stays as Codex wrote it.

The kit runs the check in every pass that finds the Codex hook part in place, whether the pass wrote it or found it current: at launch, when a device connects, and on Reinstall.
It runs once, after the pass has put in every agent's pieces, because Herdr's integration is only in place then, so one `codex app-server` session trusts Hide's entries and Herdr's together and a first install is trusted before the operator's first Codex start.
A pass that finds every entry trusted writes nothing, so a repeat leaves `config.toml` as it was.
An entry another tool's hook displaced is trusted at its new position by the next pass; a Codex started before that pass can still show the screen for Hide's entry.
A Codex without hook trust (its app-server does not know `hooks/list`, which codex-cli 0.160.0 shows as a `-32600` "unknown variant" error rather than JSON-RPC's `-32601`; both are read as unknown), a Codex that ends before it answers the handshake (no `app-server` command, or a program that is not a Codex) and a machine with no Codex are left alone and show nothing new; a Codex that answers the handshake and then ends, hangs or refuses is a failure.
Starting the app-server also makes Codex do its own bookkeeping in `~/.codex` (its databases, `installation_id`, `skills/`), which is Codex's and not Hide's.

What is trusted is exactly the entry Hide wrote, or the entry the kit recorded Herdr writing, and nothing else (`select_targets`, one function for both):

- Codex lists it from this account's `~/.codex/hooks.json` as a user hook that is not managed, so a project's hook, a plugin's and a managed one with the same command are not it;
- its event, handler type, `command` and matcher are, byte for byte, those of one of Hide's six entries (the command Hide writes for that event with this kit's helper, a command hook, and the matcher Hide writes for it: `Bash` for `PreToolUse`, none for the other five; the writer and this check read it from one function, so a matcher that changed is a `modified` entry trusted again, never one trusted blind), or of one entry in the kit record's Herdr entries for Codex (below);
- Codex does not trust it yet (`untrusted`, or `modified` after a change).

The Herdr entries are learned, never written down in Hide: the kit reads the command entries of `~/.codex/hooks.json` before its own `herdr integration install codex` call and again as soon as the call returns, under the account lock, and records the entries that call added (event, matcher, handler type, command) as `herdr_hooks.codex` in `~/.hide/kit/installed.json`, once Herdr reports the integration `current`.
The window another writer could land an entry in is that one call.
The file may only have gained entries, or lost entries the kit already recorded as Herdr's (Herdr replacing its own older command); the removal or edit of any other entry, or an install that adds more than four, records nothing and leaves the record as it was (`herdr_hook_not_learned` on the kit's standard error), and Codex shows its screen for what that install wrote.
A `hooks.json` the kit cannot read around the call leaves the record as it was (`herdr_hook_unread`).
A call that added nothing keeps the recorded entries that are still in the file: Herdr leaves an entry it already wrote alone when it reinstalls (observed on Herdr 0.9.1: an outdated script is replaced and the entry is not duplicated).
A Herdr that changes its command is followed because the kit reads what Herdr wrote at its next install of the integration, and the same pass trusts the new command; the trust recorded for the old command is not removed, as for Hide's own.
The recorded entries count only while the record also holds `herdr:codex` and Herdr reports the integration `current` in that pass, so an integration the operator installed first (the kit never installed it, and the record has neither) and a command that is merely like Herdr's, in another folder or with another argument, are not byte-equal to anything recorded and stay for the review screen.
What stays judgment rather than a rule: an entry another tool wrote in that one call, as a pure addition, would be taken for Herdr's, and a `herdr` that a device finds on its `PATH` is trusted for what it writes as it is already run.
A machine whose record holds `herdr:codex` from a build that did not keep the entries learns none, and does not heal unless Herdr adds a new entry without removing the old one: Herdr's reinstall of an entry it already wrote adds nothing for the kit to see, and an in-place replacement of its command removes an entry the kit never recorded, which an install may not do.
Such a machine shows the review screen once for Herdr's hook and, in those two cases, again for each later change of Herdr's command; approving it trusts exactly that hash.

One other tool's entries join that list, and only through the kit's record: the entries Herdr's own integration (`herdr integration install codex`) added to `hooks.json` when the kit ran it (PRD codex-herdr-hook-trust).
The kit installs that integration with Hide's hooks (see [Herdr's integration for an agent](#herdrs-integration-for-an-agent)), so the operator's agreement to Hide's install covers it: a Mac or device the kit set up opens Codex with no "Hooks need review" screen for Herdr's hook either, and Herdr knows the Codex session from its first turn.
Another tool's hook, an entry that carries Hide's marker over a different command, and any entry that is not like this are neither read nor changed: Codex still shows the review screen for them, and lists only them.
That includes an integration the operator installed before Hide did, which the kit never installed and never recorded, so on a machine where it is the only hook Codex shows "Hooks need review" with that one hook while Hide's are already trusted and run; approving that screen once covers it only, and the review screen lists no entry for Hide's spawn guard.
Hide writes `trusted_hash` and never `enabled`, so a hook the operator switched off in Codex's hook list stays off, and switching Hide's hook off there is how the operator opts out of one; switching Codex off in Settings, Agents takes Hide's entries out and Hide asks Codex for nothing.
Removing Hide's hooks leaves its trust records in `config.toml`; each one matches only the same command and starts nothing by itself.
Codex's `--dangerously-bypass-hook-trust` and `bypass_hook_trust` skip the review for every hook and are not used.

The app-server is one child per check, started through the one spawn helper, bounded by a 15 second overall and 5 second per-request deadline, by caps on what it may print, and by the kit's stop flag, and ended with its whole process tree on success, failure and timeout alike (a process that left that tree and holds its output open is not waited for: the reader thread ends when it lets go); it runs on the kit worker or the device helper, never under the runtime lock.
When Codex is there and the trust cannot be recorded, the Codex hook part reads Failed with one line, "Codex has not trusted Hide's hook: …; it will ask you to review it", the cause class goes to the diagnostic log with the pass's record (`kit apply.completed` names each part's reason), Codex's own words go to the kit's standard error as `codex_trust_failed` (seen where the kit runs in a terminal, and in a device helper's log, but not from the packaged app's detached daemon), the other parts are installed as usual, and the next pass tries again.
`status` runs every few seconds while Settings is open and starts no process, so it repeats what the last pass in this process found; a failure the operator fixed by approving the hook in Codex clears at the next pass or Reinstall, which Failed offers.
`hide-agent-hooks/tests/codex_trust.rs` runs the module against a stand-in app-server (`tests/fixtures/fake-codex.py`), and `hide-kit`'s `codex_trust_cases` runs the passes.
Codex's app-server interface is marked experimental; a method or field that changes ends as the Failed line above, and Codex shows its screen, never a wrong trust.

## Judging what is installed

`hide_agent_hooks::diagnosis` resolves one reason, in a fixed order, and the first match wins:

1. `config_unreadable` - the file could not be read or parsed, so nothing was installed into it.
2. `hooks_not_installed` - the runtime is here and carries no hook of Hide's.
3. `session_predates_install` - the hook is installed and this pane carries none of Hide's tokens, so the session was already running when it was installed. Restarting the agent instruments it (the pane's Reopen does that in place); on a Codex whose machine has the shared server on, the same finding is read as the shared server instead (`PaneConnectionReason`). A pane whose reports Herdr refuses lands here too; `last_report_failure` is what tells the two apart.
4. `hook_outdated` - the session is reporting through an older hook than this Hide writes.
5. `unknown` - genuinely unknown, and said to be.

Both the pane's own mark and the Settings diagnosis read that one function, and every projection carries the reason's stable code alongside its sentence so no surface has to recognise its own operator-facing text.
A pane on a device is judged by the same function, against that device's hooks as its kit last reported them (`agent_hooks::device_hook_status`), with the pane's tokens carried from the device's Herdr.
A device where Hide may not install, or whose platform this build does not carry, reads `hooks_not_installed`; a device whose kit Hide has not read yet reads `unknown`.

## Installing

Hide's hooks are one part of its install kit (`hide-kit`; [ARCHITECTURE.md, The install kit](ARCHITECTURE.md#the-install-kit)), which installs the same set on this machine at every launch of the packaged app and on every device at every helper connection, without asking: the operator agreed to it once, by installing the app or by adding the device.
Every entry's command is guarded, so a session whose app or helper was moved or deleted carries on without a hook error (PRD device-parity B3), and it is written in the form its runtime runs a hook on that system:

| System | Runtime | Entry |
| --- | --- | --- |
| macOS, Linux | both | `"command": "if [ -x '<kit folder>/hide-agent-hooks' ]; then exec '<kit folder>/hide-agent-hooks' hook ...; fi"` |
| Windows | Claude Code | `"command": "powershell.exe", "args": ["-NoProfile", "-NonInteractive", "-Command", "<guard>"]` |
| Windows | Codex | `"command": "<guard>"` |

On Windows `<guard>` is `if (Test-Path -LiteralPath '<kit folder>\hide-agent-hooks.exe' -PathType Leaf) { & '<kit folder>\hide-agent-hooks.exe' hook ... }`, with a quote inside the path doubled, as PowerShell reads it.
The forms follow what each runtime documents or, where its documentation is silent, what its source does:

- Claude Code passes a command string to `sh -c` on macOS and Linux, to Git Bash on Windows, or to PowerShell when Git Bash is not installed, and spawns `command` with `args` directly, with no shell, on every system (hooks reference, "Exec form and shell form", <https://code.claude.com/docs/en/hooks>; `args` since 2.1.139, below the 2.1.278 Memory needs). A string would meet two different shells on two Windows machines, so the Windows entry names PowerShell in exec form and the guard always runs in PowerShell.
- Codex documents `commandWindows` as a Windows override without saying what runs it (<https://developers.openai.com/codex/hooks>). Its source at `rust-v0.160.0`, and the same code at `rust-v0.155.1`, the oldest Codex Hide installs into, runs a command through the session's shell (`core/src/session/mod.rs`, `build_hooks_config`), which on Windows is PowerShell 7 or else Windows PowerShell, started `-NoProfile -Command <command>` (`shell-command/src/shell_detect.rs`, `default_user_shell`; `core/src/shell.rs`, `derive_exec_args`), and through `%COMSPEC% /C` only when the session reports no shell (`hooks/src/engine/command_runner.rs`, `build_command`). The file is that machine's, so the guard is written as `command`.
- A Codex session that falls back to `cmd` cannot read the PowerShell guard: its hooks fail with cmd's syntax error, the turn goes on without Hide's context, and nothing else on the machine changes.

PowerShell starts before the helper on every Windows hook, which Codex does for any hook there; the 100 ms Memory deadline above counts from the helper's own launch.
PowerShell can read what the helper prints in the console code page and write it out again (no runtime documents whether it does), so on Windows the helper prints its JSON in ASCII, every other character as a `\u` escape that decodes to the same text.
The marker is looked for in an entry's `command` and in each of its `args`, where Claude Code's Windows entry carries it, so a second install on Windows recognises its entries and converges as it does elsewhere.
The marker stays at version 6: the macOS and Linux bytes are the ones version 6 wrote (`the_posix_entry_is_exactly_what_macos_and_linux_have_installed` pins them), and no earlier build installed anything on Windows.
`hide-agent-hooks/tests/windows_hook_command.rs` runs both Windows entries the way their runtimes start them, under Windows PowerShell and PowerShell 7, with the real helper in a folder whose name has a space, a quote, brackets and a `$`, and proves stdin reached the helper by the Memory receipt only the session it read can produce, in the `windows check` lane.
Version 6 is the first guarded command, so an older entry reads outdated and the next launch or connection replaces it.
The kit folder is a path that survives a rebuild: the installed app bundle's `Contents/Resources` on macOS, the unpacked package's `resources` on Windows/Linux, and the helper root's `current` link on a device, which each new build of the helper points at itself.

A `hided` outside a desktop package installs nothing, and This machine's row in Settings says the packaged app is where the kit comes from.
`hide_kit::bundled_kit_dir` decides that from the layout: macOS `*.app/Contents/Resources`, or Windows/Linux `resources` with `app.asar` inside and Electron `hide[.exe]` in its parent folder.
A folder merely named `resources` grants no install authority.
The rule exists because a hook command outlives the process that wrote it.
It is a path stored in the operator's own configuration file and run by every future session of that agent, so the only path worth writing is one that survives a rebuild.
On 2026-09-10 a development build resolved the helper beside its own executable under `target/debug/deps`, which Cargo deletes on the next build, and wrote that path into both `~/.claude/settings.json` and `~/.codex/hooks.json`.
Every Claude and Codex session on the machine then failed four hooks per turn with `No such file or directory` until the entries were taken out by hand.
Resolving "beside the executable" was the defect; the package layout is the boundary, and `only_a_daemon_inside_an_app_bundle_has_a_kit_folder` plus `unpacked_package_requires_both_the_shell_archive_and_electron` keep it.

The kit records what it installed in `~/.hide/kit/installed.json`.
A hook it installed that the operator then removed is not put back by the next launch or connection: its part reads Removed and comes back only through Reinstall (D-26).
The `~/.hide/agent-hooks/installed-once` marker from before the kit is not such a record, so a machine that has it and no Hide hook gets one (B20).
A configuration file that could not be read is not written to on a guess: its part reads Failed with the reason, and the kit's other parts are installed anyway (B6).
A runtime whose CLI answers with a version older than the hook needs is not installed into and its part says to update the CLI; a runtime whose home folder does not exist reads Not on this machine.
The write itself runs off `Mutex<Runtime>` (on the kit worker for this Mac, on the helper for a device), and the diagnosis is read back from the file afterwards so the screen shows what the file now says rather than what was asked for.

Settings shows each machine's hook parts under Agents and its whole kit under Devices, This Mac first and then each device.
Reinstall, the `kit_reinstall` event, is offered only where a part is outdated, not installed, removed or failed, repairs only those parts, and leaves the ones in place untouched (B8); pressing it twice is one install.
Project Memory's "update hooks" sends the same Reinstall for this Mac's hook parts.
Reinstall repairs what is on: the hook part of an agent that is switched off reads Off whatever its file says, the machine row does not offer Reinstall for it, and a pane of that agent says its hook is switched off in Settings, Agents (`hooks_switched_off`), not that it was never installed.
A switch pressed while an earlier press for the same agent is still queued replaces it, so the latest press is what the machine ends up with.
The kit looks for the agents once per pass, asks the login shell for its `PATH` only when a startup file changed, and asks a CLI for its version once per version of its file, so Settings re-reading the kit every few seconds runs no subprocess.

Removal does not need the helper, and must not: it reads the configuration file and takes out the entries carrying Hide's marker, and nothing else.
Removing a device from Hide does that on the device while its helper is connected (D-16); the operator removes a hook on their own machine by editing the file, and the kit then leaves it removed.

`hide-agent-hooks doctor [--json]` prints the same judgement in a terminal, because a broken hook shows on screen only as an uninstrumented mark and the output of that command is the evidence.
Install and remove are deliberately not CLI subcommands: writing to the operator's configuration is the kit's decision, agreed to when the app was installed or the device added, not something a stray command line performs.

## Testing

The operator's real `~/.claude/settings.json` and `~/.codex/hooks.json` are never a test target.
Every test in this crate builds its own `HOME` fixture and asserts against that.
