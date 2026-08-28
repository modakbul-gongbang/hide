# Atomic agent creation and durable lineage

## Result

T3 adds one authoritative `agent.new` mutation to Herdr protocol revision 21 and exposes it as `herdr agent new`.
The operation creates the split pane, starts the requested agent process, records stable lineage, and publishes events as one commit boundary.
The implementation lives in `/Users/hoyeonlee/projects/herdr`, while this worktree preserves its architecture record and focused source patch.

## Atomic operation contract

Every request carries an `OperationContext.idempotency_key` plus the target pane, split direction, agent kind, arguments, and optional source pane.
Herdr hashes the normalized request payload and stores that fingerprint with the lineage record.
Submitting the same key and same payload returns the original stable agent and pane identities with `replayed: true` and does not create a second process or pane.
Submitting the same key with a different payload returns the typed `idempotency_conflict` error.

Pane creation uses the existing provisional split path and starts the argv process before layout commit.
If process spawn fails, Herdr returns `agent_new_spawn_failed` and rolls the split back, so snapshots cannot contain a phantom surface.
After process startup succeeds, Herdr commits runtime ownership and lineage before publishing `pane.created`, layout, and `agent.lineage_changed` events.

## Durable lineage lifecycle

The workspace owns `AgentLineageRecord` values independently of terminal detector metadata.
Each record keeps the stable agent instance ID, host/workspace/tab/pane identity, parent agent ID, spawned-from pane ID, normalized argv, idempotency key, and request fingerprint.
Workspace snapshots persist the records so session restore preserves the tree without parsing terminal output.
Renaming an agent updates the durable record.
Closing a parent pane marks its projected lineage state as `ended` while descendants keep the same parent relation.
A missing referenced parent is projected explicitly as `orphaned`.

## Public surfaces

- `src/api/schema/agents.rs` owns `AgentNewParams`, `AgentLineageInfo`, and lineage lifecycle state.
- `src/api/schema/response.rs` owns the replay-aware `AgentCreated` response.
- `src/api/schema/events.rs` and `src/api/event_hub.rs` own ordered lineage events.
- `src/app/agents.rs` owns validation, fingerprinting, replay, atomic process/layout commit, rollback, rename, and lineage projection.
- `src/workspace.rs`, `src/persist/snapshot.rs`, and `src/persist/restore.rs` own durable records and restart restoration.
- `src/cli/agent.rs` and `src/cli/spec.rs` expose the same contract to terminal callers.
- `tests/api_ping.rs` proves the behavior against isolated real Herdr server processes.

## Verification

The complete schema suite passed 43 tests.
The complete real-server `api_ping` integration target passed 12 tests.
The T3 integration test proved exact replay identity, a single pane for duplicate submission, parent-child creation, rename, ended-parent retention, and failed-spawn rollback with no pane-count change.
The workspace persistence round trip and schema request round trip passed.
`cargo check --all-targets` and `git diff --check` passed with Zig 0.15.2 and the existing shared target.

The focused commands were:

```text
ZIG=/opt/homebrew/opt/zig@0.15/bin/zig CARGO_TARGET_DIR=/Users/hoyeonlee/projects/herdr/target CARGO_INCREMENTAL=0 cargo test --locked --bin herdr api::schema::tests
ZIG=/opt/homebrew/opt/zig@0.15/bin/zig CARGO_TARGET_DIR=/Users/hoyeonlee/projects/herdr/target CARGO_INCREMENTAL=0 cargo test --locked --test api_ping
ZIG=/opt/homebrew/opt/zig@0.15/bin/zig CARGO_TARGET_DIR=/Users/hoyeonlee/projects/herdr/target CARGO_INCREMENTAL=0 cargo check --all-targets
```

## Acceptance boundary

T3 establishes the atomic create, prompt/read-ready identity, and durable lineage contract required by AC10 and AC11.
The two real Codex/Claude nonce exchange is intentionally not claimed here and remains a V6 acceptance action after the native pane runtime consumes this contract.
IDE and terminal callers share the same server mutation, so the native IDE must not invent a separate local lineage path in later tasks.

## Scope correction

Formatter-only changes in `src/app/ambient.rs`, `src/platform/mod.rs`, and the pre-existing `AmbientInfo` derive were restored to `HEAD` before this record was finalized.
Only substantive T3 additions remain in `src/api/schema/agents.rs`.
No installed app, remote host, real user workspace, user-owned pane, Electron reference, or standalone Pet was modified.
