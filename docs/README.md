# Documentation map

Start with [AGENTS.md](../AGENTS.md), then read only the current documents for the area being changed.
This index distinguishes maintained contracts and procedures from historical records and visual references.
A document's location or an old PRD citation does not make it current authority.

## Current sources of truth

| Question | Read | Executable authority or enforcement |
| --- | --- | --- |
| Working rules, routing, and the invariants that hold everywhere | [AGENTS.md](../AGENTS.md) | `herdr-core/`, `macos/`, relevant local rules |
| Architecture, Herdr versus core ownership, the wire boundary and its schema gaps, the bundled runtime | [ARCHITECTURE.md](ARCHITECTURE.md) | `herdr-core/src/runtime.rs`, `session_sync/{coordinator,projection,replica,subscription}.rs`, `wire.rs`, `HerdrRuntimeResolver`, contract and boundary tests |
| Build output, worktree caches, toolchain reuse, the harness verify entrypoints | [BUILD.md](BUILD.md) | `scripts/toolchain-env.sh`, `verify-cargo.sh`, `verify-swift.sh`, `scripts/tests/test_toolchain_reuse.py`, `test_verification_builds.py` |
| Rust implementation conventions | [herdr-core/AGENTS.md](../herdr-core/AGENTS.md) | `herdr-core/src/`, `herdr-core/tests/` |
| Swift implementation conventions | [macos/AGENTS.md](../macos/AGENTS.md) | `macos/Sources/HerdrMacOS/`, `macos/Tests/HerdrMacOSTests/` |
| Project activity ordering, task-forest and Git Overview, disk and merged-worktree cleanup, registration removal | [DESIGN.md: Projects and checkout context](../DESIGN.md#projects-and-checkout-context), [agent workflow contract](../design/agent-workflow-review.md), [bounded projection cost](PERFORMANCE_TESTING.md#projects-and-overview-cost-contract) | `project_context.rs`, `worktrees.rs`, `disk.rs`, `worktree_cleanup.rs`, `runtime.rs`, `OverviewPresentation.swift`, `CheckoutOverview.swift`, native sidebar |
| Recent project and unified-surface navigation | [DESIGN.md](../DESIGN.md#recent-navigation-in-the-native-shell), [input cost and regression owners](PERFORMANCE_TESTING.md#two-level-recent-navigation-cost-contract) | `AgentMRU.swift`, `ShellModel.swift`, `ShellModelNavigation.swift`, shortcut registry and navigation tests |
| File toolbar, document kinds (text, Markdown, image, PDF, binary), Markdown modes, draft lifecycle and preview security | [DESIGN.md: File document toolbar and Markdown](../DESIGN.md#file-document-toolbar-and-markdown) | `EditorViewerOverlay.swift`, `PDFDocumentView.swift`, `MarkdownPreview.swift`, `HighlightedCodeEditor.swift`, `files.rs` document kinds, core editor events and document tests |
| Project Home: the checkout empty state and the overlay, lanes per checkout, the progress track, agent cards and lineage nesting, the Needs You strip, the inspector, every state | [DESIGN.md: Project Home](../DESIGN.md#project-home) | `ProjectHome.swift`, `ProjectHomeCards.swift`, `ProjectHomeWrapLayout.swift`, `ProjectHomePresentation.swift`, `ShellModel.projectHomeVisible`, `HideTheme.Home`, `ProjectHomePresentationTests.swift` |
| Editor preview tab: the one replaceable slot per checkout, its promotion triggers, the italic title | [DESIGN.md: Editor preview tab](../DESIGN.md#editor-preview-tab) | `runtime/editor.rs` (`place_editor_tab`, `promote_editor_tab`), `runtime/tests/editor_preview.rs`, `ShellModelNavigation.swift` (`EditorTabTitlePresentation`), `HideTheme.Typography.previewSlant`, `EditorPreviewTabPresentationTests.swift` |
| Explorer context menu, inline naming, drag move, Git decorations, and the core-owned file events | [DESIGN.md: Explorer file management](../DESIGN.md#explorer-file-management), [agent workflow contract](../design/agent-workflow-review.md), [bounded projection cost](PERFORMANCE_TESTING.md#projects-and-overview-cost-contract) | `WorkspaceOutlineView.swift`, `WorkspaceOutlinePresentation.swift`, `BrowserPaneOpener.swift`, `changes.rs`, `files.rs` explorer operations, runtime explorer tests, `WorkspaceOutlinePresentationTests.swift`, `BrowserPaneOpenerTests.swift` |
| Terminal file drop and image integration limits | [DESIGN.md: Terminal image attachment boundary](../DESIGN.md#terminal-image-attachment-boundary) | `TerminalFileDrop.swift`, `TerminalHost.swift`, `ImeTerminalView.swift`, existing terminal input path |
| Product UI, control ownership and future native catalog review | [DESIGN.md: In-Product Components](../DESIGN.md#in-product-components) | `HideTheme.swift`, `scripts/design-control-policy.json`, `check-design-contract.mjs`, component checks and design tests |
| Shared native control appearance, selection, input, checkbox and disclosure states | [DESIGN.md: Shared control family](../DESIGN.md#shared-control-family) | `HideTextButtonStyle.swift`, `HideChoiceGroup.swift`, `HideSearchField.swift`, `HideCheckboxStyle.swift`, `HideDisclosureStyle.swift`, `HideInputSurface.swift`, `HideEmptyState.swift`, `HideMenuChipLabel.swift`, component checks |
| Icon button roles, hit areas, interaction states, and reuse | [DESIGN.md: Icon buttons and badges](../DESIGN.md#icon-buttons-and-badges) | `HideIconButton.swift`, `HideTheme.IconButton`, component checks |
| Required CI and delivery gates | [CONTRIBUTING.md](../CONTRIBUTING.md) | `.github/workflows/pr.yml` and `design-contract.yml` |
| Install, update, bundle contents | [INSTALL.md](INSTALL.md) | `scripts/build-app.sh`, bundled resources |
| Which local build is running | [dev-runtime.md](dev-runtime.md) | Bundle identity, `CoreBridge.defaultStatePath`, actual process/window |
| Performance and rendering verification | [PERFORMANCE_TESTING.md](PERFORMANCE_TESTING.md) | Regression tests, measurement tools, native run evidence |
| Browser pane use and ownership, the Explorer entrypoint | [BROWSER_PANES.md](BROWSER_PANES.md) | `plugins/browser/`, `BrowserPaneOpener.swift`, native browser viewer |
| Background AI requests: provider boundary, provider state and sticky failover, retries, logging, the measured print-mode contract | [AI_PROVIDERS.md](AI_PROVIDERS.md) | `hide-ai/src/`, `hide-ai/tests/`, router, codex and claude backend tests |
| Local Claude and Codex session location, incremental reads, and conversation event classification | [hide-session crate](../hide-session/src/lib.rs) | `hide-session/src/`, `hide-session/tests/fixtures/`, plugin session reader, Codex usage fallback |
| Which agent and model background AI uses: where the choice is stored, who writes and reads it, the defaults | [AI_PROVIDERS.md: the operator's choice](AI_PROVIDERS.md#the-operators-choice) | `hide-ai/src/settings.rs`, `herdr-core/src/ai.rs`, `ai_settings` event, `HideSettings.swift` Background AI group, `check-capability-readers-off-lock.sh` |
| Weekly Usage popover sources: Claude Code through `claude -p /usage` text, Codex through its usage endpoint and session JSONL, the failure rows | [AI_PROVIDERS.md: weekly usage display](AI_PROVIDERS.md#weekly-usage-display) | `herdr-core/src/usage.rs`, `herdr-core/src/zoneinfo.rs`, `herdr-core/tests/fixtures/claude-usage/`, `hide-ai/src/claude.rs` usage read |
| Pane task labels and attention symbols | [plugins/agent-context-labels/README.md](../plugins/agent-context-labels/README.md) | `plugins/agent-context-labels/src/`, its tests, `docs/deployment.md` under the plugin |
| Shared Herdr socket requests and event subscriptions | [Herdr wire boundary](ARCHITECTURE.md#the-herdr-wire-boundary) | `hide-herdr-client/`, `herdr-core/src/wire.rs`, plugin transports |
| Sidebar PR details, CI rollup, refresh and failure states | [status-model.md: GitHub status](status-model.md#github-status-in-the-workspace-row) | `github.rs`, `runtime.rs`, `CheckoutCardPresentation.swift`, native Workspace PR control |
| Agent status, semantic colors, Workspace aggregation, and read/unread policy | [status-model.md](status-model.md) | `sidebar.rs`, `AgentRow.swift`, `SidebarPresentation.swift`, status tests |
| The pet's subagent badge and what it may count | [status-model.md](status-model.md#the-subagent-badge) | `pet.rs::subagents_active`, `agent_hooks.rs` |
| Ownership axis, delegated grouping, the stall clock and its thresholds | [status-model.md: the stall clock](status-model.md#the-stall-clock) | `sidebar.rs::ownership_of`, `runtime.rs` stall clocks, `session_sync/coordinator.rs` agent tick |
| Where an agent's parent comes from: the `parent_pane` token | [status-model.md: where a parent comes from](status-model.md#where-a-parent-comes-from) | `wire.rs::lineage_parent`, `fork.rs`, `sidebar.rs::apply_lineage`, wire lineage test |
| Why a pane's children are visible or not, and the uninstrumented mark | [status-model.md: uninstrumented is not an unknown activity](status-model.md#uninstrumented-is-not-an-unknown-activity) | `hide-agent-hooks/src/diagnosis.rs`, `herdr-core/src/agent_hooks.rs`, `sidebar.rs::project_pane_children` |
| Agent hook installation, what is written where, and the Settings diagnosis | [agent-hooks.md](agent-hooks.md) | `hide-agent-hooks/`, `StatusSnapshot.agent_hooks`, `install_agent_hooks` event |
| Native pet window and gestures | [pet-window-macos.md](pet-window-macos.md) | `PetWindow.swift`, `PetIntegrationTests.swift` |
| Pet theme format | [theme-contract.md](theme-contract.md) | `PetTheme.swift`, each `theme.json`, theme tests |
| Pet artwork workflow | [theme-contract.md](theme-contract.md#adding-art) | Bundled manifest and artwork |
| Deterministic QA fixtures | [verification-fixtures.md](verification-fixtures.md) | Fixture/test code; performance guide governs native isolation |
| App icon and bundled marks | [dev-runtime.md](dev-runtime.md#bundled-artwork-ownership) | Resource files, icon generation script, third-party notices |

The Herdr pin lives in `macos/Sources/HerdrMacOS/Resources/herdr-bundle.json`; the matching schema lives in `contracts/herdr-api.schema.json`.
Documentation must point to those files rather than inventing another version or protocol authority.
Approved PRDs are scoped change contracts, not an always-current description of the whole product.
When code, tests, and a current contract disagree, investigate and update the responsible contract and implementation together; do not silently assume either is correct.

## References and historical records, not implementation instructions

| Material | Why retained | How to use it |
| --- | --- | --- |
| [Agent workflow UI contract](../design/agent-workflow-review.md) | Maps adopted Agents, pane focus, Overview, and Explorer Git screens and masters to their code owners | Current implementation contract; `DESIGN.md`, `status-model.md`, and `ARCHITECTURE.md` retain their domain authority |
| [Orca visual references](design-reference/README.md) and its three images | Layout and interaction inspiration | Read on demand; `DESIGN.md` wins on current UI requirements |
| `assets/hide-icon-candidates/` | Artwork candidates, including the icon script's default source | Source/reference art, not screenshots proving the app rendered |
| `fixtures/` | Small deterministic terminal input files | Inputs for owned fixtures, not test results or an alternate architecture contract |

The frozen `spikes/swift-shell-pivot/` record remains outside `docs/` and must not be rewritten to describe later changes.
Other old spike/run records and PRDs describe their own revision only.
Screenshots, traces, profiles, recordings, and per-run verdicts belong in ignored `agents/runs/<slug>/`, not in this documentation tree.

## Retired documents

The retired Rust/WGPU/CEF ADR and research matrix, porting checklist, copied legacy SSH implementation/config, milestone reports, and static-slime prompts live only in Git history.
They described completed migration work or contradicted the current shell, runtime pin, and animated theme contract.
Use Git history for a historical investigation; do not restore them as active instructions.
Old PRDs may name those removed inputs because they record the original work rather than current setup steps.

## Keeping the map reliable

- Give each new document one responsibility and add it to this map with its code/test owner.
- Add a section to an existing owner before creating a new guide; delete a guide when its unique contract has moved or its implementation has retired.
- Update the owning document in the same change as a behavior, build command, schema, or workflow change.
- Delete replaced instructions and fix their active references in the same change; retain history only when it has a named reader or decision-history purpose, with a superseded notice at the top.
- Keep procedures in their owning guide; AGENTS.md should route readers and retain critical invariants rather than copying long runbooks.
- Keep CI descriptions tied to actual workflow steps; a command in a guide is not evidence that CI executes it.
- Put performance impact and unrun native checks in PR evidence for rendering/runtime changes, following the performance guide's review policy.
