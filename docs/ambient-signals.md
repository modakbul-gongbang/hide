# Ambient Session Signals

How the Pet card shows subagent and background-task counts, and what it
never shows.

## What this is

A Herdr server can attach an optional `ambient` object to a pane's snapshot:

```json
{ "subagents_active": 2, "background_running": 1, "background_failed": 0 }
```

When present, `herdr-core::parse_snapshot` reads it into
`AgentSnapshot.ambient` and Pet renders small count badges on that pane's
card - never task names, commands, prompts, output, or file paths. See
`AgentSnapshot` in `crates/herdr-core/src/model.rs` and `AmbientSignal`.

## Scope boundary (read this before assuming more than this does)

**Only the client side lives in this repository.** The Herdr server - the
process that would actually read Claude/Codex session files - is a separate
closed binary (`~/.local/bin/herdr`) whose source is not in `herdr-pet` or
anywhere under this machine's `~/projects`. This repo cannot implement or
verify:

- Reading Claude managed-background-task records or Codex `unified_exec`
  lifecycle records from session files.
- mtime-gated polling, 8 MiB bounded reconstruction, or startup rebuild.
- The server-side authorization toggle and its restart requirement.

Everything below the "Server contract" heading is a **specification for
whoever implements that server-side reader**, not a description of code that
exists in this repository. This repo only consumes the `ambient` field if
and when a server sends it, and degrades safely when it doesn't.

## Client behavior

- **Per-target opt-in.** Settings has a toggle per target ("target" =
  `TargetConfig.id`, i.e. a configured local/remote Herdr connection). Off by
  default. Turning it on renders immediately from whatever `ambient` value
  is already cached for that target's panes; if nothing is cached yet, badges
  stay hidden until the next valid snapshot arrives. The toggle never calls
  any server configuration or restart - this app has no such command.
- **Hide on zero.** A badge only appears when its count is 1 or more.
- **Last-valid-state on malformed records.** If a snapshot's `ambient` key is
  present but its shape can't be parsed, the card keeps showing the last
  successfully parsed value for that pane (kept in memory only, never on
  disk) and Settings shows a neutral compatibility notice for that target.
  The next valid record clears the notice automatically.
- **Hidden, not warned, when absent.** If a target's snapshot never carries
  an `ambient` key at all (legacy server, or server-side authorization off),
  the card shows no ambient badges and no error - Settings shows "표시할
  ambient 데이터 없음" for that target, since the client cannot tell those
  two causes apart.
- **재시도 (retry) in Settings** just re-fetches the current snapshot. It is
  read-only by construction: there is no command in this app that changes a
  server's configuration or restarts anything.

## Privacy boundary

The only values this client ever reads from `ambient` are three counts. Any
other key in that object, or a value of the wrong type, is dropped during
parsing and never reaches app state, the rendered UI, or logs - proven per
surface:

- Parsing: `unknown_ambient_keys_and_sentinel_content_never_survive_parsing`
  in `crates/herdr-core/src/herdr.rs`.
- The exact `PetState` JSON the frontend consumes:
  `sentinel_ambient_content_never_reaches_serialized_pet_state` in
  `apps/pet-app/src-tauri/src/main.rs`.
- Rendered HTML: the sentinel-key test in `web/ambient-badges.test.mjs`
  (`node --test web/ambient-badges.test.mjs`).
- Logs/error strings: this app has no logging call site at all around agent
  data, enforced by
  `no_source_file_touching_ambient_data_contains_a_logging_call_site` in
  `apps/pet-app/src-tauri/src/main.rs`.

This matches the product decision that ambient badges show counts only,
never task names, commands, prompts, stdout/stderr, or paths.

## Server contract (implementation note for the Herdr server, not built here)

A Herdr server that wants to support this feature should:

- Add an optional `ambient: { subagents_active, background_running,
  background_failed }` object to each pane in `session.snapshot`. Omit the
  key entirely (not `null`, not zeros) when authorization is off or the pane
  isn't a supported agent kind - Pet treats "key absent" as the legacy/off
  case.
- Read only lifecycle metadata: identifiers, parent/child relationships,
  managed-background markers, completion state, exit code. Never persist,
  transmit, or log prompt text, conversation text, command contents,
  stdout/stderr, task descriptions, or file paths.
- Discover session directories only from each pane's actual Claude/Codex
  process `HOME` - never scan an account home broadly.
- Read no session file at all while server-side authorization is off,
  including at startup.
- Treat Claude managed background tasks and Codex `unified_exec` long-running
  tasks as in scope; treat detached shell children (nohup, daemons) as
  explicitly out of scope - their liveness isn't reliably recoverable from
  session files.
- Require a server restart to apply a change to the authorization setting.

## Fallback and compatibility

A Pet connected to a server that never sends `ambient` behaves exactly as it
did before this feature: no badges, no warnings, no behavior change. This is
enforced by treating `ambient` as fully optional at every layer -
`AgentSnapshot.ambient: Option<AmbientSignal>` with `#[serde(default)]`.
