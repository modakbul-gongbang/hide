# Runtime architecture

This document owns the shape of the runtime: what the Rust core holds, what the Swift shell may do, what Herdr keeps for itself, and the decisions that were made when those boundaries were crossed the wrong way.
Read it in full before changing anything under `herdr-core/` or `macos/`.
[AGENTS.md](../AGENTS.md) keeps the short form of the ownership split; this is the long form with the reasons.
The code is the executable authority: `herdr-core/src/` for the core, `macos/Sources/HerdrMacOS/` for the shell, and the tests beside each.

## The core, the shell and Herdr

The core (`herdr-core`) owns all state behind one `Mutex<Runtime>`.
The shell dispatches typed JSON events in (`herdr_core_dispatch`) and pulls state out (`herdr_core_snapshot`) when the change notifier announces.
The event sync coordinator (`session_sync/coordinator.rs`) delegates Herdr snapshot and subscription lifecycle to `session_sync/subscription.rs`, opens `events.subscribe` first and reads `session.snapshot` second, applies the topology events that follow, and refreshes agent telemetry with `agent.list` once per second.
Herdr's stream carries no sequence and cannot be resumed from a position, so subscribing before the snapshot is the only way not to lose an event between the two, and every reconnect is a fresh subscription followed by a fresh snapshot.
The price of that order is that an event emitted just before the snapshot was taken arrives as well; for one second after the snapshot the replica reconciles (`ApplyMode::Reconcile`), dropping with a diagnostic an event the snapshot already accounts for, and after that window an event the replica cannot apply is a real divergence that rebuilds it.
`wire.rs` names the day Herdr sequences its stream: a test fails when the event schema declares `sequence` or the subscribe params take `after_sequence`, because a resume cursor would then be worth building back.
A separate 250 ms coordinator tick advances every bounded asynchronous operation, so an acknowledgement or topology wait reaches its deadline even when Herdr emits no event.
A tick whose `agent.list` is unchanged publishes nothing, so an idle session recomputes no projection; the catalog's own refresh window still publishes, because the rebuild can only happen inside `publish_replica`.
The Git context refreshes local worktree state only when repository metadata, tracked paths, or Herdr worktree topology changes; disk usage refreshes when the section opens or its header refresh is pressed, and all three layers run outside the runtime mutex.
Pull requests also load once when a local Git project first appears in the sidebar and refresh from that project's menu or PR popover; these scoped requests reuse the same background reader, cache and generation coalescing.
The Changes reader is also the only Git-status owner for Explorer decorations: it reads one focused checkout while Explorer or Changes is visible or a diff tab needs it, normalizes rename and conflict state, and publishes one root-scoped changed-file set for both Swift surfaces.
The AppKit outline derives file and ancestor-folder decorations from that snapshot in memory; it never starts Git from a row, scroll, hover, or paint.
Per-pane attach threads stream PTY bytes into the runtime as terminal chunks.
Everything the shell renders comes from that one snapshot pull.
The shell decodes it strictly: one string value it does not know fails the whole decode, the bridge keeps its last good frame, and every later notification repeats the failure until the two sides agree, so an unknown value is a stalled shell, not a blank field.
The string enums the core serializes into the snapshot are therefore pinned in `contracts/snapshot-wire-enums.json`; `model.rs` tests that each variant emits the listed value and `SnapshotWireEnumTests` that each listed value decodes, so a variant added on one side fails a suite before it can reach a running shell (a `PullRequestTitle` origin once shipped as `pull_request_title` against a shell that read `pr_title`).
The bridge publishes `bridgeError` only on change and names the coding path in it, because a repeated failure republished every notification rebuilt every view observing the bridge at the notification rate.

## Project sessions and Memory

`hide-project` is the single durable identity boundary for session discovery, Memory storage, core projection, and hook retrieval.
It folds linked worktrees into their canonical main worktree, identifies plain folders in device scope, and returns a typed failure instead of falling back to another Project or a display name.
`hide-session` owns provider-specific file discovery and parsing for local Claude Code and Codex sessions, while `herdr-core` receives only provider-neutral catalog rows and archived ledger events.
Raw transcript bodies remain in the providers' files.

`hide-memory` owns Hide-native extraction and relation planning, one app SQLite store, its schema and migrations, Project hard filters, session cursors, item and revision lifecycle, provenance, receipts, query-dependent retrieval, and the active FTS5 projection.
Official Mem0 OSS v2.1.0 does not execute in Hide and is not a runtime dependency, embedded service, compatibility package, daemon, account, credential, or vector database.
Its pinned extraction and update prompt assets are retained only as audited design-reference provenance under `hide-memory/reference/mem0-v2.1.0/`.
`hide-memory/hide-native-engine-reference.json` records the exact upstream commit, audited source hashes, local prompt-asset hashes, and the explicit non-runtime role.
Every Hide-native analysis result is a proposal only: the write service validates Project identity, provenance, redaction, lifecycle, capacity, relation authority, and transaction boundaries before any durable change.
Normalized session events and the bounded active-Memory comparison set both pass through the current local redactor at the final provider egress boundary.
Relation planning receives a relevance-neutral active-Memory comparison set, rather than prompt FTS results, so same-meaning Korean and English memories do not need shared literal tokens to reach the provider's strict relation schema.
The comparison set has item, token, and serialized-byte caps and is composed with normalized events as JSON arrays under the final 64 KiB request cap.
The disclosure acceptance is versioned; a material egress-copy change disables previously enabled Projects until the operator accepts the current disclosure, without deleting their local data.
Prompt retrieval confirms lexical or meaningful cwd/path overlap before ranking; salience, extraction confidence, and recency only break ties after relevance and are never presented as semantic similarity.
The app owns the only writer connection.
Hook helpers and render-facing reads open read-only connections, fail closed on schema or projection drift, and never rebuild the index in the prompt path.
The same SQLite store owns one random authentication key per Project, and each hook receipt is authenticated over Project, runtime, session, event, and the exact ordered item revisions before the core accepts it as provided-history.
Transcript text prefixes remain display classification only: receipt authority additionally requires Claude provider-owned metadata or a Codex developer message, so human or Project instruction text cannot assert a receipt.

