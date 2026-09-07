# Pet window on macOS

Current implementation: [PetWindow.swift](../macos/Sources/HerdrMacOS/PetWindow.swift).
The old Tauri hit-window, JavaScript timer, private SkyLight space, and external-terminal-raising recipes do not apply to this shell.

## Window ownership and transparency

`PetWindowController` owns one borderless, non-activating AppKit `NSPanel` with SwiftUI content.
The panel uses a clear background, `isOpaque = false`, and `hasShadow = false`; a shadow on a transparent panel can appear as a grey backdrop.
It floats, remains visible when the app deactivates, and joins spaces through AppKit collection behavior rather than a private SkyLight space.

`PetPlacement.windowSize` owns the window size.
`PetHitRegion` defines the interactive ellipse inside that window; transparent corners pass through mouse events.
Cursor tracking updates that hit policy while visible and stops when hidden.
Do not add a second invisible hit window or a webview polling loop.

## Placement and dragging

`PetPlacement.clampedOrigin` resolves a restored or dragged origin into a connected display's visible frame.
The saved off-screen incident coordinate is covered by `PetIntegrationTests`; do not bypass the clamp when applying persisted state.
See [dev-runtime.md](dev-runtime.md) for state-file identity and fixture isolation.

`PetInteractionView` accepts the first mouse event without activating the panel.
It captures the cursor-to-window anchor on mouse down, distinguishes a click from a drag through `PetGesture.dragThreshold`, and moves through native mouse-drag events.
During a drag, the controller moves the panel without persisting each intermediate point; release stores the final origin through the core.
A completed drag must not also invoke the click action.

Clicking the pet opens Hide's agent dashboard and raises Hide's own main window.
It does not search for an external terminal by title or select an emulator tab through AppleScript.
The core remains the owner of pose, badges, visibility, and persisted placement.

## Verification

Use `PetIntegrationTests.swift` for placement, hit region, and gesture policy regressions.
Use [verification-fixtures.md](verification-fixtures.md) for scripted pet state and receipt support.
Follow [PERFORMANCE_TESTING.md](PERFORMANCE_TESTING.md) before native observation: exactly one identified bundle, an owned fixture, isolated runtime state, and coordinated foreground access.

Verify a real screenshot for transparency and visibility, and an actual native drag for first-click capture, cursor anchoring, edge clamping, and no accidental dashboard opening after release.
A receipt records what the controller believed; only the screenshot and interaction show what the operator actually saw.
