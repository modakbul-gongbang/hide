# T1 macOS preflight harness

## Outcome

`tools/t1-preflight/t1-preflight` verifies a release `.app` from one fixture manifest and writes one reviewable evidence directory owned by that manifest.

The verifier is fail-closed.

A missing hook, missing resource, undeclared helper, extra app instance, invalid telemetry line, absent Accessibility label, failed screenshot, insufficient sample count, or exceeded budget produces a structured failure and a nonzero exit.

The integrated release bundle is available at `spikes/integrated-preflight/target/bundle/herdr-integrated-preflight.app`.

The fresh four-phase run completed with `PASS` at `spikes/integrated-preflight/target/evidence/runtime-v1/result.json`, SHA-256 `16eebc6009e3ab53f5f2ea7dd3f42cd8302d7f20b9577fd651451bc60e989a68`.

Passing dry-run remains insufficient T1 acceptance evidence.

## Commands

Run stable-boundary tests from the tool directory:

```sh
cd tools/t1-preflight
python3 -m compileall -q t1_preflight tests
python3 -m unittest discover -s tests -v
```

Validate the fixture and show every planned native action without opening an app:

```sh
./t1-preflight \
  --mode dry-run \
  --app "/Applications/Herdr IDE.app" \
  --manifest fixtures/example-manifest.json
```

Inspect only the release bundle:

```sh
./t1-preflight \
  --mode static \
  --app "/absolute/path/Herdr IDE.app" \
  --manifest /absolute/path/t1-manifest.json
```

Collect full native evidence after the app implements the contract below:

```sh
./t1-preflight \
  --mode run \
  --app "/absolute/path/Herdr IDE.app" \
  --manifest /absolute/path/t1-manifest.json
```

## Manifest ownership

The schema is `herdr.t1-preflight.manifest.v1`.

`output_dir` and `runtime.scenario` must be relative to the manifest directory and cannot contain `..`.

The manifest digest includes the referenced scenario digest.

The output directory receives `.t1-preflight-owner.json` before any evidence.

An existing directory without that exact ownership marker is never modified.

An incomplete owned directory is also left untouched and requires human review.

A completed rerun with the same manifest, scenario, mode, app path, and whole app-bundle digest returns the previous result without launching the app again.

This makes an accidental second invocation convergent while keeping incomplete evidence observable.

## Release bundle contract

The `bundle` object declares these values:

- `identifier` is the required `CFBundleIdentifier`.
- `main_executable` is the bundle-relative path under `Contents/MacOS`.
- `architectures` is the required architecture set, normally `arm64`.
- `executables` contains the main executable and every nested `.app` or `.xpc` helper executable.
- `browser_executables` is the nonempty subset of helper executables that must be absent from browser-closed launches and present in the browser-included launch.
- `required_rpaths` maps an executable path to its required `LC_RPATH` values.
- `resources` lists required bundle files and may pin their SHA-256 digests.

Static verification checks `Info.plist`, `NSHighResolutionCapable`, deep strict code signing, release entitlements, every declared Mach-O executable, helper declaration completeness, architecture, per-executable signing, rpaths, linked dependencies, and resource digests.

An rpath must start with `@loader_path`, `@executable_path`, or `@rpath`.

A linked dependency must resolve through those bundle-relative forms or through `/System/Library` or `/usr/lib`.

`com.apple.security.get-task-allow=true` is rejected as a debug entitlement.

## App CLI contract

The manifest renders only readiness and state telemetry arguments.

The minimum invocation shape is:

```text
Herdr IDE \
  --t1-preflight-phase {clean_closed|warm_closed|browser_included|relaunch_closed} \
  --t1-browser-mode {browser-closed|browser-included} \
  --t1-preflight-events /manifest-owned/output/runtime/<phase>.events.jsonl \
  --t1-preflight-run-id <run_id>
```

The app must not execute the scenario or synthesize interaction internally.

The scenario is private to the harness.

The app only exposes current native state and presentation timing after externally injected input.

The app must append and flush one JSON object per line.

Every native input injection has a last-safe-point focus precondition.

The newest telemetry event must carry a `focus` object with boolean `app_frontmost`, `key_window`, and `render_view_first_responder` fields.