The coordinator's existing worker context owns session refresh, the five-second due-work poll, and one Memory analysis intent at a time.
It waits for a session file to remain unchanged for sixty seconds, reads only complete events after the durable cursor, redacts known credential patterns locally, and sends bounded normalized input through `hide-ai` outside `Mutex<Runtime>`.
The durable cursor stores file identity and the safe start offset of an incomplete final line, never that line's bytes; a later read resumes from the provider-owned session file.
Each incremental poll reads at most 1 MiB and retains no JSONL line larger than 256 KiB, while catalog and archive detail reads reject a complete session larger than 64 MiB.
The same provider, session, content-hash retry converges through a deterministic receipt instead of repeating revisions.
All filesystem, SQLite, hook-config, provider, and serialization work occurs outside the runtime mutex; applying a completed worker result is the only locked transition.
Disabling Memory stops new analysis and injection without deleting its data, while Forget, revision Undo, and confirmed Project deletion have their own explicit lifecycle operations.

The core owns the fourth right-panel section, each Project's Sessions/Memory mode, filters, actionable analysis state, and editor preview identity.
Opening Memory for a turn is one typed event that makes the panel visible, selects Sessions, enters Memory mode, and applies the exact provided-item filter in one frame.
The Swift shell renders those snapshot values and dispatches typed actions; it does not discover sessions, rank memories, infer counts, or classify failures.

The agent-context-labels plugin is a separate headless consumer of the same Herdr socket contract.
It opens one long-lived `events.subscribe` stream for pane lifecycle events, bootstraps pane state with `agent.list`, and reports metadata only after a display transition.
Its event loop also receives hook and refresh wakes through a short-lived Unix socket in the plugin state directory.
When the stream ends, the plugin reconnects with bounded exponential backoff and always starts from a fresh pane list; an event is only a prompt to list again, and nothing is read off it but its kind.

The shell holds no authority, but the core does not hand all of it to Herdr either.
Herdr owns pane existence, split geometry, zoom, cwd, agent lifecycle and the PTY; the core owns the focused project and checkout, each checkout's visible tab, the keyboard focus pane, panel visibility and text scale.
The core also owns which Claude and Codex panes show their conversation ledger in place of the terminal (`ui_state.conversation_pane_ids`): a pane opens on its terminal, enters the set only through `toggle_conversation`, and leaves it with the pane, so a refresh never turns a pane back into a conversation the operator did not ask for.
A core-owned value changes on the event that asked for it and Herdr is told afterwards, so the canvas and the focus ring never wait for a round trip.
Selecting a pane first moves the core-owned project, checkout and visible tab to the context that owns that pane, then sends one `pane.focus` request to Herdr; splitting that user action into project, tab and pane requests would expose intermediate frames and make partial refusal possible.
While that notification is pending, the Herdr workspace that owns the target showing it is read as its confirmation, whichever workspace holds Herdr's keyboard, because a checkout is keyed by path and can hold tabs from several Herdr workspaces; with nothing pending, a move of Herdr's focused tab or pane to another value is followed and a diagnostic records the ids and the origin.
A refusal or timeout keeps the core-owned context at the requested target, records a diagnostic and resolves the correlated shell request as a retryable failure; the shell presents Retry and Dismiss, and Dismiss clears only that notice.
A non-focused workspace's active tab is that workspace's memory, never a focus to follow: folding every workspace's active tab into one value per checkout let the last one overwrite the rest, and every tab focus on the other workspace timed out and snapped back.
Herdr moves that memory silently: a close that removes a workspace's active tab is followed by `tab_focused` only when the workspace holds Herdr's keyboard focus, and by nothing at all otherwise.
The replica therefore asks `workspace.get` for the replacement the moment such a close is applied and on each operation tick while the answer names a tab the stream has not delivered yet; a workspace still waiting after a bounded number of reads is a replica that cannot converge, and it is rebuilt from a fresh snapshot.
Waiting for the focus event alone froze the whole workspace's projection, so a closed pane stayed drawn as `closing…` for three to five seconds, until the operator's next tab focus or the close deadline's status check.
The focused checkout always draws the tab that holds the selected pane: a tab action moves the pane into the tab, and a pane action, a restore, or a retirement moves the tab to the pane (`align_visible_tab_with_selected_pane`).
The pending model covers the visible tab and the focused pane and nothing else: zoom, splits, closes and resizes still wait for Herdr, because their geometry decides the PTY size (commit 9570a2a).
Every Herdr-owned mutation has one core-owned operation record with a host connection generation, target and conflict-scope IDs, phase, stage, start time, absolute deadline, caller-visible message, and retry policy; the records are exported as `status.async_operations`.
Transport success is not topology truth: split, zoom, resize, move, and close remain pending until an authoritative session event or fresh snapshot identifies the exact created, changed, or absent topology.
An expired or otherwise unconfirmed mutation becomes unknown and is never resent when doing so could repeat a destructive effect; same-scope requests are rejected while independent scopes continue.
Resize intents on one pane and axis coalesce to the latest signed delta, so a fast gesture cannot create an unbounded queue.

