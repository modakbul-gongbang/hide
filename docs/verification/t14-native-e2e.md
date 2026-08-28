# T14 native E2E ownership contract

T14 adds a standard-library-only, manifest-driven harness at `tools/t14-e2e`.

The harness owns one exact release bundle executable, one unique run identity, one fresh output directory, and exact fixture IDs.

It rejects absolute paths, parent traversal, globs, duplicate IDs, duplicate ports, mixed working-directory contracts, and an executable outside `Contents/MacOS`.

Before launch it records the declared bundle identity, requires zero PIDs for the exact executable, and captures a sanitized Herdr snapshot.

After launch it requires one PID and verifies that its resolved executable is the exact declared path before every action.

Native actions are delegated to the existing T1 AppKit/AX/CGEvent adapters, while tests inject the boundary and never touch macOS.

Every key action requires `app_frontmost`, `key_window`, and `render_view_first_responder` to be true.

If an optional frontmost identity is present, its bundle identifier and PID must match the manifest and owned process.

Route evidence is reduced to stable fields and an action may require exact PTY bytes such as `38` without recording a transcript.

The harness records before and after Herdr snapshots, stable IDs, AX output, native screenshots, action routes, and exact owned-process cleanup.

Fixture resources are described as exact IDs and are never automatically deleted.

## Contract checks

```sh
./tools/t14-e2e/t14-e2e --root . --manifest tools/t14-e2e/fixtures/manifest.json --mode contract
./tools/t14-e2e/t14-e2e --root . --manifest tools/t14-e2e/fixtures/manifest.json --mode dry-run
PYTHONPATH=tools/t14-e2e python3 -m unittest discover -s tools/t14-e2e/tests -v
```

Physical mode requires `HERDR_T14_EXCLUSIVE_HANDS_OFF=1` and an explicit snapshot command in the manifest.

The default mode never focuses an app, sends input, or removes fixture resources.
