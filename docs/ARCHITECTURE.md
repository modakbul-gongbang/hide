# Runtime architecture

This document owns the shape of the runtime: what the Rust core holds, what the Swift shell may do, what Herdr keeps for itself, and the decisions that were made when those boundaries were crossed the wrong way.
Read it in full before changing anything under `herdr-core/` or `macos/`.
[AGENTS.md](../AGENTS.md) keeps the short form of the ownership split; this is the long form with the reasons.
The code is the executable authority: `herdr-core/src/` for the core, `macos/Sources/HerdrMacOS/` for the shell, and the tests beside each.

## The core, the shell and Herdr

The core (`herdr-core`) owns all state behind one `Mutex<Runtime>`.
The shell dispatches typed JSON events in (`herdr_core_dispatch`) and pulls state out (`herdr_core_snapshot`) when the change notifier announces.
The event sync coordinator (`session_sync.rs`) bootstraps from `session.snapshot`, resumes ordered topology updates through `events.subscribe`, and refreshes agent telemetry with `agent.list` once per second.
A tick whose `agent.list` is unchanged publishes nothing, so an idle session recomputes no projection; the catalog's own refresh window still publishes, because the rebuild can only happen inside `publish_replica`.
The Git section refreshes local worktree state only when repository metadata, tracked paths, or Herdr worktree topology changes; disk usage refreshes when the section opens or its header refresh is pressed, and all three layers run outside the runtime mutex.
Pull requests also load once when a local Git project first appears in the sidebar and refresh from that project's menu or PR popover; these scoped requests reuse the same background reader, cache and generation coalescing.
Per-pane attach threads stream PTY bytes into the runtime as terminal chunks.
Everything the shell renders comes from that one snapshot pull.

The shell holds no authority, but the core does not hand all of it to Herdr either.
Herdr owns pane existence, split geometry, zoom, cwd, agent lifecycle and the PTY; the core owns each checkout's visible tab, the keyboard focus pane, panel visibility and text scale.
A core-owned value changes on the event that asked for it and Herdr is told afterwards, so the canvas and the focus ring never wait for a round trip.
While that notification is pending, the Herdr workspace that owns the target showing it is read as its confirmation, whichever workspace holds Herdr's keyboard, because a checkout is keyed by path and can hold tabs from several Herdr workspaces; with nothing pending, a move of Herdr's focused tab or pane to another value is followed and a diagnostic records the ids and the origin; a refusal or a timeout keeps the core's value and says so.
A non-focused workspace's active tab is that workspace's memory, never a focus to follow: folding every workspace's active tab into one value per checkout let the last one overwrite the rest, and every tab focus on the other workspace timed out and snapped back.
The focused checkout always draws the tab that holds the selected pane: a tab action moves the pane into the tab, and a pane action, a restore, or a retirement moves the tab to the pane (`align_visible_tab_with_selected_pane`).
The pending model covers the visible tab and the focused pane and nothing else: zoom, splits, closes and resizes still wait for Herdr, because their geometry decides the PTY size (commit 9570a2a).

The notifier announces once per burst rather than once per change.
`herdr_core_snapshot` clears the announcement flag **before** it takes the lock; clearing it after the read would swallow a change that landed during the read.
Launch creates the core once, after the runtime resolution (login-shell PATH, binary, version) has finished, and the first window is presented before that resolution completes.
The first attach uses the size the view reported, or the size persisted from the last launch, and never a placeholder; a pane with no known size is held back and says it is waiting.

A clicked path is one event, not a sequence.
The shell resolves the token on the filesystem, decides which registered checkout owns it by the longest symlink-resolved prefix, and sends `reveal_path`; the core then decides the focused checkout, the right panel's visibility and section, the tree's expanded set and selection, and the editor tab together.
Dispatch is fire-and-forget, so four separate events would arrive as four frames and a refusal partway would leave the screen half moved.
A path outside every checkout never reaches the core: the shell hands it to macOS, opening a file in its default application and a folder as a Finder window, and revealing rather than opening anything whose default application is the operating system running it - an executable file, an application bundle, an installer package - because link detection is a guess over arbitrary agent output and one wrong click must not start a program.

A spawned child does not split the operator's pane.
Herdr owns split geometry and the PTY size, so a delegated child pane is really moved out - `pane.move` to a new tab in the workspace it is already in - rather than left undrawn; a tab holding nothing but delegated children then stays out of the tab strip while remaining in the checkout.
Detection is the same on every pass, so a child that arrives while Hide is running and one already split when Hide started take the same path, and a refusal is retried on a fixed interval rather than assumed to have worked.
Herdr reports a refusal as an unchanged move with a reason rather than as an error, so the decision reads `changed` instead of trusting a successful request.

