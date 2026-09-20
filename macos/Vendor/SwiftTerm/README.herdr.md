# Vendored SwiftTerm

This directory contains the SwiftTerm library target from upstream release `1.20.0` at commit `5d14406844143538cd8f8851d2d8a67c1fe443e5`.
It is vendored because the AppKit IME overlay implementation is internal and final, so the application cannot correct its rendering behavior through a public extension point.
The package manifest intentionally exposes only the library and its build-info plugin needed by Herdr.
Local changes, each covered by a test in `HerdrMacOSTests`:

- The IME marked-text overlay (`SwiftTermImeOverlayTests.swift`).
- Implicit link detection in `Terminal.swift`: a path ends at whitespace, a bare slash command is not a path, and rows a transcript hard-wraps are joined back into one link when the upper row is full or ends at a separator that the lower row's first unit could not have followed (`TerminalImplicitLinkSpanTests.swift`).
- Display pacing: `Mac/TerminalDisplayClock.swift` schedules pending damage on display-link ticks, while every visible AppKit draw callback repairs its backing store, including repeated callbacks within one tick (`TerminalDisplayClockTests.swift`, `TerminalRepaintTests.swift`).
  The AppKit view reports `terminalContentsDidDraw` and `terminalDisplayTick` to its delegate.
- Prepared row rendering retains only each visible row's latest glyphs and validates content, selection, link and blink inputs; old screen generations cannot accumulate until a bulk eviction (`TerminalRepaintTests.swift`).
- `mouseCell(with:)` on the AppKit view, so the application can route a click by its cell.
- The AppKit `keyDown(with:)` and `keyUp(with:)` overrides are `open` rather than `public`, allowing the shell to intercept an explicit clipboard image and its matching physical key release before SwiftTerm encodes Control-V, including kitty keyboard mode (`TerminalFileDropTests.swift`).
  No keyboard encoding or clipboard implementation is changed in the vendor.
- The package's macOS floor is raised from 11 to 14 for the view display link; the other platform floors are upstream's.
The upstream MIT license is preserved in `LICENSE`.
