# Verification Fixtures

How pet and shell behaviour is driven to a known state without a real agent.

## Rust: in-memory snapshot payloads

`herdr-core`'s tests build herdr snapshot-shaped JSON in memory. They cover
the `_new` unseen-token contract, per-agent state projection and ordering,
ambient parsing, the pet's pose ladder and sleep sequence, badge buckets,
the oldest-unseen click key, per-item exclusion of a broken agent record, and
the connection lifecycle (a malformed sync update keeps the last valid agent
list and reports the failure).

```sh
cargo test --manifest-path herdr-core/Cargo.toml
```

`herdr-core/tests/fixtures/` holds only the two UI-state files the load path
needs: a corrupt one, and a path that is deliberately absent. Nothing may
write to that absent path - a test that needs to persist state uses its own
temporary file.

## Swift: bundled theme and pure policy

`macos/Tests/HerdrMacOSTests` loads the real bundled theme from the
repository (located from `#filePath`, not the working directory) and asserts
that every pose the core can report has readable art, that animated webp
plays its own frames, and that sprite sheets slice into equal distinct
frames. The URL-scheme parser, the physical-key shortcut capture, the
off-screen clamp, and the click/drag threshold are pure and tested directly.

```sh
swift test --package-path macos
```

## Runtime: the scenario server

The pet's pose and badge row come from whatever the herdr server reports, and
an unseen error or a two-pane attention race cannot be produced on demand
from real agents. `macos/scripts/pet_scenario_server.py` serves scripted
`session.snapshot`, `events.subscribe`, and `agent.list` responses over a Unix
socket at the same protocol revision the real server speaks, so the app under
test runs its ordinary event-sync path, including the one-second agent refresh
that the roam and sleep transitions depend on.

```sh
macos/scripts/pet_scenario_server.py --socket /tmp/pet.sock --scenario scenario.json &
HERDR_SOCKET_PATH=/tmp/pet.sock macos/build/assembled/HerdrIDE.app/Contents/MacOS/HerdrMacOS \
  --state-path /tmp/pet-state.json \
  --verification-window-receipt /tmp/pet-receipt.json
```

A scenario is `{"agents": [{"pane_id", "state", "summary", "ambient"?}]}`
where `state` is `question`, `approval`, `error`, `working`, `done`, `idle`,
`acknowledged`, or a raw token name.
The file is re-read on every snapshot or agent-list request, so editing it
changes the next refresh; stopping the server produces the disconnected case
and restarting it proves recovery.

## The verification receipt

`--verification-window-receipt <path>` makes the shell write the pet's
observable state as JSON on every change: pose, sleep phase, connection state
and message, badge counts, the attention queue, which pane the last click
selected, the shortcut and its registration error, whether the theme loaded,
and the window's borderless/transparent/always-on-top/shadow flags plus its
requested and resolved origin.

It is the machine-readable half of a screenshot: the image shows what the
user sees, the receipt says what the app believed at that moment.

## Herdr fixture workspaces

`herdr-core/src/bin/herdr-ide-fixture.rs` creates and cleans up real herdr
workspaces and panes for shell-level verification. Every name it will touch
must begin with `herdr-ide-verify-`, and it refuses anything else before it
runs a single command.

No automated check sends keys to a real user agent pane.
