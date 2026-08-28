# T1 preflight

This directory contains the manifest-driven macOS release-bundle and native-runtime verifier for T1.

Run the local contract checks with:

```sh
python3 -m compileall -q t1_preflight tests
python3 -m unittest discover -s tests -v
```

Generate an interaction and evidence plan without requiring an app with:

```sh
./t1-preflight \
  --mode dry-run \
  --app "/Applications/Herdr IDE.app" \
  --manifest fixtures/example-manifest.json
```

Use `--mode static` for bundle inspection and `--mode run` only after the integrated app implements the telemetry contract in [the harness contract](../../docs/verification/t1-preflight-harness.md).

The exclusive T5 Option+F check uses `spikes/integrated-preflight/t5-option-meta-v4-manifest.json` and its own output identity documented in [the T5 verification contract](../../docs/verification/t5-exclusive-option-meta.md).
It requires a fresh output directory, records focus and source timelines, and injects physical keys only inside an approved hands-off window.

The app never receives the action scenario.

The harness owns all native interaction through CGEvent, macOS Accessibility, and `screencapture`.
