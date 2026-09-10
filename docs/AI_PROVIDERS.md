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
- `claude`: registered and reported as `unsupported`.
  The installed Claude Code and the pinned Herdr API expose a stable way to submit a prompt to a logged-in session and to learn when its turn completed, but no way to receive a structured answer for a given request or to keep the request out of the user's own conversation.
  Print mode, headless invocation, terminal scraping and token reuse are excluded by decision, so the state is modelled rather than worked around.
  The backend answers `Availability::Unsupported { reason }` and `AiError::Unsupported`, both caller-visible.

## Selection and fallback

With one provider connected, that provider is used.
With more than one, the configured priority decides: `codex`, then `claude`.

Every failure is one of two families.
A refusal (`ProviderUnavailable`, `NotAuthenticated`, `UsageLimited`, `Unsupported`, `Transient`, `InvalidOutput`) means the provider never took the request, so asking again repeats nothing.
A completion-unknown outcome (`Timeout`, `Cancelled`, `CompletionUnknown`) means the request was submitted and its fate is not known: the deadline passed, the caller cancelled, or the connection or child was lost after `turn/start` went out.
The codex backend draws that line at the `turn/start` write: a lost child before it is `ProviderUnavailable`, a lost or unanswered `turn/start` and anything after it is `CompletionUnknown`.

A request moves to the next provider only on `ProviderUnavailable`, `NotAuthenticated`, `UsageLimited` or `Unsupported`, and the move is written to the log as `ai.fallback` with `from` and `to`.
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
- Duplicate suppression: a second request with the same feature id, subject id and input hash joins the in-flight one instead of starting a new call.
  The in-flight entry is owned by a guard, so a leader that panics still wakes its joiners and releases the key.

What the caller does after the router gives up is the caller's decision.
The context-label plugin parks a turn for good on a settled failure and asks again after ten minutes on an environmental one.

## Logging

The router emits `ai.attempt`, `ai.request.finished`, `ai.request.joined`, `ai.fallback` and, once per UTC day, `ai.daily_rollup`.
A line carries the request id, feature id, provider, outcome class, attempt, duration, input length, output tokens and schema version.
It never carries the prompt, the input, the generated text, a token, a file path from a transcript, or a provider thread id.
The plugin writes these lines into its own `events.jsonl` next to its watcher events.

## Verifying a provider

From the repository root, after `cargo build --release --locked -p agent-context-labels`:

```sh
target/release/hide-agent-context-labels verify-provider --provider codex
target/release/hide-agent-context-labels verify-provider --provider claude
```

The first prints `codex=ready` and one verdict for a fixed transcript with `provider=codex` on it; the second prints `claude=unsupported` and exits non-zero, which is the documented state.
Only `watch` moves state left under the plugin's previous id; verification does not.
Still give a development build its own home so it never writes next to the installed watcher: `HOME=$(mktemp -d) CODEX_HOME=~/.codex`.

## Known gaps

- A Claude backend needs one of two contracts that do not exist today: a Herdr method that returns the agent's answer for a request id without reading the terminal, or a Claude Code interface that accepts a schema-bound request outside the user's session.
  Either one would let `ClaudeInteractiveBackend` move from `Unsupported` to `Ready` without touching any feature.
- Usage display in the sidebar reads what the CLIs cache locally (`herdr-core/src/usage.rs`); the router's daily rollup is a log line, not a sidebar value.
