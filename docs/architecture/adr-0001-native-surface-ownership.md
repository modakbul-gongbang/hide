# ADR-0001: Native Surface Ownership and Dependency Freeze

Status: Accepted for the T1 architecture foundation after the mandatory integrated release spike passed.

Decision date: 2026-08-27.

ADR version: 1.1.

Decision owners: Herdr protocol maintainers and Herdr IDE maintainers.

Research basis: [T1 Native Architecture Research Matrix](../research/t1-native-architecture-matrix.md).

## Context

Herdr IDE must combine a native macOS lifecycle, GPU-rendered terminal and editor surfaces, and a Chromium Browser without allowing two frameworks to own the same pixels or state.

The architecture must also support a real PTY, Korean IME, macOS Accessibility, global shortcuts, local and remote files, Keychain secrets, SSH and SFTP, structured diagnostics, a native overlay, and an IDE-owned Herdr Pet.

Official sources show that the selected components expose the required individual primitives, but they do not prove the components work together inside the packaged Herdr IDE application.

The research therefore supports a provisional dependency and ownership freeze, not a runtime acceptance claim.

## Decision drivers

- Maintained libraries are preferred when they have a current release or reviewed revision, a compatible license, and a visible security and update path.
- Long-term architecture requires one authoritative owner for protocol state, application lifecycle, each pixel surface, secrets, and diagnostics.
- Modular ownership requires narrow adapters around 0.x APIs, native ABIs, CEF revisions, and platform services.
- Grow in layers requires the sequence of architecture proof, typed protocol, native shell, terminal, files, Browser, remote access, overlay and Pet, and final end-to-end acceptance.
- Hard acceptance budgets cannot be weakened, averaged away, or replaced by an upstream product benchmark.
- A failure of the approved native composition requires a new human architecture decision rather than an unapproved framework or rendering fallback.

## Decision

Herdr owns the versioned typed workspace, tab, pane, surface, layout, event sequence, snapshot, agent identity, and lineage protocol.

AppKit through a narrow `objc2` adapter owns `NSApplication`, the main thread, windows, menus, native view hierarchy, event routing, overlay and Pet windows, and the native CEF child view lifecycle.

WGPU owns every non-Browser application pixel, including IDE chrome, navigator, tabs, split canvas, terminal, editor, overlay, and Pet rendering.

CEF owns Browser pixels, web content, Browser IME and Accessibility behavior, Browser processes, and helper processes inside an AppKit native child view.

WGPU must not receive, copy, composite, or present CEF Browser frames.

Tokio owns bounded background orchestration for protocol I/O, PTY coordination, file operations, search, SSH, SFTP, CDP, and diagnostics, while AppKit and CEF UI work remains on the required native threads.

The central command registry owns shortcut meaning and conflict resolution, while AppKit owns application-local key routing and `global-hotkey` is limited to operating-system registration.

The IDE process owns one presentation projection used by the main window, overlay, sidebar views, and Pet, so none of those surfaces independently infer agent state.

The IDE-owned Pet is one fixed `124x124` native window that preserves the approved asset, drag, state priority, clamp, persistence, and click-to-exact-pane behaviors.

The standalone Pet's `152` default and optional `124`, `152`, and `176` size variants are external reference facts and do not change the sealed IDE implementation contract.

## Surface ownership boundary

| Surface or concern | Authoritative owner | Supporting dependency | Forbidden overlap |
| --- | --- | --- | --- |
| Workspace, tabs, splits, panes, agents, lineage, restore | Versioned Herdr typed protocol | Rust protocol client and projection store | IDE-local phantom panes or independently inferred agent state |
| Application lifecycle, windows, menus, focus, native view bounds | AppKit | `objc2` and `objc2-app-kit` | Electron, Tauri, GPUI, egui, or winit as a second lifecycle owner |
| IDE, terminal, editor, overlay, and Pet pixels | WGPU over Metal | `cosmic-text` and the app renderer | Native Browser frame copies or a second UI framework |
| Terminal state and parsing | `alacritty_terminal` | `portable-pty` and transitive `vte` | A parallel terminal model or custom parser without a newly approved gap |
| Browser pixels and web semantics | CEF native child view | `cef-rs` and exact CEF binary archives | WGPU OSR, IOSurface copies, Electron, or a separate Browser window |
| WGPU surface accessibility | AccessKit macOS adapter | Narrow AppKit adapters for documented gaps | Duplicate native and AccessKit nodes for the same element |
| AppKit control accessibility | AppKit | Native control semantics | Recreating native controls in a second AX tree |
| Browser accessibility | CEF | Chromium accessibility implementation | Mirroring the Browser DOM into the WGPU AX tree |
| Secrets | macOS Keychain | `security-framework` | Config files, environment variables, logs, screenshots, or receipts |
| SSH and SFTP | `russh`, `russh-config`, and `russh-sftp` | Tokio | A silent subprocess or libssh2 fallback |
| Structured diagnostics | `tracing` and `tracing-subscriber` | Cross-process operation identifiers | Unstructured success inference or secret-bearing log payloads |

