# Native performance and rendering verification

Read this before investigating terminal lag, flicker, selection, scrolling, resize, tab switching, CPU, memory, or snapshot/attach performance.
This is a repeatable verification procedure, not a record that any particular build passed.
Keep raw evidence and the run verdict under `agents/runs/<slug>/`; never commit traces, recordings, profiles, transcripts, or screenshots.
Use [CONTRIBUTING.md](../CONTRIBUTING.md) for delivery gates and [dev-runtime.md](dev-runtime.md) for bundle identity.

## Verification layers and current CI coverage

There are three complementary layers; none replaces the other two.
This table describes the checked-in workflows, not a claim that a particular PR's remote run passed.

| Layer | What it catches | Current execution |
| --- | --- | --- |
| Deterministic regression tests and structural checks | Blank repaint buffers, cache bounds, incorrect state transitions, blocking work in forbidden paths | Rust and Swift suites plus repository checks run on every PR and main push in `.github/workflows/pr.yml`; the native design workflow adds static design checks |
| Native interaction QA | Actual focus, input, drag, scroll, resize, compositor-visible flicker, and bundle mistakes | Local, isolated, logged-in macOS session; not wired into CI |
| Controlled performance comparison | Warm/cold latency distributions, periodic stalls, CPU/lock contention, and sustained RSS | Local matched baseline/candidate measurements; no checked-in scheduled or required native performance job |

Swift renderer tests can instantiate AppKit views and compare pixels without launching and driving the complete app.
That is useful automated rendering coverage, but it does not exercise the physical display, live Herdr transport, foreground focus, or actual agent TUI interaction end to end.
The required `check-terminal-row-cache.sh` structural gate additionally checks that the draw loop reaches expensive text preparation through the cache, not through a direct call on every repaint.
It protects that source-level cost boundary; it does not measure frame latency or replace the bitmap and retention tests.
The latency summarizer's Python tests run in the required repository-invariants lane.
Browser-host Node tests and fixture/replay commands do not become CI gates merely because this guide lists them.
Check the workflow before claiming any of them runs automatically.

### Maintenance and review policy

1. Every change runs the applicable CI regression gates; a rendering/performance bug gets a deterministic regression test when it can reproduce the observed failure economically.
2. Changes to terminal drawing, selection, input/scroll routing, display pacing, geometry, focus, or attach lifecycle also require the affected native scenarios before being called verified.
   Record the exact bundle and evidence; if the run is unavailable, mark native QA unrun and leave that acceptance claim open.
3. Changes to caching, scheduling, snapshots, locking, dependencies, or claims of improved speed/memory require the affected controlled baseline/candidate measurements as well.
   Apply the ten-minute RSS procedure only when making a memory claim; do not impose it on unrelated documentation changes.
4. The change author records those results in the PR's existing Evidence section, and the reviewer checks coverage and exclusions as well as CI.
   A green `verify` job cannot enforce the local native requirement by itself today.
5. Keep the regression test beside its component, reusable measurement tools in `scripts/`, procedure and comparison policy here, and each run's raw evidence under `agents/runs/`.
   Never turn a one-off trace or historical number into a hardcoded universal latency limit.

For future unattended coverage, add an owned logged-in macOS QA runner only after app isolation, permissions, fixture setup, cleanup, and baseline identity are reproducible without an operator's desktop.
Serialize native jobs on that desktop and keep API credentials and private transcripts out of untrusted PR jobs.
Start with an explicitly triggered native smoke lane, then add scheduled measurements and reviewed thresholds after enough comparable runs exist.
This is the proposed next automation step, not infrastructure this repository already has.

## 1. Define the claim before running anything

### High-frequency action contracts

Review the entire input dependency path, including shared observable state and overlays, even when the diff does not touch an event handler.
State the cost per input, its notification fan-out, how it scales with total versus visible items, and what bounds pending work.
An asynchronous task still costs work and can accumulate a queue; it is not a performance exemption.

| Action | Required work | Work that must not follow every input |
| --- | --- | --- |
| Sidebar wheel | Resolve the actual target and deliver native scrolling | Project/catalog rebuild, unrelated state publication, disk/network I/O |
| Tooltip dismissal / hover exit | Publish a real transition once and cancel obsolete reveal | Repeated no-op publication to every tooltip consumer |
| Hidden shortcut hints | No target exposure projection while hidden | Per-control recomputation of the complete hint set |
| Terminal wheel / typing | Prompt delivery preserving routing, ordering, and signed scroll quantity | Wait for an unrelated frame; drop intentional input as a duplicate |
| Drag / repaint | Update affected geometry or damaged visible content | Per-event persistence or rebuilding unchanged rows |