The notifier announces once per burst rather than once per change.
`herdr_core_snapshot` clears the announcement flag **before** it takes the lock; clearing it after the read would swallow a change that landed during the read.
Launch creates the core once, after the runtime resolution (login-shell PATH, binary, version) has finished, and the first window is presented before that resolution completes.
The first attach uses the size the view reported, or the size persisted from the last launch, and never a placeholder; a pane with no known size is held back and says it is waiting.
An attach asks for the grid the view itself reports whenever one is known, because that grid is the one the frame guard accepts: starting at a settled size the view had already left held every frame until a resize, and a pane that is not drawn cannot send one through the display tick, so the shell also samples a settled size from the run loop one display period apart (`TerminalHost.Coordinator`).
A frame at a foreign grid is diagnosed once per grid, not once per frame; the held frames themselves are the ordinary signal that the resize has not landed.

A clicked path is one event, not a sequence.
The shell resolves the token on the filesystem, decides which registered checkout owns it by the longest symlink-resolved prefix, and sends `reveal_path`; the core then decides the focused checkout, the right panel's visibility and section, the tree's expanded set and selection, and the editor tab together.
Dispatch is fire-and-forget, so four separate events would arrive as four frames and a refusal partway would leave the screen half moved.
A path outside every checkout never reaches the core: the shell hands it to macOS, opening a file in its default application and a folder as a Finder window, and revealing rather than opening anything whose default application is the operating system running it - an executable file, an application bundle, an installer package - because link detection is a guess over arbitrary agent output and one wrong click must not start a program.

A spawned child does not split the operator's pane.
Herdr owns split geometry and the PTY size, so a delegated child pane is really moved out - `pane.move` to a new tab in the workspace it is already in - rather than left undrawn; a tab holding nothing but delegated children then stays out of the tab strip while remaining in the checkout.
Detection is the same on every pass, so a child that arrives while Hide is running and one already split when Hide started take the same path, and a refusal is retried on a fixed interval rather than assumed to have worked.
Herdr reports a refusal as an unchanged move with a reason rather than as an error, so the decision reads `changed` instead of trusting a successful request.

Ownership is the fifth derived status axis and it is read off the lineage, never stored.
A delegated row can only be Working or Seen, so a child's question or completion never enters the operator's own attention groups; instead it is a signal in every ancestor's read fingerprint, so the ancestor turns unread and its badge reports the count, while the ancestor's own group stays whatever its own axes say.
The lineage is therefore built before the read axis is applied on every ingest, and there is no clock, timer or second store for it.
`docs/status-model.md` owns both rules.
The Agents `My Work` view filters only the core-final Delegated answer and leaves visible orphans in operator-owned groups; `All` changes only the shell's session-local visibility projection.
Overview groups the same canonical agents by the checkout their pane is in and nests a child under its parent only from authoritative child IDs; a parent in another worktree is named in a caption, never inferred.
The Overview has no selection of its own: a row click dispatches the existing pane-selection event, a header click the checkout-focus event, and the `N files` chip one `overview_open_section` event that focuses the checkout and switches the panel to History together, so a refusal cannot leave the screen half moved.
`agent_start_in_checkout` creates a tab in a Herdr workspace used exclusively by that project through the task-operation slot that `create_worktree` already uses, and the shell starts the chosen provider in the created pane on the same path.
When every known Herdr workspace for the checkout is also associated with another registered project, Add Tab and agent start create a fresh workspace with the checkout path instead of leaking the other project's label and tabs into the new surface.

What an agent has spawned in-process is not on Herdr's wire at all.
The hook helper reports it through the `pane.report_metadata` socket method, which Herdr defines as display-only pane metadata, and the core reads it back out of the pane tokens its ordinary snapshot already carries; `herdr-core/src/agent_hooks.rs` is the only place that reads those tokens.
A count Hide cannot read is reported as unknown, never as zero.

A pane's parent travels the same channel, and it is the only lineage there is: Herdr records none.
Whoever creates a child pane declares its parent as the pane token `parent_pane` through `pane.report_metadata`; Hide's own fork does (`fork.rs`, three socket calls: `pane.split`, `agent.start`, then the declaration under source `hide`), and so does an orchestrator that starts its child with `agent.start` (sasu's dispatch, under its own source).
`wire.rs::lineage_parent` reads that token into the one `spawned_from_pane_id` the sidebar's lineage is built from; `docs/status-model.md` owns the contract.

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

### Explicit terminal attachments

`ImeTerminalView` and `TerminalFileDrop` own only native pasteboard ingress.
Finder file URLs, Command-V images and Control-V images become one `terminal_attachment` intent carrying the original pane ID, bracketed-paste mode and a generated request UUID.
Control-V without an image and ordinary text paste retain SwiftTerm's input path; active IME composition is not consumed by image ingress.
Clipboard preparation completes that same intent with `terminal_attachment_ready`, after one off-main task validates encoded bytes and pixel dimensions and writes a private PNG beside the app state under `TerminalClipboard`.
No clipboard image bytes enter the JSON event or the render mutex.

`runtime/attachments.rs` owns admission, captured terminal and remote connection generations, one active worker, retry, cancellation and an ordered 64 KiB input reservation.
Subsequent input to the originating pane is held until file preparation and transfer succeed, then the complete quoted path payload and held bytes enter the existing terminal writer together.
No Enter is synthesized, and an upload failure never forwards held input, because it may contain an Enter that would submit an incomplete prompt.
Retry keeps immutable prepared bytes; cancel discards held input explicitly.
Missing, closed, released or reconnected targets retire the intent and cannot forward its data to a replacement session or fall back to the local host.
Only admission, completion and actionable failure transitions publish a notice through `status.async_operations`; terminal-specific actions render in the originating pane.

`terminal_attachments.rs` reads only explicitly selected regular files, rejects final-component symlinks, special files, control-character paths and files that change while read, and constructs ordered quoted terminal input.
The bounds are eight files, 20 MiB per file and 40 MiB per intent; clipboard decoding additionally allows at most 16 megapixels.
`remote/attachments.rs` transfers the immutable bytes with the existing authenticated `RusshSftpTransport`, not a shell command or a subprocess, and returns only remote paths.
Remote staging lives in the remote user's private `.hide-terminal-attachments` directory, with generated exclusive filenames, 0700 directory and 0600 file permissions, ownership checks and same-byte verification before adopting a completed upload on retry.
Connection setup is bounded at 15 seconds and transfer work at 45 seconds; cleanup has its own bounded connection and operation timeouts.
Neither filesystem nor network I/O runs under `Mutex<Runtime>`.

