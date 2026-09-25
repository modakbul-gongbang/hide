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
The Changes reader is also the only Git-status owner for Explorer decorations: it reads the checkout in front, on this machine or a device, while Explorer or Changes is visible or a diff tab needs it, and publishes one root-scoped changed-file set for every surface.
The read is `Call::Changes` to that checkout's host (`hide-host/src/git.rs`), which runs Git with its working directory at the opened root, normalizes rename and conflict state, and answers only the registered folder's paths; the reader (`changes.rs`) runs it on a `BackgroundRead` thread every two seconds or on a changed request, driven by its own pump (`ChangesPump`) that the core starts once rather than by a Herdr session's coordinator, so a device's History answers even when this machine runs no Herdr; a device helper that failed is not asked again by the refresh.
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
`agent_start_in_checkout` creates a tab in a Herdr workspace used exclusively by that project through the task-operation slot that `create_worktree` already uses.
The worker that created the pane then starts the chosen provider in that exact pane with `agent.start` (`worktree_control::start_task_agent`), after the creation is published, so both shells share one executor and neither starts an agent itself.
The answer lands on the task receipt's own axis (`agent_phase`: `starting`, `started`, `failed` or `unknown`, with `agent_message`): a failure keeps the worktree and the pane, a provider missing from the daemon's PATH fails before Herdr is asked, a transport failure is `unknown` because the agent may be running, and only a definite failure is offered again (`task_agent_retry`, which reuses the recorded pane and refuses when it is gone).
The receipt cannot be acknowledged while the agent is still `starting`.
Worktree deletion has the same single executor: `remove_worktree` closes the checkout's panes on the close worker, which then asks the checkout's host to recheck Git's registration, HEAD, branch, protected base, nested worktrees and dirt against the confirmed request, look through its ignored folders for another Git repository (refusing when one is found, a folder cannot be read, or 2,000,000 entries or 30 seconds run out, since removal deletes ignored content), and run non-force `git worktree remove` and, when chosen, `git branch -d` (`Call::WorktreeRemove`, `hide_host::worktrees::remove_confirmed`); the phases are `closing`, `removing`, `finished` and `failed`.
No shell reports a removal's completion, so there is no event a client could send to claim one.
Only a pane outside the set Herdr confirmed closed counts as one that appeared during the confirmation, because the navigator still lists the closed pane until Herdr's close events are applied.
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
A device's checkout strip follows the same rule (`place_checkout_strip`, reached through `place_device_strips`): its file tabs keep the slot they were dropped in, and a drag that changes its Herdr order sends `tab.move` to that device's own Herdr with the id that Herdr knows, so the strip moves only when the device reports the order; a device that is not connected takes no Herdr move.

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
Every item names its device: a file tab closed on a device is that device's, a pane or tab close is this machine's.
Reopen restores the newest item of the device in front and `recent_closed` counts only that device's items, so a reopen on one device never restores work on another and never takes another device's newer close off the stack; a device's file reopens through that device's host.
A device's Herdr pane and tab closes are not recorded, so a device has nothing of that kind to reopen (PRD S5.5 D-21 exception).
Recording them needs the capture and marker-adoption protocol below run against the device's own Herdr and its agents' resume on that machine; that stays open until the device control path carries the layout export and the `HIDE_REOPEN_INTENT` adoption, and until then a closed device pane is reopened by making a new one there.
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
The first frame after a valid handshake is `daemon` (`version`, `pid`, `schema_version`, `host_name`, the state paths, the Herdr binary and socket, the idle policy); Settings > General reads it, and it never carries the token.
The daemon owns the core's one Settings observation flag (`ai_settings.observing`, which runs the provider probe and the hook diagnosis): a client's `observing` is only that connection's demand (`hided/src/demand.rs`), the flag follows the first observer in and the last one out, and a connection that closes releases its demand, so a closed tab never leaves the probe running.
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

### Settings and workspace management in the web shell