## Exact dependency and tool pins

Every accepted 0.x crate, git revision, native ABI, and external verification tool is exact-pinned in the lockfile or evidence manifest.

An update is accepted only after license and advisory review plus the focused compatibility checks named in this table.

| Component | Accepted pin | License | Update and compatibility gate |
| --- | --- | --- | --- |
| AppKit | The exact macOS SDK and Xcode build identity used by the release spike must be recorded because AppKit has no independent semantic version. | Apple platform SDK terms | Re-run launch, lifecycle, Retina, focus, IME, AX, sleep and wake, and bundle checks after an SDK or deployment-target change. |
| `objc2` | `0.6.4` | MIT | Re-run all native adapter, ownership, and main-thread checks after an update. |
| `objc2-app-kit` | `0.3.2` | Zlib OR Apache-2.0 OR MIT | Re-run AppKit view, event, IME, AX, and child-view checks after an update. |
| `wgpu` | `30.0.1`, release commit `40f4a34ebaf56f9a046231f54125ad046239d3f3` | MIT OR Apache-2.0 | Re-run Metal, Retina, resize, presentation latency, idle CPU, RSS, and device recovery checks after an update. |
| `alacritty_terminal` | Git revision `ede2ac144da4dec4c075bfa803aacf3b3739bce6`, or a later released crate proven to contain its zero-width-character memory bound | Apache-2.0 | Review the replacement diff and re-run adversarial parser, scrollback, alternate-screen, resize, selection, and latency checks. |
| `portable-pty` | `0.9.0` | MIT | Re-run child launch, attach, resize, signal, exit, takeover, file-descriptor cleanup, and shutdown checks. |
| `cosmic-text` | `0.19.0` | MIT OR Apache-2.0 | Re-run Korean shaping, bidi, fallback, glyph cache, malformed-font, Retina, and performance checks. |
| `cef` crate | `151.8.0+151.3.24`, cef-rs commit `a2e15ae659c4b3957883e34de879bd8b38360ce5` | Apache-2.0 OR MIT | Keep the binding and CEF Chromium revision paired and repeat the complete Browser, CDP, IME, AX, helper, bundle, security, and RSS gate for every update. |
| CEF binary | `151.3.24+g2384915+chromium-151.0.7922.174` | CEF BSD-style license plus Chromium third-party notices | Verify official archive identity and checksum before packaging and retain all required notices. |
| `accesskit` | `0.24.1` | MIT OR Apache-2.0 | Re-run semantic-tree, action, focus, selection, text, VoiceOver, and duplicate-node checks. |
| `accesskit_macos` | `0.26.3` | MIT OR Apache-2.0 | Re-run the macOS AX runtime gate and any narrow native gap adapters. |
| `global-hotkey` | `0.8.0` | Apache-2.0 OR MIT | Re-run permission, reserved-key, collision, remap, disabled-state, wake, and idle CPU checks. |
| `ignore` | `0.4.33` | Unlicense OR MIT | Re-run symlink, ignore hierarchy, cancellation, invalid path, and traversal-bound checks. |
| `grep-searcher` | `0.1.17` | Unlicense OR MIT | Re-run binary-file, large-file, cancellation, result-bound, and invalid-encoding checks. |
| `grep-regex` | `0.1.14` | Unlicense OR MIT | Re-run invalid-expression, complexity-bound, cancellation, and match-offset checks. |
| `notify` | `8.2.0` | CC0-1.0 | Re-run rename, remove, coalescing, duplicate, overflow, and external-save conflict checks. |
| `ropey` | `1.6.1` | MIT | Re-run Unicode edit, selection, undo, large-file, external-revision, and save-latency checks. |
| `security-framework` | `3.7.0` | MIT OR Apache-2.0 | Re-run create, read, update, delete, denial, locked-Keychain, accessibility-class, and redaction checks. |
| `russh` | `0.63.1`, source commit `d3ae702a43a163946f258297e398dc216339d5ce` | Apache-2.0 | Review cryptographic advisories and re-run host-key, authentication, agent, proxy, tunnel, PTY, reconnect, and cancellation checks. |
| `russh-config` | `0.58.0`, source commit `d3ae702a43a163946f258297e398dc216339d5ce` | Apache-2.0 | Re-run alias, include, precedence, invalid-config, and secret-boundary checks. |
| `russh-sftp` | `2.4.0`, source commit `e145c1f7ece99f41f558949ef59731f2cd1a9dfe` | Apache-2.0 | Re-run path, permission, partial transfer, cancellation, conflict, and remote-mutation checks. |
| `tokio` | `1.53.1` | MIT | Re-run idle CPU, blocking isolation, cancellation, shutdown, and main-thread isolation checks. |
| `tracing` | `0.1.44` | MIT | Re-run event schema, operation correlation, bounded payload, and pre-serialization redaction checks. |
| `tracing-subscriber` | `0.3.23` | MIT | Re-run filter, rotation, cross-process correlation, and no-secret artifact checks. |
| chromux verification tool | Package `0.29.1`, source commit `93f770f4bccf7fe8c867da285963fc09f30f4577` | MIT | Re-run external CDP attach, page ownership, detach, reattach, and profile persistence against each paired CEF revision. |