Both staging locations admit at most 128 files and 256 MiB.
Cleanup scans only their immediate generated entries on the next explicit attachment intent and expires entries older than 24 hours; there is no background janitor and successful paths may therefore survive longer while idle.
Local clipboard PNGs remain available after a successful local paste, while remote success releases the local PNG after upload.
Cancellation, retirement and destruction attempt to remove only that intent's exact generated files; failed remote cleanup is diagnostic and remains bounded by the next-intent expiry policy.
Original user-selected files are never deleted.
This boundary has no provider-specific draft, composer or attachment shelf.

### Reopening locally closed work

The core owns one session-local, twenty-item LIFO stack for file tabs and local Herdr pane or tab closes initiated through Hide.
Browser-only panes, remote closes, and topology changes reported by another Herdr client never enter it.
Before sending a Herdr close, a background worker exports immutable layout facts and the core records the close intent in its target scope; it enters `closing` immediately, moves selection only to a confirmed surviving item, and starts the external effect after capture succeeds.
The captured item is a reservation separate from the twenty confirmed entries, so a failed or unknown close cannot evict older undo history.
A definitive Herdr refusal releases only that reservation; a transport failure or malformed acknowledgement keeps it available, starts one read-only status check, and reports the uncertainty inline.
The status check applies its fresh session snapshot to the navigator before classifying the reservation, so a target confirmed absent cannot remain drawn until another unrelated event arrives.
Only an authoritative absence promotes a reservation into the confirmed LIFO stack, in original user request order; the newest unresolved reservation blocks reopen from silently selecting an older item.
The snapshot exposes the count, top label, pending reservations, in-flight state, async operation records, and inline notices; the Swift shell routes the menu and shortcut and renders those values without keeping a second stack.
Unknown agent activity is a separate close guard: the core refuses local and remote destructive close until a fresh status is available, while ordinary working or unresolved demand uses the existing one-time confirmation.

Recreation also runs outside `Mutex<Runtime>`.
Pane restore uses the captured parent path, direct neighbor, split direction, original first-child ratio, and cwd, falling back to the tab's current pane and then the checkout root when the original facts no longer exist.
A direct pane sibling uses `pane.split`; a sibling subtree is wrapped at its captured nested parent with `layout.apply`, preserving the surrounding tree and every existing ratio.
Tab restore prunes Browser leaves, applies the remaining exported layout, restores its workspace position, and starts each captured agent in the new pane with the captured session id when the agent kind supports resume.
The create, split, and applied-layout requests stamp their panes with a reserved `HIDE_REOPEN_INTENT` environment value containing the closed-item key and mutation stage.
On a retry, the worker adopts only a pane or layout carrying that exact marker; a new id, matching cwd, or matching workspace or tab label is never ownership evidence, so an unrelated agent cannot be interrupted as part of restore.
Both a recovered layout and the first `layout.apply` result must contain the exact expected terminal-pane count before any agent starts.
An under-count is repaired once by reapplying the fully tagged intended layout to the owned tab and revalidating the returned count; an over-count is never adopted or overwritten because the extra pane may have another owner.
The pinned create and split mutations have no operation context, and the request envelope id is correlation only, so this marker is the topology evidence that makes an acknowledged-late mutation converge without guessing.
The pinned Herdr can reuse a just-closed pane id while its old PTY still owns the agent process, so the restore worker interrupts that reused process before `agent.start` applies the explicit resume arguments.
A failure before a pane or tab exists retains the same closed item for retry; once a tab exists, failed agent starts degrade to a shell or fresh session and publish a pane-local notice rather than offering a partial pane retry.
File reads use the filesystem worker, and missing, unreadable, already-open, and successfully reopened files each have an explicit result.
A file close carries its matching debounced save in the same core event, writes that exact path and contents outside the runtime mutex, and removes the tab only after the current draft saves successfully.
No reopen notice uses the shell's modal interaction alert.

## The Herdr wire boundary

The bundled Herdr release is pinned in one place, `macos/Sources/HerdrMacOS/Resources/herdr-bundle.json`, and `contracts/herdr-api.schema.json` is derived from it: it is what that exact binary answers to `api schema --json`, never a copy from a Herdr checkout.
`hide-herdr-client/build.rs` turns the five sub-schemas into Rust modules under `hide_herdr_client::wire` at build time; generated source stays in `OUT_DIR` and is never committed.
`herdr-core/src/wire.rs` is the only core boundary that converts generated values into the core's projection and event inputs; shared request and subscription encoding lives in `hide-herdr-client`.
Do not write new wire deserialization structs in `session_sync/{projection,replica}.rs` or import generated types into domain, runtime or sidebar code.
The event envelope is consumed as generated (`event` and `data`); the stream carries no protocol, host or sequence, and the snapshot names no host, so a remote snapshot's identity is the host Hide reached the socket through, stamped by the caller of `wire::remote_snapshot`.
Request envelopes still name their method explicitly because generation does not discriminate method constants; use generated parameter types inside them.
The envelope `id` is request correlation, never retry identity; mutation convergence must use an operation context on a method whose pinned schema actually carries one, or reconcile the resulting topology before retrying a method that does not.
`live.rs` and `remote.rs` also use this boundary for response decoding and generated request parameters.
The boundary preserves remote protocol diagnostics before decoding the complete generated snapshot, and the isolated pinned-server probe checks the control responses.
Terminal input, scroll, resize and release messages and the parameterless snapshot request remain boundary-owned schema gaps, with tests that require migration when their parameter types appear.


