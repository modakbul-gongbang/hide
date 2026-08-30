# T10 automated regression matrix

Date: 2026-08-30.

Source under test: current `prd/hide-rebrand` HEAD `d021e73`.

## Fresh command results

`CARGO_TARGET_DIR=/tmp/hide-finisher-cargo cargo test --manifest-path herdr-core/Cargo.toml --locked` passed with 91 library tests, 25 FFI integration tests, and 0 doc tests.

`swift test --package-path macos --scratch-path /tmp/hide-finisher-swift` passed with 69 tests.

No build or test output directory in the worktree was deleted during this verification.

## Regression coverage

| Area | Tests and observed contract | Regression risk protected |
| --- | --- | --- |
| Workspace assembly | `workspace::tests::discovers_git_default_branch_and_worktrees_without_mutating_the_repository`, `workspace::tests::flat_folder_is_visible_without_implicit_git_init`, and `workspace::tests::temporary_discovery_does_not_duplicate_a_registered_repository` pass. | Prevents registration from losing real git metadata, worktrees, or non-git folders, and prevents a pane-discovered checkout from duplicating a registered repository. |
| Checkout projection and selection | `runtime::tests::focusing_checkout_selects_only_the_target_checkout_pane`, `runtime::tests::a_plain_terminal_pane_cwd_is_reconciled_into_its_registered_checkout`, `runtime::tests::an_unregistered_worktree_layout_projects_into_the_selected_checkout`, `runtime::tests::a_returned_pane_id_selects_its_layout_when_other_panes_share_the_cwd`, and `runtime::tests::checkout_path_matching_uses_component_boundaries` pass. | Prevents a workspace or worktree click from retaining another checkout's tab, pane grid, cwd, or file-tree context, which was the reported P0 transition failure. |
| Herdr version and source chain | `version::tests::live_socket_wins_over_every_executable_candidate`, `version::tests::compatible_installed_runtime_wins_when_socket_is_absent`, `version::tests::old_installation_falls_back_to_the_pinned_bundle_with_guidance`, and `version::tests::agent_cli_support_is_presence_based_without_a_version_gate` pass. | Prevents a stale installed binary from winning over the live daemon, prevents a compatible install from being ignored, and prevents a below-minimum install from failing silently instead of selecting the pinned bundle with guidance. |
| Agent state ordering and unseen tokens | `sidebar::tests::seven_authoritative_states_keep_fixed_symbols_and_seen_tokens_stay_idle`, `sidebar::tests::sort_rank_is_primary_and_activity_descending_breaks_ties`, and `sidebar::tests::one_broken_agent_excludes_only_itself_and_names_why` pass. | Protects `INV-herdr-unseen-token`: only unseen attention tokens promote a state, acknowledged tokens fall back to idle, fixed symbols remain mapped, sort rank remains primary, and one malformed pane does not discard healthy agents. |
| Search filtering and Enter routing | `hideSearchFiltersAgentsAndCheckoutsWithoutTerminalEntries` passes. It proves terminal-only entries are excluded, agent and checkout queries filter correctly, whitespace is normalized, and the pure route values are `.agent(paneID:)` and `.checkout(workspaceID:checkoutID:)`. The Search field's Enter handler dispatches the first filtered entry through that route function. | Prevents Search from exposing ordinary terminal panes, losing workspace/device context, or routing Enter to the wrong pane or checkout. |
| Snapshot delta and pane rendering contract | `ffi_contract::delta_reads_send_only_what_the_cursors_have_not_seen`, `ffi_contract::session_snapshot_exposes_authoritative_recursive_layout_and_per_pane_state`, `ffi_contract::session_snapshot_keeps_authoritative_agent_order_and_tokens`, `chunkOnlyDeltaDecodesWithoutSections`, `laggingCursorDeltaSurfacesTheDropMarker`, and the pane-grid presentation tests pass. | Prevents a high-volume terminal chunk delta from replacing the revisioned layout section, prevents the shell from falling back to a pane-id list, and makes cursor gaps visible instead of rendering stale or clipped terminal state. |

The matrix covers the T10 requirements across R2, R3, R5, and R6 and retains the pane-rendering contract that guards the reported layout failure.

## Evidence boundary

The test commands are deterministic local checks and do not replace the native installed-app screenshots or the dedicated mini live check recorded in the continuation verification.

The full installed-app and mini evidence remains in [hide-rebrand-continuation-20260830.md](../../hide-rebrand-continuation-20260830.md).
