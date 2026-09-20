# Remote interface parity

The operator approved the bounded quick implementation and chose design A on 2026-09-20.

## Observable behavior

- Recent Panels and Recent Projects retain their existing titles, agent marks, checkout context, selection and keyboard navigation.
  Remote rows additionally show a separate bounded `Remote · <device label>` mark with the complete host identity in accessibility help.
  Local rows need no extra mark, and an unresolved remote label shows its known device ID rather than implying locality.
- The bottom-sidebar device selector uses Hide's compact trigger and a custom two-line picker.
  Each row shows the actual device name, Local or Remote, connection state, and agent count when available.
  The selected device carries a checkmark; keyboard navigation previews without switching, activation selects, and Escape dismisses.
  Empty, connecting and unavailable states remain understandable without exposing diagnostics.
- Clipboard images through Command-V or Control-V, and Finder file drops, resolve to usable paths on the terminal's own machine.
  The original pane and connection generation own the asynchronous intent, independently of later focus changes.
  Transfers and staging are bounded and private, and no Enter is synthesized.
  A failed transfer does not forward queued typing or submit without its attachment.
  Retry remains bound to the original live session, and cancellation explicitly identifies held input that will be discarded.
- Local and mini use the same latest approved context-label plugin commit and current Claude/Codex hooks.
  Preserve live operator sessions and verify exactly one watcher per host without restarting either Herdr server.

## Design system

The selected A composition reuses the two-line list, selected wash, badge, compact menu chip and existing typography.
The remote-location badge uses a named maximum-width token, `HideTheme.recentLocationMaxWidth`, shared with Pen.
Reusable device-row and remote-location states belong in the design library; screen candidates and visual evidence stay under the ignored run directory.

## Out of scope

Remote editor, Git, conversation, tab geometry and reorder parity remain separate work.
There is no attachment shelf, provider-native image-acceptance claim, automatic submission, credential change, or automatic installed-app replacement.

## Acceptance

Run repository Rust, Swift and design checks, local/remote presentation and attachment regressions, native candidate observation, and both hosts' runtime parity checks.
Run visible Code, Fidelity and Security reviews and resolve current-scope findings.
Record unrun interaction or provider checks explicitly rather than treating path transfer as successful provider ingestion.