## The bundled Herdr runtime

The app runs the Herdr it bundles: `HerdrRuntimeResolver` verifies the bundled binary against the manifest digest and asks that binary for `status server --json` before starting it on the selected socket.
A socket filesystem node is never health evidence because a crashed Unix listener can leave one behind.
At launch and after a connected server stops answering, the shell runs one bounded five-attempt backoff that converges on either the responding server or one bundled server start; blocking probes run off the main actor.
A server that is already running is joined as it is when its protocol matches.
When it does not match, the core projects the required protocol and the running server's protocol and version as typed status.
The shell blocks every local Herdr mutation before dispatch.
When the running Herdr is older, the native alert points to the safe restart guide; when Hide is older, it points to Hide Releases; when the comparison is unknown, it offers only copyable safe diagnostics and dismissal.
Hide never stops or replaces a responding server automatically because doing so could interrupt panes owned by another client.
There is no installed-CLI candidate list and no version floor; the pin is exact.
The Swift shell reads the manifest at launch, and `scripts/fetch-herdr-runtime.sh` downloads and verifies the asset against it for both `scripts/build-app.sh` and `macos/scripts/build_dev_app.sh`; `scripts/check-herdr-pin-single-source.sh` fails when any of those restates the value.
Move the pin with `scripts/bump-herdr.sh <release-tag>` (a stable `v0.8.3` or a `preview-...` tag), which verifies the asset, writes the contract that binary reports, and rewrites the tag, version and digest tokens in the README, install guide and third-party notice.
`.github/workflows/herdr-update.yml` polls for a new stable release weekly and opens a PR with that bump after running both test suites; it never merges, because the core's Herdr behavior assumptions are only asserted against fixtures this repository wrote.

## hided and the WebSocket boundary

`hided` is the product daemon that owns one `herdr-core::Core` on its creating thread.
Axum workers send dispatch and snapshot commands over a channel; they never call create/dispatch/snapshot/on_change/destroy from another thread.
It binds `127.0.0.1` only.
The WebSocket handshake is the client's first JSON frame `{token, schema_version, have_revision?, have_terminal_sequence?}`.
A mismatched token, Origin, or schema version, or a ninth concurrent client, is closed with a reason code (`invalid_token`, `origin_not_allowed`, `schema_mismatch`, `client_limit`) and a diagnostic log line.
After a valid handshake the daemon reads from the cursors the client sent, using the same `have_revision` / `have_terminal_sequence` as `herdr_core_snapshot`, and then one delta frame per core notification burst.
A client with cursor 0 gets a self-contained `snapshot`; a reconnecting client resumes with a `delta` that carries only what changed while it was away.
When the core cannot serve the cursor, because it dropped terminal chunks the client never saw or the client's revision is ahead of the core's after a daemon restart, the daemon re-reads from zero and sends a `snapshot` (`server::classify_frame` owns that rule).
A frame the daemon cannot produce at all (the core owner thread gone, an empty read, bytes that do not decode) ends that client's loop, which the browser sees as a bare close and answers by reconnecting; the daemon logs it as `ws.snapshot_failed` with the stage, the only trace of why.
The web shell counts snapshots (`viewGeneration`) and re-requests its terminal view on each one, so a resync redraws the pane instead of trusting what it had drawn.
The token comparison is constant in the token's length (`subtle`), so a refusal does not leak how much of the token a caller guessed.
Client frames are core events (`schema_version`, `kind`, `payload`).
HTTP is static assets and `GET /health` (`pid`, `version`, `schema_version`, `clients`).
No HTTP request dispatches a core event.
`hide` owns lifecycle: instance lock, `~/.local/state/hide/hided.json` mode 0600, default-browser open with `#token=`, idle exit ten minutes after the last client, and `hide serve --keep-alive`.
It does not start a Herdr server.
A release `hided` carries `web/dist` inside the binary (`hided/build.rs`); a debug build reads the directory from disk, so `pnpm build` shows up without a cargo rebuild.
The core spawns `herdr terminal session control` for every pane attach and refuses without a binary, so `hided` resolves one from `HERDR_BIN_PATH`, the variable Herdr sets in every pane it manages, then from the first `herdr` on PATH; with neither it logs `herdr_bin.missing` and no pane terminal can attach.
Swift coexistence (PRD B5) is decided per socket, not per process: `hided/src/coexist.rs` finds the `HerdrMacOS` process, reads its environment from the kernel, applies the shell's own socket rule (`HERDR_SOCKET_PATH`, else `$HOME/.config/herdr/herdr.sock`), and only an attach to that same socket prompts, or refuses without a TTY.
An isolated socket never prompts, because the shell on the operator's socket next to a daemon on a private one is the ordinary e2e and measurement arrangement.
The web shell holds no UI authority: it draws the snapshot, writes terminal chunks straight into xterm.js, and sends one event per operator action.
With `probe=1` in the page URL it also installs `window.__hideProbe`, the only way to read the WebGL-drawn terminal from Playwright or a CDP driver; without the query the writer path is the plain `term.write`.

### Panes, tabs and the attach window in the web shell

