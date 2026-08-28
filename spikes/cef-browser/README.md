# CEF native child probe

This probe tests the approved CEF ownership boundary without off-screen rendering or texture copying.
AppKit owns `NSApplication`, the parent `NSWindow`, and its content view.
CEF owns the Browser pixels in the native child `NSView` created by `WindowInfo::set_as_child`.

## Pinned inputs

- `cef = 151.8.0+151.3.24` from crates.io.
- Upstream tag `cef-v151.8.0+151.3.24`, peeled commit `a2e15ae659c4b3957883e34de879bd8b38360ce5`.
- CEF/Chromium version `151.3.24`, downloaded by `cef-dll-sys` from the CEF automated-build index and verified by its archive metadata.
- `objc2 = 0.6.4`, `objc2-foundation = 0.3.2`, and `objc2-app-kit = 0.3.2`.
- cef-rs and objc2 are dual MIT or Apache-2.0.
- The CEF binary distribution keeps its BSD-style and third-party notices inside the framework resources and must ship those notices with the app.

The CEF archive and extracted framework are build inputs under `target/` and are not committed.
No environment variable is part of the probe contract.
The executable requires explicit CLI values for its owned loopback fixture URL, absolute fixture profile path, and CDP port.

## Build

```sh
./scripts/build-release.sh
```

The output is `target/bundle/herdr-cef-probe.app`.
The script checks the main executable, CEF framework, and all five helper bundles, then applies and verifies an ad-hoc local signature after assembly.
A distribution build still needs the product's Developer ID signing, hardened runtime, notarization, and stapling pipeline.

## Manual runtime contract

Serve `fixture/` on an owned loopback port, then launch exactly one app instance:

```sh
python3 -m http.server 45731 --bind 127.0.0.1 --directory fixture
open target/bundle/herdr-cef-probe.app --args \
  --url=http://127.0.0.1:45731/ \
  --probe-profile-dir="$PWD/target/fixture-profile" \
  --remote-debugging-port=45732
```

The app emits one-line JSON diagnostics for start, native child attachment, first-responder transfer, point bounds, Retina backing scale, shutdown, and each explicit failure stage.
Use `lsof -nP -iTCP:45732 -sTCP:LISTEN` to prove the CDP listener address instead of assuming it is loopback-only.

Attach chromux with:

```sh
chromux open herdr-cef-probe --cdp-url http://127.0.0.1:45732 --tab active
chromux snapshot herdr-cef-probe --interactive
chromux close herdr-cef-probe
```

For this external CDP mode `chromux close` detaches and does not close the CEF page.
Reattach using the same command and verify that the URL and fixture local storage remain.

## Security boundary

CEF's `remote_debugging_port` is a process-wide debugging server.
It has no per-view capability token and enumerates all debuggable targets in the process.
Therefore the direct port is acceptable only for this single-view disposable probe.
The product must put a loopback-only, session-capability CDP gateway in front of CEF and expose only the target mapped to the stable Browser view ID.
Binding CEF directly as the product endpoint would not satisfy the approved scoped-CDP contract.

CEF embeds Chromium and must be updated on Chromium's security cadence.
Pinning this version makes builds repeatable but does not authorize leaving it stale.
The product needs an automated CEF release monitor, archive provenance check, and a rebuild path for every supported macOS architecture.
