# Vendored SwiftTerm

This directory contains the SwiftTerm library target from upstream release `1.20.0` at commit `5d14406844143538cd8f8851d2d8a67c1fe443e5`.
It is vendored because the AppKit IME overlay implementation is internal and final, so the application cannot correct its rendering behavior through a public extension point.
The package manifest intentionally exposes only the library and its build-info plugin needed by Herdr.
Local changes cover the IME marked-text overlay (`HerdrMacOSTests/SwiftTermImeOverlayTests.swift`) and implicit link detection in `Terminal.swift`: a path ends at whitespace, a bare slash command is not a path, and rows a transcript hard-wraps are joined back into one link when the upper row is full or ends at a separator that the lower row's first unit could not have followed (`HerdrMacOSTests/TerminalImplicitLinkSpanTests.swift`).
The upstream MIT license is preserved in `LICENSE`.