The center draws the visible tab's `pane_layouts` entry as nested CSS grids (`web/src/PaneGrid.tsx`): a split is a two-track grid sized by Herdr's ratio and every leaf is a pane with its own xterm instance (`web/src/terminals.ts`, keyed by pane id).
An instance lives as long as the core streams its pane (PRD S2 D-05, amendment 1): leaving a tab parks its terminals in a hidden lot in the document, still fed by the chunks the core keeps sending for every attached pane, and coming back re-parents them into the pane hosts and fits them, so the last frame is on screen in the same tick and only a size change goes out (`terminal_viewport` with `new_view: false`, `terminal_resize`).
A chunk for a pane that has never been shown is dropped; its first show requests a full frame with `new_view`, as does a parked pane's next show after a self-contained snapshot.
`retainTerminals` disposes an instance only when the core reports the pane `released` or stops listing it; the attach rule itself is unchanged: the core attaches the visible tab's panes on its own tick, keeps the last five shown tabs attached, and reports `released` for the rest, which the pane header draws as a caption whose click sends `reconnect_pane`.
The store keeps the `rest` section structurally shared across frames (`web/src/share.ts`): the core resends the whole section whenever any part changes, so an untouched workspace, tab or pane row keeps its object reference and its memoized row does not re-render.
A divider drag moves a guide line and sends one `resize_pane` on release, computed as the Swift `PaneResizeDragPolicy` does (the first subtree's last pane, the travel over the split's span); a change outside the core's `0.001..=0.5` sends nothing.
A zoomed tab draws only the zoomed pane, over the whole canvas, so its fit and `terminal_resize` follow the full geometry Herdr gave the PTY; the other panes' terminals stay parked and fed.
The wheel is Herdr's (PRD S2 B19): the instance has no local scrollback, a wheel event over a pane becomes whole rows by the Swift `PaneScrollPolicy` (`web/src/wheel.ts`: trackpad pixels accumulate with their remainder, a wheel notch moves at least one row), the rows of one animation frame go out as one `terminal_scroll` with the cell under the pointer and the crossterm modifier bitset, and the core answers with the viewport frame; ⌥ + wheel is left to the browser.
The wheel has one owner: the capture listener stops the event before xterm sees it, because xterm turns a wheel over a buffer without scrollback (ours, and any alternate-screen program) into cursor-key bytes on the PTY and answers a mouse-tracking program with its own wheel report, both of which Herdr already decides from `terminal_scroll`.
Keyboard focus has one reporter: a pane's textarea gaining focus under the pointer is the operator moving focus and goes out as `focus_pane`, while the focus the shell itself moves to follow the snapshot's focused pane (`focusTerminal`) is never reported back, because that echo, arriving at the core while Herdr's confirmation of the previous move was still in flight, kept two panes trading focus on a slow runner.
A pointer gesture follows the Swift `TerminalPointerRoutingState` (PRD S2 B20): a drag selects locally in xterm, and a single primary click that never left its cell is replayed on release as one `terminal_click` with the pressed cell and the crossterm modifiers, after the `focus_pane` the press already sent; ⌥ + press and a multi-click stay local and send nothing.
The cell is read against xterm's own screen element, which is exactly `cols` by `rows` cells; the pane host around it is larger by the fit's remainder, and a cell derived from the host drifts by up to one row and one column toward the far edge, which is where a full-screen program's one-row hint sits (the first real-session clicks on Claude Code's "Jump to bottom" missed for that reason).
The core decides what the program hears (`events.rs`: the detected Claude agent gets an SGR press and release, anything else nothing), and xterm's own mouse reports never fire because Herdr's frames carry no mouse mode: a `terminal.frame` is a `CSI row;1H` redraw of every row with only synchronized-output and cursor-visibility modes, so xterm's mouse tracking is never switched on.
The same frames pad every row to the full width with written spaces and never soft-wrap, so a copy of the selection is assembled in `web/src/selection.ts` rather than by xterm: trailing whitespace is trimmed from every row, a row whose last cell is non-blank continues onto the next row with no break, and a real line end stays `\n`, and the leading whitespace every non-blank line shares is removed as the program's margin while the indentation between lines is kept (a single line loses its leading spaces entirely, and a drag that starts past column 0 leaves its cut first line out of the margin); a hard line that exactly fills the width is joined too, which a continuation flag in the frame record would close.
An Escape the shell answers (a cycle, a close confirmation, the sheet, the find bar) is stopped at the window capture listener, because the pane's textarea keeps keyboard focus under the sheet and xterm would send the same press to the program as an ESC byte.
Closing mirrors the Swift flow in `web/src/close.ts`: an unknown activity status asks for `refresh_status` first, a working pane asks once, an idle pane closes with `confirmed: false`.

### The `$HOME` filesystem boundary

`hided/src/boundary.rs` is the one place the boundary is enforced, in the dispatch path before an event reaches the core (`server::apply_boundary`).
A `create_workspace`, or a `remote_file_list` for the `local` target, whose path does not resolve under `$HOME` (a `..` segment, a symlink whose target leaves home, a file, a missing directory, a relative path) is answered to that client as a `path_refused` frame with a reason code (`outside_home`, `home_root`, `not_found`, `not_a_directory`, `invalid_path`), logged as `path.refused` with the path capped at `LOGGED_PATH_CAP`, and never forwarded.
The filesystem is only ever asked about paths under home: a path written outside home gets `outside_home` before anything is read, and a path under home is resolved one component at a time from home, where a symlink's target is tested as written the same way before it is followed (`SYMLINK_HOPS` bounds the chain).
So the reason a client reads never says whether a path outside home exists, not even through a symlink it planted under home: an escaping symlink answers `outside_home` whether its target exists, is a file, or is missing, and `not_found`, `not_a_directory` and `invalid_path` only ever describe a path under home.
An accepted workspace path is forwarded as the canonical path that was checked.
A `remote_file_list` for any other target names a path on that remote machine, which the boundary knows nothing about, and is forwarded untouched; the core's own checks on it are unchanged.
A `remote_file_list` for the `local` target is answered by hided itself as a `directory_list` frame, because the core's event lists a registered remote target's checkout and has no local listing; the listing carries subdirectories only, hides dotted names and symlinks that leave home, and is capped at `LIST_CAP` with `truncated` set.
A `file_list` is the Explorer's line for one folder inside a checkout the user already registered: the path has to resolve under that root (a folder that leaves it, or a root that is not registered, is answered `outside_checkout`) and the listing carries files as well as directories, keeps dotted names, and drops `.git` alone.
Those rows follow the Swift order - directories first, then a case-insensitive name comparison in which a run of digits compares by value - and are capped at `LIST_CAP` with `truncated` set.
The two lines stay separate because their policies differ, and the `directory_list` frame names the event that asked in `kind`, so a client routes one answer to the registration flow and the other to the folder it is showing.
A request of `~` lists the home directory and answers with its real path, which is how the web shell learns `$HOME` for the checks it can make before sending anything (outside home by prefix, already registered, not in the listing it holds).
The boundary root is read from `HOME` at boot and is not configurable; an allowed-roots setting is an S5 candidate.
Both frames and the reason codes are in `contracts/hided-ws.schema.json`.

