# Swift shell working conventions

This file covers conventions already established under `macos/`.
Read the repository `AGENTS.md` for core versus shell ownership and runtime integration.
Read `DESIGN.md` for visual decisions and `CONTRIBUTING.md` for required gates.
Do not restate those contracts here.

## Placement and ownership

- Extend the existing feature file or type that owns the behavior instead of creating a parallel owner.
- Keep core snapshot decoding, delta application, and C ABI bridging in `CoreBridge.swift`.
- Keep application coordination and snapshot observation in `ShellModel.swift`.
- Keep launch-time executable and environment resolution in `RuntimeEnvironment.swift`.
- Views read snapshot-backed values, render them, and dispatch the event that represents the user's intent.
- Do not mirror a core-owned value as independent SwiftUI authority.
- When an interaction needs several core-owned changes to appear together, dispatch the existing atomic event shape instead of sequencing view-local mutations.
- Cross-reference the root `AGENTS.md` before changing which side owns a value.

## Views and presentation

- Keep reusable data-to-label, color, ordering, and availability decisions in a `*Presentation` value type.
- Keep a presentation type beside its feature owner, whether that owner is a focused `*Presentation.swift` file or the view or model that alone consumes it.
- Keep `View.body` focused on composition and event dispatch; move branching decisions into the presentation owner when they need direct tests.
- Reuse the shared shell controls named in the root `AGENTS.md` before adding a new control shape.
- Take chrome colors, typography, spacing, radii, and shadows from `HideTheme` as directed by `DESIGN.md`.
- Add a missing token to `HideTheme` and `DESIGN.md`; do not settle it with an inline chrome value.

## Errors and diagnostics

- Use an existing typed `Error` with `LocalizedError` when callers need to distinguish or present reusable failures.
- Surface asynchronous and process failures through the owning notice, receipt, or thrown error.
- Preserve process termination status and stderr when they are part of the caller-visible failure.
- Treat cancellation or replacement as a no-op only where that lifecycle path is explicit.
- Use the logger already owned by the subsystem: launch diagnostics in `RuntimeEnvironment`, latency diagnostics in `TerminalLatency`, and structured verification receipts through `VerificationReceipt`.
- Do not add `print` calls to production sources.

## Tests

- Put shell tests in `macos/Tests/HerdrMacOSTests/` and name files `*Tests.swift`.
- Use Swift Testing with `import Testing`, `@Suite`, `@Test`, `#expect`, and `#require`.
- Name test functions as lower-camel-case statements of the observable outcome.
- Test presentation decisions directly without rendering when the output is a value.
- Use AppKit or SwiftUI hosts only when window, layout, focus, input, or drawing behavior is the subject.
- Mark tests `@MainActor` only when the exercised framework boundary requires it.
- Build filesystem fixtures under `FileManager.default.temporaryDirectory` and remove them with `defer`.
- Assert the explicit unavailable or failure state instead of accepting an empty view as success.

## Change discipline

- Do not add style rules that a formatter or linter can enforce.
- Keep comments for ownership, framework behavior, lifecycle, or failure semantics that the code alone does not reveal.
- When visible behavior changes, update the current owner routed by `docs/README.md` and verify it in the native app as required by the root instructions.

`CLAUDE.md` beside this file is a symlink to this file so both supported runtimes load the same nested instructions.