If the snapshot is missing, malformed, or any field is false, the harness aborts with `environment_focus_interference` before posting the CGEvent.

That outcome is environment interference, not a product input failure, and the harness never attempts to steal focus or bypass the user's active application.

Every line has these identity fields:

```json
{
  "schema": "herdr.t1-preflight.telemetry.v1",
  "run_id": "t1-native-example",
  "phase": "warm_closed",
  "pid": 12345,
  "seq": 1,
  "monotonic_ns": 1000000,
  "event": "app.usable"
}
```

`seq` starts at one and increases by exactly one.

`monotonic_ns` strictly increases within a phase.

The PID, run ID, and phase must match the process launched by the harness.

Malformed, rewritten, truncated, reordered, or identity-mismatched telemetry fails the run.

## Required telemetry

The manifest declares four exact phases and maps each one to an explicit launch mode.

`clean_closed` and `warm_closed` must use the same browser-closed launch mode.

`browser_included` must use the exact browser-included launch mode and declares its bounded `profile_settle_ms` in the manifest.

`clean_closed`, `warm_closed`, and `relaunch_closed` must all use the exact browser-closed launch mode.

The harness never turns the Browser on dynamically inside the browser-closed process.

The `clean_closed` phase emits `app.usable` with `launch_mode=browser-closed` and then emits `phase.complete` when the harness sends native Cmd+Q.

The `warm_closed` phase emits the following state and presentation events:

- `app.usable` means the native main window has presented a usable frame and includes `launch_mode=browser-closed`.
- `fixture.ready` includes `window_id`, `workspace_count=7`, `pane_count=11`, `browser_open=false`, `cef_initialized=false`, and `cg_screen_points` for `terminal` and `editor` hit targets.
- `terminal.input_presented` includes the unique native-input `probe`, `input_monotonic_ns`, and `present_monotonic_ns` after that probe is visible in the real terminal pane.
- `zoom.entered` includes `window_id` and `before_topology_hash` after native Cmd+Shift+Enter.
- `zoom.restored` includes `after_topology_hash` after the second native Cmd+Shift+Enter.
- `focus.changed` includes `from` and `to` after a CGEvent mouse click.
- `window.resized` includes `logical_width`, `physical_width`, and `scale_factor` after an Accessibility resize action.
- `ime.committed` includes `text` and `marked_observed=true` after physical key codes pass through the active Korean input method.
- `state.persisted` includes `state_hash` for the state that the relaunch phase must restore.
- `phase.complete` is flushed before the process exits in response to native Cmd+Q.

The `browser_included` phase is a separate release-app process with `launch_mode=browser-included`.

It emits `app.usable`, then emits `browser.profile.ready` with `browser_open=true`, `cef_initialized=true`, `window_id`, and a loopback `cdp_http_endpoint`.

The app does not choose or report the profile settle interval.

The harness waits the manifest-owned `runtime.phases.browser_included.profile_settle_ms` before sampling.

The harness proves at least one manifest-declared Browser helper exists in that process tree, queries CDP `/json/version` and `/json/list`, captures the Browser window, and measures the separate process-tree RSS profile.

The `browser_included` phase also emits `phase.complete` in response to native Cmd+Q.

The `relaunch_closed` phase emits `app.usable` with `launch_mode=browser-closed`, `relaunch.restored` with the matching `state_hash`, and `phase.complete` after native Cmd+Q.

`phase.complete` is state telemetry for a real native termination request, not permission for an internal auto-exit.

The app may emit `input.focus.state` between required events so the harness can re-read focus immediately before every key or mouse injection.

## Native action boundary

The runtime harness routes all CGEvent key and mouse calls through one wrapper that checks the newest focus snapshot immediately before posting the event.

The release app's structured runtime report also records the same focus state and a bounded routing timeline.

Timeline sources are intentionally separate: `appkit` is the received user/native event, `app-routing` is IDE command or input dispatch, `herdr` is an external topology request or snapshot, and `pty` is terminal transport.

This separation makes an unexpected `pane.split` observable without attributing it to a user key when the event came from Herdr state synchronization.

The harness activates the exact launched PID through System Events.

Terminal probes are posted as CGEvent Unicode keyboard events and submitted with a physical Return key event.

Cmd+Shift+Enter zoom is posted as a CGEvent physical key event.