The exact CEF arm64 and x86_64 archive names, SHA-1 values, and published sizes are recorded in the research matrix and must be copied into the release evidence manifest without substitution.

## IME boundary

AppKit is responsible for converting native key and text-service events into the focused WGPU terminal or editor model.

Each WGPU text surface must expose an `NSTextInputClient` adapter with marked text, selected and replacement ranges, attributed substring queries, first rect for character range, valid attributes, command handling, cancellation, and focus transfer.

Composition text must remain distinct from committed PTY or editor input, and a focus change, pane close, tab switch, zoom transition, or window deactivation must resolve composition deterministically.

The candidate window geometry must be expressed in the correct AppKit screen coordinate space under Retina scale, window movement, pane resizing, and cross-display movement.

CEF retains Browser key, composition, candidate, and web-content behavior after AppKit routes focus to the native child view.

No event may be committed once by AppKit and again by the Browser, terminal, or editor path.

The T1 release spike proves physical Korean commit through the WGPU terminal path and visible Korean content in the CEF native child.

Complete composition, candidate, cancellation, replacement, and focus-transfer behavior across terminal, editor, and Browser remains final-product acceptance work.

## Accessibility boundary

Native AppKit controls publish their native semantic nodes, WGPU surfaces publish AccessKit nodes, and the native CEF child publishes its Chromium accessibility subtree.

The application must join those providers through focus and view hierarchy without duplicating a node, hiding a child subtree, or presenting a pixel-only canvas.

The WGPU tree must expose sidebar trees, tabs, split panes, buttons, settings, status, terminal, editor, zoom state, selection, labels, values, disabled state, expanded state, and meaningful actions.

AccessKit does not currently claim rich-text or hypertext support, so terminal and editor text range requirements must be validated and any narrow AppKit adapter must be documented before implementation expands.

The T1 release spike proves a live AX tree with meaningful navigator, tab, terminal, and editor labels.

VoiceOver navigation, complete keyboard-only operation, visible focus, notifications, text selection, dynamic updates, and Browser boundary traversal remain final-product acceptance work.

## Threat and failure boundaries

