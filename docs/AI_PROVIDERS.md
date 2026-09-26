# Background AI providers

`hide-ai/` is the one boundary through which a Hide feature asks a language model for something in the background.
Its consumers are the context-label plugin under `plugins/agent-context-labels/` and Project Memory extraction under `hide-memory/`; later features reuse the same boundary rather than a provider client of their own.
This guide owns the boundary's rules; the code under `hide-ai/src/` and the tests under `hide-ai/tests/` are its executable authority.

## Ownership split

A feature owns its prompt, its output schema, and the parsing of the answer into its own type.
It submits an `AiRequest` (feature id, caller-made request id, subject id, system prompt, input, schema, deadline, schema version) and receives an `AiResult`: a `serde_json::Value` already validated against that schema, and the provider that produced it.
There is no output token ceiling in the request, because no provider contract honours one; the prompt and the schema are what keep an answer short.

`hide-ai` owns everything about providers: which are installed and logged in, the child process and its protocol, timeouts and cancellation, the retry policy, fallback between providers, duplicate suppression, and the structured error a caller branches on.
No feature type lives in the crate, and no provider detail leaks out of it.

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
It may fall through to the other logged-in provider under the same bounded router policy, and the disclosure names both that possibility and the fact that the stored analysis batch records which provider answered.
The disclosure also names active-Memory retransmission, and its stored version disables previously enabled Projects when this material data-egress description changes so that the operator must opt in again.
Not-authenticated, unavailable, usage-limited, and exhausted outcomes pause new analysis without disabling existing local Memory search or Sessions browsing.
Logs retain request, feature, provider, Project/session subject IDs, counts, duration, and outcome, but never transcript text, prompt text, Memory body, file path, credential, or provider thread ID.

## Providers

The user's installed and logged-in CLIs are the only credentials for background AI requests.
The background AI boundary reads no token file: the codex backend references the user's `auth.json` only through a symlink in the private `CODEX_HOME` it sets (see the codex bullet below), and never opens the file itself.
The one environment variable it sets is that `CODEX_HOME`, pointing the app-server at the private directory; it adds none for claude.
`hide-ai` is also the only code that starts a `claude` or `codex` child: the Weekly Usage reader below asks the same `ClaudeCliBackend` for its `/usage` text rather than running the CLI itself.