Settings (`web/src/SettingsSheet.tsx`, rules in `web/src/settings.ts`) opens from the sidebar's gear or ⌥, and carries the native sheet's sections less Pet: General reads the `daemon` frame and `status.{herdr,environment,diagnostics}`, Appearance writes `accent_hex` and `font_size` through `ui_state_update`, Agents reads `status.{background_ai,agent_hooks}` and sends `ai_settings` and `install_agent_hooks`, Devices sends `register_device` (with the helper consent the add form asks for and an optional device Herdr socket), `device_host_consent`, `device_host_retry`, `test_device`, `retry_connect`, `focus_device` and `remove_device` (the sidebar's device switcher sends `focus_device` too), and Shortcuts writes `browser_shortcut_bindings`.
Every row shows what the snapshot says; an edit is pending until the snapshot carries it, and a refusal is matched to the edit by the core's `last_error` kind and time.
Every page names the daemon's machine (`host_name`) as the owner of what it stores - appearance, shortcuts, Background AI, hooks and the device list - and says, while a device is selected, that selecting it changes where files, Git and panes run and not where those values live (PRD S5.5 B35); the Agents page then says that hooks and AI settings for the device's agents are set up by Hide on that machine and are never copied or installed over SSH (B37).
A device row shows only what that device reported (its Herdr version, its helper's platform, and the staged connection test's SSH, auth, Herdr, protocol, PTY, SFTP and Git results), and a refused connection names its step (`problem`: `host_key_changed`, `host_key_unknown` or `authentication`, read off the core's own known_hosts and sign-in words by `remote::connection_problem`) with the action it needs; Hide never edits known_hosts or asks for a password (B36, B38).
A sign-in refusal is the auth stage's failure alone in the test, since the handshake and known_hosts check had passed.
Copy diagnostics carries versions, paths, states and the core's recent diagnostics, redacts anything token-shaped, and has no terminal output or pane input among its inputs.
`retry_connect` is a new connection attempt: it retires the device's coordinator and transports and connects again from its registration, keeping the device focus, and a connected device refuses it.
`remove_device` removes Hide's own record of a device and nothing on it (PRD S5.5 B26): its registration, its project registrations and pins, its expanded folders, its open file tabs and its closed items go, while its host, Herdr server, panes, agents and folders are left as they are; a dirty tab closed this way leaves its draft in the browser, where a draft no tab stands for is offered to export or discard, and the confirmation counts the projects, tabs and drafts before anything is removed.
Before `remove_device` is sent the page writes every draft of that device's tabs it still has queued, and a draft that could not be stored (B44) holds the removal: the confirmation names it until it is saved or exported (Export releases exactly the text it downloaded, so a later edit holds the removal again, and the confirmation then says that draft leaves only as the exported file, because the page cannot confirm the download was kept), since closing its tab would lose it; once the drafts are being stored the removal can no longer be kept from the dialog.
The web's `ui_state_update` echo omits `workspace_registrations` and `device_registrations`, which their own events own, so a stale echo can never undo a registration.
The accent swatches read the `--color-accent-choice-*` tokens; the chosen value replaces `--color-accent`, and `font_size` scales the interface text tokens through `--interface-scale` while the terminal and editor sizes stay their own.

The sidebar's project and checkout rows carry a `⋯` menu, also on right-click (`web/src/RowMenu.tsx`, rules in `web/src/workspaceManage.ts`): Pin/Unpin and Remove project for a registered project on any device, New worktree for a Git project on any device, a purpose for any checkout, and deletion for a linked worktree whose gate allows it on any device; an action a row cannot use is drawn disabled with its reason.
The dialogs (`web/src/WorkspaceDialogs.tsx`) send `create_worktree`, `set_checkout_purpose` and `remove_worktree` and read only the receipt for their own request (`task_operation` by kind, id and target; `worktree_removal` by checkout and id), so another page's or an older request's answer is never shown as theirs.
A created worktree's pane is focused with one `focus_pane` once the snapshot lists it, because only a focus request is tracked until Herdr confirms it; a late focus event for the pane left behind cannot undo the move.
A dialog or sheet returns keyboard focus to a terminal through `focusTerminal` on the core's focused pane (`restoreFocus`), never by re-focusing the textarea that held it, which would be reported as the operator moving there.

The device switcher sits at the bottom of the sidebar (`web/src/DevicePicker.tsx`), the web form of the native chip and `HideDevicePicker`, and sends `focus_device`.
Selecting an SSH device makes it the context (`web/src/remote.ts`, the remote Agent area in `web/src/WorkspaceScreen.tsx`): the sidebar lists that host's Herdr workspaces and agents and the canvas draws its visible tab from `status.remote[].session`, the projection the core already builds over SSH with every id scoped to the target (`remote:<target>:pane:…`).
The web follows the host's own focus rather than keeping a selection, because the core attaches exactly the panes of that host's focused tab; a pane is placed by the rectangle Herdr reports for it, and there is no divider because a remote pane's size is its host's.
Keystrokes, scroll and viewport go out with the scoped pane id, which the core writes to that host's terminal session; focus, split, zoom, close pane, new tab, tab focus and close tab go out as `remote_control` with the device as `target_id` and a fresh `request_id`, and the core checks every id against that host's session before anything is sent, so a stale or local id is refused rather than retargeted.
The pane's own id decides where a click-to-focus goes, and a close confirmation carries the device it was asked about.
A device that is not connected takes no command: the web says so and sends nothing, keeps the last session on screen under a Retry banner while it is `stale`, and draws the connection's own state when there is no session.
The Explorer, file tabs and History run on the device's own helper; worktree creation and deletion, registration, pin and project removal run through the device's Herdr and helper (see Device catalogs); a tab drag and reopen closed act on the device (see Tab reorder ownership and Reopening locally closed work); find in pane stays this machine's and is refused with the reason; a remote checkout's purpose is written through the core when the host's Herdr is 0.9.1 or newer.
The core also refuses `create_pane`, whose target is implicit, while a remote device is focused (`pane.device_mismatch`), so a split never falls back to this machine.
Typing into a remote pane makes it the core's terminal pane, so `focus_device` back to this machine, or removing the selected device, hands the keyboard back to `ui_state.selected_pane_id` or the drawn tab's focused pane (`return_keyboard_to_local_pane`).
Registering or removing a device refreshes only the device rows (`rebuild_device_rows`); rebuilding the whole catalog there dropped every checkout's tabs until the next session publish.

### Workspaces in the web shell

The web shell has three screens (PRD S6 D-02): Main lists every registered Project on every device, a Project's Overview lists its Workspaces and agents, and a Workspace is one checkout's working space (`web/src/{MainScreen,WorkspaceScreen}.tsx`, rules in `web/src/navigation.ts`).
Which screen is showing is the shell's own location (`ui.ts`), not core state: the page starts on the Workspace the core has in front only when the core marks it `resumed`, the Workspace the operator last chose, in this process or before a restart (D-11): a `focus_checkout`, `focus_pane` or `focus_tab` the core accepted, or a device `remote_control` focus or Workspace open once the device's front lands there, so a refusal chooses nothing and a front only Herdr's own focus moved is not resumed; a first run, a last Workspace that is gone, or any other front starts on Main once the device in front has settled (`startupScreen` in `navigation.ts`), and moving between screens creates or ends nothing.
A device Workspace is known only when that device's session arrives, so the first screen waits for the device in front rather than for this machine's Herdr.
Main and Overview read only the catalog, the device sessions and the agent rows the snapshot already carries; a device that is connecting or cannot answer shows why and leaves its counts unknown rather than zero.

A Workspace's layout (Agents only, Agents and Views, Views only), its two tools, the boundary between its Agent and View regions and the arrangement of its View areas belong to the core (S6 D-10, S7 D-11), keyed by device and checkout path in `herdr-core/src/workspace_views.rs` and kept in `workspace-views.json` beside `core-state.json`, a versioned file of its own so neither the old settings nor the Swift shell's state is rewritten.
Only `hided` names that file (`CoreOptions.workspace_views_path`); without it the core keeps today's single-surface editor, which is what the Swift shell still runs, and none of the View area keys below reaches the Swift snapshot.
The shell sends one `workspace_view` event for a layout, a tool, a boundary or a reveal (`mode`, `explorer`, `changes`, `agent_share`, and `reveal`, a file of the front Workspace, checked by path components like any file read, whose folders unfold in the Explorer in the same event), and the snapshot's `workspace_view` is the front Workspace's entry; it is absent from the wire when no Workspace is in front, so the Swift snapshot keeps its keys.
The global `ui_state.right_panel_visible` and `right_panel_section` are projected from the front Workspace's tools, so the Changes reader and the device Explorer watch keep the one gate they had.
That projection is never saved: `core-state.json` keeps the right panel it held when the process started (`ui_state_to_save`), so an older build started on the same state directory opens with its own panel.
A new Workspace starts with its agents alone and the Explorer shown, since it has no View yet.

Schema 2 of that file (PRD S7) stores each Workspace's View areas as a binary split tree (`layout`, rules in `herdr-core/src/view_layout.rs`).
A split node has an `axis`, `row` for first-left and second-right or `column` for first-top and second-bottom, and a `ratio`, the first child's share, clamped to 0.15..0.85; a leaf is an area holding its displays in tab order and naming its `active` display.
A display is one tab showing one file or diff (`path`, `kind`, `committed`, `preview`, `last_focused_unix_ms`), and the Workspace names its `active_area`, the one it last used, where the next file opens.
Area, display and split ids (`a<n>`, `d<n>`, `s<n>`) are minted by the core from the Workspace's `next_id` and never reused in that Workspace, so an id in an event names one thing for the Workspace's life; a Workspace with no layout has one empty area `a1`.
The same invariants hold on load and after every change: an area has at most one preview display, its `active` names one of its displays or is null when it has none, an empty area exists only as the root, `active_area` names an existing area, and the caps hold.
The caps are core constants, published in the snapshot as `layout.limits`: 6 areas (`MAX_VIEW_AREAS`), 3 split nodes on the path from the root to any area (`MAX_SPLIT_DEPTH`) and 64 displays per Workspace (`MAX_VIEW_DISPLAYS`, S6's `MAX_VIEW_TABS`).
A split that would pass the areas or the depth is refused with `view_layout.limit` and its reason, an open that would add a 65th display is refused with `view_layout.display_limit`, and nothing changes; an open of a document that is already displayed is a focus and is never refused, and a file that carries more than the caps is trimmed on load with a diagnostic, never on an operator's action.
The display cap holds on every path: a preview open beside a preview whose document holds work (which stays and makes the open an addition), Reopen Closed, a file the Explorer creates and a revealed file are refused before anything is read or made, a read that lands after the views filled is refused as it lands (a Reopen Closed then keeps its item), and the tree's own operations refuse a 65th display, so nothing can pass it.
A schema 1 file (S6) migrates once: its tabs become the displays of one area `a1` in their saved order, its active tab becomes that area's active display, its preview flags are kept, and the next save writes schema 2.
Any other version, a file that does not parse and a migration that fails all take the S6 path: the file is renamed aside as `<name>.unreadable-<ms>`, the defaults load with a `workspace_views.unreadable` diagnostic, so nothing is resumed and the page starts on Main, and when the rename fails the store is frozen so nothing is written over the original; saves are atomic and run on a worker, coalesced, never under the runtime mutex.

A display never holds a document.
It binds at runtime to the S5.5 editor tab, the one buffer per device, checkout and document (the tab id, `editor_documents` and the save pipeline), and any number of displays may bind the same tab, so two views of a file share one text, one dirty state and one save queue rather than copies that could diverge (S7 D-03).
Closing a display that is not its document's last removes the display alone, and a `pending_save` sent with it is not needed; closing the last one runs the existing `file_close` path, save-then-close with the carried `pending_save` and the conflict protection, so a document leaves only once its buffer is safe.
The web's frame may still show another view of a document the core already holds in one, so a close of the last view of a dirty, saving or failed document that carries neither `pending_save` nor `discard: true` (the operator's Don't Save) is refused as `view_layout.unsaved`, naming the document, and nothing changes.
A document that becomes dirty, is saving or has a save state promotes every display of it in the same event, because a preview is replaced by the next click and such a document must never be.
In web mode the per-checkout preview slot of `place_editor_tab` gives way to one preview display per area: a preview open retargets the active area's preview display in place, and its old document is retired when no other display shows it, like the preview replacement before it, with no Recent Closed entry; the Swift shell keeps the per-checkout slot.

A layout change is one `view_layout` event whose `action` says which: `focus` (a display becomes its area's active display, its area the active area, and its `last_focused` is stamped), `focus_area` (keyboard focus moving between areas), `move` (a display to an index in its own area, which reorders, or in another existing area), `split` (a display into a new area at one `edge` of an area, taking half of it), `resize` (a split's ratio), `close`, `keep_open` and `retry` (an unavailable display's file read again).
Each is one operator action and one frame, and it applies to the front Workspace only; a moved or split display becomes its area's active display and that area the active one.
Every payload names its Workspace as `workspace: {device_id, path}`, copied from the `workspace_view` of the frame the operator acted on, because ids like `d2` repeat across Workspaces and the front can move between that frame and the event; an action naming any other Workspace changes nothing and leaves a `view_layout.stale_workspace` diagnostic with both Workspaces and the action, and no screen error, since the screen already shows the other Workspace.
Every action may arrive twice, so each converges: `move`, `resize` and the two focuses carry their target state, `split` carries a `request_id` whose repeat among the Workspace's last 32 (kept in memory only) changes nothing, and closing a display that is already gone is a no-op with a diagnostic.
An unknown id is refused with `view_layout.unknown_*` and its reason, and so is a split that would change nothing, a display split away from an area it holds alone.
A move or close that empties an area collapses it in the same event and its sibling takes the space; the last area stays, empty.

In web mode `file_open` places a document this way: one already displayed in the front Workspace focuses its most recently focused display, and pins it when the open is not a preview; otherwise a preview open retargets the active area's preview display, or appends one when the area has none, and an ordinary open appends a pinned display to the active area.
`file_open` with `beside: true` is Open to the side: a pinned display of the document in the area next to the active one (right, then left, then down, then up among the tree's neighbours), a new area to the right when there is only one area (within the caps), or a focus of the display already in that area; when that split is refused as the read lands, a display of the document already open is focused, and with none the open is refused with the split's reason rather than doubled up in the area it was asked from; it rides on `file_open` so the request still crosses hided's path boundary like any open.
`changes_select` places diff displays by the same rules, `file_keep_open` pins every preview display of its document, `file_focus` focuses its most recently focused display, and `file_close` closes every display of it and then the document through the existing guards.
`file_draft` and `file_conflict` carry the `tab_id` of their document, which the web always sends, because with several documents on screen the active one is no longer necessarily the one that was typed into.
Opening, restoring, renaming, closing and removing a device create and remove editor tabs by paths the layout does not see, so after every event the core reconciles the two: a display whose document tab is gone is removed, and an editor tab of a restored Workspace that no display shows gets a pinned display in the active area.
Reads land on workers, so two can land with no event between them, and an open reconciles before it places its document: otherwise a preview it replaces could still be unbound from the restored document it shows, which would then come back pinned beside the new one.
The reconcile holds the display cap too: such a tab of a Workspace that already has 64 views waits off screen, is reported once with a `view_layout.display_limit` diagnostic, and shows once a view is closed.

With the regions separate, choosing an Agent tab no longer takes the editor off screen: `editor.active_tab_id` is the document of the active area's active display, null while that display is not open (`runtime/workspace_view.rs`), where before every terminal choice deactivated it.
The core also owns when the hidden area comes back (D-08): an explicit file open, reveal or History selection from Agents only switches to Agents and Views, and an explicit agent or tab choice from Views only does the same; a status change, a hover or a menu opening moves nothing, and a refused event moves nothing either.
A device agent or tab chosen through `remote_control` brings the Agent area back on the Workspace that holds it, resolved from the device session in the same event, because the device's front moves only when its Herdr answers.
Choosing a Workspace or an agent on another device than the one in front is one event: `focus_checkout`, `focus_pane` and `remote_control` carry `focus_device`, and the core brings that device forward only when it accepts the request, so a refusal leaves the device in front where it was.

The snapshot's `workspace_view.layout` is the front Workspace's tree with its `active_area`, `limits` and `display_count`, each display carrying its `tab_id`, `label`, `kind`, `committed`, `preview` and `state` with an optional `reason`; `editor.tabs` still lists every document.
A restored Workspace's tree, ratios, tab order, preview flags, active displays, active area, mode and tools are published the first time it is in front in a process, before any file is read, so the layout appears whole rather than tab by tab.
Each display then moves through its own `state`: `waiting` while its Workspace's root cannot be read yet, with a `reason` naming the device state (`Waiting for <device> to connect` while its helper connects), then `opening` while its read is in flight, a Retry's read included, then `open` once it is bound to a document or diff tab, or `unavailable` with the read failure as its `reason`.
A device helper that cannot be used, one refused for its protocol, unsupported, on a changed identity or not allowed, gives the device's own reason, the one History and Settings show, and its displays keep `waiting` so they open once the device is fixed; Retry on a waiting display is refused with that reason.
The wait is `view_root_ready`: on this machine a read waits until hided has opened the checkout's root, and on a device until its helper is ready and its catalog has moved its checkouts into their Projects (`ingest_host_established` and `refresh_device_catalog` ask again), because a read before the helper is refused and would mark every file unavailable, and a restore before the grouping names a Workspace the device no longer shows.
A restore opens into the Workspace in front, so it runs only while that is still the Workspace the last sync recorded; a front that moved since waits for the sync rather than filling one Workspace with another's files.
The reads land in any order and each fills its own display in place, because the order lives in the stored tree rather than in the order the documents arrive; S6's recording of tabs in strip order, and the reordering once the last read landed, are gone.
A Workspace records nothing into the file until its restore has landed, so a Workspace whose files cannot be reached, or a quit before the reads land, keeps the layout it saved.
An unavailable display keeps its place with Close view and Retry, `retry` reads its file again or, for a diff, opens its diff tab again, and every other display and area is untouched; nothing is started in Herdr, no split or zoom is replayed, and a terminal layout another client changed meanwhile wins.
A diff display stored without its Changes group is repaired on load to the working group, so it binds to the diff tab its restore opens instead of standing beside a second display of it.
Unsaved text returns through the web's per-document draft reconcile in IndexedDB (S5.5), never through this file.
Removing a device forgets its Workspaces' entries, and a pending save is joined when the core is dropped, so a change made just before quitting is on disk.

Two kinds of state stay in the web and are never dispatched.
A drag's preview (the floating tab, the insertion line, the edge overlay with its label) is local, and only a valid drop sends one `view_layout` `move` or `split`; Escape, a drop outside, an ineligible or vanished target and too little room send nothing, and nothing is resized or saved during the drag.
The narrow-window arrangements (the tools as an overlay, one working region with a switch, the active View area alone with an area switcher) are decided from the window's size as it renders and never stored, so widening restores the stored layout because nothing replaced it.
The web also owns the pixel rule, because it has the geometry: an edge takes a drop, and a Split item is enabled, only when the target area's size along the split axis is at least twice `--size-workspace-area-min` (a `row` split) or twice `--size-view-area-min-height` (a `column` split) plus the divider, and the split stays within `layout.limits`; the core owns the counts and the depth and refuses whatever passes them.

A pane's delegated children and ancestors come from the pane rows the core already projects (`children.chips`, `lineage_path`; rules in `web/src/lineage.ts`), a device's panes from that device's own lineage (`sync_remote_pane_relations`, with each declared parent scoped to the device in `replica.rs`): a parent's header lists every direct child on one scrolling row, a child's header has a compact Return, and the pane menu lists parent, siblings and children, each moved to only by its explicit Open.
Each of those moves is one `focus_pane` carrying a `request_id` (on a device, `remote_control` with `report_pane_focus_outcome`), and the core's `status.pane_focus_request` is the only answer the header shows: pending until Herdr's layout confirms, or failed with the core's reason and Retry when it can be retried; a second click on the same target while one is in flight is dropped, and a failure never splits the parent or makes a pane.

### The `$HOME` filesystem boundary

`hided/src/boundary.rs` is the one place the boundary is enforced, in the dispatch path before an event reaches the core (`server::apply_boundary`).
A `create_workspace`, or a `remote_file_list` for the `local` target, whose path does not resolve under `$HOME` (a `..` segment, a symlink whose target leaves home, a file, a missing directory, a relative path) is answered to that client as a `path_refused` frame with a reason code (`outside_home`, `home_root`, `not_found`, `not_a_directory`, `invalid_path`), logged as `path.refused` with the path capped at `LOGGED_PATH_CAP`, and never forwarded.
The filesystem is only ever asked about paths under home: a path written outside home gets `outside_home` before anything is read, and a path under home is resolved one component at a time from home, where a symlink's target is tested as written the same way before it is followed (`SYMLINK_HOPS` bounds the chain).
So the reason a client reads never says whether a path outside home exists, not even through a symlink it planted under home: an escaping symlink answers `outside_home` whether its target exists, is a file, or is missing, and `not_found`, `not_a_directory` and `invalid_path` only ever describe a path under home.
An accepted workspace path is forwarded as the canonical path that was checked.
A `remote_file_list` for any other target names a path on that remote machine, which the boundary knows nothing about, and is forwarded untouched; the core lists it through that device's helper (`host_access::list_folder`), the same confined listing the web Explorer gets, and the Swift shell's remote file panel is its only sender until that shell is retired.
A device with no consent answers that panel with the `not_allowed` file state and a message naming what allowing installs and runs; the panel shows it with an Allow control that sends `device_host_consent`, and the core lists the panel's root again once the helper is ready.
A `remote_file_list` for the `local` target is answered by hided itself as a `directory_list` frame, because the core's event lists a registered remote target's checkout and has no local listing; the listing carries subdirectories only, hides dotted names and symlinks that leave home, and is capped at `LIST_CAP` with `truncated` set.
A `file_list` is the Explorer's line for one folder inside a checkout the user already registered: the path has to resolve under that root (a folder that leaves it, or a root that is not registered, is answered `outside_checkout`) and the listing carries files as well as directories, keeps dotted names, and drops `.git` alone.
Those rows follow the Swift order - directories first, then a case-insensitive name comparison in which a run of digits compares by value - and are capped at `LIST_CAP` with `truncated` set.
The two lines stay separate because their policies differ, and the `directory_list` frame names the event that asked in `kind`, so a client routes one answer to the registration flow and the other to the folder it is showing.
A request of `~` lists the home directory and answers with its real path, which is how the web shell learns `$HOME` for the checks it can make before sending anything (outside home by prefix, already registered, not in the listing it holds).
The boundary root is read from `HOME` at boot and is not configurable; widening it is a security-policy decision the web Settings does not take, and registering a folder outside home stays with the existing CLI path.
Both frames and the reason codes are in `contracts/hided-ws.schema.json`.

### The checkout-root boundary and the Explorer frames

`hided` widens the file line from `$HOME` to the registered checkout roots the core snapshot carries (PRD S3 D-01, B11).
Every Explorer path - listing, open, reveal, save, create, rename, move, trash, index and file bytes - is checked against the root the event names, or the focused checkout's root, on the real path.
A path under a root is walked one component at a time from that root, so a symlink whose target leaves the checkout is refused as `outside_checkout` before its target is read, and a sibling whose name merely starts with the root's path is not inside it.
Roots come from `rest.navigator.workspaces` (a local workspace whose checkout exists) and are replaced wholesale whenever that section changes: the boot seed reads the whole snapshot, and the notify path keeps a revision and a terminal-sequence cursor so a delta that changed only terminal chunks carries no `rest` and never reaches the boundary (`spawn_root_refresh`, `carries_roots`).
The boundary pins each registered root's physical path and file identity across repeated snapshots and refuses an Explorer request when the root's current spelling no longer resolves to that pin.
On Windows, root handles deny delete sharing so a root cannot be renamed while the core uses its capability, and directory rows are enumerated from the verified handle.
`file_list` opens relative to a verified root handle and enumerates that handle, then filters each child against the same pin, so replacing the pathname during enumeration cannot expose an outside directory's rows.
The daemon supplies opened checkout-root handles to the core's Rust API; editor reads, saves and Explorer mutations resolve under those handles when their workers perform I/O, while the Swift shell retains its existing ambient path behavior and the C ABI remains unchanged.
Trash moves the selected item through its opened parent handle into a private temporary directory outside the mutable checkout path before handing that staged path to the platform Trash API; a failed handoff restores through the retained handles or retains the recovery stage when restoration fails, and an unavailable same-volume move leaves the original in place.
`file_index` retains the checked root handle and opens one directory at a time from it, so a wide checkout does not hold every sibling directory open; ignore files are read only from nonblocking regular-file handles.
Explorer watch polling retains opened folder handles and rebinds an expanded folder when a changed parent reveals that its path now names a different directory.
`apply_boundary` answers a refused path with a `path_refused` frame and a `path.refused` log line, and forwards an accepted one as the canonical path that was checked.
`file_list` answers the Explorer's listing for one folder under a root: files and directories, hidden names included, `.git` dropped, directories first then the Swift natural order, capped at `LIST_CAP`.
`file_bytes` streams a file's bytes behind the same boundary: the client names a path, an offset and a length, and the daemon answers with one or more binary frames (a 4-byte big-endian header length, the header JSON, then the bytes) of at most 4 MiB, refusing a read past `MAX_FILE_BYTES` (256 MiB) with a `file_bytes_error` frame; the image, PDF and video viewers read through it and build a blob, and no HTTP endpoint serves file bytes (S1 D-03).
The bare browser cannot establish that the daemon is on the viewer's own machine from a loopback URL, because an SSH tunnel can also present `localhost`; its oversized-file action therefore downloads through `file_bytes` even at a loopback URL.
The download asks for 4 MiB ranges and writes each into a browser-selected file when a streaming file writer is available; it does not accumulate an unbounded download in the page.
This S3 stage assumes a browser with a file-save picker for downloads above 256 MiB.
Without that browser API, the existing Blob download remains available for one protocol read of at most 256 MiB and reports a larger read's refusal beside the button; there is no separate unbounded fallback.
The browser WebSocket explicitly refuses `open_external` with `untrusted_client`, even when the page URL is loopback and the socket has its normal token, so an SSH-forwarded browser cannot start a program on the daemon host.
The bounded host opener remains implemented for a future transport that can establish local ownership: macOS `open`, Windows `ShellExecuteW` default-file association, Linux `xdg-open`, or a validated absolute Unix `HIDE_OPEN_COMMAND` CLI helper.
No such trusted transport is present in S3.
For `file_bytes`, hided verifies the path of the opened file handle against the registered checkout root and streams from that same handle, so a symlink swap between path validation and open cannot redirect the read outside the checkout.
On Unix, the byte source is opened with nonblocking flags before its handle is checked as a regular file, so a FIFO cannot stall the WebSocket while waiting for a writer.
The Windows association preserves the default-app action without passing a checkout filename through `cmd`.
At most four opener requests may run at once and at most twelve launches are accepted per minute; crossing a cap answers `over_budget`.
On Unix, a private `hided` supervisor owns an explicit `HIDE_OPEN_COMMAND` CLI helper and all processes that remain in its process group; normal stop, timeout, or daemon death including `SIGKILL` ends that group.
The override must not detach into another session or process group, which this ownership boundary cannot supervise.
The normal `open` or `xdg-open` utility is started directly as an OS default-application handoff, since `xdg-open` can stay attached to the application for its lifetime; the daemon does not signal that process or its descendants after acceptance.
Tokio's process driver attempts to reap a short-lived default utility after the handle is dropped; it does not own the registered application's lifetime.
An opener request is accepted when its utility starts, not when the application confirms it opened the file.
Windows `ShellExecuteW` returns after handing the document to its registered application without a CLI child; `HIDE_OPEN_COMMAND` is rejected at boot on Windows because that override has no equivalent owner-death supervision.
The retained host opener refuses files the handler would run or install - application bundles, installers, terminal sessions, scripts, locators, executable files, and files whose header identifies a program - with `not_openable` instead of launching them.
The bare browser downloads oversized files through `file_bytes` wherever the daemon runs.
`file_index` is the ⌘P index (`hided/src/index.rs`): one lazy walk per root of each device honoring `.gitignore` without a git process (`hide_host::index::walk`), capped at 50,000 files and 50,000 folders, ranked by the Swift fuzzy score and returned as at most 80 rows; the first query for a root answers `indexing: true` while the walk runs.
Search by file contents does not exist in the web shell on either machine: ⌘P matches names, and the device reaches the same palette with the same caps (PRD S5.5 B5, D-21 exception).
The reason is that content search was never part of the web shell this parity is measured against, and adding it means a new capped `hide_host` search over the walk, its cancellation and a result view, which is a product addition rather than parity.
The operator's result is that a name search works the same on either machine, and a search for text inside files is run in a terminal pane (`rg`) on that machine.
It is re-reviewed when content search is added to the web shell for this machine; the device then takes the same `hide_host` search through its helper.
A device's root is walked by its helper (`Call::Index`) on the index worker, its rows are that device's paths, the answer names the device so the palette never draws another device's list, and a walk the helper could not make is answered as `unavailable` once and walked again on the next query.
`directory_changed` is the watch (`hided/src/watch.rs`): one owner task polls the opened handles for the focused checkout's root and its most recently expanded folders, at most 64 in total, releasing the least recently expanded first, and announces a change once the burst goes quiet (200 ms).
The web's listing cache uses the same 64 (`web/src/watch.ts`) and draws a refresh badge on a folder past the cap instead of a live listing.
A device's folders are its helper's to read, so a device Explorer is watched by stamping (`hided/src/device_watch.rs`, `Call::Stamps`, `hide_host::list::stamps`): while the selected device's Explorer is showing and its helper is ready, one request every two seconds asks for a stamp of its front checkout's root and watched folders (the same 64, by the same rule), each folder opened from the pinned root and stamped by its device, inode and modification time, and a folder whose stamp moved is announced as `directory_changed` naming the device.
A new target is stamped at once, and a folder that joins the watch set in the same checkout is announced once on its first stamp, because the page listed it when it was expanded, possibly before that stamp; hided admits a device frame only for a folder under a root the catalog carries for that device.
The watch asks nothing for a closed Explorer, another device or a helper that is not ready, so it never starts a helper connection and ends with the surface; the next poll waits for the last answer, and a failed poll is logged once as `device_watch.failed` and asked again at the next interval.
Every `directory_changed` names its device (`local` for this machine's), and a page drops a frame for a device it is not showing, so the same path on two devices never re-reads the other's listing.
Terminal attachments (`hided/src/attachments.rs`) are the bytes-in path: the browser reads a dropped file's or pasted image's bytes but cannot name its path, so it stages one file per upload in binary frames and commits a batch, and hided writes the staged files and sends the one `terminal_attachment` the Swift shell sends, with the staged paths as `paths`.
Both the id and the name are flattened to one path component, one id and one path identify one stage, a stage whose bytes do not match its declared size is dropped at eof, every arriving byte is charged to a 256 MiB staged budget with a 60-second grace on a fresh commit, hided's own attachments directory starts empty each run, staged files and their directories stay private (0600/0700), a stage commits only into the root its mode opened, a completed stage leaves the open-upload cap and a connection's uncommitted stages are released when it goes, and the shell's own attachment events are the daemon's to send: a client that sends `terminal_attachment`, `terminal_attachment_ready` or `terminal_attachment_action` is answered with an error frame and it never reaches the core, because those events name arbitrary paths.
A clipboard image stages at the exact path the core reads it from, and hided reports `terminal_attachment_ready` itself.
The caps are 20 MiB per file and 40 MiB and 8 files per batch, checked here and again in the core; a refusal is one line over the pane (B15).
A `file_list`, `file_open`, `reveal_path` or `file_save` that names a `device_id` other than `local` is not a path on this machine, so it skips this boundary: the device's helper judges it against the pinned root, and hided answers that device's `file_list` itself from the helper, or with `directory_unavailable` and a code (`not_ready`, `busy`, `unknown`, `refused`) when it cannot.
A device's `file_bytes` names the checkout `root` beside the path: hided admits only a root the catalog carries for that device and asks the helper for one range of at most 4 MiB at a time (`Call::Bytes`, `hide_host::bytes::read`), sending each as the same binary frame before asking for the next, so a device read holds one range in memory as a local one does; every range names the file it came from, and a file that changes between ranges ends the read as `read_failed` rather than joining two files' bytes.
A helper's range is accepted only as the range asked for (the same offset and file size, never longer, and shorter only where the read ends), so a helper cannot stretch one read into millions of round trips or end it early as if the file were shorter.
One client runs at most two device reads at once: a third ends the oldest, which is answered `file_bytes_error` with reason `superseded` and its own request id, so whatever waited on it settles (D-15).
Every frame and reason code lives in `contracts/hided-ws.schema.json`.

### The Explorer, editor and viewers

The Explorer (`web/src/ExplorerTree.tsx`) and History (`web/src/HistoryList.tsx`) are the Workspace's two tools (`web/src/Tools.tsx`), each shown while the front Workspace's `workspace_view` says so (see Workspaces in the web shell): the core owns which folders are expanded (`ui_state.expanded_paths`) and hided answers one listing per folder, so an Explorer row is a pure function of those, the checkout's changed-file set and the file's icon (`web/src/explorer.ts`, unit-tested without a browser).
`@tanstack/react-virtual` lays out a 10,000-row folder; nothing in a row runs git or reads the disk (B16).
The core computes a checkout's changed files only while Explorer or History is visible or a diff tab is active (in the web shell, any area's active display that is a diff), and the web renders only a Changes snapshot whose root matches the front checkout's `navigator.changes_root_path`.
Each answer names the device and folder it was read for (`ChangesKey`); the core drops one that no longer matches the checkout in front, and moving to another device's checkout clears the published set in the same frame, so the same path on two devices never shows the other's changes (B22).
History keeps the core's uncommitted and branch groups separate, and a row sends one `changes_select` event with its path, group and preview intent; it starts no Git read from render, scroll or click.
A workspace registration can name a folder below the Git top level while the catalog projects its checkout at the repository root; `navigator.changes_root_path` keeps History on the registered folder for that checkout, on this machine or a device.
The host refuses a root that is not its repository's top level and a scope that is not a real folder of the checkout (a link swapped in is refused), scopes both groups and selected diffs to it, and presents a rename crossing the boundary as an addition or deletion without naming the outside path; a failed or refused read is `unavailable_reason`, never an empty list, unless a list from an earlier read of the same checkout is on screen: that list stays with the failure as `stale_reason`, and both shells' History and Explorer say it may be out of date.
Its Git commands run from the root's opened handle (`fchdir` before exec) with `--literal-pathspecs`, and the request names the root identity the channel pinned, so a checkout replaced or moved after it was opened is refused rather than redirecting Git to another repository.
Every Git command the host runs ends within its deadline: it leads its own process group and the deadline stops the group, so a hook, filter, fsmonitor or textconv it started cannot hold its output open and keep one of the helper's four workers; output still held open a moment after git ended makes the call unfinished, never an empty answer that a dirty or nested-repository check would read as clean; `git worktree remove` is judged by the registration and folder read back afterwards, so an answer cut short after Git removed the folder still reads as removed.
Each requested diff's Git stdout is captured only to the 256 KiB wire budget plus a UTF-8 boundary, then the child is ended with the existing truncation notice; a large patch is never fully buffered before the limit.
A diff is the core's bounded unified patch in the `changes` snapshot, rendered as a read-only CodeMirror 6 view with old and new number gutters; a diff tab has no editor document, autosave or draft buffer.
Several diffs can be on screen at once (PRD S7 A5): the Changes request names every visible diff display of the front Workspace, the area-active displays whose kind is diff and so at most the area cap, and one `Call::Changes` answers them all in `diffs`.
The snapshot carries one entry per display in `changes.diffs` (`path`, `committed`, `text`, `notice`, each within `MAX_DIFF_BYTES`), omitted when empty, and a diff display renders the entry for its own path and group, so two visible diffs never draw each other's patch; `changes.diff`, `selected_path` and `selected_committed` stay as they were for the Swift shell and History's selection.
The diffs are taken under the folder History reads, the front Project's registered folder, while a Workspace is keyed by its checkout, so every Project registered in one checkout shares its views; a diff display outside that folder gets no text and a `notice` naming the folder, and shows its diff when a Project that holds it is in front.
The request and its answer changed shape, so the helper protocol version moved with them (`hide-host/src/protocol.rs`): the core refuses a device helper that speaks another protocol, as for any protocol change.
A device always runs the helper this Hide carries, installed by the digest of its bytes, so only a rebuilt or reinstalled Hide clears that refusal, and its reason says so; a development `hided` carries the `hide-host-helper` beside its own executable, so a stale build there is installed and refused again on every connection.
The patch wire does not carry complete old and new documents, so `@codemirror/merge`'s two-document view would require an extra read and is not used.
A single click opens the active View area's preview display, a double click or ⌘⇧K pins it, and each area's tab bar draws its own displays, in italics while preview (see Workspaces in the web shell).
An opened document is revealed by merging its ancestors into `ui_state.expanded_paths` (`actions.revealAncestors`), not by the core's `reveal_path`, which promotes the tab and would contradict the preview the palette's Enter promises (B3, B12); the core's `selected_path` then highlights the row.
The editor is CodeMirror 6 (`web/src/editor/`): language packs load per document kind, an edit saves itself after 600 ms of idle and ⌘S saves at once (D-10), a disk change becomes the core's conflict choice, and Markdown Live hides the same markup the Swift view hides (`markdownLive.ts` is a pure plan) while a leading YAML frontmatter block is drawn in its own eight-line pane that scrolls inside itself (`frontmatter.ts` finds the block; the editor then holds one document in two views, and the split and merge never enter either history, D-11).
The documents on screen travel in their own delta section, `documents`, beside `editor` (PRD S7 A4): `visible` lists the file documents shown by the front Workspace's area-active displays while its mode shows Views, so at most six, in tree order, and `changed` carries each whose own revision passed the reader's `have_revision`.
Each document keeps its own revision, so a keystroke re-sends only the document it changed, where the `editor` section used to re-send the whole active document on every change; `visible` goes out whenever the section does, a reader with cursor 0 and every full snapshot frame get every visible document, and the section is absent when nothing changed for that reader.
A snapshot read tells a document, and the `changes` section with every diff on screen, moved by an edit number (`model::Edited`) rather than by comparing contents: the value is mutable only through a call that takes a new number, and a landing Changes read replaces the section only when it differs, so per visible document a read costs a few map lookups and one number comparison, and a document is copied only when it was edited.
The web keeps the documents whose ids are in `visible`, overwrites the changed ones and drops the rest; in web mode `editor.document` is always null and `editor.active_tab_id` names the active area's document, and the Swift snapshot never carries the section.
Each display is its own CodeMirror view over its document and keeps its own scroll position and selection by display id.
Displays of one document are kept in step in the page through a per-document channel (`web/src/editor/sync.ts`), which applies the minimal change outside the undo history and marks it as a pending echo, so the other view shows a keystroke before the core's echo arrives and that echo neither moves a caret nor replaces newer input.
Drafts, autosave and close-with-save stay keyed per document (S5.5), so two views of a file write one draft and queue one save.
Image, PDF and video viewers read hided's bytes; the web decides video from the extension because the core has no Video kind (D-09).
Unsaved drafts live in IndexedDB (`web/src/buffers.ts`, store `drafts_v4`), keyed by the daemon host, the device, the checkout root and the real path (PRD S5.5 B9-B12).
The host is hided's persistent `host_id` (`hided/src/state_file.rs`, the `host-id` file in its state directory, sent in the `daemon` frame), so a draft written against one Hide host is never opened by another that serves the same browser origin.
A draft for an open document is restored on reconnect, and one whose contents the core already holds is removed only once the core reports the document clean: a dirty core holding the same text is a refused or pending save, and the stored copy is the only one a daemon restart keeps (B13-B15).
A draft no open tab stands for is a recovery item (`web/src/DraftRecovery.tsx`): a status line counts them, and the review sheet offers Open (its own checkout on its own device, after Show device or Show checkout moves there), Export as a `.draft` download, or a two-step Discard; nothing is discarded for its age or because its tab closed.
A rename or move carries the draft to the new identity; a draft that cannot be stored, including when every stored draft together would pass 512 MiB or the browser's quota is full, keeps its tab editable, marks it "kept in this tab only" and offers Export there, and while any open tab holds such a draft every other clean document opens read-only with the reason, so no new edit is made that could not be kept; saving that draft releases the hold, and no stored draft, on any device, is removed to make room (D-14, B44).
The v4 upgrade copies every older draft into `drafts_v4` as an unverified recovery item (no host or device) inside the upgrade transaction, so an interrupted upgrade keeps the old stores and runs again; only an open of this machine's own checkout at the same root and path claims one, and a device's document never does.
Edits are coalesced to one active and one pending committed write per document with a bounded total queue; a move retires the old identity in the same transaction that claims the new one.

### Device file hosts and document saves

`hide-host` is the one file authority for a checkout on any machine (PRD S5.5 D-05): opening a root and pinning its identity, listing a folder, reading a document with its content revision, and saving it.
The daemon's own machine runs it in process (`host_access::InProcessHost`); an SSH device runs the same code as `hide-host-helper serve`, which speaks one JSON line per request over an exec channel of a dedicated SSH connection (`remote/host.rs`).
Both are a `HostChannel`, so the core's document code (`files.rs`, `runtime/documents.rs`) and hided's listing (`boundary.rs`, `server.rs::device_listing`) make the same call whichever device the checkout is on, and neither keeps a second copy of the root, link or listing rules.
A helper runs only with the operator's consent for that device (D-20, D-23): `register_device` with `host_consent` or a later `device_host_consent` records it on the registration with the consent contract and the install root (`HIDE_HOST_HELPER_ROOT`, default `~/.local/share/hide/host-helper`), and the first connection binds it to the SSH user, host, port and host key that answered.
A different identity (`identity_changed`) or a build whose contract or root differs (`outdated`) starts nothing until the operator allows it again.
Revoking starts no new work: the connection admits nothing more, even from a worker that still holds it (a worktree removal between its pane closes and its host call, a facts walk), a request still waiting for a slot is refused unsent, and a save held for the helper to become ready is dropped with its draft kept; a request admitted earlier but not yet written is refused unsent too; the requests already sent answer on the old connection and settle to their real results, and the connection closes once they have (or after 250 s, more than a request's longest wait to be written and answered).
Revoking deletes no draft, remote file or installed helper.
The install root is refused unless it and every folder above it cannot be changed by another account: each is owned by the device account or by root and writable by neither its group nor others, except a root-owned sticky folder such as `/tmp`; the folders are checked as spelled while they are created and again on the root's resolved real path from `/`, home and its parents included, and the helper is then installed under and started from that resolved path, so no link on the spelled path is followed between the check and the launch (OpenSSH's rule for key files, which a device that signs in with a key already meets). A folder whose mode the device does not report is refused, and a group-writable one (a `umask 002` home on some Linux systems) is refused with the folder and the `chmod go-w` fix named.
The helper file itself is reused, or started after its upload, only as the account's own regular file that no group or other account can write, with its mode reported.
The helper is uploaded and replaced only when the device lacks this build's bytes, runs only while its connection lives, and is never started by anything but a file, Git or project-facts request; there is no resident process and nothing at login.
The daemon ships one helper build per device platform (`HelperPackages`), and the macOS app carries the same build in `Contents/Resources/host-helper`; a device whose platform has no package is `unsupported` with that reason, and another platform's build is never substituted.
Every connection attempt takes its number from one runtime-wide counter that removing a device never resets, so an attempt started before a device was removed and added again under the same id cannot settle, close or bind consent for the new connection.
The product's device platforms are macOS on arm64 and x86_64; the S5.5 verification covered two arm64 Macs, and x86_64 packaging is not finished.
Admission per device is four running and thirty-two waiting requests; past that a request is refused as `busy`, never dropped.

Every document read runs on a worker with the runtime lock released, this machine's disk included (`runtime/documents.rs::start_document_open`), and the tab lands when the read does; a reveal of a file moves the checkout, the panel and the tree only then, and only while the checkout the operator had in front is still in front, so a read that fails moves nothing.
A document is pinned at open to its device, its root path with the identity the host reported, and its path relative to that root (`DocumentPlace`), and every save goes back through that pin: a root that was replaced since is refused, and a save after a reconnect goes to the new helper.
A device root's identity is the one the channel pinned when it first touched the root (`host_access::pinned_root`), for opens and Explorer changes as for listings, so a folder made again at the same path after it was listed is refused; an open that is refused this way unpins it, and the operator's next open adopts the new folder.
The draft's base is a content revision (`sha256:<hex>`), not a modification time.
A save is exchange-and-verify, not a compare-and-swap (`hide-host/src/save.rs` states exactly what it guarantees): the file is hashed against the base, the draft is written beside it and exchanged in one atomic rename (`RENAME_SWAP`, `RENAME_EXCHANGE`), and the displaced file is hashed again; a change that landed in between is exchanged back and the save is a conflict, so a change complete at the path is never overwritten and the original is never left truncated.
A third writer that replaces the path during that swap back is not lost either: its file ends up beside the path under the save's temporary name and is kept, and the conflict names the revision now at the path so Keep Editing can adopt it.
A filesystem with no atomic exchange refuses the save and the draft stays, exportable from the conflict or unknown bar.
Saves and revision reads in one folder hold an advisory lock, exclusive and shared, so Hide's own read-back waits for a save still running in a helper whose connection already ended.
One save runs per document and only the newest draft waits behind it.
Reload and a save exclude each other on one tab: Reload is refused while a save is running, waiting, queued or unsettled (`file.reload_busy`), and a save is not sent while a reload of that tab is in flight (`file.save_during_reload`), so neither takes the other's revision or slot away.
A save asked for while the device's helper is still connecting has not been sent: it is `waiting`, goes out when the helper is ready, and is dropped with the draft kept and the reason shown if the connection fails; that is a different state from `unknown`, whose save was sent.
A save whose answer was lost to a timeout or a dropped connection is `unknown`: it is never resent, further saves wait, and when the device's helper is ready the file is read back: the draft's revision means it saved, the base's means it has not reached the file yet (`file.save_not_applied`, and the tab's save state `not_applied`, which the bar words "Not saved yet" rather than "Not saved", because a helper whose connection ended may still finish it; Retry is offered there, and a retry that meets the late write lands as a conflict, never over it), anything else is a conflict.

An Explorer change (`file_create`, `dir_create`, `path_rename`, `path_move`, `path_trash`) is one `hide_host` request too (`hide-host/src/mutate.rs`, `files::apply_explorer_operation`), made by the host of the checkout in front on a worker: this machine's in process, a device's by its helper.
The event names the device whose tree asked, and a change for another device than the one in front is refused (B34); hided lets a device's change past this machine's checkout roots and the helper confines every path to the opened root.
Trash stages the item in the device's temporary directory before handing it to the Trash, so on a Linux device whose `/tmp` is another filesystem the move is refused and the item stays; Linux devices are outside this stage's supported platforms.
Nothing is replaced: a creation opens exclusively and a rename or move uses the kernel's exclusive rename, so a name that appears meanwhile is refused and both items stay (B16).
A trash names the item by the inode the tree listed (`directory_list` entries carry it) and is refused if another item took that path; the item moves through its opened parent into a private staging folder outside the checkout, is checked again there, and only then goes to that machine's Trash, and a Trash that refuses it puts it back, so nothing is ever deleted permanently (B17).
On success the core carries its own paths in that checkout only: the device's expanded folders, and each open tab with the place its saves go to, so a renamed file saves to its new path; a trashed file's tab keeps its draft and shows the file as removed, and a save does not recreate it (B18).
A change whose answer was lost is reported as an unknown result and the tree reads the folder again.

### Device catalogs

A device's projects are grouped by the same rule as this machine's (`device_catalog.rs`): a project is a repository identified by its main worktree on that device, and each checkout a tab sits in is a row under it, so two Herdr workspaces in one repository are one project and a workspace whose tabs sit in two repositories is split between them.
The facts come from the device's helper (`Call::Project`, answered by `hide_project::facts` there), asked on a worker once per directory for the life of a helper connection; the runtime keeps Herdr's session as it came (`device_raw_sessions`) and publishes the grouped one.
Ids stay scoped to the device: a project is `remote:<device>:project:<sha256(device, root)>`, so the same path or repository on two devices is two projects, and a checkout id names exactly one Herdr workspace (`remote:<device>:checkout:<workspace>`, or `…#<hash>` for a second checkout a workspace's tabs sit in), which is what `focus_workspace`, `create_tab` and a purpose send to the host (`checkout_id` in `remote_control`).
A directory the helper has not answered for stays its Herdr workspace's own row and `status.remote[].catalog` says why (`resolving`, `unavailable` with the reason, or the folders it refused); it is never grouped by a guess and never read on this machine, and a registration on another device is never inspected here.
A device's worktrees are read by its helper too (`Call::Worktrees`, the same `hide_host::worktrees::read` this machine runs in process), on a worker after the device's facts answer and again after each task there, and are carried onto its rows by path as the helper reported them (`device_catalog::apply_worktrees`), so a same path on this machine never lends a device its branch, dirt or gate.
Creating or deleting a device's worktree takes that device's Herdr and helper together (`WorktreeTarget::device`): the branch is checked (`Call::BranchCheck`) and the new directory resolved (`Call::Directory`) by the helper before Herdr's `worktree.create` runs there, a deletion is gated on the device's facts, closes that device's panes by Herdr's own ids and removes on the helper, and the receipts (`task_operation`, `worktree_removal`) carry `device_id`; the branch-description mirror of a purpose is written on this machine only.
A device's registrations live with this machine's in `ui_state.workspace_registrations`, keyed by the device's project id, and are carried onto its grouped session (`device_catalog::apply_registrations`): a registered project Herdr has no workspace in keeps its row with its main checkout and no tabs (`…#registered`, which names no Herdr workspace), and only a registered row carries a pin and the panes a removal would close.
Registering on a device asks its helper (`Call::Registrable`, `hide_host::register::check`), which judges the folder against that device's own home, `~` included, and names the project it belongs to; hided passes a device's `create_workspace` through instead of applying this machine's home, and nothing is created on the device.
Removing a device's project closes its panes through that device's Herdr by their own ids and then removes only the registration, as on this machine.
Opening a registered device project Herdr has no workspace in (its row, or a new tab asked for its `…#registered` checkout) creates one at the registered folder through that device's remote control (`workspace.create`), keyed by the folder while in flight; any other checkout id that names no Herdr workspace is refused.

A device's checkouts are found in the same catalog lookup as this machine's (`catalog_checkout`: the navigator, then each device's Herdr session), and the checkout in front is the selected device's own focus (`front_checkout`), so a file opened on a device lands in the checkout that asked and showing it moves no focus on this machine.
Each device authenticates as its own ssh config says: an absent `SSH_AUTH_SOCK` stops only a device whose config names no `IdentityFile` or `IdentityAgent`, and that device's row says so.
A local path is judged inside its checkout after resolving the folder that holds it, so a shell that spells `/var` for `/private/var` still opens it, while the document keeps the path as the shell sent it.

### The shortcut registry

`web/src/shortcuts.ts` is one table, command to chord per host, matched on `KeyboardEvent.code` at the window capture phase ahead of xterm and Chrome's defaults and never during IME composition (`web/src/keyboard.ts`).
The `⌘/` sheet is generated from the table.
Settings > Shortcuts rebinds the seven browser pane commands (split right and down, zoom, close pane, larger, smaller and reset text); the Swift host's eighth, Toggle Conversation, has no web surface.
The overrides live in the core's `ui_state.browser_shortcut_bindings`, apart from the Swift host's `shortcut_bindings`, because the hosts reserve different keys; a save that omits the field keeps them.
The window listener, the sheet and the editor all read one effective registry (`effectiveRegistry`), and a chord is refused in its row before it is saved when it has no ⌘, ⌥ or ⌃, is one Chrome or macOS keeps, or is another command's; a stored map that fails the same rules is dropped whole and the defaults run with a diagnostic.
While a row records, the listener runs no command, and IME composition never records.
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
| Settings | ⌘, | ⌥, (moved: Chrome keeps ⌘,) | TODO |
| Keyboard shortcuts | - | ⌘/ | TODO |