| Boundary | Threat or failure | Required containment and recovery |
| --- | --- | --- |
| Herdr protocol | Version mismatch, missing event, duplicate operation, stale snapshot, or host-ID collision can create a phantom or wrong-target pane. | Require a version handshake, host-scoped stable identifiers, ordered sequence, gap detection, snapshot resync, and idempotent operation identifiers before changing visible state. |
| PTY and parser | Untrusted terminal bytes can trigger memory growth, escape-sequence abuse, parser stalls, or misleading screen state. | Use the fixed Alacritty revision, bound scrollback and zero-width data, fuzz and replay hostile input, and keep parser failure visible inside the exact pane. |
| PTY process | Spawn, attach, resize, signal, exit, or takeover can leak children or file descriptors. | Give each child an ownership record, cancellation path, deadline, explicit exit state, and deterministic cleanup on pane close or application shutdown. |
| Text and fonts | Malformed fonts, large fallback sets, bidi input, or glyph-cache growth can stall or exhaust the renderer. | Bound caches and fallback work, use trusted packaged defaults, isolate font parsing, and surface degraded text without crashing the application. |
| WGPU | Device loss, surface loss, zero-size bounds, stale scale, or resize races can blank or overlap content. | Recreate only the affected GPU resources, keep AppKit view bounds authoritative, preserve model state, and show a recoverable renderer failure when recreation fails. |
| CEF content | Untrusted web content can exploit a stale Chromium revision or escape intended Browser capability. | Pin and promptly update CEF, retain its process isolation and sandbox where supported, minimize command-line switches, and expose no native bridge beyond reviewed typed operations. |
| CEF bundle | Missing, unsigned, mismatched, or wrongly located framework, helper, locale, resource, or rpath can fail only after installation. | Verify exact archive provenance, helper identities, signatures, resources, rpaths, clean-context launch, relaunch, and full process-tree identity in the release artifact. |
| CDP gateway | A broad or unauthenticated debugging endpoint can disclose cookies, page data, or control to another local process. | Bind loopback only, use session-scoped unguessable capability data, authorize source and Browser identities, redact endpoints from artifacts, and revoke the endpoint when the view closes. |
| chromux lifecycle | A client close operation can accidentally close a page owned by the IDE or leave an attached session stale. | Use the external-view contract, make the IDE own the page and profile, distinguish detach from close, and verify detach and reattach without page or profile loss. |
| Accessibility | Missing, duplicate, stale, or secret-bearing semantic nodes can block assistive use or disclose sensitive content. | Keep one provider per surface, update nodes from the authoritative projection, redact secrets, and inspect the live installed application with VoiceOver and AX tooling. |
| Shortcuts | Reserved keys, duplicate bindings, Option and Meta ambiguity, or global registration failure can invoke the wrong command. | Resolve commands in one registry, detect conflicts before activation, show registration failure, preserve application-local alternatives, and never silently remap. |
| Files and search | Symlink escape, path traversal, watcher overflow, rename races, invalid encoding, or stale save can read or overwrite the wrong file. | Canonicalize within the authorized root, retain stable file identity and revisions, bound search, cancel stale work, and require explicit conflict resolution before mutation. |
| Keychain | Secrets can leak through config, environment, diagnostics, screenshots, crash output, or overbroad Keychain accessibility. | Store only in Keychain with an explicit accessibility class, redact before serialization, show denial or locked state, and use synthetic secrets for all evidence. |
| SSH and SFTP | Silent host-key acceptance, credential leakage, proxy confusion, partial mutation, or reconnect against the wrong host can compromise remote work. | Require explicit host-key policy, typed connection stages, exact host identity, bounded retries, cancellation, atomic transfer where possible, and visible partial or uncertain outcomes. |
| Async runtime | Blocking PTY or file work, orphan tasks, cancellation races, or UI-thread access can freeze the application or preserve false success. | Isolate blocking work, use structured task ownership and cancellation, marshal UI work to its owner thread, and finish shutdown with explicit incomplete-operation states. |
| Diagnostics | Logs can expose secrets or report success before the authoritative owner confirms it. | Redact before serialization, correlate by operation and stable target IDs, bound payloads, record stage transitions, and derive visible success only from confirmed state. |
| Pet and overlay | Independent polling, stale priority, or drag and click confusion can diverge from the IDE or steal focus. | Consume the shared presentation store, keep one native gesture authority, suppress release clicks after drag, clamp by current displays, and perform click-to-pane by exact stable identity. |

## Rejected and deferred alternatives