### Two-level recent navigation cost contract

`AgentMRU.swift` contains the shared recent-item ordering and held-cycle implementation, with project and per-project tab adapters.
`ShellModel` observes incoming core navigation revisions and caches all unified surfaces; it does not reconstruct this projection for repeated Tab input, editor content deltas, or find-only deltas.
A topology/rest snapshot or active editor change reconciles retained history; retained storage is O(P + T) for P projects and T unified surfaces.
Projection follows the existing tab strip and indexes source tabs instead of searching the retained list once per strip entry.
Editor source indexing is per checkout; agent lookup is indexed once per device and visits each checkout pane once.
Reconciliation is O(A + N + T + C*E), with A agents, N panes, C checkouts and E retained editor tabs; this cost is outside repeated key input.
Starting a gesture snapshots its relevant MRU in O(P) or O(Tproject); every subsequent step changes one index in O(1), with no core dispatch until commit.
Only the active cycle publishes once per changed highlight; repeated cancellation and a one-item cycle publish no cycle change.
Empty and single-item navigation and closing an empty strip produce no notice or shell publication; these are normal no-ops, not failures.
Reconciliation emits existing structured trace events with reason, removal count, and snapshot revision, without paths, labels, or shell-wide notice updates.
The existing cycle presentation has a dedicated observable owner read only by the overlay; preview steps send no shell-wide notification and therefore do not rebuild the retained sidebar or tab strip.
The overlay projects at most nine rows through cached identity lookups, regardless of retained list size.
Its agent marks reuse `AgentBadge` and the bundled-image cache, without scanning retained agents or scheduling image loads per repeated key.
The synthetic-key release check has one replaceable timer; repeated key-up cannot queue unbounded commits.
Search arrow input retains one selected result ID and scans the current result IDs in O(R), with no core dispatch until activation.
A changed highlight redraws only the search sheet, reusing its existing project/agent projection; this is O(W*A + R) for W projects and A retained agents, not a shell-wide notification.
Search arrow navigation schedules no per-key task or timer; scrolling follows only a changed selected ID, and repeated input at either list boundary keeps the selection unchanged.
Both search sheets share `HideSearchKeyboard` and its identity-based selection model.
File-search arrow work is bounded by the existing 80-result limit, independent of the retained file index; query filtering keeps its existing background ranking and rejects cancelled queries or replaced indexes before publication.
`WorkspaceFileSearchTests` covers selected-file activation, filtering to zero results, retired selections and repeated movement at the result limit.
Numbered agent routing checks event type and modifiers before one physical-key lookup; unrelated text input does not scan the agent list or publish navigation state.

Regression owners are `RecentNavigationTests` (2, 9, and 10,000 retained entries over 20,000 input steps, bounded visible rows, deletion convergence), `RecentNavigationIntegrationTests` (actual core restoration and notification counts), and `PaneShortcutSettingsTests` (physical numbered keys, modifier ownership and release).
Native verification must additionally exercise both directions, modifier hold/release, Escape, project restoration across checkouts, and terminal/file/diff/Browser surfaces in the isolated fixture.
Report those native checks as unverified if the sole-running-instance gate blocks launching the dev bundle; deterministic tests do not prove native event delivery or Korean text legibility.

### Regression ownership and honest coverage

| Boundary | Automated owner | What remains outside that test |
| --- | --- | --- |
| No-op publication and cancelled tooltip reveal | `HideTooltipTests` | Physical wheel monitor delivery and compositor latency |
| Stationary native-scroll routing and pointer crossing | `PaneShortcutSettingsTests` | Complete SwiftUI sidebar frame cost and physical trackpad behavior |
| Native sidebar row ownership and scrolling | `SidebarListTests` | System event-monitor cost and physical input-to-presentation latency |
| First/subsequent wheel delivery and keyboard order | `live.rs` writer tests | Transport-to-visible-scroll latency |
| Render repair and prepared row retention | Swift renderer tests and `check-terminal-row-cache.sh` | Live output and display presentation |
| Counts, exclusions, unknown refresh rates, retained stalls | `scripts/tests/test_terminal_latency.py` | Causal pairing of an input with its requested content change |

The Rust/Swift tests and latency-summary tests run in PR CI.
Hidden-overlay projection cost and whole-sidebar isolation do not yet have an end-to-end automated guard; review the source and profile the affected native scenario rather than calling them covered by the tooltip tests.
For a growth claim, hold the visible row count fixed, vary total retained items and input count independently, and compare work and pending-queue growth after warm-up.
This controlled scaling run remains local QA, not an implemented CI benchmark.
Assert stable observable boundaries rather than private helper call graphs; use a targeted structural gate only when the cost contract cannot be observed economically in component tests.
Restore a realistic old defect temporarily and predict which regression will fail before running it; keep the mutation out of the final diff.
Do not turn scheduler-sensitive elapsed time into a universal performance threshold.