### The checkout-root boundary and the Explorer frames

`hided` widens the file line from `$HOME` to the registered checkout roots the core snapshot carries (PRD S3 D-01, B11).
Every Explorer path - listing, open, reveal, save, create, rename, move, trash, index and file bytes - is checked against the root the event names, or the focused checkout's root, on the real path.
A path under a root is walked one component at a time from that root, so a symlink whose target leaves the checkout is refused as `outside_checkout` before its target is read, and a sibling whose name merely starts with the root's path is not inside it.
Roots come from `rest.navigator.workspaces` (a local workspace whose checkout exists) and are replaced wholesale whenever that section changes: the boot seed reads the whole snapshot, and the notify path keeps a revision and a terminal-sequence cursor so a delta that changed only terminal chunks carries no `rest` and never reaches the boundary (`spawn_root_refresh`, `carries_roots`).
`apply_boundary` answers a refused path with a `path_refused` frame and a `path.refused` log line, and forwards an accepted one as the canonical path that was checked.
`file_list` answers the Explorer's listing for one folder under a root: files and directories, hidden names included, `.git` dropped, directories first then the Swift natural order, capped at `LIST_CAP`.
`file_bytes` streams a file's bytes behind the same boundary: the client names a path, an offset and a length, and the daemon answers with one or more binary frames (a 4-byte big-endian header length, the header JSON, then the bytes) of at most 4 MiB, refusing a read past `MAX_FILE_BYTES` (256 MiB) with a `file_bytes_error` frame; the image, PDF and video viewers read through it and build a blob, and no HTTP endpoint serves file bytes (S1 D-03).
The bare browser cannot establish that the daemon is on the viewer's own machine from a loopback URL, because an SSH tunnel can also present `localhost`; its oversized-file action therefore downloads through `file_bytes` even at a loopback URL.
The download asks for 4 MiB ranges and writes each into a browser-selected file when a streaming file writer is available; it does not accumulate an unbounded download in the page.
This S3 stage assumes a browser with a file-save picker for downloads above 256 MiB.
Without that browser API, the existing Blob download remains available for one protocol read of at most 256 MiB and reports a larger read's refusal beside the button; there is no separate unbounded fallback.
`open_external` remains available to a trusted local client that can establish machine locality: the path passes the same boundary check and must be a regular file, and the daemon runs the host's opener (macOS `open`, Windows `ShellExecuteW` default-file association, Linux `xdg-open`, or a validated absolute Unix `HIDE_OPEN_COMMAND` CLI helper) and answers `open_external_result`.
The Windows association preserves the default-app action without passing a checkout filename through `cmd`.
At most four opener requests may run at once and at most twelve launches are accepted per minute; crossing a cap answers `over_budget`.
On Unix, a private `hided` supervisor owns an explicit `HIDE_OPEN_COMMAND` CLI helper and all processes that remain in its process group; normal stop, timeout, or daemon death including `SIGKILL` ends that group.
The override must not detach into another session or process group, which this ownership boundary cannot supervise.
The normal `open` or `xdg-open` utility is started directly as an OS default-application handoff, since `xdg-open` can stay attached to the application for its lifetime; the daemon does not signal that process or its descendants after acceptance.
Tokio's process driver attempts to reap a short-lived default utility after the handle is dropped; it does not own the registered application's lifetime.
An opener request is accepted when its utility starts, not when the application confirms it opened the file.
The WebSocket handles the bounded launch handshake on a blocking worker, so terminal input and snapshots can continue on the same connection.
Only the latest opener attempt on a connection emits its result; an older asynchronous completion cannot replace the newest result in the web shell's one-result state.
Windows `ShellExecuteW` returns after handing the document to its registered application without a CLI child; `HIDE_OPEN_COMMAND` is rejected at boot on Windows because that override has no equivalent owner-death supervision.
The frame is a page's request rather than the operator's own click, so the shell's rule holds here too and a file the handler would run or install - an application bundle, an installer, a terminal session or script, a shell or interpreter script, a locator that hands its target to another program, anything with an execute bit, anything whose own header is a program - is refused with `not_openable` instead of launched; a viewer whose daemon is on another machine downloads the bytes through `file_bytes` instead.
`file_index` is the ⌘P index (`hided/src/index.rs`): one lazy walk per root honoring `.gitignore` without a git process, capped at 50,000 files, ranked by the Swift fuzzy score and returned as at most 80 rows; the first query for a root answers `indexing: true` while the walk runs.
`directory_changed` is the watch (`hided/src/watch.rs`): one owner task watches the focused checkout's root and its most recently expanded folders, at most 64 in total, releasing the least recently expanded first, and announces a change once the burst goes quiet (200 ms).
The web's listing cache uses the same 64 (`web/src/watch.ts`) and draws a refresh badge on a folder past the cap instead of a live listing.
Terminal attachments (`hided/src/attachments.rs`) are the bytes-in path: the browser reads a dropped file's or pasted image's bytes but cannot name its path, so it stages one file per upload in binary frames and commits a batch, and hided writes the staged files and sends the one `terminal_attachment` the Swift shell sends, with the staged paths as `paths`.
Both the id and the name are flattened to one path component, one id and one path identify one stage, a stage whose bytes do not match its declared size is dropped at eof, every arriving byte is charged to a 256 MiB staged budget with a 60-second grace on a fresh commit, hided's own attachments directory starts empty each run, staged files and their directories stay private (0600/0700), a stage commits only into the root its mode opened, a completed stage leaves the open-upload cap and a connection's uncommitted stages are released when it goes, and the shell's own attachment events are the daemon's to send: a client that sends `terminal_attachment`, `terminal_attachment_ready` or `terminal_attachment_action` is answered with an error frame and it never reaches the core, because those events name arbitrary paths.
A clipboard image stages at the exact path the core reads it from, and hided reports `terminal_attachment_ready` itself.
The caps are 20 MiB per file and 40 MiB and 8 files per batch, checked here and again in the core; a refusal is one line over the pane (B15).
Every frame and reason code lives in `contracts/hided-ws.schema.json`.