- Electron is rejected because it duplicates application and Browser runtime ownership and is structurally misaligned with the Browser-closed process budget.
- Tauri is rejected because its webview application shell duplicates AppKit and WGPU ownership, including for the Pet migration.
- GPUI is rejected because it would replace direct AppKit and WGPU integration and follows Zed's fast-moving monorepo API.
- egui and eframe are rejected because they add an immediate-mode UI and lifecycle owner over the same surfaces.
- winit is rejected as the application lifecycle owner, with only a future narrow raw-window interop gap eligible for reconsideration.
- CEF off-screen rendering, IOSurface transfer, shared-memory frames, texture copying, and WGPU Browser composition are rejected because they split Browser ownership and add a second frame pipeline.
- A custom CEF bridge is rejected until a specific cef-rs blocker is proven and a narrow audited bridge receives a separate decision.
- `libghostty`, `termwiz`, and direct `vte` are rejected because they introduce a competing parser, terminal model, or foreign-function owner.
- A custom `openpty` implementation is rejected while `portable-pty` satisfies the bounded process responsibility.
- Direct `swash` is rejected because `cosmic-text` already bounds the font stack and may use it transitively.
- `crop 0.4.3` and Ropey `2.0.0-beta.1` are rejected for the first editor in favor of stable Ropey `1.6.1`.
- tree-sitter is deferred and excluded from the initial dependency graph because full syntax and LSP behavior is not a T1 requirement and every grammar adds separate provenance.
- A full hand-written `NSAccessibility` tree is rejected in favor of AccessKit plus only documented narrow native adapters.
- `keyring` is rejected because the macOS-only application needs explicit native error and accessibility-class behavior from `security-framework`.
- `openssh` is rejected as the primary transport because a subprocess path would create a second SSH model, while an explicitly approved compatibility path may be reconsidered after a proven russh gap.
- `ssh2` is rejected because its blocking libssh2 and C supply chain do not improve the selected Tokio integration.
- `async-std` is rejected because it is deprecated and would split cancellation and shutdown ownership.
- `log` and `env_logger` are rejected as the primary diagnostics pipeline because they do not enforce typed operation correlation or the required redaction boundary.
- official.browser and terminal-browser are rejected as runtimes because the former is unmaintained and the latter uses Electron and an off-screen frame-copy path.

## Layer sequence

1. T1 freezes the provisional ownership and pins, then proves the release application composition and hard budgets.
2. The Herdr repositories establish one versioned typed surface and ordered snapshot contract.
3. The native shell establishes AppKit lifecycle, WGPU layout, command routing, projection state, and accessibility boundaries.
4. The terminal establishes real PTY attachment, parser safety, text shaping, IME, selection, and presentation latency.
5. The file workbench establishes bounded traversal, search, buffer, save-conflict, and watcher behavior.
6. The Browser establishes a native CEF child, paired helper bundle, source identity, scoped CDP, and chromux lifecycle.
7. Remote access establishes host-key policy, SSH config, PTY, SFTP, reconnect, and reverse Browser control stages.
8. The global overlay and Pet consume the same presentation projection and exact focus identities.
9. The installed release application completes protocol, native UI, Browser, remote, accessibility, security, bundle, and performance acceptance.

No later layer may use a local shim to hide a failed contract or bypass a pending earlier gate.

## Cross-repository evidence identity

The judged worktree for this ADR covers `herdr-ide`, while later approved protocol and parity work also spans `herdr`, `herdr-agent-context-labels`, and `herdr-pet`.

A trustworthy multi-repository result requires the base and result commit for every repository, repository URL, dirty-state disclosure, protocol version, canonical schema SHA-256, generated-client SHA-256 where applicable, cross-repository test command and result, release application build identity, and the evidence manifest SHA-256.

Every event and screenshot used for parity must also record the host-scoped stable IDs and the exact repository commit set that produced it.

The archived Pet source pin remains evidence for parity behavior and asset provenance, but no edit or runtime observation from that separate repository is implied by this ADR.

Missing cross-repository commit or schema identity is not a reason to change this ownership model, but it blocks a trustworthy claim that T2 or later integration is complete.

## Hard performance gates

No official source or upstream benchmark proves or disproves the integrated release numbers below.

The T1 result therefore comes from the exact integrated release bundle and manifest-owned four-phase harness rather than an architecture estimate.