### Measurement claim

Write the symptom, exact reproduction sequence, expected visible result, affected clients, and baseline/candidate revisions in the run record.
Separate these questions: does the content render correctly, how long does an internal stage take, how long until the user sees the requested change, and does retained memory grow?
A faster median does not prove flicker is fixed; a bounded cache does not prove lower p95 latency.
Agree on an acceptance threshold and measurement boundary before comparing results.
Historical measurements from another build or machine load are context, not universal pass thresholds.

Use the same pane content, grid, font/text scale, display, refresh rate, build configuration, and interaction sequence in both builds.
Compare idle and driven conditions separately, distinguish cold-start from warmed-up draws, and alternate baseline/candidate trials to expose changes in machine load.
Report sample count, nearest-rank p50/p95, maximum, excluded observations, and trial conditions, not only the best run.
If the conditions cannot be matched, label the result a functional reference or an uncontrolled observation, not performance parity.

## 2. Identify the build and protect the operator

Before visual checks, record the exact executable path, bundle identifier/version, PID, source revision, dirty diff, core archive hash, bundled Herdr version, and build configuration.
Inspect running processes rather than assuming the app launched from this checkout is the visible one.

```sh
pgrep -fl HerdrMacOS
```

Exactly one Hide instance must be running during native verification.
If an operator instance is open, coordinate a QA window before quitting it normally; never kill all matching processes.
Record its exact bundle path and restore that same bundle afterward, leaving its Herdr server and terminals intact.
A worktree-specific bundle identifier or separate app state file does not isolate the Herdr server's shared focus.
Native automation takes foreground focus in the logged-in macOS session; it cannot promise uninterrupted simultaneous operator use.
Use a separately authorized machine/session if foreground interference is unacceptable.

Build a resource-complete, signed bundle using the existing scripts:

```sh
bash macos/scripts/build_dev_app.sh
# For optimized performance measurements, in a separate build checkout:
zsh scripts/build-app.sh
```

The dev script builds a debug Swift shell with a release Rust core; it is not a release-performance baseline.
The release script writes `dist/` artifacts and requires a version tag or its documented `HIDE_VERSION` input; do not run it over another run's retained outputs.
Archive baseline and candidate separately, including their Swift resource bundles and pinned runtime.
Use independent checkout-local Rust and Swift build directories; do not share release output directories across revisions.
The shell links `target/release/libherdr_core.a`, so a relocated Cargo output alone does not change the linker input.
Verify the linked archive hash and expected runtime diagnostics: a fresh Swift package fingerprint can otherwise accompany a stale core archive.
Building, testing, installing, launching, committing, and merging are separate states; report each accurately.

## 3. Isolate runtime state before making fixtures

