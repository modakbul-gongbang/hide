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

Five events are registered: `SessionStart`, `UserPromptSubmit`, `SubagentStart`, `SubagentStop`, and `Stop`.
`SessionEnd` is not registered by either, so the `Stop` sweep is what closes a turn out.

Every entry carries `--runtime claude-code|codex` and `--source hide-subagents@<version>` inside its command (in Claude Code's Windows entry, inside its `args`).
The runtime argument selects that runtime's stdout envelope; the version-6 marker makes an installation whose command is not guarded against a missing helper outdated, so the next launch or connection replaces it.
That marker is the whole basis for judging what is installed: the source name proves the entry is Hide's, and the version after the `@` separates a current hook from an outdated one.
Nothing else is read from the command but the helper's quoted path, and the helper does not pass the marker on: it is an install marker, not the metadata source (see below).

## What the hook returns and reports back

On `SessionStart`, the helper writes one runtime JSON envelope whose `hookSpecificOutput.additionalContext` combines the existing one-line worktree-purpose instruction with the bounded Project Memory capsule when Memory is enabled.
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
The kit's `Codex를 pane마다 실행` part turns that daemon off on each machine, and every Codex Hide starts passes `--no-daemon`, so a Codex runs its hooks in its pane; `src/codex_daemon.rs` is the only code that changes the setting, through `codex features` (`docs/ARCHITECTURE.md`, The install kit).
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

For local and connected device agent delivery, `UserPromptSubmit` also pulls at most five pending letters and 8 KiB of letter context from the sibling `hide` CLI within one total two-second budget.
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
The Settings screen learns of it because the coordinator re-reads the diagnosis once a second while the Settings agents tab is on screen (`settings_observed`, the same flag the Background AI group sets), and reads nothing while it is not.

## Other agents: skill and guidance hook

Claude Code and Codex are the agents the kit has always had a hook for.
Every other agent Hide knows is one row of `hide-kit/src/agents.rs` (`ADAPTERS`), and one switch per agent per machine turns its pieces on and off, in Settings, Agents and in each device's row.
A row carries the agent's detection (a program on the login `PATH` or the usual install folders, or a folder it creates under the home; the names `goose`, `amp`, `droid`, `copilot` and `kilo` are other programs too, so those agents are detected by their folder alone), the folder it reads skills from and the systems the documentation confirms that folder on, whether Hide writes a hook for it, the oldest version whose documentation has that hook, and the official page the row's answers come from (`doc_url`).
A test fails a row with no `https` `doc_url`, so a claim in the table below always has a page behind it.

Two pieces are written per agent, and nothing else:

- The skill stub `hide-browser/SKILL.md` in the folder the agent reads.
  It is a few lines that point at `hide browser help`, so it stays right as the CLI's guide changes.
  Its folder is `~/.agents/skills` for the agents that read it, `~/.claude/skills` for Claude Code (which documents that it does not read the shared folder), and the agent's own folder for Kiro, Qwen Code and Cline.
  A shared folder is written while any agent that reads it is on and set up, and it is removed only when none is.
  A file is Hide's only when its marker line, `<!-- hide-skill@<version>: ... -->`, is the first line after the front matter; a file that merely mentions `hide-skill@` is never replaced or removed, and the agent's row says a skill that Hide did not write is already there.
  A stub with Hide's marker over text that is not Hide's was edited by the operator: no pass rewrites it and a switch-off leaves it, the row reads Outdated with that reason, and only Reinstall puts Hide's text back.
  A stub of an older marker version is Hide's own and is replaced by the next pass.
  An agent's own folder (`~/.claude`, `~/.kiro`, `~/.qwen`, `~/.cline`) is never created for the stub: its presence is how the kit judges the agent, so a pass that made it would change what the next pass finds.
- The guidance hook, for the agents below marked as done.
  It is one `SessionStart` entry, in the agent's own format, whose command is `hide-agent-hooks hook --runtime <agent id> --event SessionStart`.
  `hide-agent-hooks` writes it (`src/guidance.rs`) and nothing else does, under the marker `hide-guidance@1` that proves an entry is Hide's and separates a current one from an older one.
  Its output is the one-line worktree-purpose instruction, one fixed line that points at `hide browser help`, and the live Workspace guidance when the daemon answers, in the field the agent documents.
  It prints no Memory capsule, because the Memory receipt is read from Claude and Codex transcripts, and it keeps no counters and writes no file.
  A second delivery of the same session therefore changes nothing: the output is a pure function of the daemon's answer, and a test runs the hook twice and compares.

Gemini CLI, Qwen Code, Factory Droid, Copilot CLI and Kiro each keep their entry the way their documentation shapes it.
Gemini and Qwen take an entry in `~/.gemini/settings.json` and `~/.qwen/settings.json`.
Factory Droid takes `~/.factory/hooks.json`, or the `hooks` key of `~/.factory/settings.json` when that file already has hooks, because creating `hooks.json` beside them would shadow them.
A removal that leaves `hooks.json` with nothing in it deletes the file, since an empty one would still shadow hooks the operator later keeps in `settings.json`; a `hooks.json` the operator created empty and Hide then wrote into goes the same way.
A removal takes Hide's hook out of whatever group holds it and drops the group only when no hook is left, so another tool's hook that shares a group with Hide's (`{"matcher": "*", "hooks": [Hide's, theirs]}`) stays.
Copilot CLI and Kiro read a folder of hook files, so Hide owns one whole file, `~/.copilot/hooks/hide-guidance.json` and `~/.kiro/hooks/hide-guidance.json`, and deletes it when only Hide's scaffolding is left.
Another tool's entries are counted before and after and survive, and a file that does not parse is left untouched and reported.
Kiro's hook needs CLI 3.0, so a Kiro whose version cannot be read, or is older, gets the skill and not the hook, and its row says why.
Where no minimum is documented there is no version gate.
Non-Copilot guidance hooks are not written on Windows, because their commands are shell commands and their documentation names no Windows form.

The record `~/.hide/kit/installed.json` keeps the operator's choice per agent (`agents`) and the pieces Hide installed (`hook:<agent>`, `skill:<folder>`), and an older build ignores both.
With no choice on record Claude Code and Codex are on, as they have been since their hooks became part of the kit, and every other agent is off.
A piece that was installed and is gone stays gone until Reinstall, and an agent switched off keeps nothing of Hide's and gets nothing back from a later pass.
Detection only decides whether a switch can work: an agent that is not set up on a machine has no switch there, and the choice for it is not recorded.

### Support table

Hook "done" means Hide writes the guidance hook; "follow-up" means the documentation supports a hook and Hide does not write it yet; the other rows have no command hook that can put text into a session's context.
Every row gets the skill stub where the system column says so.

| Agent | Skill folder (systems) | Hook | Why, and the page that says so |
| --- | --- | --- | --- |
| Claude Code | `~/.claude/skills` (all) | done: five-event hook, a kit part | [skills](https://code.claude.com/docs/en/skills) |
| Codex | `~/.agents/skills` (macOS, Linux) | done: five-event hook, a kit part | [skills](https://learn.chatgpt.com/docs/build-skills) |
| Gemini CLI | `~/.agents/skills` (macOS, Linux) | done: guidance `SessionStart` | [skills](https://geminicli.com/docs/cli/skills/), hooks at geminicli.com/docs/hooks |
| Qwen Code | `~/.qwen/skills` (macOS, Linux) | done: guidance `SessionStart` | [skills](https://qwenlm.github.io/qwen-code-docs/en/users/features/skills/) |
| Factory Droid | `~/.agents/skills` (macOS, Linux) | done: guidance `SessionStart` | [skills](https://docs.factory.com/cli/configuration/skills), hooks at docs.factory.com/cli/configuration/hooks-guide |
| Copilot CLI | `~/.agents/skills` (all) | done: guidance `sessionStart` | [skills](https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/add-skills) |
| Kiro | `~/.kiro/skills` (macOS, Linux) | done: guidance `SessionStart`, CLI 3.0 and newer | [skills](https://kiro.dev/docs/skills/), hooks at kiro.dev/docs/hooks/types |
| OpenCode | `~/.agents/skills` (macOS, Linux) | follow-up: hooks are JS plugins, and the context field is an experimental plugin hook read from the repository, not a documented command hook; the operator's OpenCode already runs `~/.claude/settings.json` hooks through a bridge plugin, so a second hook could deliver guidance twice | [skills](https://opencode.ai/docs/skills/) |
| Cursor | `~/.agents/skills` (macOS, Linux) | follow-up: `~/.cursor/hooks.json` `sessionStart` returns `additional_context`, but the official pages do not say it runs in the CLI | [skills](https://cursor.com/docs/context/skills) |
| Augment | `~/.agents/skills` (macOS, Linux) | follow-up: `SessionStart` with `hookSpecificOutput.additionalContext`, documented, not built yet | [skills](https://docs.augmentcode.com/cli/skills) |
| Junie | `~/.agents/skills` (all) | follow-up: `SessionStart` is Early Access, with no version stated | [skills](https://junie.jetbrains.com/docs/agent-skills.html) |
| Cline | `~/.cline/skills` (all) | follow-up: a `TaskStart` file hook whose `contextModification` field is in the repository's README, not on docs.cline.bot, and the CLI and the VS Code extension read different hook folders | [skills](https://docs.cline.bot/customization/skills) |
| Amp | `~/.agents/skills` (macOS, Linux) | none: a plugin API, not command hooks; only `agent.start` can inject | [skills](https://ampcode.com/docs/customize/skills) |
| Goose | `~/.agents/skills` (macOS, Linux) | none: plugin hooks exist, but the documentation gives `SessionStart` no way to add context | [skills](https://goose-docs.ai/docs/guides/context-engineering/using-skills/) |
| Kilo Code | `~/.agents/skills` (all) | none: hooks are TS plugins only | [skills](https://github.com/Kilo-Org/kilocode/blob/main/packages/kilo-docs/pages/customize/skills.md) |
| Crush | `~/.agents/skills` (all) | none: the only hook event is `PreToolUse` | [README](https://github.com/charmbracelet/crush/blob/main/README.md) |
| Pi | `~/.agents/skills` (all) | none: TS extensions, no command hooks | [skills](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/skills.md) |
| Grok | `~/.agents/skills` (macOS, Linux) | none: `SessionStart` cannot add context, only tool-call events can | [skills](https://docs.x.ai/build/features/skills-plugins-marketplaces) |
| Kimi Code | `~/.agents/skills` (macOS, Linux) | none: `SessionStart` is fire-and-forget; injection is documented for `UserPromptSubmit` only | [skills](https://github.com/MoonshotAI/kimi-code/blob/main/docs/en/customization/skills.md) |
| Mistral Vibe | `~/.agents/skills` (macOS, Linux) | none: no `SessionStart` event | [README](https://github.com/mistralai/mistral-vibe/blob/main/README.md) |

An agent is a row only when its documentation confirms where it reads skills and how it is detected.
Roo Code is not a row because it was shut down in May 2026, and Aider is not a row because it reads no skills.
Hide does not write an agent's `AGENTS.md` or `CLAUDE.md`, its MCP configuration, its model settings, or its Codex hook trust review, and it runs no installer for an agent.

## Judging what is installed

`hide_agent_hooks::diagnosis` resolves one reason, in a fixed order, and the first match wins:

1. `config_unreadable` - the file could not be read or parsed, so nothing was installed into it.
2. `hooks_not_installed` - the runtime is here and carries no hook of Hide's.
3. `session_predates_install` - the hook is installed and this pane carries none of Hide's tokens, so the session was already running when it was installed. Restarting the agent instruments it. A pane whose reports Herdr refuses lands here too; `last_report_failure` is what tells the two apart.
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
The kit looks for the agents once per pass, and asks a CLI for its version once per version of its file, so Settings re-reading the kit every few seconds runs no subprocess.

Removal does not need the helper, and must not: it reads the configuration file and takes out the entries carrying Hide's marker, and nothing else.
Removing a device from Hide does that on the device while its helper is connected (D-16); the operator removes a hook on their own machine by editing the file, and the kit then leaves it removed.

`hide-agent-hooks doctor [--json]` prints the same judgement in a terminal, because a broken hook shows on screen only as an uninstrumented mark and the output of that command is the evidence.
Install and remove are deliberately not CLI subcommands: writing to the operator's configuration is the kit's decision, agreed to when the app was installed or the device added, not something a stray command line performs.

## Testing

The operator's real `~/.claude/settings.json` and `~/.codex/hooks.json` are never a test target.
Every test in this crate builds its own `HOME` fixture and asserts against that.