| Gate | Exact acceptance threshold | Required measurement identity | Current result |
| --- | --- | --- | --- |
| Warm usable launch | At most `1.000 s` to the PRD-defined usable state. | Release bundle SHA-256, exact machine and OS, fixture, warm definition, monotonic clock, and observed launch. | `PASS`, `186.092 ms` |
| Terminal input to present | p95 at most `50 ms`. | Physical input timestamp, presented-frame timestamp, release build, real PTY fixture, frame instrumentation, and at least 20 samples. | `PASS`, `n=32`, p95 `39.045 ms` |
| Stable idle CPU | Mean at most `1.0%`. | Release build, declared Browser-closed process tree, settle interval, sample period, fixture state, and display state. | `PASS`, mean `0.18%` |
| Browser-closed RSS | At most `200 MiB` with the fixed fixture of 7 workspaces and 11 panes. | Full declared application process tree, fixture manifest, steady-state samples, and peak. | `PASS`, peak `75.594 MiB` |
| One-Browser RSS | At most `800 MiB` including CEF and every helper. | Full declared application and CEF process tree, exact page and profile fixture, steady-state samples, and helper inclusion proof. | `PASS`, peak `579.031 MiB` |

A miss on any threshold blocks dependent implementation and requires the user to decide whether to change architecture, scope, or the sealed budget.

The implementer must not label a debug build, source inspection, archive size, another product's benchmark, one sample, app-only RSS, or a process-alive check as performance evidence.

## Mandatory T1 release spike evidence slots

Every slot identifies the exact release `.app` bundle, exactly one running instance, and immutable evidence paths with SHA-256 values.

These rows prove architecture feasibility only.

The complete feature behavior named by AC3, AC7, AC22, AC25, and AC26 remains gated by its dependent implementation tasks and final verification rows.

| ID | Required observation | Build and machine | Command or harness | Artifact path and SHA-256 | Result and reviewer |
| --- | --- | --- | --- | --- | --- |
| S01 | AppKit main-thread lifecycle, one main window, WGPU Metal surface, Retina resize, and observable retry recovery render without blank or overlapping application pixels. | macOS `15.1` build `24B2083`, Apple M4 Pro, arm64 release bundle | `t1-preflight --mode run` | `runtime-v1/runtime-evidence.json`, SHA-256 `8f102e52f2d92091e919a24319d94ad344e3b61c7ea2b0ca7a164aa8f1003523`; `04-split.png`, SHA-256 `4de3b8bd116ac4376c82467d19e02c1f15c53c719a5cc095c461e70f101dd859` | `PASS`, Implementor review 2026-08-27 |
| S02 | A real PTY launches, presents output, accepts physical input, reports presentation latency, and is cleaned up on application shutdown. | Same exact bundle and machine | Same harness, `warm_closed` | `runtime-v1/runtime-evidence.json`, same SHA-256 | `PASS`, full terminal parity deferred to T5 and T15 |
| S03 | Physical Korean input commits through the WGPU terminal path and Korean content renders in the CEF native child without duplicate commit in the measured path. | Same exact bundle and machine | Same harness, `ime.physical_keys` and Browser-included phase | `runtime-v1/runtime/warm_closed.events.jsonl`; `browser-included.png`, SHA-256 `7f8cdd51bf75576d9bc297ac5ddb7b203616549943d329559a14d352646a000a` | `PASS`, complete IME matrix deferred to final verification |
| S04 | The paired CEF revision renders as an AppKit native child, starts packaged helpers, exposes a healthy loopback CDP page, preserves its profile, and shuts down cleanly. | Same exact bundle and machine | Same harness plus the two-run permission-state discriminator | `runtime-v1/runtime-evidence.json`, same SHA-256; `notification-permission-discriminator-20260827/result.json`, SHA-256 `e05df2a3f48ea81f6d1d9934c3fa855b66b8f122bbd971756d52e74b4ae6026a` | `PASS`, full chromux lifecycle deferred to T8 and V5 |
| S05 | Physical `Cmd+Shift+Enter` routes through the native command path, toggles pane zoom, and restores the prior split and focus. | Same exact bundle and machine | Same harness, `zoom.shortcut` | `05-zoomed.png`, SHA-256 `857075cbeac7c4c56d57966abb72e76be85b31f262b56f8480703c44f9c7f7ef`; `runtime-v1/runtime-evidence.json`, same SHA-256 | `PASS`, central registry and global shortcut behavior deferred to T10 and T11 |
| S06 | Live AX inspection exposes meaningful navigator, tab, terminal, and editor roles and labels in the running native application. | Same exact bundle and machine | Same harness, `ax.snapshot` | `runtime-v1/ax/03-split.txt`, SHA-256 `839c485b3e80f99ca0ece557bba4dc1d00e0b53442e27de0242fed52805df3a0` | `PASS`, full VoiceOver and CEF traversal deferred to V4 and V5 |
| S07 | Focus, z-order, native child bounds, Retina resize, Browser visibility, and terminal zoom preserve the measured split topology and focus restore. | Same exact bundle and machine | Same four-phase harness | `runtime-v1/runtime-evidence.json`, same SHA-256; three native screenshots under `runtime-v1/screenshots/` | `PASS`, full cross-surface and sleep or wake matrix deferred to T15 |
| S08 | The release bundle contains the exact CEF framework and five helper variants, passes deep signature and executable inspection, launches from a clean exact-instance state, and relaunches with persisted state. | Same exact bundle and machine | Same harness, bundle inspection plus `relaunch_closed` | `runtime-v1/bundle-evidence.json`, SHA-256 `4e99f166136622c2587d7217a2ed2ebcc7bba7657565aa68908c74e151809cff` | `PASS`, final icon, Pet, notices, and completed-product bundle remain V8 work |
| S09 | Warm launch, terminal input-to-present p95, idle CPU, Browser-closed full process-tree RSS, and one-Browser RSS including helpers meet every hard threshold. | Same exact bundle and machine | Same harness with 10 process samples per profile and 32 input probes | `runtime-v1/runtime-evidence.json`, same SHA-256 | `PASS`, `186.092 ms`, `39.045 ms`, `0.18%`, `75.594 MiB`, `579.031 MiB` |
| S10 | The spike records exact source, dependency, CEF, manifest, bundle, executable, dirty-state, and evidence identities needed to reproduce the architecture decision. | Source `ac7723f73c6483bd1bd58d36c7663a43ade93bc8`, run-owned untracked spike disclosed | Research matrix, ADR, runtime manifest, bundle manifest, and harness result | `runtime-v1/result.json`, SHA-256 `16eebc6009e3ab53f5f2ea7dd3f42cd8302d7f20b9577fd651451bc60e989a68`; manifest SHA-256 `1355bb4d95ed277de66eb51e12d30fadaf519230103bac663a7f0c7271490f7d` | `PASS`, cross-repository product contracts remain T2 through T16 work |

