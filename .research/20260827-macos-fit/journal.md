# macOS operational fit journal

## Search coverage

At least 12 independent search angles were run, including Apple ScreenCaptureKit permission APIs, AX trust APIs, Screen Recording and Accessibility settings, TCC attribution to command-line children, LaunchAgent versus LaunchDaemon context, SSH/background execution, lock/loginwindow/Fast User Switching, code signing and notarization, MDM PPPC policy, Apple Events/PostEvent, multi-display/Retina coordinate conversion, semantic AX versus raster control, and native CLI/MCP tool implementations.
Important pages were opened after search.
The vendored research engine was also attempted against Apple Developer and GitHub raw pages; it exhausted its fetch grid and requested a rendered-browser fallback, so the web browser fetch path was used and succeeded.

## Permission and identity findings

- Screen Recording and Accessibility are separate TCC grants.
  Apple exposes `CGPreflightScreenCaptureAccess` for a non-prompting screen-capture check and `AXIsProcessTrustedWithOptions` for Accessibility trust.
  The AX prompt option is asynchronous and does not change the current call's Boolean return.
  Sources: https://developer.apple.com/documentation/coregraphics/cgpreflightscreencaptureaccess%28%29 and https://developer.apple.com/documentation/applicationservices/1459186-axisprocesstrustedwithoptions
- Apple's ScreenCaptureKit sample says the first capture prompts and the app needs a restart after the grant.
  Source: https://developer.apple.com/documentation/screencapturekit/capturing-screen-content-in-macos
- TCC behavior for CLI children is operationally tied to the responsible host/code identity.
  Peekaboo tells users to grant the reported bridge host or the Terminal/editor running the CLI, warns that Homebrew Cellar path changes can invalidate the enabled entry, and documents that a signer migration created a new TCC identity even though bundle identifiers stayed stable.
  Source: https://github.com/openclaw/Peekaboo/blob/main/docs/permissions.md
- Apple code requirements define how a verifier recognizes previously seen signed code, and MDM PPPC identities distinguish app bundles by bundle ID and non-bundled binaries by installation path.
  Sources: https://developer.apple.com/documentation/security/applying-code-requirements and https://developer.apple.com/documentation/devicemanagement/privacypreferencespolicycontrol/services-data.dictionary/identity
- Screen Capture cannot be silently granted by legacy PPPC; it can only be denied, or policy can let a standard user approve it.
  Accessibility profile granting is deprecated in macOS 26.2 and removed in macOS 27 in favor of declarative app settings.
  Sources: https://developer.apple.com/documentation/devicemanagement/privacypreferencespolicycontrol/services-data.dictionary and https://developer.apple.com/documentation/devicemanagement/appsettingsappdictionaryobject

## Launch and session findings

- A LaunchDaemon has no user knowledge or WindowServer access.
  Apple recommends a system daemon only for user-independent work and a per-user LaunchAgent for GUI work, connected by IPC.
  Source: https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/DesigningDaemons.html
- Fast User Switching keeps inactive sessions running but they do not receive physical keyboard or mouse input.
  Core Graphics depends on WindowServer and session-scoped resources must carry session identity.
  Source: https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPMultipleUsers/Concepts/FastUserSwitching.html
- SSH/background callers should attach to an already-running, permissioned host in the active Aqua GUI session.
  Peekaboo explicitly warns caller-local capture in SSH, LaunchAgent, Codex, and background launchd contexts can return wallpaper/redacted images despite apparently successful CoreGraphics calls.
  Source: https://github.com/openclaw/Peekaboo/blob/main/docs/permissions.md
- Pre-login capture is possible, but not through a generic CLI launched by a daemon.
  Apple DTS says the supported screen-sharing architecture is daemon plus GUI agent over IPC; a globally installed agent may load for both `Aqua` and `LoginWindow`, and ScreenCaptureKit works in pre-login context on macOS 14.4+.
  Source: https://developer.apple.com/forums/thread/814152
- Persistent screen access for VNC apps is a restricted entitlement that requires an Apple request.
  Source: https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.persistent-content-capture