### The Explorer, editor and viewers

The Explorer (`web/src/ExplorerTree.tsx`) is the right panel's section, drawn where the core's `right_panel_visible` and `right_panel_section: "explorer"` say (`web/src/RightPanel.tsx`, D-13): the core owns which folders are expanded (`ui_state.expanded_paths`) and hided answers one listing per folder, so a row is a pure function of those, the checkout's changed-file set and the file's icon (`web/src/explorer.ts`, unit-tested without a browser).
`@tanstack/react-virtual` lays out a 10,000-row folder; nothing in a row runs git or reads the disk (B16).
The core computes a checkout's changed files only while its Explorer or Changes surface is visible, so showing the panel's Explorer sends that ui state (⌘⇧B, or opening a file, which reveals its row) or the rows carry no Git colour; the panel's other sections arrive with S4 and S5.
A single click opens the checkout's preview tab, a double click or ⌘⇧K promotes it, and the tab strip draws file and diff entries beside Herdr tabs, in italics while preview.
An opened document is revealed by merging its ancestors into `ui_state.expanded_paths` (`actions.revealAncestors`), not by the core's `reveal_path`, which promotes the tab and would contradict the preview the palette's Enter promises (B3, B12); the core's `selected_path` then highlights the row.
The editor is CodeMirror 6 (`web/src/editor/`): language packs load per document kind, an edit saves itself after 600 ms of idle and ⌘S saves at once (D-10), a disk change becomes the core's conflict choice, and Markdown Live hides the same markup the Swift view hides (`markdownLive.ts` is a pure plan) while a leading YAML frontmatter block is drawn in its own eight-line pane that scrolls inside itself (`frontmatter.ts` finds the block; the editor then holds one document in two views, and the split and merge never enter either history, D-11).
Image, PDF and video viewers read hided's bytes; the web decides video from the extension because the core has no Video kind (D-09).
Unsaved buffers live in IndexedDB (`web/src/buffers.ts`), keyed by checkout root and real path: an open document's buffer is restored on reconnect, a closed one is discarded with a diagnostic, a rename or move carries the buffer to the new path, and one nobody claimed for 14 days goes on the next start; a buffer that cannot be stored leaves editing alone and marks its tab "kept in this tab only" (B8, D-14).
The v3 database upgrade keeps shipped v1 path-keyed drafts until a live core tab supplies their checkout root, and copies completion-build v2 root-keyed drafts into the current store in one upgrade transaction.
Edits are coalesced to one active and one pending committed write per document with a bounded total queue; a move retires the old identity in the same transaction that claims the new one.

### The shortcut registry

`web/src/shortcuts.ts` is one table, command to chord per host, matched on `KeyboardEvent.code` at the window capture phase ahead of xterm and Chrome's defaults and never during IME composition (`web/src/keyboard.ts`).
The `⌘/` sheet is generated from the table.
The Electron column is empty until that host exists (TODO: fill it from `ShellMenuCommand.swift` and `PaneShortcutSettings.swift` when the Electron host lands).

| Command | Swift | Browser | Electron |
| --- | --- | --- | --- |
| New tab | ⌘T | ⌥T (moved: Chrome reserves ⌘T) | TODO |
| Close tab | ⌘W | ⌥W (moved) | TODO |
| Reopen closed tab | ⌘⇧T | ⌥⇧T (moved) | TODO |
| New workspace | ⌘⇧N | ⌥⇧N (moved) | TODO |
| Next / previous recent tab | ⌃Tab / ⌃⇧Tab | ⌥` / ⌥⇧` (moved) | TODO |
| Next / previous recent project | ⌥Tab / ⌥⇧Tab | ⌥Tab / ⌥⇧Tab | TODO |
| Search, Open file, Toggle right panel | ⌘K, ⌘P, ⌘⇧B | same chords; ⌘K and ⌘P answered by the palettes | TODO |
| Project home | ⌘⇧H | same chord, answered "준비 중" | TODO |
| Save file | ⌘S | ⌘S | TODO |
| Toggle left sidebar, Toggle sidebar view, Toggle right panel, Find in pane, Keep open | ⌘B, ⌘E, ⌘⇧B, ⌘F, ⌘⇧K | same chords | TODO |
| Split right / down | ⌘D / ⌘⇧D | ⌘D / ⌘⇧D | TODO |
| Zoom pane | ⌘⌥↩ | ⌘⌥↩ | TODO |
| Close pane | ⌘⇧W | ⌥⇧W (moved: Chrome reserves ⌘⇧W) | TODO |
| Larger / smaller / reset text | ⌘= / ⌘- / ⌘0 | same chords | TODO |
| Move to Trash | ⌘⌫ (Explorer tree only) | not intercepted; a terminal gets ^U | TODO |
| Keyboard shortcuts | - | ⌘/ | TODO |
