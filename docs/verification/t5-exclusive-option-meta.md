# T5 exclusive Option+F revalidation

The T5 harness owns the `t5-option-meta-v4` verification profile and the `herdr-t5-option-meta-v4-physical` run/output identity.
It never reuses the T1 `runtime-v1` output or its Unicode-probe latency as T5 evidence.

The physical Option+F and latency check is intentionally not run while the MacBook is in active user use.

Run it only inside an agreed hands-off window after confirming that no user-owned app needs the keyboard or pointer.

The harness aborts before the first CGEvent unless the newest app telemetry snapshot reports all three values as `true`:

```text
focus.app_frontmost=true
focus.key_window=true
focus.render_view_first_responder=true
```

After the terminal target click, the harness waits for the first structurally newer `input.focus.state` event.
It never treats the pre-click snapshot as proof for the key injection.
If that fresh event is false or incomplete, the harness raises `environment_focus_interference` before posting the key.

The harness records the macOS frontmost application identity through an in-process ctypes Objective-C runtime boundary calling `NSWorkspace.sharedWorkspace().frontmostApplication()` at each input boundary and at the first focus-loss transition.
The typed identity contains `bundle_identifier`, `localized_name`, `process_identifier`, `executable_name`, and `probe_duration_ms`.
The identity is external to the app telemetry and is stored with the existing focus booleans, event sequence, and monotonic timestamp.

The v4 warm scenario sends one harmless unmodified physical `8` control through the same focus and frontmost observation boundary immediately before Option+F.
The control must prove an `appkit` to `app-routing` to `herdr` to `pty` chain with PTY bytes `38` and must retain the declared bundle identifier and PID after delivery.
Any frontmost mismatch fails closed before the key is posted as `environment_focus_interference`.
The Rust app also emits `app.lifecycle.activate`, `app.lifecycle.resign`, `window.lifecycle.key`, and `window.lifecycle.resign` events so the external timeline can distinguish lifecycle transitions from input routing.

The exact command is:

```sh
cd /Users/hoyeonlee/projects/herdr-ide.worktrees/herdr-ide-rust-native-800mb/tools/t1-preflight
HERDR_T5_EXCLUSIVE_HANDS_OFF=1 \
LC_ALL=en_US.UTF-8 LC_CTYPE=en_US.UTF-8 LANG=en_US.UTF-8 \
./t1-preflight --mode run \
  --app "/Users/hoyeonlee/projects/herdr-ide.worktrees/herdr-ide-rust-native-800mb/spikes/integrated-preflight/target/bundle/herdr-integrated-preflight.app" \
  --manifest "/Users/hoyeonlee/projects/herdr-ide.worktrees/herdr-ide-rust-native-800mb/spikes/integrated-preflight/t5-option-meta-v4-manifest.json"
```

The declared output is `spikes/integrated-preflight/target/evidence/t5-option-meta-v4-physical`.
An existing unowned, incomplete, or identity-mismatched directory is a hard harness error and must not be deleted or reused.

`HERDR_T5_EXCLUSIVE_HANDS_OFF=1` is a human acknowledgement for this destructive-to-focus test surface; it does not grant the harness permission to auto-click, bypass TCC, or force another app out of the foreground.

If focus is lost or cannot be proven, the run ends with structured `environment_focus_interference` evidence and no product FAIL is recorded.

Review the resulting `runtime/*events.jsonl` timeline for `appkit`, `app-routing`, `herdr`, and `pty` sources before interpreting Option+F latency.
The warm phase must contain exactly one `input.plain_key_control.*` chain proving `38`, followed immediately by exactly two `input.option_meta.*` chains, each proving `1b 66` at the PTY boundary with all three focus values true.

The v4 physical run is `NOT_RUN_BY_DESIGN` until the user confirms a fresh exclusive hands-off window.
T5 remains open until this clean, exclusive run proves the control route, Option+F route, and the PRD latency gate.