## Decision consequences

The architecture has clear owners and avoids a second application framework, a duplicate Browser renderer, and a duplicate protocol store.

The design intentionally pays the integration cost of custom WGPU IME and Accessibility plus CEF native-child packaging because those responsibilities are required by the approved product boundary.

CEF adds a large and fast-moving security and packaging surface, so the Browser revision cannot be updated or left stale without explicit evidence.

The selected 0.x crates reduce custom implementation but require narrow adapters and repeated compatibility proof.

The exact Alacritty git revision creates a temporary source pin that must move to the first verified release containing the memory-bound fix.

Herdr Pet's pinned standalone source exposes sizes `124`, `152`, and `176` with default `152`, which is recorded as a factual mismatch while the IDE-owned implementation remains fixed at the sealed `124x124` contract.

The research portion and every T1 architecture-feasibility slot now pass.

This permits T2 to begin but does not close any later feature task or final verification row.

## Reopen conditions

This ADR must be reopened before adding Electron, Tauri, GPUI, egui, a winit-owned lifecycle, Browser OSR, IOSurface or texture frame copies, a custom CEF bridge, a second async runtime, or a second protocol store.

This ADR must also be reopened when a selected dependency loses its compatible license or maintenance path, a critical update cannot be adopted, a native architecture is dropped, a required IME or Accessibility path is impossible, or a hard performance gate fails.

Failure does not authorize an automatic fallback.

The implementer must stop dependent work, preserve the evidence, describe the exact failed boundary and alternatives, and request a new user decision.

## Version history

| ADR version | Date | Change |
| --- | --- | --- |
| 1.1 | 2026-08-27 | Recorded the passing integrated release spike, the user-approved `800 MiB` Browser budget, the permission-state discriminator, real screenshots and AX evidence, clean shutdown, and the measured T1 performance results while keeping final feature verification deferred to its dependent tasks. |
| 1.0 | 2026-08-27 | Recorded the provisional native ownership decision, exact dependency pins, rejected alternatives, threat and failure boundaries, cross-repository identity requirements, hard performance gates, and mandatory release-spike evidence slots. |