Ownership is the fourth derived status axis and it is read off the lineage, never stored.
A delegated row can only be Working or Seen, so a child's question or completion never enters the operator's own attention groups; a per-child stall clock is what brings work back when it stops being anybody's problem.
`docs/status-model.md` owns both rules.

What an agent has spawned in-process is not on Herdr's wire at all.
The hook helper reports it through `herdr pane report-metadata`, which Herdr defines as display-only pane metadata, and the core reads it back out of the pane tokens its ordinary snapshot already carries; `herdr-core/src/agent_hooks.rs` is the only place that reads those tokens.
A count Hide cannot read is reported as unknown, never as zero.

An attach lives only while its tab is in the last five shown.
Herdr renders a pane for every attached client, so an attach nobody is looking at costs a child process here and a render there for the life of the process; visiting eight tabs used to leave eight attaches alive.
The core keeps the most recently shown tabs (`ATTACHED_TAB_LIMIT`) and releases the rest, which is the ordinary session drop, not a new path.
A released pane keeps its projection entry carrying the transport state `released`, because the sidebar and the pane header read their state from there and a missing entry reads as a failure; the shell drops that pane's canvas and its held bytes on that state, so the tab redraws from Herdr's own frame on the next visit.
Nothing re-attaches it until it is shown again: an idle tick attaches only the visible tab's panes.

Tab reorder ownership is decided per drag, not per checkout.
Herdr orders the tabs inside one of its workspaces and has no order that spans two of them, so a strip slot is refilled from the workspace that slot already belongs to and Hide owns how the workspaces and the file tabs interleave.
A drag that changes the moved tab's own workspace subsequence sends one `tab.move` with an index counted in that workspace; a drag that only steps over another workspace's tabs settles locally with no Herdr call.
Deciding this for the whole checkout is what refused every drag in a checkout two Herdr workspaces share, which is the ordinary arrangement for a repository opened twice.

A pane that is going away ends its attach quietly.
Herdr closes the PTY before it reports the pane gone, so the attach child ends while the pane is still drawn; projecting that as `ended` is what flashed "terminal attach ended" over a pane the operator had just closed.
A close Hide asked for, or a pane Herdr has already stopped listing, projects `closing` with no notice chunk and keeps the pane's last frame until it is removed. Every other reason still reports `ended` with its message.

## The Herdr wire boundary

The bundled Herdr release is pinned in one place, `macos/Sources/HerdrMacOS/Resources/herdr-bundle.json`, and `contracts/herdr-api.schema.json` is derived from it: it is what that exact binary answers to `api schema --json`, never a copy from a Herdr checkout.
`herdr-core/build.rs` turns the five sub-schemas into Rust modules under `herdr_contract::wire` at build time; generated source stays in `OUT_DIR` and is never committed.
`herdr-core/src/wire.rs` is the only boundary that converts generated values into the core's projection and event inputs and builds generated subscription parameters.
Do not write new wire deserialization structs in `session_sync.rs` or import generated types into domain, runtime or sidebar code.
The pinned event schema currently omits protocol, host and sequence: only the boundary's minimal metadata envelope is handwritten, and its schema-gap test requires deletion when the fork declares those fields.
Request envelopes still name their method explicitly because generation does not discriminate method constants; use generated parameter types inside them.
`live.rs` and `remote.rs` also use this boundary for response decoding and generated request parameters.
The boundary preserves remote protocol diagnostics before decoding the complete generated snapshot, and the isolated pinned-server probe checks the control responses and CLI-created agent envelope.
Terminal input, scroll, resize and release messages and the parameterless snapshot request remain boundary-owned schema gaps, with tests that require migration when their parameter types appear.


## The bundled Herdr runtime

The app runs the Herdr it bundles: `HerdrRuntimeResolver` verifies the bundled binary against the manifest digest and starts it on the default socket when no server is running there; a server that is already running is joined as it is when its protocol matches, and refused with the two revisions and the `herdr server stop` remedy when it does not.
There is no installed-CLI candidate list and no version floor; the pin is exact.
The Swift shell reads the manifest at launch, and `scripts/fetch-herdr-runtime.sh` downloads and verifies the asset against it for both `scripts/build-app.sh` and `macos/scripts/build_dev_app.sh`; `scripts/check-herdr-pin-single-source.sh` fails when any of those restates the value.
Move the pin with `scripts/bump-herdr.sh <release-tag>` (a stable `v0.8.3` or a `preview-...` tag), which verifies the asset, writes the contract that binary reports, and rewrites the tag, version and digest tokens in the README, install guide and third-party notice.
`.github/workflows/herdr-update.yml` polls for a new stable release weekly and opens a PR with that bump after running both test suites; it never merges, because the core's Herdr behavior assumptions are only asserted against fixtures this repository wrote.

