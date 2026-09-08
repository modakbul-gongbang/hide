# Documentation map

Start with [AGENTS.md](../AGENTS.md), then read only the current documents for the area being changed.
This index distinguishes maintained contracts and procedures from historical records and visual references.
A document's location or an old PRD citation does not make it current authority.

## Current sources of truth

| Question | Read | Executable authority or enforcement |
| --- | --- | --- |
| Architecture, ownership, integration boundaries, working rules | [AGENTS.md](../AGENTS.md) | `herdr-core/`, `macos/`, relevant local rules |
| Recent project and unified-surface navigation | [DESIGN.md](../DESIGN.md#recent-navigation-in-the-native-shell), [input cost and regression owners](PERFORMANCE_TESTING.md#two-level-recent-navigation-cost-contract) | `AgentMRU.swift`, `ShellModel.swift`, shortcut registry and navigation tests |
| Product UI and design tokens | [DESIGN.md: In-Product Components](../DESIGN.md#in-product-components) | `HideTheme.swift`, component checks and design tests |
| Icon button roles, hit areas, interaction states, and reuse | [DESIGN.md: Icon buttons and badges](../DESIGN.md#icon-buttons-and-badges) | `HideIconButton.swift`, `HideTheme.IconButton`, component checks |
| Required CI and delivery gates | [CONTRIBUTING.md](../CONTRIBUTING.md) | `.github/workflows/pr.yml` and `design-contract.yml` |
| Install, update, bundle contents | [INSTALL.md](INSTALL.md) | `scripts/build-app.sh`, bundled resources |
| Which local build is running | [dev-runtime.md](dev-runtime.md) | Bundle identity, `CoreBridge.defaultStatePath`, actual process/window |
| Performance and rendering verification | [PERFORMANCE_TESTING.md](PERFORMANCE_TESTING.md) | Regression tests, measurement tools, native run evidence |
| Browser pane use and ownership | [BROWSER_PANES.md](BROWSER_PANES.md) | `plugins/browser/`, native browser viewer |
| Sidebar PR details, CI rollup, refresh and failure states | [status-model.md: GitHub status](status-model.md#github-status-in-the-workspace-row) | `github.rs`, `runtime.rs`, `CheckoutCardPresentation.swift`, native Workspace PR control |
| Agent status, semantic colors, Workspace aggregation, and read/unread policy | [status-model.md](status-model.md) | `sidebar.rs`, `AgentRow.swift`, `SidebarPresentation.swift`, status tests |
| Optional ambient counts and privacy | [status-model.md](status-model.md#ambient-signals-subagents-background-tasks) | `sidebar.rs::parse_ambient`, `pet.rs::ambient_totals` |
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
