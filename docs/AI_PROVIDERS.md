# Background AI providers

`hide-ai/` is the one boundary through which a Hide feature asks a language model for something in the background.
The first consumer is the context-label plugin under `plugins/agent-context-labels/`; later features (naming, summarising, classifying) reuse the same boundary rather than a provider client of their own.
This guide owns the boundary's rules; the code under `hide-ai/src/` and the tests under `hide-ai/tests/` are its executable authority.

## Ownership split

A feature owns its prompt, its output schema, and the parsing of the answer into its own type.
It submits an `AiRequest` (feature id, caller-made request id, subject id, system prompt, input, schema, deadline, schema version) and receives an `AiResult`: a `serde_json::Value` already validated against that schema, and the provider that produced it.
There is no output token ceiling in the request, because no provider contract honours one; the prompt and the schema are what keep an answer short.

`hide-ai` owns everything about providers: which are installed and logged in, the child process and its protocol, timeouts and cancellation, the retry policy, fallback between providers, duplicate suppression, and the structured error a caller branches on.
No feature type lives in the crate, and no provider detail leaks out of it.

## Providers

The user's installed and logged-in CLIs are the only credentials.
Hide reads no token file and adds no environment variable.

- `codex`: `codex app-server --listen stdio://`, the official JSON-RPC surface of the Codex CLI, driven with an ephemeral read-only thread, the feature's system prompt as the base instructions, every optional feature disabled, and the feature's output schema attached to the turn.
  The default model is `gpt-5.6-luna`.
  Availability is read from `account/read`; the thread leaves nothing in `~/.codex/sessions`.
  `codex exec` is not used.
- `claude`: `claude -p --output-format json`, print mode, one child process per request.
  Print mode is Claude Code's only official structured-output surface; there is no `app-server` equivalent in `claude --help`.
  It was excluded by decision until 2026-09-10, when that exclusion was withdrawn; terminal scraping and token reuse remain excluded, and no credential file is read.
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

## Selection and fallback

With one provider connected, that provider is used.
With more than one, the configured priority decides: `codex`, then `claude`.

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

The router emits `ai.attempt`, `ai.request.finished`, `ai.request.joined`, `ai.fallback`, `ai.provider.degraded`, `ai.provider.recovered` and, once per UTC day, `ai.daily_rollup`.
A line carries the request id, feature id, provider, outcome class, attempt, duration, input length, output tokens and schema version.
It never carries the prompt, the input, the generated text, a token, a file path from a transcript, or a provider thread id.
The plugin writes these lines into its own `events.jsonl` next to its watcher events.

## Verifying a provider

From the repository root, after `cargo build --release --locked -p agent-context-labels`:

```sh
target/release/hide-agent-context-labels verify-provider --provider codex
target/release/hide-agent-context-labels verify-provider --provider claude
```

Each prints that provider's availability and one verdict for a fixed transcript with the answering provider named on it, and exits non-zero when the provider cannot answer, naming the class.
Only `watch` moves state left under the plugin's previous id; verification does not.
Still give a development build its own home so it never writes next to the installed watcher: `HOME=$(mktemp -d) CODEX_HOME=~/.codex`.

## Known gaps

- A Claude usage limit carries no reset time the code can read.
  The 429 result frame names the window only inside a localised sentence in `result` ("You've hit your session limit, resets 6:42pm (Asia/Seoul)"), so the backend reports `UsageLimited { retry_after: None }` and the router's ten minute default cooldown applies to a window that is really five hours or seven days.
  Keeping the shared default is a decision taken on 2026-09-10, not an oversight: a 429 result frame reports `usage` zeros and `total_cost_usd` 0, so the park is self-correcting at the cost of about a second of latency and one log line every ten minutes, and it is what notices the reset soonest.
  Closing this properly needs a structured reset field in the result frame, not a parser for that sentence.
- The Claude backend does not check that the configured model is one the account offers, because Claude Code exposes no model list command; the codex backend checks `model/list`.
  A model the account cannot use is therefore discovered as a request failure rather than as an availability state.
- A Claude usage limit has not been observed against the live account.
  The mapping was measured end to end instead, by answering the CLI's own API request with each status and reading the frame it printed; see `agents/runs/hide-ai-claude-backend/claude-print-mode-measurements.md`.
- Usage display in the sidebar reads what the CLIs cache locally (`herdr-core/src/usage.rs`); the router's daily rollup is a log line, not a sidebar value.
