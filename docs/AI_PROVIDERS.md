# Background AI providers

`hide-ai/` is the one boundary through which a Hide feature asks a language model for something in the background.
Its consumers are the core's agent label analyzer (`herdr-core/src/labels/analyzer.rs`), Project Memory extraction under `hide-memory/`, and the Start dialog's worktree name; later features reuse the same boundary rather than a provider client of their own.
This guide owns the boundary's rules; the code under `hide-ai/src/` and the tests under `hide-ai/tests/` are its executable authority.

## Ownership split

A feature owns its prompt, its output schema, and the parsing of the answer into its own type.
It submits an `AiRequest` (feature id, caller-made request id, subject id, system prompt, input, schema, deadline, schema version) and receives an `AiResult`: a `serde_json::Value` already validated against that schema, and the provider that produced it.
There is no output token ceiling in the request, because no provider contract honours one; the prompt and the schema are what keep an answer short.

`hide-ai` owns everything about providers: which are installed and logged in, the child process and its protocol, timeouts and cancellation, the retry policy, fallback between providers, duplicate suppression, and the structured error a caller branches on.
No feature type lives in the crate, and no provider detail leaks out of it.

The Start dialog of the Overview uses feature id `worktree_name` (`herdr-core/src/ai.rs::suggest_worktree_name`): it sends the issue's title and at most 2,000 characters of its body, asks for `{slug}` against a schema, and the core keeps only lowercase ASCII words joined by hyphens, at most 48 characters, behind the issue-number prefix the dialog passed; the deadline is 30 seconds, a failure leaves the dialog's own name in place, and Settings › Issues turns the feature off.