- `codex`: `codex app-server --listen stdio://`, the official JSON-RPC surface of the Codex CLI, driven with an ephemeral read-only thread, the feature's system prompt as the base instructions, every optional feature disabled, and the feature's output schema attached to the turn.
  The default model is `gpt-5.6-luna`.
  Availability is read from `account/read`; the thread leaves nothing in `~/.codex/sessions`.
  `codex exec` is not used.

  `hide-ai` owns this process (see [Process ownership and budgets](#process-ownership-and-budgets)).
  It runs the app-server under a private, owner-only (`0700`) `CODEX_HOME` in a temporary directory (`$TMPDIR/hide-ai-codex-home-<pid>-<nanos>`) that is created holding nothing but a symlink to the user's `auth.json` (from the environment's `CODEX_HOME`, or `~/.codex/auth.json`), and no `config.toml`.
  The app-server then writes its own state beside the symlink (`installation_id`, `models_cache.json`, its sqlite databases, `shell_snapshots/`, `skills/`, `tmp/`; 2 to 5 MB a session, measured 2026-09-20), but no `sessions/` rollout, so the ephemeral thread leaves no transcript.
  With no config the app-server starts none of the MCP servers the user's real config declares, which measurement (2026-09-17) showed `-c mcp_servers={}` and a per-thread `config` override both failed to prevent.
  The directory is removed when the session ends; if it or the symlink cannot be created the request fails with `ProviderUnavailable(codex_home_unavailable:<stage>:<kind>)` rather than falling back to `~/.codex`.
  An owner that dies without running `Drop` (a `kill -9`, which the CI kill test performs) leaves its directory behind, and nothing sweeps them yet.
  The credential file is referenced through the symlink and never read.
  codex's own token refresh writes through it to the real file: the default `file` credential store opens `CODEX_HOME/auth.json` for truncating write, no temp file and no rename, so the symlink is followed and survives (read from `codex-rs/login/src/auth/storage.rs` at `rust-v0.155.1`).
  A `keyring` or `auto` credential store keys the entry by the canonical `CODEX_HOME` path, so a private home has no entry and the provider reports `needs_login`; that mode is not supported here.
  Shutdown is one graceful path on every exit (`Session::drop`, a session swap, an over-budget restart): close stdin (its EOF is the app-server's own shutdown signal and ends the whole tree), then SIGTERM, then SIGKILL, each after a three-second grace.
  After ten idle minutes with no completed request the app-server is shut down and `ai.app_server.idle_exit` is logged; the next request starts a fresh one.
- `claude`: `claude -p --output-format json`, print mode, one child process per request.
  Print mode is Claude Code's only official structured-output surface; there is no `app-server` equivalent in `claude --help`.
  It was excluded by decision until 2026-09-10, when that exclusion was withdrawn; terminal scraping and token reuse remain excluded, and the background AI boundary reads no credential file.
  The default model is `haiku`.
  Availability is read from `claude auth status --json`: `loggedIn` decides `Ready` against `NeedsLogin`, a binary that is not on `PATH` is `NotInstalled`, and a probe that answers nothing readable is `Unavailable` rather than either guess.
  The answer is the result frame's `structured_output`, the field the CLI validated against the feature's `--json-schema`; the `result` string is never parsed, because without a schema print mode returns the object inside a fenced code block and reading that back would accept an unvalidated shape.
  The prompt body travels on stdin, so no transcript reaches an argument vector or a process listing.

  The child's argument vector is not a preference.
  Measured on claude 2.1.267, a bare `claude -p` carries the whole agent harness into the system prompt, 32,903 cached input tokens, and answers the wrong question: it reviews the transcript instead of classifying it.
  Adding `--system-prompt` with `--tools ''` and `--setting-sources ''` takes the same request to 1,188 input tokens and returns a schema-validated answer.
  Dropping any of those three is not a cost regression, it is a wrong answer, so `hide-ai/tests/claude_cli.rs` asserts the vector the child actually received.
  The rest of the vector keeps the turn from reaching anything else or outliving itself: `--strict-mcp-config`, `--disable-slash-commands`, `--no-session-persistence`, `--permission-prompts none`.
  `--bare` cannot be used: it reads `ANTHROPIC_API_KEY` only and never the OAuth keychain, so it is incompatible with the subscription login that is the user's own credential.

  One child per request, never a persistent one: a second request to a live child reuses its `session_id` and the conversation accumulates.
  A cancelled or expired request kills the child, which is safe because print mode holds no state worth draining.

## The operator's choice

Which agent answers, and which of its models, is a setting.
It lives in `~/Library/Application Support/hide/ai.json`, a sibling of Hide's own `state.json`, and `hide-ai/src/settings.rs` owns the path, the schema and the rule that turns a choice into a `RouterConfig`.

Both processes that care link this crate, which is why the file is where it is.
Hide writes it: the Settings `Background AI` group dispatches one `ai_settings` event, the core applies it to the snapshot at once and queues the write, and the session-sync coordinator performs the write off the runtime mutex.
The context-label plugin reads it: `provider::settings` loads it at startup and the watcher re-reads it on every scan, on the same boundary that already re-reads the plugin's own `settings.json`, so a choice made in Settings reaches the next label without restarting the watcher.
A changed choice rebuilds the router, because a backend is constructed with its model; an unchanged one leaves the router and its sticky failover state alone.

A path under a plugin's configuration directory was rejected: it would tie Hide to one plugin id and break when a second plugin consumes the boundary.
A Herdr plugin action was rejected because an action takes no parameters, and a new daemon or socket was rejected as a third contract between two consumers.

The file names a provider and a model per provider:

```json
{
  "provider": "codex",
  "models": { "codex": "gpt-5.6-luna", "claude": "haiku" }
}
```

The defaults are the backends' own constants: `codex` with `gpt-5.6-luna`, `claude` with `haiku`.
A file that is not there means nobody has chosen, so the defaults stand and nothing is reported.
A field that is missing takes its default and a field the crate does not know is ignored, so an older Hide reads a file a newer one wrote.
A file that exists and cannot be read is never taken as the defaults in silence: Hide states the reason on the Settings group and the plugin writes `ai_settings_unreadable` to its log, and only then do the defaults apply.
The plugin writes that line when the reason changes, not when it reads the file.
It re-reads the choice on every scan and rotates nothing, so a line per read would grow its log for as long as the file stayed broken; a repaired file is recorded once too, as `ai_settings_readable`, so fixing it is visible in the same place.
A write that fails says so on the same group, because a choice the operator made and the file on disk must not silently disagree.
A write that succeeds marks the choice as chosen at once, so the group stops calling it the default without waiting for the next launch to read the file back.
A session with no home directory to write to reports that on the group for the same reason: the choice has already left the runtime, so it cannot be dropped quietly.

Choosing a provider reorders the ordinary background-feature priority and changes nothing else.
The context-label feature may fall through to the other provider under the policy below.
Project Memory uses that ordinary priority and fallback policy, while retaining the router's retry, cooldown, cancellation, duplicate-suppression, process, and budget rules.

Settings shows each provider's availability and the models it offers.
Both come from asking the provider, so no model list is written into the core or the shell; `AiRouter::availability()` and `AiRouter::models()` are the source.
Asking costs child processes, so the probe is a capability reader like the project panel's: the session-sync coordinator drives it, the work runs on a worker thread, the runtime mutex is never held across it, and it asks nothing at all while the group is off screen.
In the web shell the daemon holds that flag for every connected page: each page reports only its own demand, and a page that closes or disconnects releases it (`hided/src/demand.rs`).
`scripts/check-capability-readers-off-lock.sh` asserts that structurally, by name.

## Process ownership and budgets

A background feature asks a resident process for an answer, so `hide-ai` owns that process the way `oh-my-principle`'s resident-process practice requires: one spawn helper, one shutdown path, a child that dies with its owner, and caps that turn a leak into a reported failure rather than a larger number.
This exists because on 2026-09-17 a single label watcher held 1,699 `codex app-server` descendants and 11.6 GB for two idle days with no signal at all.

The macOS shell converts SIGTERM into AppKit's ordinary termination path and explicitly destroys herdr-core before exit.
That path cancels the analysis coordinator and reaches each backend's existing child shutdown rather than relying on Swift object deinitialization to happen before process exit.

Every child the crate starts goes through one spawn helper (`hide-ai/src/process.rs`).
The codex app-server is owned through the stdin pipe it inherits: when the owner dies, the pipe closes and the whole tree ends, which is what makes a `kill -9` of the owner leave no survivors.
The claude backend starts one child per request and kills it on every path.

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
The context-label plugin treats `OverBudget` as an environmental failure: it keeps its last label and asks again after ten minutes, the same as any other environmental failure.

## Weekly usage display

The sidebar footer's Weekly Usage chips and popover ([UI_BEHAVIOR.md: Weekly usage](UI_BEHAVIOR.md#weekly-usage)) show a separate read-only capability owned by `herdr-core/src/usage.rs`.
It uses the user's existing CLI logins to read each provider's seven-day account window, and it never routes a model request through `hide-ai`.

For Claude Code, the core runs `claude -p "/usage" --output-format json --no-session-persistence` through `ClaudeCliBackend::usage_text` and parses the `result` text the CLI prints.
`/usage` is a local command: the CLI authenticates against its own keychain item, makes no model turn (`duration_api_ms` 0, cost 0), and with `--no-session-persistence` leaves nothing under `~/.claude/projects/`, in `claude --resume`, or in Hide's Agent Conversation list.
Hide holds no Claude token at any point and never opens the keychain itself; the earlier direct keychain read is gone because an ad hoc signed dev build has a new code identity on every rebuild, so macOS revoked "always allow" and the row fell to a three-second timeout.
The child receives exactly `HOME`, `PATH`, `USER`, `LOGNAME` and `TMPDIR` (`hide_ai::USAGE_ENVIRONMENT`) and runs in Hide's state directory.
`USER` is what lets the CLI find its keychain account; without it the CLI prints `/cost` text as if logged out.
`HERDR_*` and `CLAUDECODE` are withheld on purpose: without `HERDR_ENV` the operator's Herdr and hide agent hooks exit early, and any other hook in the operator's `settings.json` runs as it would for any `claude -p`.
`--bare` cannot be used, because it never reads the keychain.
`CLAUDE_CONFIG_DIR` is no longer an environment key Hide reads: the child does not receive it, so the CLI uses its default configuration directory.

The parser reads the `Current week` lines as `Current week (<scope>): <n>% (used|left)[ · resets <Mon> <D>[, <YYYY>] at <h>[:mm](am|pm) (<IANA zone>)]`.
`Current week (all models)` is the row, every other `Current week` line is a scoped bucket under it, and `left` is `100 - n`.
A `Current session` line only proves the CLI read its login; nothing past its prefix is parsed, because the CLI prints the session line without a reset until a session starts, and a first read after an idle morning once failed on that line alone.
The CLI prints ` · resets …` only when the window has a reset and the year only when the reset falls in another year.
A row line without a reset is the `reset_missing` failure, a bucket line without one is an unavailable bucket, and a reset the reader cannot read is `reset_format`.
The reset instant is the printed year's wall clock in the printed zone, or without a year the next wall-clock match, resolved through the system tz database (`herdr-core/src/zoneinfo.rs`); a match that passed within the last window is the reset that just passed, so the row reads as expired until the next read.
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
The Swift shell reports its main window and its popover, and the web shell its page visibility and its popover, through the two `ui_state_update` hints `contracts/hided-ws.schema.json` declares as `uiStateUsageHints`.
The previous Claude `.usage-cache.json` input is not read.
Failures produce one structured event containing only provider, HTTP status where there is one, and error kind.

## Selection and fallback

With one provider connected, that provider is used.
With more than one, the configured priority decides: the operator's chosen provider, then the other.
Nobody having chosen means the default order, `codex` then `claude`.

`AiRouter::provider_state()` answers the standing question a result alone cannot: the `selected` provider is the one the priority puts first, `active` is the provider a request made now would run on, and `degraded` says why `selected` stepped aside and what remains of its wait.
`active` is `None` when every provider is degraded, which is a reachable state now that both providers can answer, and is reported rather than filled in with a provider that cannot answer.
`provider_state()` is a query: it never logs and never runs a request, though it may refresh a stale availability answer, which is what makes a return to the selected provider visible.
`availability()` still reports each provider's own reason next to it.

Every failure is one of two families.
A refusal (`ProviderUnavailable`, `NotAuthenticated`, `UsageLimited`, `Unsupported`, `Transient`, `InvalidOutput`) means the provider never took the request, so asking again repeats nothing.
A completion-unknown outcome (`Timeout`, `Cancelled`, `CompletionUnknown`) means the request was submitted and its fate is not known: the deadline passed, the caller cancelled, or the connection or child was lost after `turn/start` went out.
The codex backend draws that line at the `turn/start` write: a lost child before it is `ProviderUnavailable`, a lost or unanswered `turn/start` and anything after it is `CompletionUnknown`.

A request moves to the next provider only on `ProviderUnavailable`, `NotAuthenticated`, `UsageLimited` or `Unsupported`, and the move is written to the log as `ai.fallback` with `from` and `to`.
No provider reports `Unsupported` since the Claude backend started answering; the class stays in the taxonomy for a provider that has no driveable contract, and today nothing produces it.

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
The context-label plugin parks a turn for good on a settled failure and asks again after ten minutes on an environmental one.

## Logging

The router emits `ai.attempt`, `ai.request.finished`, `ai.request.joined`, `ai.fallback`, `ai.provider.degraded`, `ai.provider.recovered`, `ai.budget.exceeded`, `ai.app_server.over_budget` and, once per UTC day, `ai.daily_rollup`; the codex backend emits `ai.app_server.idle_exit`.
A line carries the request id, feature id, provider, outcome class, attempt, duration, input length, output tokens and schema version.
Every codex `ai.request.finished` also carries the app-server pid and the process measurement (`app_server_pid`, `descendants`, `rss_bytes`), or `measurement=unavailable` where the platform cannot measure it.
It never carries the prompt, the input, the generated text, a token, a file path from a transcript, or a provider thread id.
The plugin writes these lines into its own `events.jsonl` next to its watcher events.

## Verifying a provider

From the repository root, after `cargo build --release --locked -p agent-context-labels`:

```sh
target/release/hide-agent-context-labels verify-provider --provider codex
target/release/hide-agent-context-labels verify-provider --provider claude
```

Each prints that provider's availability and one verdict for a fixed transcript with the answering provider named on it, and exits non-zero when the provider cannot answer, naming the class.
For codex it also prints the app-server's descendant count and resident size, so a leak is visible from the command rather than only from the log.
The pid held and measured is whatever `codex` on `PATH` starts: the pnpm install's node wrapper, whose one descendant is the real `codex app-server`, so a healthy tree prints `descendants=1` (measured 2026-09-20; a native binary prints 0), and the first MCP server would make it 2.
Only `watch` moves state left under the plugin's previous id; verification does not.
Still give a development build its own home so it never writes next to the installed watcher: `CODEX_HOME=$HOME/.codex HOME=$(mktemp -d) target/release/hide-agent-context-labels verify-provider --provider codex`.
The assignments are expanded left to right, so `CODEX_HOME` has to be written before `HOME` is replaced; `HOME=$(mktemp -d) CODEX_HOME=~/.codex` expands `~` against the temporary home and reports `needs_login`.
The claude path cannot take the isolated home, because Claude Code keys its login to `HOME`; run it with the real home and expect one `live_provider_verified` line in the installed watcher's `events.jsonl`.

## Known gaps

- A Claude usage limit carries no reset time the code can read.
  The 429 result frame names the window only inside a localised sentence in `result` ("You've hit your session limit, resets 6:42pm (Asia/Seoul)"), so the backend reports `UsageLimited { retry_after: None }` and the router's ten minute default cooldown applies to a window that is really five hours or seven days.
  Keeping the shared default is a decision taken on 2026-09-10, not an oversight: a 429 result frame reports `usage` zeros and `total_cost_usd` 0, so the park is self-correcting at the cost of about a second of latency and one log line every ten minutes, and it is what notices the reset soonest.
  Closing this properly needs a structured reset field in the result frame, not a parser for that sentence.
- The Claude backend does not check that the configured model is one the account offers, because Claude Code exposes no model list command; the codex backend checks `model/list`.
  A model the account cannot use is therefore discovered as a request failure rather than as an availability state.
  The same gap is why `ClaudeCliBackend::models()` answers from `claude::MODEL_ALIASES` rather than from the CLI: `claude --help` documents `--model` with three of the four aliases as examples and there is no list subcommand, and parsing that sentence would be a worse contract than naming them.
  It is the one model list in the codebase a provider is not asked for, and it stays inside the provider boundary; neither the core nor the shell holds one.
  The Settings model control always offers the configured model even when it is not on the provider's list, so reaching the screen never silently changes the operator's choice.
- A Claude usage limit has not been observed against the live account.
  The mapping was measured end to end instead, by answering the CLI's own API request with each HTTP status and reading the frame it printed; the run that measured it is local evidence, not a tracked file.
- Weekly usage in the toolbar reads Codex's usage endpoint, with Codex session JSONL as an offline fallback only, and Claude Code's `/usage` text; the router's daily rollup is a log line, not a popover value.
