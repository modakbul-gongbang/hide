# Integrated native preflight

This spike proves one macOS process and one AppKit window with explicit pixel ownership.
AppKit owns `NSApplication`, the `NSWindow`, native child views, focus, resize, and shortcut routing.
WGPU 30.0.1 owns IDE chrome, terminal, and editor pixels through Metal.
CEF owns Browser pixels in its native child `NSView` through `WindowInfo::set_as_child`.
The browser is never copied through OSR or a WGPU texture.

## Lifecycle boundary

Browser-closed and browser-included are separate launch modes.
`--browser-closed` does not load or initialize CEF and therefore must start no CEF helper.
Browser-included requires an explicit `http://127.0.0.1:<port>/` URL, absolute persistent profile path, and CDP port.
It creates the CEF-compatible `NSApplication` subclass first, installs the AppKit delegate, calls `finishLaunching()`, and then lets `cef::run_message_loop()` drive events.
The context-ready and AppKit-parent callbacks converge through `create_if_ready`, so neither callback order is assumed.
Dynamic CEF initialization inside a browser-closed process is intentionally outside this contract.

`Cmd+Shift+Enter` is routed centrally by the application subclass when CEF is active and by the WGPU `NSTextInputClient` view when CEF is dormant.
Terminal and editor zoom hide the CEF parent.
Browser zoom keeps the WGPU navigator, 46-point tab strip, zoom indicator, and 30-point status strip visible while CEF expands only across the remaining canvas.

## Pinned inputs

- Rust crate `cef = 151.8.0+151.3.24`, upstream tag `cef-v151.8.0+151.3.24`, peeled commit `a2e15ae659c4b3957883e34de879bd8b38360ce5`.
- `objc2 = 0.6.4`, `objc2-app-kit = 0.3.2`, and `objc2-foundation = 0.3.2`.
- `wgpu = 30.0.1` with Metal only.
- `portable-pty = 0.9.0` for the real `/bin/zsh -f` child.

`Cargo.lock` pins the exact crates.io checksums because Cargo ignores semver build metadata while resolving version requirements.
cef-rs and objc2 are dual MIT or Apache-2.0.
The CEF binary distribution's BSD-style license and third-party notices must remain in the shipped framework resources.
CEF embeds Chromium and requires an owned security update cadence, archive provenance checks, rebuilds for every supported architecture, Developer ID signing, hardened runtime, notarization, and stapling before distribution.

Primary references:

- [cef-rs repository](https://github.com/tauri-apps/cef-rs/tree/cef-v151.8.0%2B151.3.24)
- [CEF general usage](https://chromiumembedded.github.io/cef/general_usage.html)
- [CEF macOS application protocol](https://github.com/chromiumembedded/cef/blob/master/include/internal/cef_application_mac.h)
- [wgpu documentation](https://docs.rs/wgpu/30.0.1/wgpu/)
- [objc2 documentation](https://docs.rs/objc2/0.6.4/objc2/)
- [portable-pty documentation](https://docs.rs/portable-pty/0.9.0/portable_pty/)

## Build

```sh
./scripts/build-release.sh
```

The output is `target/bundle/herdr-integrated-preflight.app`.
The script checks the main executable, CEF framework, five helper bundles, Info.plist, ad-hoc signature, architecture, bundle size, and main executable SHA-256.
The machine-readable result is `target/evidence/bundle-manifest.json`.

## Focused verification

```sh
cargo test --locked
cargo tree --locked -i objc2
```

The layout tests fix the cross-surface zoom frame and exact restore contract.
The browser tests fix dormant closed mode and literal `127.0.0.1` inputs.
The preflight tests fix telemetry identity and fail-closed artifact ownership.

## Runtime modes

Closed mode:

```sh
target/bundle/herdr-integrated-preflight.app/Contents/MacOS/herdr-integrated-preflight \
  --browser-closed \
  --t1-preflight-phase warm \
  --t1-preflight-events "$PWD/target/evidence/closed.events.jsonl" \
  --t1-preflight-scenario "$PWD/fixture/scenario-browser-closed.json"
```

Included mode requires an owned loopback fixture server:

```sh
python3 -m http.server 45731 --bind 127.0.0.1 --directory fixture
target/bundle/herdr-integrated-preflight.app/Contents/MacOS/herdr-integrated-preflight \
  --browser-url http://127.0.0.1:45731/ \
  --browser-profile "$PWD/target/fixture-profile" \
  --remote-debugging-port 45732 \
  --t1-preflight-phase warm \
  --t1-preflight-events "$PWD/target/evidence/included.events.jsonl" \
  --t1-preflight-scenario "$PWD/fixture/scenario-browser-included.json"
```

The JSONL schema is `herdr.t1-preflight.telemetry.v1` with run identity, PID, phase, gap-free sequence, and strictly increasing monotonic timestamp.
The app emits only events it owns and observes.
The external harness owns screenshots, accessibility capture, process-tree profiling, chromux attach and detach, persistence comparison, and relaunch comparison.

CEF's raw remote debugging port has no stable per-pane capability token.
This direct port is acceptable only for the disposable single-Browser spike.
The product integration must put a loopback-only session-capability gateway in front of CEF and expose only the target mapped to the stable Browser pane ID.
