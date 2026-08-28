# T4 native shell runtime verification

## Build identity

```text
package=herdr-ide@0.1.0
bundle=dist/Herdr IDE.app
bundle_identifier=dev.herdr.ide
architecture=aarch64
executable_sha256=e3c7dffca0c647038e1dde88273d97d2a6fe6e9efc41851e717c3440acbdc9cc
codesign=PASS deep strict
plist_lint=PASS
```

## Automated checks

```text
cargo test --lib: 20 passed, 0 failed
cargo check --all-targets: PASS
cargo fmt --all -- --check: PASS
git diff --check: PASS
```

## Exactly-one runtime inventory

```text
intended_bundle_instances=1
process_id=55470
window_id=20644
pty_child_process_id=55478
adapter=Apple M4 Pro (Metal)
physical_size=2360x1440
scale_factor=2.0
first_usable_ms=201.422
```

## Split accessibility snapshot

```text
process=herdr-ide
window_count=1
window_role=AXWindow
role=AXList description=Workspaces navigator
role=AXTabGroup description=Workspace tabs
role=AXTextArea description=Terminal pane A
role=AXTextArea description=Editor pane B
```

## Zoom accessibility snapshot

```text
process=herdr-ide
window_count=1
window_role=AXWindow
role=AXList description=Workspaces navigator
role=AXTabGroup description=Workspace tabs
role=AXTextArea description=Terminal pane A
zoom_state=Pane zoomed
hidden_editor_ax_element=absent
```

## Native menu and interaction

```text
menu_bar_items=Apple, Herdr IDE, View
view_menu_item=Toggle Pane Zoom
cmd_shift_enter_enter_zoom=PASS
cmd_shift_enter_restore_split=PASS
view_menu_enter_zoom=PASS
view_menu_restore_split=PASS
clean_cmd_q_exit=PASS
remaining_bundle_instances=0
```

## Screenshot hashes

```text
native-shell-split.png sha256=a31d069f19cc54e3f6a7a8f7b12bf7e7d0df9af62ec40be0cb9195a78aea116c
native-shell-zoomed.png sha256=15ac8962f7147bb36450348d140e7efbf763c40dce103abf6b3c48fa5ecb8dcb
native-runtime.json sha256=955df3d0530165a633560a67532bfb1b992b8bf812a1c3fc17e1d5dceecc47f3
```

The first surface acquisition returned one recoverable `temporarily-unavailable` result before the successful present.
The first implementation kept that recovered error visible in the pane.
The final implementation clears only a recovered `Render failed:` state after a successful present, keeps the model dirty until AppKit presents the clean frame, retains unrelated PTY failures, and covers both failure classifications with regression tests.
Both final screenshots were reopened at original resolution and contain no hostname, account name, or stale `Render failed:` text.
