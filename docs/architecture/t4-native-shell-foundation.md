# T4 native shell foundation

## Result

The repository now has a root Rust application package that produces a signed `Herdr IDE.app` with an AppKit lifecycle and a WGPU Metal workbench.
The release bundle renders a persistent left navigator, a tab strip, a flexible two-pane canvas, a status surface, and window-local pane zoom without Electron or Node.
The current runtime truthfully shows `Reconnecting to local Herdr` until a T5 connection supplies an authoritative protocol-21 snapshot.

## Ownership boundaries

- `src/app.rs` owns `NSApplication`, the main window, first responder, native menus, text input, the AppKit accessibility bridge, and the frame loop.
- `src/render.rs` owns WGPU Metal surface creation, IDE geometry, glyph rendering, presentation, and recoverable surface failures.
- `src/domain.rs` owns protocol revision checks, complete snapshot validation, monotonic event application, stale state, and full-resync recovery.
- `src/navigator.rs` owns the active Workspaces, Agents, or Worktrees view plus selection and expansion persistence with an atomic versioned document.
- `src/layout.rs` owns split ratios and transient zoom presentation state scoped to one window and tab.
- `src/presentation.rs` owns the shared stable-agent projection, authoritative logo mapping, summary fallback, and the common error, attention, working, idle, ended priority.
- `src/accessibility.rs` owns the renderer-independent semantic tree that the AppKit bridge exposes as meaningful native roles, labels, values, selection, and zoom state.
- `scripts/build-app.sh` owns deterministic release compilation, bundle assembly, plist validation, executable permissions, ad-hoc signing, and signature verification.

## Projection and failure contract

A snapshot is accepted only when the protocol revision matches 21, workspace, tab, pane, and agent IDs are unique in scope, every layout leaf exactly matches a typed pane, and active focus targets exist.
An event sequence gap marks the projection `Stale`, leaves the last confirmed state unchanged, and requires a full snapshot.
A later full snapshot replaces the projection and returns it to `Connected` while preserving the same stable IDs from authoritative data.
The visible status bar renders `Connected`, `Reconnecting`, `Stale`, `Failed`, or `Action required` rather than hiding connection failure in logs.

## Navigator and presentation

Navigator selection and expansion are stored independently for Workspaces, Agents, and Worktrees views.
The state document has an explicit schema version and uses write-then-rename publication, so repeating a save converges on one valid file.
The runtime does not create a fake local workspace while no Herdr snapshot exists.
One shared presentation store converts authoritative agent kind and stable identity into exactly one Codex, Claude, or neutral logo choice and one common priority ordering.

## Pane zoom

`Cmd+Shift+Enter` and the native View menu dispatch to the same zoom action.
The navigator and tab strip stay visible while the focused pane occupies the main canvas.
The exact pre-zoom ratios and focus are restored on toggle.
Zoom state is retained independently when switching tabs in the same app session, cleared before topology mutation, and cleared with a visible reason when its stable target disappears.
No Herdr layout mutation is issued by the zoom model.

## Accessibility and visual verification

The release app exposed one `AXWindow` containing an `AXList` navigator, `AXTabGroup`, and two `AXTextArea` pane elements in split mode.
Zoom mode kept the navigator and tab group and removed the hidden sibling pane from the accessibility children.
The application menu bar exposed `Herdr IDE` and `View`, and `View` exposed `Toggle Pane Zoom`.
The final split and zoom screenshots are `docs/screenshots/t4/native-shell-split.png` and `docs/screenshots/t4/native-shell-zoomed.png`.
The final runtime report is `docs/screenshots/t4/native-runtime.json`.

## Verification

Twenty deterministic library tests passed, including snapshot validation, event gap resync, navigator persistence, repeated save convergence, shared agent ordering, semantic accessibility, zoom restore, tab switch, target removal, and transient render recovery.
`cargo check --all-targets`, `cargo fmt --all -- --check`, and `git diff --check` passed.
The release bundle passed plist lint, deep strict code-sign verification, and a physical `Cmd+Q` clean exit.
Exactly one intended release bundle instance was present during visual and interaction checks.
The final runtime used Apple M4 Pro Metal at 2360 by 1440 backing pixels with scale factor 2.0 and reached its first usable frame in 201.422 milliseconds.

## Principle application

Engineering principle 2 is satisfied by promoting the already-proven T1 AppKit and WGPU surface instead of introducing a second desktop framework.
Engineering principles 4 and 10 are satisfied by typed projection failures, visible connection state, and structured render recovery events.
Engineering principle 11 is satisfied by duplicate-safe navigator persistence and deterministic zoom toggles.
Design principles 2, 4, 5, and 7 are satisfied by keeping the navigator persistent, showing derived connection and zoom state, extending the approved low-chrome shell, and encoding pane hierarchy visually and through AX roles.

## Deferred boundary

T4 establishes the native lifecycle, workbench, projection, presentation, menu, zoom, and accessibility foundations.
T5 owns authoritative Herdr connection and complete PTY attach and topology commands, T6 owns populated navigator hierarchies, and T8 owns the CEF Browser surface.
No installed production app, remote host, Electron reference, standalone Pet, or real user workspace was changed.