Pane focus is changed with a CGEvent mouse click at the screen coordinate published by `fixture.ready`.

Window resize is performed through the macOS Accessibility window action.

Screenshots use `/usr/sbin/screencapture -l <window_id>` against the real native window.

The AX snapshot comes from System Events against the exact launched PID.

Korean IME proof uses physical Dubeolsik key codes, not app-internal selector calls and not a direct committed string.

The manifest declares the required Korean input source ID.

The harness verifies that this input source is already active and fails if it is not.

The harness does not mutate the user's global input-source setting.

The app must report that marked text was observed before the expected Korean commit.

## Process and performance evidence

Before each phase, the harness resolves executable identity with macOS `libproc` and rejects any pre-existing process running the exact main executable.

After launch, exactly one main executable PID must exist and it must equal the PID launched by the harness.

Process-tree samples include the root app, bundle-contained helper executables, and only the absolute external executables explicitly listed by the manifest, such as `/bin/zsh`.

An undeclared descendant fails the run.

RSS is the maximum sum of resident KiB across the process tree during each sample window.

CPU is the average sum of `%CPU` across the browser-closed process-tree samples after the manifest-owned settle delay.

Browser-closed sampling also requires `cef_initialized=false` telemetry and zero manifest-declared Browser helper processes.

Browser-included sampling requires `cef_initialized=true`, at least one manifest-declared Browser helper process, and a healthy loopback CDP page target.

The required budgets are:

| Check | Budget | Aggregation |
| --- | ---: | --- |
| Warm usable | `<= 1000 ms` | Harness monotonic time from process spawn to observed `app.usable` |
| Terminal input to present | `p95 <= 50 ms` | Nearest-rank p95 across at least 20 externally injected probes |
| Idle CPU | `<= 1%` | Mean process-tree CPU in the browser-closed profile |
| Browser closed RSS | `<= 200 MiB` | Maximum process-tree RSS with 7 workspaces and 11 panes |
| Browser included RSS | `<= 800 MiB` | Maximum process-tree RSS in the separate browser-included launch |

The manifest may choose a stricter budget but cannot loosen any of these hard ceilings.

The `clean_closed` phase is the first browser-closed launch after proving no exact instance exists.

The immediately following `warm_closed` phase uses the same browser-closed launch mode and is the measured warm launch.

The `browser_included` phase then measures CEF and CDP in its own owned process.

The final `relaunch_closed` phase proves the closed-mode state persisted through actual process exit and relaunch.

## Accessibility and visual evidence

`runtime.expected_ax_labels` declares the minimum meaningful Accessibility labels.

The external AX query must find every label in the actual window tree.

Code declarations are not AX evidence.

At least one split screenshot and one zoom screenshot must be real nonempty image files captured from the native window ID.

Code declarations and process existence are not visual evidence.

The semantic verdict also requires exact zoom topology restore, a real focus transition, Retina resize consistency, and matching persisted and relaunched state hashes.

## Failure and cleanup policy

All command failures include argv, exit code, stdout, stderr, and duration in structured evidence.

All harness contract failures emit a JSON object to stderr and write `failure.json` when the output directory is already owned.

The harness never kills a pre-existing process.

On a failure after launch, it resolves the launched PID's executable again and sends SIGTERM only when that path still equals the exact manifest app executable.

It never sends SIGKILL automatically.

It never terminates a descendant process directly.

A descendant or root process that survives normal app exit remains visible as a failed run for human investigation.

## Evidence layout

A full run produces these manifest-owned artifacts:

```text
output_dir/
  .t1-preflight-owner.json
  bundle-evidence.json
  runtime-evidence.json
  result.json
  runtime/
    clean_closed.events.jsonl
    clean_closed.log
    warm_closed.events.jsonl
    warm_closed.log
    browser_included.events.jsonl
    browser_included.log
    relaunch_closed.events.jsonl
    relaunch_closed.log
  ax/
    *.txt
  screenshots/
    *.png
```

`result.json` is `PASS` only when static bundle inspection, runtime budgets, native interactions, AX evidence, screenshots, zoom restore, focus, resize, and relaunch all pass.

`STATIC_PASS` and `DRY_RUN` are narrower outcomes and must never be presented as full T1 acceptance.