Read the current official CLI and socket references required by [AGENTS.md](../AGENTS.md#herdr-api-contract), and check the bundled binary's contract before using integration commands.
Use that exact binary for the private server, fixture CLI, and reference TUI, not whichever executable happens to be on PATH.
Every invocation, including cleanup, must use the same explicit routing environment:

| Input | Required isolation |
| --- | --- |
| `HERDR_SESSION` | A unique run-owned session name |
| `HERDR_SOCKET_PATH` | An unused short absolute socket path |
| `HERDR_CONFIG_PATH` | A private config file under the run directory |
| `XDG_CONFIG_HOME`, `XDG_STATE_HOME` | Private configuration and state roots under the run directory |
| `HERDR_PANE_ID`, `HERDR_TAB_ID`, `HERDR_WORKSPACE_ID` | Clear inherited identifiers before launching the fixture |
| `HERDR_ENV` | Clear inherited nesting marker when starting the standalone reference TUI |
| Hide `--state-path`, `--workspace-root` | Explicit run-owned app state file and disposable checkout |
| Hide `--verification-no-remote` | Disable configured remote targets in the debug bundle while exercising a live private local server |

Check the pinned runtime's path behavior when updating it.
The tested session layout stores sessions under `<XDG_CONFIG_HOME>/herdr/sessions/<HERDR_SESSION>`; changing `HERDR_CONFIG_PATH` alone does not isolate session data.
The client socket inserts `-client` before `.sock`; allow room for that suffix in the platform's Unix socket path limit.
Do not repurpose `HOME` or assume a private local socket disables SSH discovery.
Inspect remote registrations, automatic SSH connection attempts, and remote client state too; an unexpected remote connection is an isolation failure to resolve before interacting.
Pass `--verification-no-remote` to a debug bundle when the scenario does not exercise remote behavior; release builds ignore this verification-only argument.
Do not edit the operator's SSH configuration or stop remote services to make a local fixture pass.

Save process/socket ownership before launch, prove the private server has zero workspaces before creating fixtures, and verify that the operator server gained no QA connection.
Use explicit fixture IDs, not current-focus shortcuts, for mutations.
Record operator agent count separately from private pane/agent and attached-child counts: isolation does not remove shared machine load.

## 4. Reproduce with real native interactions

Use the installed Peekaboo CLI directly, not an MCP server.
Missing Screen Recording or Accessibility permission blocks native automation.

```sh
/opt/homebrew/bin/peekaboo permissions status --json
/opt/homebrew/bin/peekaboo app list --json
```

Discover windows with `peekaboo window list --app <exact-app> --json`, then take a fresh `peekaboo see --app <exact-app> --json` snapshot.
Confirm the exact PID/window before each mutation, prefer fresh element IDs, and verify the result with another observation.
If an automation command reports a failed postcondition, inspect actual state before retrying; the action may already have happened.
If daemon-backed targeting is wrong, inspect local CLI help and use `--no-remote` with the same exact target, then verify again.
Coordinate origins can differ between move, drag, and screenshot operations; never reuse unverified coordinates.
Do not automate credentials, unlock prompts, or authentication.

Exercise ordinary shell history, a long Claude transcript, and a long Codex transcript separately: their repaint and mouse-routing behavior differs.
Use disposable fixtures and fork inactive transcripts when needed; do not resume or send input to an operator's live agent.
Typing tests should leave text unsubmitted unless command execution or model work was explicitly placed in scope.
Live output-generation load is a separate test, not something proved by typing into an idle transcript.

| Scenario | Verify visibly and record |
| --- | --- |
| Continuous typing and deletion | Input appears, surrounding content remains intact, no whole-body blanking; include Korean IME and wide glyphs when relevant |
| Wheel up/down and reversal | Requested content moves, first wheel is not artificially delayed, cancellation and history boundaries do not wedge later input |
| Text selection | Multiline highlight is stable during output; check inside, exactly on, and 1 px outside each edge so top-row dragging does not scroll inside or leave an outside dead band |
| Continuous output | Parsing progresses and visible content updates without periodic stalls or unbounded row retention |
| Resize and split | Held content survives intermediate sizes and is replaced by a matching full frame at settled geometry |
| Tab switch, hide, and revisit | Correct pane appears, hidden panes do not draw, released attaches are not retained indefinitely, revisiting restores content |
| Repeated repaint | Selection, exposure, and repeated AppKit callbacks preserve text even within one display tick |

Capture real screenshots for visible claims; a running process or passing unit test is not UI evidence.
For flicker, record a bounded window region with `screencapture -v -R <x,y,width,height>` and inspect the resulting dimensions and frame rate before analysis.
Do not assume `-l` window targeting crops a video just because it crops a still image.
Inspect both full frames and a terminal-body crop that excludes stable chrome and input footers.
A blank-frame detector must be calibrated against the fixture's background and known populated/blank frames; record crop and thresholds instead of copying another run's coordinates.
No detected blank frame means only that the detector found none at the capture rate.
For example, 60 fps video can miss a single 120 Hz frame, and whole-body blank detection does not cover partial corruption.

## 5. Measure the boundary actually under discussion

Replace placeholders below with recorded values; write every output into the run directory.
Capture debug intervals from the exact app PID:

```sh
/usr/bin/log stream --process <app-pid> --level debug --style ndjson \
  --predicate 'subsystem == "me.grab.hide" AND category == "TerminalLatency"' \
  > <run-dir>/terminal-latency.ndjson
python3 scripts/summarize-terminal-latency.py <run-dir>/terminal-latency.ndjson \
  --started-after <unix-seconds>
```

Record the capture process so only that process is stopped after the observation window.
The debug mirror carries the same interval end values as the signposts and does not require Instruments.
The summarizer reports nearest-rank percentiles and exclusions; keep the raw trace with the result.
It aggregates recorded interval-end events only.
An interval still pending when capture stops may appear in neither completed samples nor exclusions; compare against observed input counts and report missing endings rather than treating them as successes.
Use distinct capture windows per scenario and stop capture at the end; `--started-after` is a lower bound, not an end-time filter.

| Interval | What it measures | What it does not prove |
| --- | --- | --- |
| `key_to_send` | Main-actor input delegate to actual transport flush | Physical keypress to visible application response |
| `receive_to_draw` | Delivery to the registered terminal view through software drawing | Server rendering time or compositor presentation time |
| `wheel_to_draw` | Wheel event to the next draw | A causally corresponding scroll repaint |
| `tab_to_first_draw` | Traced tab activation to its first draw | Correct content, which still needs visual confirmation |

At a history boundary a wheel may change nothing; its interval can remain pending until much later unrelated output.
That can yield a many-second `wheel_to_draw` value without a many-second visible stall.
Retain such observations and explain them, but never call the unfiltered wheel metric end-to-end scroll latency or remove outliers merely because they look bad.
For a causal comparison, associate the input timestamp with the requested content-region change and use the same window-server display timestamp method in Hide and the reference TUI.
If that pairing is unavailable, report only the internal proxy and functional scroll behavior.
Automation command duration includes focus, IPC, and event injection overhead; it is not the operator's key latency.
Hidden, released, consumed, capacity-limited, and pre-window intervals are exclusions, never zero-latency successes.
A reported refresh rate of zero means unknown, not a zero-Hz display.

### CPU, lock contention, and idle work

Sample the exact app PID and its private Herdr server in separate short windows during both idle and driven conditions:

```sh
/usr/bin/sample <app-pid> 3 -file <run-dir>/app-sample.txt
/usr/bin/sample <private-server-pid> 3 -file <run-dir>/server-sample.txt
uptime
ps -p <app-pid>,<private-server-pid> -o pid,ppid,%cpu,rss,etime,command
```

Retain contemporaneous load, process lists, selected pane/grid, attached children, refresh rate, foreground interruptions, and Screen Sharing/WindowServer activity.
Inspect symbolication before calculating mutex-wait ratios; predominantly `???` frames mean no usable answer, not zero contention.
Long sampling windows under high load have failed to symbolicate in past runs; collect several short valid windows and report rejected windows too.
State the denominator and thread when reporting a wait fraction, and distinguish waiting on the runtime mutex from time spent holding it.
During idle observation, inspect snapshot publications, `rest` revisions, attach counts, and git subprocess activity rather than inferring no work from a static UI.
`bash scripts/measure-git-section-idle.sh` drives that idle observation for the Git section against an isolated server and records the subprocess and publication counts the paragraph above asks for.
Core diagnostics are mirrored in the app state directory's `Logs/core.jsonl`, with one previous 1 MiB file; copy both into the run evidence before rotation loses the relevant window.

### Memory and renderer replay

For an RSS claim, take eleven one-minute samples across ten minutes per build and retain process lists, endpoints, range, and median.
Keep pane count, occlusion, warm-up, and workload comparable; memory pressure can lower RSS without an allocation improvement.
An isolated renderer replay can identify repeated row preparation, retained generations, and eviction spikes, but cannot establish native input latency.
Replay the same captured frames on both revisions, use optimized builds, and report cold draw, warm distribution, retained-entry peak, and bulk-release events separately.
A cache that stays bounded may remove periodic destruction spikes while leaving median/p95 unchanged or slightly higher; report that result as it is.

## 6. Preserve the architecture while fixing the cause

- Keep the sidebar in `SidebarList`, backed by the platform table, with existing row actions and styling.
  A custom window wheel cache cannot protect earlier system event observers: cursor processing can hit-test the hosting tree before `PaneCommandWindow.sendEvent` runs.
  Compare that full call path when investigating scroll delay, rather than measuring only the app's wheel handler.
  Replacing the native list with a `ScrollView` containing nested SwiftUI rows reintroduces whole-document responder traversal; `SidebarListTests` guards this native ownership boundary, not a universal latency threshold.
- Tooltip dismissal, hover exit, and anchor retention publish only actual state changes.
  Mutating a struct held in `@Published` can emit even when its method returns without changing a field; compute the next value before assigning it.
  Exercise repeated dismissal with no visible tooltip, because wheel events must not invalidate all tooltip-bearing controls.
  The balloon overlay contains only visible tooltips or exposed hints, resolves hint exposure once per update, and skips target projection while hints are hidden.
- Keep subprocesses, blocking I/O, and large serialization outside `Mutex<Runtime>`.
  `snapshot_delta_payload` takes owned data under the lock; `serialize_snapshot_delta` serializes without a runtime to lock.
  Extend `PrecomputedCatalog`, `CatalogCache`, and `RootIndex` rather than adding per-tick or per-tab git calls; stale precomputation keeps the accepted catalog.
- Size snapshot traffic by changes: terminal sequence cursors, rarely-changing revisioned `rest`, and per-event scalars.
  An unused heartbeat timestamp can still dirty `rest` and resend the full navigator every second.
- Send every whole-row wheel promptly; combine signed rows only from consecutive requests already waiting in the writer queue, send no cancelling sum, and never wait for a terminal frame or timer.
  Preserve actual pointer cell/modifiers and Herdr-owned mouse/history routing; do not invent fallback geometry, reconstruct history from viewport frames, or append a same-size resize to force repaint.
  Resolve the first wheel at its real AppKit target, then reuse that route only while events stay consecutive and stationary, so a sidebar gesture does not repeat SwiftUI's responder-tree hit test on every tick.
  A missing view size emits its diagnostic once and sends no wheel.
  The accepted matching-pane Claude policy sends SGR press/release without Enter and records its detection basis; other or unknown panes retain local selection.
- Parse immediately, settle geometry over two stable display ticks, and submit pending damage once per display tick.
  Hidden panes remain undrawn; matching full frames replace a held canvas after geometry/control transitions.
  AppKit draw callbacks repair backing stores and must not be rejected because a draw already occurred in the same tick.
  Retain only each visible row's latest prepared state, not thousands of obsolete generations until a global flush.
  Keep keyboard delivery direct from the main-actor delegate to the writer without another asynchronous hop.
- Announce once per burst and clear the notifier latch before taking the snapshot lock.
  Read-then-clear can swallow a concurrent change.

Follow engineering principles 1, 7, 12, and 13: remove obsolete paths, reuse existing mechanisms, test observable outcomes, and fix the failure class.
For a backing-store bug, repeated draws into fresh pixel buffers should preserve nonempty content; a test that merely approves a frame gate repeats the faulty assumption.
For timing tests, distinguish a deterministic policy threshold from eventual UI delivery under scheduler load.
Do not weaken an externally promised deadline to make a flaky test pass.

## 7. Regression gates, cleanup, and verdict

### Terminal link activation

Links activate only on Command+Click, using SwiftTerm's existing `hoverWithModifier` policy.
Unmodified pointer movement does not resolve implicit links or advertise link activation.
The change adds no pointer state, timer, snapshot publication or consumer fan-out; link work stays bounded by the visible row and existing hover state.
Ordinary clicks keep delayed replay, ordinary drags keep local selection, and Option+drag keeps the mouse-aware application route.
`TerminalLinkActivationTests` drives the real AppKit terminal host with implicit and OSC 8 links and observes activation, replay and selection.
The test fails against the previous hover policy; its maintenance cost is one native fixture with no provider or timing dependency.
Native QA must additionally inspect modifier press/release, cursor and highlight on the isolated candidate.

Use the existing suite wrappers, then the applicable gates in [CONTRIBUTING.md](../CONTRIBUTING.md):

```sh
bash scripts/rust-test.sh
bash scripts/swift-test.sh
python3 -m unittest discover -s scripts/tests -p 'test_terminal_latency.py'
```

The Swift wrapper rebuilds/checks the Rust archive before linking; it also accepts a test filter as its first argument for a focused iteration.
Run the full relevant suite before delivery, and retain pre-fix failure plus post-fix success for the specific regression when feasible.
Do not substitute a renderer microbenchmark for native QA or a native smoke test for long-duration/load coverage.

Stop only owned recording/logging processes, the test app, fixture clients, and the explicitly routed private server.
Verify their exit and socket cleanup before removing or trashing only the exact recorded private state paths.
Never use broad process-name kills, a workspace root as a deletion target, or an unscoped `herdr server stop`.
Restore the recorded operator bundle and verify a real screenshot, exactly one Hide instance, and unchanged operator server ownership.

The run verdict must include:

- Exact revisions, bundles, configuration, PIDs, runtime pin, and isolation evidence.
- Reproduction steps and observed results per client/scenario, with local evidence paths.
- Metric boundaries, distributions, sample counts, exclusions, load, and comparison conditions.
- Regression tests and gates run, failures, and remaining checks explicitly marked unrun or blocked.
- Cleanup/restoration evidence and whether the fix was merely committed, built, installed, or merged.

Use a qualified verdict when coverage is bounded: “no whole-body blanking observed in these recordings” is supportable; “all performance issues resolved” is not.

## Projects and Overview cost contract

Shared control hover, pressed and keyboard-focus appearance stays in local SwiftUI state.
`HideSearchField` observes the existing search keyboard modifier's focus instead of creating another focus owner.
The shared input surface and empty-state renderer add no timers, tasks, I/O or core state.
Row hover/focus remains local to visible controls, and native sidebar/outline scrolling is retained.
Search migration retains its existing filtering and result-ID reconciliation cost; it does not add another search index or per-keystroke subprocess.
These visual transitions neither dispatch runtime events nor mark the snapshot rest payload dirty.
Choice controls publish only a changed selection; repeated activation of the selected option has no action.
Their work scales with the small visible choice set, not the retained project catalog.

Project activity and checkout-pane context reuse canonical agent projection, current topology and cached worktree HEAD metadata.
The existing single `git log -1` read now returns timestamp and subject together; no timer or additional Git subprocess is introduced.
A changed projection indexes agents and visits retained panes once, then sorts projects and each project's checkouts by cached keys.
Cost is O(agents + panes + projects log projects + sum(checkouts log checkouts)); Overview reads the existing checkout/agent aggregation and does not duplicate pane rows.
No work is scheduled by hover or by an unchanged agent-list tick.
Identical catalog snapshots settle to the same ordering and revision, so the shell receives no redundant navigator update.

UI persistence reuses the runtime worker context with one active save and one pending flag; serialization, write, fsync and rename occur outside Runtime's mutex.
Sequential writes prevent an older state from overwriting a newer one; intermediate UI saves may coalesce, while pane input never enters this queue.
A write failure publishes the existing caller-visible save error.
FFI destruction stops producers and joins the last save outside the mutex; forced process termination does not guarantee a pending save.
Standalone unit runtimes without a worker context retain synchronous persistence outside any shared runtime mutex.

Regression owners are `projects_follow_authoritative_activity_and_identical_snapshots_settle`, `overview_tracks_live_checkout_panes_and_drops_retired_lineage`, and the two `removing_registration_*` tests.
Native acceptance uses many private projects, Search and disclosure, live pane retirement/movement, and successful versus in-use registration removal.
Measure baseline and candidate idle/driven work separately with the same project/pane count; tests alone do not prove native responsiveness.

### Project history, disk and cleanup

Only the selected open Overview project requests a bounded history read through WorktreeReader.
One `git log` includes all real worktree HEADs and known main/base refs, retaining ordered parents, decorations, shallow boundaries and a continuation frontier outside the 512-commit window.
Overview-only request changes reuse the worker's catalog cache; closing Overview and unchanged ticks perform no extra Git commands.
Graph geometry rebuilds when history or checkout HEAD identities change; agent status updates do not recompute ancestry.
Linear chains fold around worktree/ref boundaries; rendering is bounded by the history window and retained worktree count.
List rows use the existing lazy native scrolling and search keyboard patterns.

Disk reuses DiskReader, triggered by opening Git/Overview or explicit refresh, with one inflight read and coalesced pending input.
The filesystem walk counts `st_blocks * 512`, partitions nested checkout/shared-Git roots by longest ownership, and deduplicates `(device, inode)` across components.
It counts a symlink's own allocation without following it and rejects alias roots rather than escaping the declared boundary.
It is bounded to thirty seconds and one million visited/pending entries per request; failed or incomplete components have no total and remain visible beside a confirmed subtotal.
This is allocated disk accounting, not physical reclaim estimation for APFS clones.
Opening or refreshing replaces the measurement; no timer, hover or per-row subprocess measures disk.

Cleanup uses the existing action worker context with one active review/removal, never the Runtime mutex, for Git, disk and fresh schema-decoded Herdr snapshots.
Cleanup protects both launch `cwd` and current `foreground_cwd` from the generated snapshot contract; it does not change navigation projection policy.
Review reads are non-mutating; confirmation rechecks each target immediately before `git worktree remove` without force.
Git and Herdr have no shared atomic filesystem transaction: a state change after the last Herdr check cannot be reserved against by this contract.
The UI therefore describes a fresh eligibility check rather than a permanent unused guarantee; Git independently refuses dirty or locked removal.
A stale, missing or failed check is caller-visible and never becomes permission to delete.
Completed intents are retained until dismissal; duplicate confirmation does no work, and retry through a fresh review excludes already removed targets.

Regression owners include `overview_inspection_does_not_focus_or_repeat_publish`, `overview_history_preserves_real_merge_parents_and_reads_all_heads_once`, `overview_close_and_idle_do_not_run_additional_git_commands`, disk filesystem fixtures, cleanup filesystem fixtures and `OverviewPresentationTests`.
Native acceptance additionally covers Tree/List inspection versus explicit focus, narrow Korean/English wrapping, unknown/partial summaries, and cleanup review/cancel/exclusion/success/stale refusal in private fixtures only.

### Local image attachment cost and regression ownership

OS image drag enters through the terminal's existing AppKit destination, with only a pasteboard type check on drag entry and URL extraction on drop.
It adds no work to ordinary mouse movement, wheel routing, selection or terminal frame parsing.
The core input hook compares the canonical base64 single carriage return before any attachment lookup; non-Enter input adds only that constant comparison.
Enter alone visits at most sixteen shelves and four pane intents, with no decoding, I/O or asynchronous hop; ordinary key publication remains the existing core behavior.
A separate local viewport observation performs an O(1) public scroll-state read after feed, size and scroll callbacks.
It retains one Boolean pair, one weak view and at most one pending main-queue publication per terminal; unchanged state wakes no view consumer.
Only that pane's shelf/chip and attachment composition observe this local signal.
Because ordinary wheel history belongs to Herdr, the attachment feature also retains at most sixteen pane-scoped viewport observers until their panes close.
Each observer reads `session.snapshot` for its retained event cursor, opens the existing scoped scroll subscription, and reads `pane.get` once at bootstrap.
A cursor evicted during bootstrap permits one fresh-snapshot retry; a second failure is caller-visible, with no recurring socket query, subprocess, provider parsing or work under the runtime mutex.
Reader cancellation is bounded by the 500 ms read timeout, and repeated numeric offsets while away publish no rest change.
The generated `pane.scroll_changed` event is separate from sequenced topology events and is decoded only at `wire.rs`.
Unknown/failed observation collapses the shelf and blocks handoff with an explicit notice; Return to prompt uses the existing ordered writer's empty-input reset and starts a fresh observation.
The core observes actual bottom transitions through the existing rest publication; this adds at most a departure and return publication per gesture, not one per wheel or output frame.
The core retains at most four visible images per pane and sixteen image intents/copies across live panes; late decoder results cannot recreate removed items.
A single native decoder serializes bounded reads of at most 20 MiB plus one byte and ImageIO thumbnails of at most 192 pixels, rejecting inputs over 40 million pixels.
This work and temporary-file cleanup run outside the core mutex and main-thread pointer path.
Command+V image ownership is checked at AppKit key-equivalent, direct key-down and Paste entrypoints; consuming an image stops further routing of that event.
Other keys perform only the modifier/key check, with no pasteboard read or publication.
Plain text falls through to the existing responder path.
At most five clipboard items are materialized so the fifth can report the four-image limit; each image has the same bounded validation/private-copy pipeline as drag.
The OS pasteboard read is synchronous at the paste boundary; decode/conversion and file I/O remain on the serial decoder.
Preparation releases its held clipboard data, while one request sequence watermark deduplicates ingress with constant retained state.
The composition forwards active-view and local-bottom changes only; availability alone never publishes a core rest revision.
The explicit first Enter scans at most four pane intents and the existing agent metadata, reuses the ordered writer and emits no provider-query subprocess or transcript parsing.
Preparation and pane switches never start handoff; that preserves the exact cancellation window.
No timer appends Enter after transport completion; provider readiness must be reviewed before a later ordinary Enter.
The shelf introduces real terminal geometry changes only when its visible content changes; native QA must exercise resize and scroll while it appears and disappears.

Each stage, prepare/explicit-handoff result and transport result publishes one actual state transition in the existing revisioned rest section.
The rest consumers observe those infrequent transitions; ordinary PTY chunks do not republish the shelf or decode images.
Repeated stage/preparation/availability/completion/removal intents without a change publish nothing and never repeat terminal input.
The transport completion carries a local attachment correlation through the existing writer, with no new Herdr wire method or provider-output consumer.
It is consumed as shelf state rather than appended as terminal bytes or a synthetic transcript message.

`image_cancellation_precedes_explicit_handoff_and_preserves_other_input` observes first/middle/last cancellation, unchanged typed bytes, no delivery from preparation or pane switches, explicit ordered handoff without Enter, loading/away/unsupported refusals, separate queue/completion states, stale-generation refusal, partial-write retention, repeated-intent convergence, capacity and retirement.
The resource regression checks private copy byte equality, unchanged originals, PNG/JPEG and clipboard TIFF conversion, image-versus-text/mixed clipboard routing, monotonic ingress deduplication after cancellation, actual adaptive grid row growth and invalid/oversized failures.
The socket viewport regression starts with expired event history and observes initial state, 100 changing offsets, return and disconnect, checking that only bottom transitions and explicit failure reach its caller.
The AppKit paste regression exercises image Command+V with ordinary and enhanced terminal keyboard modes, direct key-down delivery and text passthrough; clipboard validation remains covered through the real private-file service.
`TerminalViewportSignalTests` uses an offscreen real AppKit terminal to check local history, 200 output feeds while away, return/focus, alternate buffer and 20,000 unchanged observations with no extra view publications.
Native QA separately compares Claude's fixed composer and Codex's scrollable composer, rapid wheel/output, drop while away and pending persistence across pane switches.
These native screen and load checks are required before claiming acceptance; an offscreen geometry fixture is not a screenshot of the candidate.
Their maintenance cost is bounded deterministic fixtures with no provider/network dependency; actual provider capability and native OS-drag/rendering checks remain a separate isolated matrix.
No latency or RSS improvement is claimed from these tests.