## Semantic automation and display findings

- AX exposes attributes, values, supported actions, and direct action invocation.
  Standard AppKit controls expose accessibility automatically, but custom controls must implement it; unsupported, invalid, or timed-out targets are explicit AX errors.
  Sources: https://developer.apple.com/documentation/applicationservices/1462091-axuielementperformaction and https://developer.apple.com/documentation/appkit/accessibility-for-appkit
- Therefore the robust order is AX semantic action first, screenshot/OCR for visual evidence or inaccessible surfaces, and coordinates only as an explicit last resort bound to the exact fresh snapshot.
  Peekaboo and screencommander implement this hybrid pattern.
  Sources: https://github.com/openclaw/Peekaboo/blob/main/docs/MCP.md and https://github.com/0xSMW/screencommander
- macOS logical points are not pixels.
  Backing scale is per display/window, display configuration can change dynamically, and Apple's conversion APIs should be used instead of assuming a fixed 2x factor.
  Sources: https://developer.apple.com/documentation/appkit/nsscreen/backingscalefactor and https://developer.apple.com/documentation/appkit/nsscreen/screens and https://developer.apple.com/library/archive/documentation/GraphicsAnimation/Conceptual/HighResolutionOSX/Explained/Explained.html
- A safe wire format should include display ID, global logical bounds, delivered raster size, native/output scale, origin convention, and snapshot/reference ID.
  Peekaboo documents this exact versioned `coordinate_context` approach and says not to assume a fixed Retina factor.
  Source: https://github.com/openclaw/Peekaboo/blob/main/docs/MCP.md

## Distribution and concrete tools

- Local source builds do not inherently require notarization, but broad direct distribution should use Developer ID signing, hardened runtime, secure timestamp, and notarization.
  Apple explicitly applies this to apps and command-line targets.
  Sources: https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution and https://developer.apple.com/documentation/xcode/creating-distribution-signed-code-for-the-mac
- Peekaboo is the strongest complete reference: signed app/CLI, permission onboarding and diagnostics, code-signature-validated bridge, AX-first actions, ScreenCaptureKit/CG capture, exact snapshot ownership, coordinate metadata, CLI and stdio MCP.
  It requires macOS 15+.
  Sources: https://github.com/openclaw/Peekaboo and https://github.com/openclaw/Peekaboo/blob/main/docs/permissions.md and https://github.com/openclaw/Peekaboo/blob/main/docs/MCP.md
- screencommander is a simpler native Swift CLI/MCP reference for macOS 14+: doctor output, stable permission failures, AX tree, ScreenCaptureKit, coordinate metadata, event observation, action-tier fallback.
  It does not document the same stable signed app broker model.
  Source: https://github.com/0xSMW/screencommander
- AXorcist is a signed/notarized semantic AX CLI/library and is useful as a focused building block, but it is not a raster capture or complete computer-use system.
  Source: https://github.com/openclaw/AXorcist
- MCPMacControl uses a signed `.app` identity and target-focus checks, but is more screenshot/coordinate-first and also exposes PTY shell sessions, so its MCP capability set should be narrowed before use with an autonomous agent.
  Source: https://github.com/sstraus/McpMacControl
- `cliclick` is a low-level fallback only: Terminal must hold Accessibility permission and the tool explicitly cannot control the login window.
  Source: https://github.com/BlueM/cliclick

## Resolution of contested claims

- Claim: GUI automation from a root LaunchDaemon should work if it is privileged.
  Verdict: REFUTED by Apple daemon documentation and Apple DTS.
- Claim: pre-login ScreenCaptureKit is impossible.
  Verdict: REFUTED for product-grade screen-sharing agents on macOS 14.4+; still out of scope for a generic Codex/Herdr CLI unless it adopts the global agent architecture and restricted deployment path.
- Claim: a tool's `headless` flag means it works without an Aqua/WindowServer session.
  Verdict: REFUTED as an interpretation.
  Tool docs use headless to mean no menu-bar UI or stdio operation; Apple session constraints still apply.
- No live permission or UI mutation was performed, so there was no code/system verification phase.