Project Memory uses feature id `project_memory` through Hide's native analysis, relation-planning, persistence, and retrieval boundary.
Official Mem0 OSS does not execute and is not a runtime dependency or service.
Pinned Mem0 OSS v2.1.0 extraction and update prompt assets remain only as audited design-reference provenance; their exact upstream commit, source hashes, local asset hashes, and non-runtime role live in `hide-memory/hide-native-engine-reference.json`.
Hide's strict schema carries candidate text, kind, extraction confidence, source offsets, and `new`, `same`, `supersedes`, `conflicts`, or `discard` relation proposals.
The write service validates those proposals against source events, Project identity, privacy, provenance, lifecycle, and resource caps.
The local search path uses active-only FTS5 plus bounded literal and path fallback; it admits only query-relevant candidates, then uses salience, extraction confidence, and recency as tie-breakers rather than semantic-similarity signals.
It sends only locally redacted, normalized human and assistant events after the durable session cursor, plus a relevance-neutral active-Memory comparison set used to find duplicates, updates, and conflicts across languages.
The comparison set is bounded by item, token, and exact serialized-byte budgets; normalized events and comparison items are composed as JSON arrays under one final 64 KiB request-input cap, so individually valid inputs cannot combine into a permanently unprocessable batch.
Both fields pass through the current local redactor again at the final provider egress boundary.
The feature layer verifies the relation and provenance before the single-writer store changes anything; provider output never has direct write authority.
Memory analysis reuses this boundary's selected provider, availability, bounded same-provider retry, duplicate suppression, process ownership, cancellation, and budgets.
It may fall through to a provider the operator added under fallback (see [Selection and fallback](#selection-and-fallback)), and only to one, and the disclosure names both that possibility and the fact that the stored analysis batch records which provider answered.
The disclosure also names active-Memory retransmission, and its stored version disables previously enabled Projects when this material data-egress description changes so that the operator must opt in again.
Not-authenticated, unavailable, usage-limited, and exhausted outcomes pause new analysis without disabling existing local Memory search or Sessions browsing.
Logs retain request, feature, provider, Project/session subject IDs, counts, duration, and outcome, but never transcript text, prompt text, Memory body, file path, credential, or provider thread ID.

## Providers

The user's installed and logged-in CLIs are the only credentials for background AI requests.
The background AI boundary reads no token file: the codex backend references the user's `auth.json` only through a symlink in the private `CODEX_HOME` it sets (see the codex bullet below), and never opens the file itself.
It sets two environment variables: that `CODEX_HOME`, pointing the app-server at the private directory, and `MAX_THINKING_TOKENS=0` on a claude model turn (see the claude bullet).
`hide-ai` is also the only code that starts a background AI child of any agent: the Weekly Usage reader below asks the same `ClaudeCliBackend` for its `/usage` text rather than running the CLI itself.

The providers are a registry (`hide-ai/src/registry.rs`), in this fixed order: `claude`, `codex`, `gemini-cli`, `grok`, `opencode`, `pi`, `cursor`.
`ProviderId` is a copyable name for one entry, written in `ai.json` as its id; `PROVIDERS` lists them, `ProviderId::descriptor()` gives the label, the program names looked up on the login `PATH`, the default model and `agent`, the id of the same agent in the install kit (`claude-code`, `codex`, `gemini-cli`, `grok`, `opencode`, `pi`, `cursor`), and `build_backend` makes the backend of one entry.
An empty model means the CLI's own default: no `--model` flag is sent.
A new agent is one registry entry and one backend; the router, the settings and the core iterate `PROVIDERS` and name no provider.

- `codex`: `codex app-server --listen stdio://`, the official JSON-RPC surface of the Codex CLI, driven with an ephemeral read-only thread, the feature's system prompt as the base instructions, every optional feature disabled, and the feature's output schema attached to the turn.
  The default model is `gpt-5.6-luna`.
  Availability is read from `account/read`; the thread leaves nothing in `~/.codex/sessions`.
  `codex exec` is not used.

  `hide-ai` owns this process (see [Process ownership and budgets](#process-ownership-and-budgets)).
  It runs the app-server under a private, owner-only (`0700`) `CODEX_HOME` in a temporary directory (`$TMPDIR/hide-ai-codex-home-<pid>-<nanos>`) that is created holding nothing but a symlink to the user's `auth.json` (from the environment's `CODEX_HOME`, or `~/.codex/auth.json`), and no `config.toml`.
  The app-server then writes its own state beside the symlink (`installation_id`, `models_cache.json`, its sqlite databases, `shell_snapshots/`, `skills/`, `tmp/`; 2 to 5 MB a session, measured 2026-09-20), but no `sessions/` rollout, so the ephemeral thread leaves no transcript.
  With no config the app-server starts none of the MCP servers the user's real config declares, which measurement (2026-09-17) showed `-c mcp_servers={}` and a per-thread `config` override both failed to prevent.
  The directory is removed when the session ends; if it or the symlink cannot be created the request fails with `ProviderUnavailable(codex_home_unavailable:<stage>:<kind>)` rather than falling back to `~/.codex`.
  An owner that ends without running `Drop` (a `kill -9`, or a process exit while a session is open) leaves its directory behind, so every `CodexAppServerBackend` sweeps them when it is built, on a thread of its own (`hide-ai/src/codex_home.rs`).
  The sweep removes a `hide-ai-codex-home-<pid>-<nanos>` directory under `$TMPDIR` only when it is a plain directory the current user owns and no process has that pid; a pid that is alive keeps its directories even if it was reused by an unrelated process, because a dead owner's leftover costs disk while deleting a live owner's breaks its app-server.
  A name that does not parse is never touched, symlinks are never followed, so the user's `auth.json` stays, and one sweep removes at most 32 directories so a backlog (344 dead owners' homes on one machine on 2026-09-28, counted by checking each name's pid with `kill -0`) is cleared over several starts.
  A sweep that removed, failed to remove or could not read anything logs `ai.codex_home.swept` with `removed`, `bytes`, `failed`, `unreadable` and `capped`, plus `first_error=<kind>` when a removal or the directory listing failed (a listing error ends the sweep early); a temporary directory it could not read, or a sweep thread that could not be started, logs `ai.codex_home.sweep_failed` with the stage and the error kind.
  The credential file is referenced through the symlink and never read.
  codex's own token refresh writes through it to the real file: the default `file` credential store opens `CODEX_HOME/auth.json` for truncating write, no temp file and no rename, so the symlink is followed and survives (read from `codex-rs/login/src/auth/storage.rs` at `rust-v0.155.1`).
  A `keyring` or `auto` credential store keys the entry by the canonical `CODEX_HOME` path, so a private home has no entry and the provider reports `needs_login`; that mode is not supported here.
  Shutdown is one graceful path on every exit (`Session::drop`, a session swap, an over-budget restart): close stdin (its EOF is the app-server's own shutdown signal and ends the whole tree), then SIGTERM, then SIGKILL, each after a three-second grace.
  After ten idle minutes with no completed request the app-server is shut down and `ai.app_server.idle_exit` is logged; the next request starts a fresh one.
- `claude`: `claude -p --output-format json`, print mode, one child process per request.
  Print mode is Claude Code's only official structured-output surface; there is no `app-server` equivalent in `claude --help`.
  It was excluded by decision until 2026-09-10, when that exclusion was withdrawn; terminal scraping and token reuse remain excluded, and the background AI boundary reads no credential file.
  The default model is `sonnet`.
  Availability is read from `claude auth status --json`: `loggedIn` decides `Ready` against `NeedsLogin`, a binary that is not on `PATH` is `NotInstalled`, and a probe that answers nothing readable is `Unavailable` rather than either guess.
  The answer is the result frame's `structured_output`, the field the CLI validated against the feature's `--json-schema`; the `result` string is never parsed, because without a schema print mode returns the object inside a fenced code block and reading that back would accept an unvalidated shape.
  The prompt body travels on stdin, so no transcript reaches an argument vector or a process listing.

  The child's argument vector is not a preference.
  Measured on claude 2.1.267, a bare `claude -p` carries the whole agent harness into the system prompt, 32,903 cached input tokens, and answers the wrong question: it reviews the transcript instead of classifying it.
  Adding `--system-prompt` with `--tools ''` and `--setting-sources ''` takes the same request to 1,188 input tokens and returns a schema-validated answer.
  Dropping any of those three is not a cost regression, it is a wrong answer, so `hide-ai/tests/claude_cli.rs` asserts the vector the child actually received.

  A model turn runs with thinking off: the child gets `MAX_THINKING_TOKENS=0` whatever the operator's environment says, and the same test file asserts the value the child received.
  Print mode thinks by default, and a background answer is a short JSON object; on 2026-10-02 the context label spent a median 2,656 output tokens (at most 9,425) to return about 100, which put its median at 28 seconds and 43 of 252 requests past the 60-second deadline.
  No flag turns it off: `--settings '{"alwaysThinkingEnabled":false}'` left the thinking in place, and the variable removed it.
  Measured on claude 2.1.287 with the same label-style prompt, haiku with thinking answered in 56 to 81 seconds, haiku without it in 5 to 8 seconds but broke the prompt's rules (step verbs, listed items), and sonnet without it in 3.6 to 7 seconds with about 165 output tokens and the best answers, which is why `sonnet` is the default.
  The rest of the vector keeps the turn from reaching anything else or outliving itself: `--strict-mcp-config`, `--disable-slash-commands`, `--no-session-persistence`, `--permission-prompts none`.
  `--bare` cannot be used: it reads `ANTHROPIC_API_KEY` only and never the OAuth keychain, so it is incompatible with the subscription login that is the user's own credential.

  One child per request, never a persistent one: a second request to a live child reuses its `session_id` and the conversation accumulates.
  A cancelled or expired request kills the child, which is safe because print mode holds no state worth draining.

  The model list is the CLI's own.
  `ClaudeCliBackend::models()` starts `claude -p --input-format stream-json --output-format stream-json --verbose --no-session-persistence --tools '' --setting-sources '' --strict-mcp-config --disable-slash-commands` with the same neutral directory, writes one `initialize` control request to stdin, reads the `control_response`, and ends the child; no model turn is made.
  Only `response.response.models[].value` is read, and `default` is left out because it is the empty choice every menu already has.
  The same response carries the logged-in account, which the parser never reads and nothing logs, stores or snapshots.
  The list is held for ten minutes (a menu opening is not a child start) and the child is bounded at twenty seconds; a list the CLI could not give is `ModelCatalog::Unknown` with `claude_models_unreadable:<stage>`, never a guess.
  The shape was observed on 2026-10-06 against the installed, logged-in CLI; the observed values (`opus`, `fable`, `sonnet`, `haiku` and full names) are the account's, not a contract, which is why they are read each time.
- `gemini-cli`, `grok` and `pi` answer in text through one shared one-shot runner (`hide-ai/src/runner.rs`, `text_cli.rs`), and `opencode` and `cursor` are registered but unsupported.
  See [The text-mode CLIs](#the-text-mode-clis).

## The text-mode CLIs

Gemini CLI, Grok and Pi have no output-schema surface the way Claude Code and Codex do.
Each is asked, in its system prompt, for exactly one JSON object matching the feature's schema, and the object is read back from the text it printed: the whole text, the body of a code fence, or the span from the first `{` to the last `}`, in that order.
The router validates every answer against the feature's schema, so a wrong shape is `InvalidOutput` whatever the CLI said.
A failure is classified from the CLI's documented exit codes and what it printed to stderr: an authentication word or HTTP 401 or 403 is `NotAuthenticated`, a rate or quota word or 429 is `UsageLimited`, an overload word or 5xx is `Transient`, and anything else is `CompletionUnknown`, because the child ran and the prompt may have been submitted.
Nothing the CLI printed on failure is kept; the error carries the provider and the exit status.

The shared runner starts every child through `hide_platform::process::OwnedChild`, so it dies with its owner and on every exit path.
It bounds standard output at 8 MiB and standard error at 64 KiB (more is `InvalidOutput`, never a larger buffer), ends the child at the deadline or when the caller cancels, and gives it either the process environment or the login variables plus the few it names.
A prompt or system prompt that must be a file is written by `PrivateFile` into a `0700` folder of its own as a `0600` file, removed on every path; a folder whose owner died is swept at the next start by pid, the same rule as the codex home.
The transcript never reaches an argument vector.

| Agent | Command | Prompt, system prompt | Read-only by | Models | Login |
| --- | --- | --- | --- | --- | --- |
| `gemini-cli` | `gemini -p <instruction> --output-format json --approval-mode plan [--model <m>]`, in the private file's folder | transcript on stdin, system prompt in a `0600` file named by `GEMINI_SYSTEM_MD` | `--approval-mode plan`, the CLI's read-only mode | a fixed list the CLI documents (`auto`, `pro`, `flash`, `flash-lite`), reported as `Fixed` because it cannot be asked | not probed; learned from a request failure, so an installed CLI reads `Ready` |
| `grok` | `grok --prompt-file <f> --output-format json --json-schema <s> --system-prompt-override <sys> --tools "" --no-subagents --disable-web-search --max-turns 1 [--model <m>]` with `GROK_MEMORY=0` and `GROK_DISABLE_AUTOUPDATER=1` | transcript in a `0600` prompt file | no tool, no sub-agent, no web search, one turn | `grok models`: the `* default` line and `- <id>` lines | `grok models` printing "not authenticated" is `NeedsLogin` |
| `pi` | `pi -p --no-tools --no-session --no-extensions --no-mcp --no-skills --no-prompt-templates --no-context-files --thinking off --system-prompt <sys+schema> [--model <m>] <instruction>` | transcript on stdin | no tool, no extension, no MCP, no session | `pi --list-models`, read tolerantly | `pi auth check --provider <p>` when the model is `provider/id` (exit 0 ready, 1 or 2 not), otherwise a non-empty model list |

Gemini exit 42 (input error) and 53 (turn limit) are `InvalidOutput`.
Grok's answer is `structured_output` when present and its `text` otherwise.
Each CLI's own flags are the contract; a flag one of them stops accepting is a failed request that names the exit status, not a silent change of behaviour.

`opencode` and `cursor` are `UnprovenReadOnlyBackend`s.
Neither CLI documents a mode that guarantees a one-shot, tool-free, read-only run from a script, and a background feature must never be the reason an agent edits a file, so they are listed and never asked: availability is `NotInstalled` when no program is found and otherwise `Unsupported { reason: "cannot_guarantee_read_only" }`, `models()` is `Unknown` with the same reason, and `execute` returns `Unsupported`.
Settings shows them as installed and not selectable, with that reason.
Making either selectable is a change to its backend and to this table, with a documented read-only mode cited, not a setting.

Proof level, as of 2026-10-06: Claude and Codex have been run live; Gemini CLI, Grok and Pi are proven by scripted stand-ins (`hide-ai/tests/text_clis.rs`, which records the argument vector, the standard input, the working directory, the environment and the file contents the child received) because none is installed on the machine that built this, so their flags, exit codes, output shapes and login probes follow the CLIs' published documentation and are not yet observed.
Grok's `--tools ""` giving a tool-free run, where the validated object sits in Grok's JSON output, and the format of `pi --list-models` are the unobserved parts; the parsers accept more than one shape and an unreadable list is `Unknown`, not empty.

## The operator's choice

Which agent answers, and which of its models, is a setting.
It lives in `~/Library/Application Support/hide/ai.json`, a sibling of Hide's own `state.json`, and `hide-ai/src/settings.rs` owns the path, the schema and the rule that turns a choice into a `RouterConfig`.

Hide writes it: the Settings Hide AI tab (`web/src/settings/HideAiTab.tsx`, rules in `web/src/hideAi.ts`) dispatches one `ai_settings` event per intent, the core applies it to the snapshot at once and queues the write, and the session-sync coordinator performs the write off the runtime mutex.
The core reads the file once, when this Mac's session-sync coordinator starts, and from then on holds the choice in the runtime, so a file edited by hand while `hided` runs is read at the next start.
The core's label analyzer reads the runtime's choice before each analysis, so a choice made in Settings reaches the next label without restarting anything, and an analysis already running finishes on the choice it started with.
A changed choice rebuilds the router, because a backend is constructed with its model; an unchanged one leaves the router and its sticky failover state alone.

The file names a provider, a model per provider, whether Hide AI is on, an ordered fallback list, and whether agent labels are made at all:

```json
{
  "provider": "claude",
  "models": { "claude": "sonnet", "codex": "gpt-5.6-luna" },
  "enabled": true,
  "fallback": [{ "provider": "codex", "model": "gpt-5.6-luna" }],
  "agent_summary": true
}
```

`enabled` is the Settings › Hide AI switch and is on when the field is absent.
Off, `AiRouter::execute` returns `AiError::Disabled` at once and asks no provider anything, and a feature treats it as nothing to do, never as a failure.
`fallback` is the list of providers a request may move to, in order, each with the model it is asked with (absent means that provider's own default); it is empty when the field is absent.
A provider the registry does not know is ignored wherever it appears, so a file written by a newer Hide still reads, and `models` keeps the keys it does not know.
`AiSettings` is the one owner of the rules: `set_provider` removes the chosen provider from `fallback` and adopts the model it had there, `add_fallback` refuses the chosen provider and a duplicate (`FallbackRefusal`), `remove_fallback` and `set_fallback_model` edit one entry, and `models_by_provider()` is the model of every provider for building backends.

`agent_summary` is the Settings › Hide AI › Features Agent summaries switch (PRD overview-request-view D-11), on when the field is absent.
Off, the label analyzer asks nothing and cancels the request it is running, and no surface shows a label; worktree naming and Project Memory keep their own switches.
Turning it does not rebuild the router, because it is not part of the router's choice.

The defaults are the registry's own constants: `claude` with `sonnet`, `codex` with `gpt-5.6-luna`, and the CLI's own model for every other agent.
`provider` is in the file only once it was chosen (`AiSettings.chosen`), so a file that only turned a switch makes no choice for the operator, and a settings value with nothing chosen asks no model anything (`router_config().enabled` is `enabled && chosen`).
`AiSettings::provider_for_first_run(availability)` is the one rule for what a first run chooses: the first provider in the fixed order that is `Ready`, and none when none is, which leaves the feature off until the operator chooses.
The core applies it, not the settings crate: while nothing is chosen and Use Hide AI is on, the coordinator asks only the agents that are switched on in this Mac's kit and installed whether they are signed in (`AiRequest.selecting`, availability only, no model list, every five minutes while the tab is closed and at once while it is open), and the first of the fixed order that answers `ready` is chosen with its default model and the choice is stored (D-18, D-27).
A stored choice is never replaced by that rule, an agent that is not switched on in the kit is never chosen by it, and with none signed in nothing is chosen and Hide features run without a model; signing in later is noticed without any setting being touched (B45, B47, B48).
A file Hide wrote before this default changed names `haiku` for claude like any other choice, so it keeps haiku until the operator picks a model in Settings.
A file that is not there means nobody has chosen, so the defaults stand and nothing is reported.
A field that is missing takes its default and a field the crate does not know is ignored, so an older Hide reads a file a newer one wrote.
A file that exists and cannot be read is never taken as the defaults in silence: Hide states the reason on the Hide AI tab and writes an `ai_settings` `settings.unreadable` record to its diagnostic log, and only then do the defaults apply.
A write that fails says so on the same tab, because a choice the operator made and the file on disk must not silently disagree.
A write that succeeds marks the choice as chosen at once, so the tab stops calling it the default without waiting for the next launch to read the file back.
A session with no home directory to write to reports that on the tab for the same reason: the choice has already left the runtime, so it cannot be dropped quietly.

Choosing a provider changes which provider is asked first and nothing else.
Every feature, Project Memory included, asks the chosen provider and then the fallback list under the policy below, while the router's retry, cooldown, cancellation, duplicate-suppression, process, and budget rules apply to all of them.

The Hide AI tab reads `status.background_ai`, one section the core publishes (`herdr-core/src/model.rs`, `BackgroundAiSnapshot`): `enabled`, `provider` (null until one is chosen), `chosen`, `agent_summary`, one `providers` row per registered agent in the fixed order (`agent` is the kit adapter id, `state` one of `ready`, `needs_login`, `usage_limited`, `not_installed`, `unavailable`, `unsupported`, `unread`, `installed`, `selectable`, `retry_at_ms`, `model`, `models`, `models_fixed`, `cli_default`, and `models_unavailable_reason`), the ordered `fallback` list, and `refusal`.
Every reason in it is a code the shell turns into words, never prose, an account, a path or a conversation, and the cause of a failed model-list read stays in the diagnostic log (B68).
`ai_settings` carries `enabled`, `provider` (Runs on), `model` (for Runs on, or for an agent in the fallback list, which keeps its own), `fallback_add`, `fallback_remove`, `agent_summary` and the two observation hints; the core refuses an agent that the last read did not find selectable (`ai_settings.provider_not_selectable`), an unknown one (`ai_settings.unknown_provider`), the Runs on agent as its own fallback (`ai_settings.fallback_is_runs_on`) and a listed one twice (`ai_settings.fallback_listed`).
`refusal` is how the label analyzer last found the chosen agent (`AiStanding`, written on the analyzer's thread after each job): its reason class, the end of a usage limit when the agent said, and the listed agent a request made now would run on (`using`), or none when no listed agent can answer, which is the Runs on row's reason (B41, B42).
It is absent while the chosen agent answers and while Hide AI is off.

With Hide AI off or no agent chosen, the label analyzer answers every job as stopped without asking anything and the runtime says `agent_summary()` is false, so rows show the session's own text and a turn is made once it is on again; worktree naming returns the dialog's own name; Project Memory analysis waits (the project stays enabled, its saved Memory and Sessions keep working).
A request already running when the switch turns off finishes or is cancelled by the analyzer's own check, and `AiError::Disabled` reaching a turn is a wait (`AnalysisFailure::retry_after`), not a parked failure.

Project Memory's disclosure is version 2: analysis may go to the agent Hide AI runs on or one added under fallback.
Opening a Memory store from the earlier version disables the projects that accepted version 1 until the operator accepts again; their stored Memory, search and Sessions stay.

Settings shows each provider's availability and the models it offers.
Both come from asking the provider, so no model list is written into the core or the shell; `AiRouter::availability()`, `AiRouter::models()` and `AiRouter::statuses()` are the source, and they cover every registered provider whether or not it is chosen.
`statuses()` answers each as a `ProviderStatus { provider, availability, selectable, parked_for }`: `selectable` is true for `Ready` and for a provider parked by a usage limit (it will answer again), and false for `NotInstalled`, `NeedsLogin` and `Unsupported`, whose `availability` carries the reason the screen shows.
An agent that is installed and not selectable is shown with that reason, never hidden.
Asking costs child processes, so the probe is a capability reader like the project panel's: the session-sync coordinator drives it, the work runs on a worker thread, the runtime mutex is never held across it, and it asks nothing at all while the group is off screen.
In the web shell the daemon holds that flag for every connected page: each page reports only its own demand, and a page that closes or disconnects releases it (`hided/src/demand.rs`).
`scripts/check-capability-readers-off-lock.sh` asserts that structurally, by name.

### What leaves the machine

Hide AI sends redacted conversation summaries and Project Memory batches to the agent the operator chose in Settings (Runs on) and to the agents the operator added under fallback, and to no other.
A registered agent that is neither chosen nor in the fallback list is never asked a request: its program may be installed and logged in, and it is only inspected, for the Settings screen, which starts no model turn.
Because the list is the operator's, adding an agent to it is the consent for that agent, and Project Memory's disclosure names it (its version moves when this description changes, so earlier opt-ins are asked again).

## Process ownership and budgets

A background feature asks a resident process for an answer, so `hide-ai` owns that process the way `oh-my-principle`'s resident-process practice requires: one spawn helper, one shutdown path, a child that dies with its owner, and caps that turn a leak into a reported failure rather than a larger number.
This exists because on 2026-09-17 a single label watcher (the retired plugin's process) held 1,699 `codex app-server` descendants and 11.6 GB for two idle days with no signal at all.

`hide stop`, and `hide connect` replacing a daemon of another build, send `hided` SIGTERM, then SIGKILL after five seconds if it has not exited (`hided/src/cli.rs`); `hided` turns SIGTERM and SIGINT into its graceful stop (`hided/src/lib.rs`), and dropping the core cancels the running label analysis, which ends its provider child, and joins the analyzer thread (`Core::drop`), so on that path no label request outlives the daemon.
On the SIGKILL path, and for a child no explicit shutdown reaches, a background AI child's end relies on the OS closing the inherited stdin pipe when the owning process exits.

Every child the crate starts goes through one spawn helper (`hide-ai/src/process.rs`), and the one-shot CLIs through the runner on top of it (`hide-ai/src/runner.rs`).
The codex app-server is owned through the stdin pipe it inherits: when the owner dies, the pipe closes and the whole tree ends, which is what makes a `kill -9` of the owner leave no survivors.
The claude backend and the text-mode CLIs start one child per request and kill it on every path.

The caps live in `RouterConfig` as measured constants, not settings (a setting with no value is unlimited, which is what a cap removes):

| Cap | Default | Measured against |
| --- | --- | --- |
| `max_in_flight` | 1 | requests running at once across the router |
| `max_per_minute` | 30 | requests admitted in any 60-second window |
| `max_app_server_descendants` | 4 | processes under the child the crate started, transitively, after a turn |
| `max_app_server_rss_bytes` | 1 GiB | resident size of that child and everything under it |
| `max_consecutive_restarts` | 3 | over-budget restarts before the provider is failed |

The two request-rate caps are checked before a request is submitted; crossing one returns `AiError::OverBudget { cap, measured }` at once and logs `ai.budget.exceeded`.
`OverBudget` is a refusal: it is safe to retry and it does not move to another provider, because the cap is the account's, not the provider's.
An in-flight request and any other provider are untouched.

The two process caps are measured after each codex turn (macOS `libproc`: `proc_listchildpids` and `proc_pidinfo`, no subprocess).
Crossing one logs `ai.app_server.over_budget` with the measured value and the cap, ends the app-server through the graceful shutdown path, fails the request with `OverBudget`, and lets the next request start a fresh app-server.
A request that completes under the cap clears the restart count; three consecutive restarts that do not clear it fail with `ProviderUnavailable(app_server_restart_cap)`.
On a platform without the kernel query the measurement is `Unavailable`: the process caps are not enforced and the log line says `measurement=unavailable` rather than a zero.
The label worker treats `OverBudget` as an environmental failure: it keeps the goal, drops the turn's line and end, and asks again after ten minutes, the same as any other environmental failure (`AnalysisFailure::retry_after`).

## Weekly usage display

The sidebar footer's Weekly Usage chips and popover ([UI_BEHAVIOR.md: Weekly usage](UI_BEHAVIOR.md#weekly-usage)) show a separate read-only capability owned by `herdr-core/src/usage.rs`.
It uses the user's existing CLI logins to read each provider's seven-day account window, and it never routes a model request through `hide-ai`.

For Claude Code, the core runs `claude -p "/usage" --output-format json --no-session-persistence` through `ClaudeCliBackend::usage_text` and parses the `result` text the CLI prints.
`/usage` is a local command: the CLI authenticates against its own keychain item, makes no model turn (`duration_api_ms` 0, cost 0), and with `--no-session-persistence` leaves nothing under `~/.claude/projects/`, in `claude --resume`, or in Hide's Agent Conversation list.
Hide holds no Claude token at any point and never opens the keychain itself; the earlier direct keychain read is gone because an ad hoc signed dev build has a new code identity on every rebuild, so macOS revoked "always allow" and the row fell to a three-second timeout.
The child receives exactly the variables `hide_platform::process::LOGIN_CHILD_VARIABLES` names (`hide_ai::USAGE_ENVIRONMENT`): `HOME`, `PATH`, `USER`, `LOGNAME` and `TMPDIR` on macOS and Linux, and `PATH`, `PATHEXT`, `SystemRoot`, `USERPROFILE`, `USERNAME`, `TEMP`, `TMP`, `APPDATA`, `LOCALAPPDATA`, `ComSpec`, `windir`, `SystemDrive`, `ProgramFiles`, `ProgramFiles(x86)`, `ProgramData`, `HOMEDRIVE`, `HOMEPATH` and `CLAUDE_CODE_GIT_BASH_PATH` on Windows, where Node reads its home from `USERPROFILE` and does not start without `SystemRoot`, and the Claude CLI needs Git Bash, which it finds through `CLAUDE_CODE_GIT_BASH_PATH` or `ProgramFiles`.
It runs in Hide's state directory (`~/.hide/state` by default).
`USER` is what lets the CLI find its keychain account; without it the CLI prints `/cost` text as if logged out.
`HERDR_*` and `CLAUDECODE` are withheld on purpose: without `HERDR_ENV` the operator's Herdr and hide agent hooks exit early, and any other hook in the operator's `settings.json` runs as it would for any `claude -p`.
`--bare` cannot be used, because it never reads the keychain.
`CLAUDE_CONFIG_DIR` is no longer an environment key Hide reads: the child does not receive it, so the CLI uses its default configuration directory.

The parser reads the `Current week` lines as `Current week (<scope>): <n>% (used|left)[ · resets <Mon> <D>[, <YYYY>] at <h>[:mm](am|pm) (<IANA zone>)]`.
`Current week (all models)` is the row, every other `Current week` line is a scoped bucket under it, and `left` is `100 - n`.
A `Current session` line only proves the CLI read its login; nothing past its prefix is parsed, because the CLI prints the session line without a reset until a session starts, and a first read after an idle morning once failed on that line alone.
The CLI prints ` · resets …` only when the window has a reset and the year only when the reset falls in another year.
A row line without a reset is the `reset_missing` failure, a bucket line without one is an unavailable bucket, and a reset the reader cannot read is `reset_format`.
The reset instant is the printed year's wall clock in the printed zone, or without a year the next wall-clock match, resolved through a tz database (`herdr-core/src/zoneinfo.rs`, on `jiff`: the system's `/usr/share/zoneinfo` on macOS and Linux, the database `jiff` bundles on Windows, which has none, so a Windows host answers reset times too); a match that passed within the last window is the reset that just passed, so the row reads as expired until the next read.
The observed 2.1.274 output is fixed as a test fixture under `herdr-core/tests/fixtures/claude-usage/`, and the parsed reset agrees with the CLI's own `.usage-cache.json` value for the same window.
Only English output is parsed; another locale reads as unavailable.

Claude failures are classified, never shown as a zero:

- `/cost` text with no `Current` line (the CLI could not read its login): `Sign in with claude to see usage`.
- A `claude` binary not on `PATH`: a row naming the binary, looked up again on the next read.
- A timeout (the child is killed at 30 seconds) or a child that exits non-zero or reports `is_error`: transient, so a success from the last 15 minutes stays with a `Last checked Nm ago · offline` tooltip; with nothing to keep, `Claude Code weekly usage response is unavailable`.
- stdout that is not a result frame, or a `Current` line the parser does not know: `Claude Code weekly usage response is unavailable` at once, whatever was kept.
- A parsed reset that has already passed: the existing expired row.

The child runs on a `BackgroundRead` worker, so the coordinator thread that applies every Herdr pane event never waits on it; one child runs at a time, and dropping the reader cancels a child still running at shutdown.
The failure event carries the provider and the failure kind only; no terminal text, token, or account identifier is logged or placed in a snapshot.

For Codex, the core reads `auth.json` under `CODEX_HOME` or `~/.codex`; an invalid explicit `CODEX_HOME` disables that read instead of falling back to `HOME`.
The access token exists only long enough to build the HTTPS authorization header, and Hide never refreshes, persists, logs, or includes it in a snapshot.
The Codex usage endpoint is called outside `Mutex<Runtime>`; a 429 honors `Retry-After` in either delay-seconds or HTTP-date form, or falls back to bounded 5, 10, and 15 minute delays.
An offline response keeps a non-expired success for at most 15 minutes; Codex can then fall back to the latest weekly window in a local session JSONL file.

Both providers are first read one second after launch, every five minutes while a shell window is visible, and at once when the popover opens on a read older than a minute.
The web shell reports its page visibility and its popover through the two `ui_state_update` hints `contracts/hided-ws.schema.json` declares as `uiStateUsageHints`.
The previous Claude `.usage-cache.json` input is not read.
Failures produce one structured event containing only provider, HTTP status where there is one, and error kind.

## Selection and fallback

A request is asked of the chosen provider and then of each provider in the `fallback` list, in that order, once each, and of nothing else (`RouterConfig::priority` is exactly that list).
A provider that is registered and not on it is inspected for Settings and never asked, so a login the operator never meant to use is not spent.
Nobody having chosen means the default provider, `claude`, with no fallback.

One request has one overall deadline: its own deadline times `overall_deadline_factor` (2), capped at `overall_deadline_cap` (90 seconds) and never less than its own deadline.
Every provider and every retry gets what remains of it, and once nothing remains the request ends with the refusal it last saw (`ai.fallback` is not logged for a hop that was never taken).
A fixed 30 second request therefore spends at most 60 seconds across all providers, where the old one-deadline-per-provider rule could spend that many times over.

`AiRouter::provider_state()` answers the standing question a result alone cannot: the `selected` provider is the one the priority puts first, `active` is the provider a request made now would run on, and `degraded` says why `selected` stepped aside and what remains of its wait.
`active` is `None` when every provider on the list is degraded, which is a reachable state, and is reported rather than filled in with a provider that cannot answer.
`AiRouter::last_answered()` names the provider that most recently produced an answer, which is the only way to say "answered by the fallback" after the request that did so has returned.
`provider_state()` is a query: it never logs and never runs a request, though it may refresh a stale availability answer, which is what makes a return to the selected provider visible.
`availability()` still reports each provider's own reason next to it.

Every failure is one of two families.
A refusal (`ProviderUnavailable`, `NotAuthenticated`, `UsageLimited`, `Unsupported`, `Transient`, `InvalidOutput`) means the provider never took the request, so asking again repeats nothing.
A completion-unknown outcome (`Timeout`, `Cancelled`, `CompletionUnknown`) means the request was submitted and its fate is not known: the deadline passed, the caller cancelled, or the connection or child was lost after `turn/start` went out.
The codex backend draws that line at the `turn/start` write: a lost child before it is `ProviderUnavailable`, a lost or unanswered `turn/start` and anything after it is `CompletionUnknown`.

A request moves to the next provider only on `ProviderUnavailable`, `NotAuthenticated`, `UsageLimited` or `Unsupported`, and the move is written to the log as `ai.fallback` with `from` and `to`.
`Unsupported` is what `opencode` and `cursor` answer, so a fallback list naming one moves on to the next entry.
A text-mode CLI's failure it cannot classify is `CompletionUnknown` and ends the request: moving it on could ask two agents the same question.
`Disabled` is neither a refusal nor a completion-unknown outcome; nothing was asked.

That move is sticky, so the reason outlives the request that found it.
A provider under an account-wide cooldown is not asked for an answer and is not asked whether it is logged in either: the cooldown already answers, and probing a provider that cannot answer for hours is the poke stickiness exists to stop.
An outage or a missing login is remembered by the availability cache for its window instead.
When the cooldown expires the router drops that provider's cached availability with it, asks the provider once, and returns to it when it can answer; nobody has to tell the router the limit reset.
Because a sticky failover has no second provider to try, `ai.fallback` never fires again, so entering and leaving failover each get their own event: `ai.provider.degraded` names the selected provider, its reason and the provider now active, and `ai.provider.recovered` names the return.
Each is logged once per transition, not once per request.
A completion-unknown outcome is final: one call, no second attempt on the same provider, no attempt on another, because a duplicated completion is worse than a missing label.
An answer the schema refused is retried on the same provider only.
There is no silent fallback: the result names the provider that answered, the log names it too, and a failure says why none did.

## Retry policy

- `Transient`: exponential backoff, at most four attempts on the same provider.
- `InvalidOutput`: at most two attempts, because the input is deterministic.
- `Timeout`, `Cancelled`, `CompletionUnknown`: no retry anywhere; the caller decides what to do with the turn.
- `Internal`: the request's leader thread panicked; every caller waiting on the same intent receives this error, the intent's key is freed, and the next call starts fresh.
- `NotAuthenticated`: no retry; availability is re-read after its cache window.
- `UsageLimited`: an account-wide cooldown on that provider, the provider's reset time when it reports one and ten minutes otherwise; every subject waits together.
  `codex` reports its reset time from `account/rateLimits/read`; `claude` reports none, so it takes the default (see Known gaps).
- Duplicate suppression: a second request with the same feature id, subject id and input hash joins the in-flight one instead of starting a new call.
  The in-flight entry is owned by a guard, so a leader that panics still wakes its joiners and releases the key.

What the caller does after the router gives up is the caller's decision.
The label worker parks a turn for good on a settled failure and asks again after ten minutes on an environmental one (after the provider's reset time when a usage limit reports one); a turn end that timed out is asked once more before it is parked (PRD overview-request-view D-33), so a turn costs at most three requests.

## Logging

The router emits `ai.attempt`, `ai.request.finished`, `ai.request.joined`, `ai.fallback`, `ai.provider.degraded`, `ai.provider.recovered`, `ai.budget.exceeded`, `ai.app_server.over_budget` and, once per UTC day, `ai.daily_rollup`; the codex backend emits `ai.app_server.idle_exit`, `ai.codex_home.swept` and `ai.codex_home.sweep_failed`.
A line carries the request id, feature id, provider, outcome class, attempt, duration, input length, output tokens and schema version.
Every `ai.request.finished` of a provider that declares itself measurable (`AiBackend::measurable()`, today only codex, whose app-server is resident) also carries the app-server pid and the process measurement (`app_server_pid`, `descendants`, `rss_bytes`), or `measurement=unavailable` where the platform cannot measure it.
It never carries the prompt, the input, the generated text, a token, a file path from a transcript, or a provider thread id.
Hide's core writes the events of its own routers and backends (the label analyzer, the Settings probe and Project Memory) to its diagnostic log, `Logs/core.jsonl` beside `state.json`, as records with `component` `ai` and the event name as `kind`.

## Known gaps

- A Claude usage limit carries no reset time the code can read.
  The 429 result frame names the window only inside a localised sentence in `result` ("You've hit your session limit, resets 6:42pm (Asia/Seoul)"), so the backend reports `UsageLimited { retry_after: None }` and the router's ten minute default cooldown applies to a window that is really five hours or seven days.
  Keeping the shared default is a decision taken on 2026-09-10, not an oversight: a 429 result frame reports `usage` zeros and `total_cost_usd` 0, so the park is self-correcting at the cost of about a second of latency and one log line every ten minutes, and it is what notices the reset soonest.
  Closing this properly needs a structured reset field in the result frame, not a parser for that sentence.
- The Claude backend does not check that the configured model is one the account offers; the codex backend checks `model/list`.
  A model the account cannot use is therefore discovered as a request failure rather than as an availability state.
  The list `models()` offers is the CLI's own, read through `initialize`, so no model list is written into the code except Gemini CLI's documented aliases, which its CLI cannot be asked for.
  The Settings model control always offers the configured model even when it is not on the provider's list, so reaching the screen never silently changes the operator's choice.
- Gemini CLI, Grok and Pi have not been run live by this repository's checks; see the proof level under [The text-mode CLIs](#the-text-mode-clis).
  Gemini's login is learned only from a failed request.
- Cursor and OpenCode cannot be selected until a read-only one-shot mode is documented for them.
- A Claude usage limit has not been observed against the live account.
  The mapping was measured end to end instead, by answering the CLI's own API request with each HTTP status and reading the frame it printed; the run that measured it is local evidence, not a tracked file.
- Weekly usage in the toolbar reads Codex's usage endpoint, with Codex session JSONL as an offline fallback only, and Claude Code's `/usage` text; the router's daily rollup is a log line, not a popover value.
