# Herdr protocol v21 vertical slice

## Result

T2 changes the authoritative Herdr protocol from revision 20 to revision 21 without a compatibility shim.
The implementation lives in `/Users/hoyeonlee/projects/herdr`, and the exact uncommitted source delta is preserved in `docs/verification/t2-herdr-protocol-v21.patch` for the implementation record.
The Herdr source checkout remains uncommitted because this approved run is local-only.

## Contract

`PaneInfo` now exposes a stable public `pane_id` plus a tagged `surface` instead of exposing a terminal runtime ID as the pane identity.
Terminal surfaces carry an optional `agent_instance_id` and a required attach endpoint containing host scope, session scope, transport, protocol revision, and refreshable `terminal_id`.
Editor and Browser surface variants carry kind-specific identities and cannot be interpreted as terminals.
`herdr pane attach <pane-id>` resolves the current typed terminal endpoint before attaching.
Agent-only operations on a plain terminal return the machine-readable `not_agent_backed` error.

`session.snapshot` now includes the owning host scope and the event journal sequence captured with the complete resource projection.
Pane collection fails with `snapshot_incomplete` if a layout leaf cannot be projected instead of silently dropping the pane.
An unavailable event journal fails snapshot creation with `event_journal_unavailable` instead of returning sequence zero.

`events.subscribe` requires `after_sequence`.
Domain events carry the protocol revision, host scope, and one global monotonic sequence.
The server replays retained events after the cursor in global order and returns `event_gap` with the retained range and resync instruction when the cursor is stale or ahead.
Parameterized agent-status subscriptions normalize legacy pane aliases to canonical stable IDs once at subscription start.
Polling-only output and scroll subscriptions retain their specialized envelopes while ordered domain events use the v21 sequenced envelope.

`OperationContext` defines the mutation replay identity accepted by the atomic `agent.new` work in T3.
Keys are required, limited to 128 bytes, and restricted to ASCII alphanumeric characters plus `-`, `_`, `.`, and `:`.
The request correlation ID is deliberately separate from the idempotency key.

## Module boundaries

- `src/api/schema/*` owns typed public resources, snapshot cursors, sequenced events, and operation identity.
- `src/api/event_hub.rs` owns the bounded ordered journal and typed gap detection.
- `src/api/server.rs` owns cursor validation, ordered streaming, and externally visible resync errors.
- `src/api/subscriptions.rs` owns parameter normalization at the protocol boundary.
- `src/app/creation.rs` and `src/app/api/session.rs` project complete authoritative runtime state.
- `src/app/terminal_targets.rs` separates terminal attachment from agent-only targeting.
- `src/cli/pane.rs` resolves typed pane attachment.
- `src/protocol/wire.rs` owns the incompatible revision boundary.

## Verification

The schema generation guard passed with 42 schema tests.
The ordered journal suite passed 3 tests.
The subscription server suite passed 4 tests, including global ordering and typed gaps.
The plain-terminal agent target test passed and returned `not_agent_backed`.
The snapshot bootstrap test passed.
The full macOS `api_ping` integration target passed 11 of 11 tests against real spawned Herdr server processes.
That integration pass covered protocol revision 21, typed pane payloads, snapshot cursor resume, mixed polling and ordered subscriptions, agent status transitions, TTL presentation changes, and agent lookup.
`cargo check --locked --bin herdr` passed with only the intentionally pre-T3 unused `OperationContext` warning.
`cargo fmt --all -- --check` and `git diff --check` passed.

The focused commands used the existing shared target with Zig 0.15.2:

```text
ZIG=/opt/homebrew/opt/zig@0.15/bin/zig CARGO_TARGET_DIR=/Users/hoyeonlee/projects/herdr/target CARGO_INCREMENTAL=0 cargo test --locked --bin herdr api::schema::tests
ZIG=/opt/homebrew/opt/zig@0.15/bin/zig CARGO_TARGET_DIR=/Users/hoyeonlee/projects/herdr/target CARGO_INCREMENTAL=0 cargo test --locked --bin herdr api::event_hub::tests
ZIG=/opt/homebrew/opt/zig@0.15/bin/zig CARGO_TARGET_DIR=/Users/hoyeonlee/projects/herdr/target CARGO_INCREMENTAL=0 cargo test --locked --bin herdr api::server::tests::subscriptions_
ZIG=/opt/homebrew/opt/zig@0.15/bin/zig CARGO_TARGET_DIR=/Users/hoyeonlee/projects/herdr/target CARGO_INCREMENTAL=0 cargo test --locked --test api_ping
```

## Assumptions and deviations

Host scope uses the operating system hostname plus the named Herdr session because that pair is already the service routing identity and requires no new persistent registry.
The operation replay type is introduced in T2 and becomes live in the atomic `agent.new` mutation in T3, which removes its temporary dead-code warning.
The proof patch excludes formatter-only drift in `src/api/schema/agents.rs`, `src/app/ambient.rs`, and `src/platform/mod.rs`; those unrelated source hunks were restored to the Herdr checkout before T3 evidence was finalized.
No Electron source, installed app, remote host, real user workspace, or user-owned pane was changed during T2.
