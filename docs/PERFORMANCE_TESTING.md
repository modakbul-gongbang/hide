# Performance and rendering verification

Read this before investigating terminal lag, flicker, selection, scrolling, resize, tab switching, CPU, memory, or snapshot/attach performance.
This is a repeatable verification procedure, not a record that any particular build passed.
Keep raw evidence and the run verdict under `agents/runs/<slug>/`; never commit traces, recordings, profiles, transcripts, or screenshots.
Use [CONTRIBUTING.md](../CONTRIBUTING.md) for delivery gates and [dev-runtime.md](dev-runtime.md) for bundle identity.

## Verification layers and current CI coverage

There are three complementary layers; none replaces the other two.
This table describes the checked-in workflows, not a claim that a particular PR's remote run passed.

| Layer | What it catches | Current execution |
| --- | --- | --- |
| Deterministic regression tests and structural checks | Blank repaint buffers, cache bounds, incorrect state transitions, blocking work in forbidden paths | The Rust workspace suite and repository invariant checks run on every PR and main push in `.github/workflows/pr.yml`; `design-contract.yml` adds static design checks |
| Automated end-to-end | Real window launch, attach to a private Herdr server, IPC between the desktop host and `hided`, occluded-window rendering, packaging mistakes | Playwright drives the web shell in a browser and the desktop app through `desktop/e2e/fixture.ts`; they run in the three `web-e2e` shards and the `desktop-e2e` job of `pr.yml`, all on macOS, and are required by `verify` |
| Controlled performance comparison | Warm/cold latency distributions, periodic stalls, CPU/lock contention, sustained RSS, and cost that grows with process uptime | Local matched baseline/candidate measurements (`scripts/web-shell-measure/run.sh`); no checked-in scheduled or required performance job |

The repository-invariant Python tests run in the required `checks` lane of `pr.yml`.
Fixture/replay commands under `scripts/web-shell-measure/` do not become CI gates merely because this guide lists them.
Check the workflow before claiming any of them runs automatically.

### Maintenance and review policy

1. Every change runs the applicable CI regression gates; a rendering/performance bug gets a deterministic regression test when it can reproduce the observed failure economically.
2. Changes to terminal drawing, selection, input/scroll routing, snapshot delivery, geometry, focus, or attach lifecycle also require the affected `desktop/e2e/*.spec.ts` or `web/e2e/*.spec.ts` scenario before being called verified.
   When a scenario cannot be automated yet, exercise it manually against the packaged app and record that the check was manual.
3. Changes to caching, scheduling, snapshots, locking, or claims of improved speed/memory require the affected controlled baseline/candidate measurement (`scripts/web-shell-measure/run.sh`) as well.
   Apply it only when making a memory or latency claim; do not impose it on unrelated documentation changes.
4. The change author records those results in the PR's existing Evidence section, and the reviewer checks coverage and exclusions as well as CI.
5. Keep the regression test beside its component, reusable measurement tools in `scripts/`, procedure and comparison policy here, and each run's raw evidence under `agents/runs/`.
   Never turn a one-off trace or historical number into a hardcoded universal latency limit.

## 1. Define the claim before running anything

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

Before visual checks, record the exact executable path, source revision, dirty diff, bundled Herdr version, build configuration, and PID for `hided` and, for the desktop app, its dev or packaged Electron process.
Inspect running processes rather than assuming the build launched from this checkout is the visible one.

```sh
pgrep -fl hided
```

The operator's app and an isolated candidate may run simultaneously; there is no global single-instance requirement.
Identify the candidate by executable path, PID, and exact window ID before capture or interaction, and re-resolve them after any restart.
Keep the operator's app running by default; building and automated tests do not require quitting it.
Never quit, restart, activate, or otherwise manipulate the operator's app, panes, or server for QA without explicit coordination; never kill all matching processes.
Prove the private server, socket, `HIDE_STATE_DIR` and `HIDE_DESKTOP_USER_DATA_DIR` are isolated before running both apps; ambiguous targeting or shared state blocks the affected check, not the operator's work.
Background exact-window screenshots need not activate the candidate, but foreground keyboard, IME, drag and focus scenarios can interrupt the logged-in user's work; coordinate a bounded foreground QA window for those, or use a separately authorized machine/session.

Build with the existing scripts:

```sh
bash scripts/verify-cargo.sh release
pnpm --dir desktop package
```

`verify-cargo.sh release` builds the release `hided`, `hide`, `hide-host-helper`, and `hide-agent-hooks` binaries; release `hided` embeds `web/dist`, so run `pnpm --dir web build` first.
`pnpm --dir desktop package` (`desktop/scripts/package.mjs`) runs that release build, fetches the pinned Herdr through `scripts/fetch-herdr-runtime.sh`, and packages `desktop/out/hide-darwin-<arch>/hide.app`; it refuses to produce an app if any binary it ships is missing or not executable.
For an unpackaged dev run, `pnpm --dir desktop dev` finds this worktree's `target/{debug,release}/hide`.
Archive baseline and candidate from separate worktrees so each keeps its own `target/` and `desktop/out/`; never redirect build output with `CARGO_TARGET_DIR` or share it across revisions.
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
| `HIDE_STATE_DIR`, `HERDR_BIN_PATH` | A run-owned `hided` state directory and pinned Herdr binary path |
| `HIDE_DESKTOP_USER_DATA_DIR` | A run-owned Electron user-data directory for the desktop app |

`desktop/e2e/fixture.ts` builds exactly this environment and refuses to launch unless `HOME`, `HIDE_STATE_DIR`, `HIDE_DESKTOP_USER_DATA_DIR`, and `HERDR_SOCKET_PATH` all resolve under the run's own temporary directory; `web/e2e/herdr-fixture.ts` does the same for the private Herdr server and clears the inherited `HERDR_PANE_ID`/`HERDR_TAB_ID`/`HERDR_WORKSPACE_ID`/`HERDR_ENV` identifiers.
Follow that same pattern for an ad hoc fixture: forward `HERDR_SESSION`, `HERDR_CONFIG_PATH`, `XDG_CONFIG_HOME`, `XDG_STATE_HOME` and the resolved socket to every child Herdr process, so the server it starts and the core that reads it stay inside the same routing boundary.
Check the pinned runtime's path behavior when updating it.
The tested session layout stores sessions under `<XDG_CONFIG_HOME>/herdr/sessions/<HERDR_SESSION>`; changing `HERDR_CONFIG_PATH` alone does not isolate session data.
The client socket inserts `-client` before `.sock`; allow room for that suffix in the platform's Unix socket path limit.
Do not repurpose `HOME` or assume a private local socket disables SSH discovery.
Inspect remote registrations, automatic SSH connection attempts, and remote client state too; an unexpected remote connection is an isolation failure to resolve before interacting.
SSH devices are registrations kept under the daemon's state directory, so a run-owned `HIDE_STATE_DIR` is what keeps a scenario that does not exercise remote behavior from making an SSH connection attempt.
Do not edit the operator's SSH configuration or stop remote services to make a local fixture pass.

Save process/socket ownership before launch, prove the private server has zero workspaces before creating fixtures, and verify that the operator server gained no QA connection.
Use explicit fixture IDs, not current-focus shortcuts, for mutations.
Record operator agent count separately from private pane/agent and attached-child counts: isolation does not remove shared machine load.

## 4. Reproduce with real interactions

`desktop/e2e/*.spec.ts` and `web/e2e/*.spec.ts` are the primary reproduction path: Playwright drives the real window against an isolated `hided` and a private, pinned Herdr server, and both run in CI.
Read the closest existing spec before writing a new one; `desktop/e2e/browser.spec.ts` covers the occluded-window case; the fixture launches every desktop app with `--disable-backgrounding-occluded-windows` and without activating it (see [BUILD.md](BUILD.md#the-desktop-app)), so an occluded window keeps painting for capture.

For manual QA on a packaged or dev build that a spec cannot yet reach, use the tools and procedure in [VERIFICATION.md: Manual native QA](VERIFICATION.md#manual-native-qa); it owns which tool can address the candidate without reaching the operator's app.
Do not activate, raise, move or unminimize the window merely to obtain a screenshot without coordination.
Occluded-window capture is not proof of minimized, hidden, or off-Space capture support; report blank, stale, or unavailable frames explicitly rather than silently focusing the app.
Confirm the exact PID/window before each mutation, prefer fresh element IDs, and verify the result with another observation.
Do not automate credentials, unlock prompts, or authentication.

Exercise ordinary shell history, a long Claude transcript, and a long Codex transcript separately: their repaint and scroll behavior can differ.
Use disposable fixtures; do not resume or send input to an operator's live agent.
Capture real screenshots for visible claims; a running process or passing unit test is not UI evidence.
For flicker, record a bounded window region and inspect the resulting dimensions and frame rate before analysis; a blank-frame detector must be calibrated against the fixture's background and known populated/blank frames.

## 5. Measure the boundary actually under discussion

### CPU, lock contention, and idle work

Sample `hided`'s PID and its private Herdr server in separate short windows during both idle and driven conditions:

```sh
/usr/bin/sample <hided-pid> 3 -file <run-dir>/hided-sample.txt
/usr/bin/sample <private-server-pid> 3 -file <run-dir>/server-sample.txt
uptime
ps -p <hided-pid>,<private-server-pid> -o pid,ppid,%cpu,rss,etime,command
```

Retain contemporaneous load, process lists, selected pane/grid, attached children, and refresh rate.
Inspect symbolication before calculating mutex-wait ratios; predominantly `???` frames mean no usable answer, not zero contention.
State the denominator and thread when reporting a wait fraction, and distinguish waiting on the runtime mutex from time spent holding it.
During idle observation, inspect snapshot publications, `rest` revisions, attach counts, and git subprocess activity rather than inferring no work from a static UI.

### Memory and long-uptime degradation

For an RSS claim, take eleven one-minute samples across ten minutes per build and retain process lists, endpoints, range, and median.
Use `scripts/web-shell-measure/memory.py` (see "Web shell echo and frame measurement" below) as the reference sampler for the browser process tree's RSS, `hided`'s RSS, and the page's JS heap.
Keep pane count, occlusion, warm-up, and workload comparable; memory pressure can lower RSS without an allocation improvement.

A build that is fast at launch can still become slow after hours, because cost that grows with process age is invisible in any short window taken on a fresh process.
Whenever the report is "it got slow", record `hided`'s uptime first and treat a fresh-launch measurement as the baseline, not the answer.

```sh
ps -p <hided-pid> -o pid,etime,%cpu,rss,command
vmmap --summary <hided-pid> | grep -E "Physical footprint"
```

`etime` against a thread's or the process's `%cpu` gives its average busy fraction over the whole run; compare the peak physical footprint against the current one, since a peak far above the current value means the process held a large transient working set at some point.
The restart test separates accumulated state from workload: quit `hided`, relaunch it against the same private server so the panes and their processes are unchanged, and repeat the same sample window within a minute.
A hot path that disappears on relaunch and returns only after hours is accumulated state and needs an ownership or bounding fix, not a faster implementation of the same path.
A hot path that is equally hot on the fresh process is workload, and belongs to the boundaries above.
Record uptime, the restart time, and both sample windows in the run directory, and state which of the two conclusions the evidence supports.

## 6. Preserve the architecture while fixing the cause

- Keep subprocesses, blocking I/O, and large serialization outside `Mutex<Runtime>`.
  `snapshot_delta_payload` (`herdr-core/src/runtime/snapshot_delta.rs`) takes owned data under the lock; `serialize_snapshot_delta` serializes without a runtime to lock.
  Extend `PrecomputedCatalog`, `CatalogCache`, and `RootIndex` (`herdr-core/src/session_sync.rs`, `herdr-core/src/workspace.rs`) rather than adding per-tick or per-tab git calls; stale precomputation keeps the accepted catalog.
- Announce once per burst and clear the `ChangeNotifier` (`herdr-core/src/handle.rs`) latch before taking the snapshot lock.
  Read-then-clear can swallow a concurrent change.
- Size snapshot traffic by changes: terminal sequence cursors, rarely-changing revisioned `rest`, and per-event scalars.
  An unused heartbeat timestamp can still dirty `rest` and resend the full navigator every second.
- Keep async operation records bounded by active intent and conflict scope.
  A close or topology mutation uses an absolute five-second stage deadline; expiry becomes a caller-visible unknown result and never schedules a destructive resend.
  Status checks are read-only and are started only for an ambiguous close or an explicit status action, so unknown activity does not become a polling loop.

Follow engineering principles 1, 7, 12, and 13: remove obsolete paths, reuse existing mechanisms, test observable outcomes, and fix the failure class.
For timing tests, distinguish a deterministic policy threshold from eventual UI delivery under scheduler load.
Do not weaken an externally promised deadline to make a flaky test pass.

## 7. Regression gates, cleanup, and verdict

Use the existing suite wrappers, then the applicable gates in [CONTRIBUTING.md](../CONTRIBUTING.md):

```sh
bash scripts/verify-cargo.sh test
bash scripts/verify-web.sh
```

Run the full relevant suite before delivery, and retain pre-fix failure plus post-fix success for the specific regression when feasible.
Do not substitute a unit test for end-to-end coverage or an end-to-end smoke test for long-duration/load coverage.

Stop only owned recording/logging processes, the test app, fixture clients, and the explicitly routed private server.
Verify their exit and socket cleanup before removing or trashing only the exact recorded private state paths.
Never use broad process-name kills, a workspace root as a deletion target, or an unscoped `herdr server stop`.
Leave the operator's app untouched unless its shutdown was explicitly coordinated; in that case restore it afterward.
Verify the owned candidate exited, the operator's app remains available, and operator server ownership is unchanged; multiple independently identified instances are not themselves a verification failure.

The run verdict must include:

- Exact revisions, bundles, configuration, PIDs, runtime pin, and isolation evidence.
- Reproduction steps and observed results per client/scenario, with local evidence paths.
- Metric boundaries, distributions, sample counts, exclusions, load, and comparison conditions.
- Regression tests and gates run, failures, and remaining checks explicitly marked unrun or blocked.
- Cleanup/restoration evidence and whether the fix was merely committed, built, installed, or merged.

Use a qualified verdict when coverage is bounded: "no whole-body blanking observed in these recordings" is supportable; "all performance issues resolved" is not.

## Web shell echo and frame measurement

`scripts/web-shell-measure/run.sh` measures the product `hided` the way the S0 spike measured its prototype, so the numbers stay comparable to the S0 spike baseline.
It owns every process it starts: an isolated pinned Herdr server on a socket inside a run-specific mode-0700 directory under `/tmp` (`isolated-env.sh`, the same routing table as section 3), one linked Git checkout and workspace with a `stty -echo -icanon; cat` pane, the release `hided` with its embedded `web/dist`, and one Google Chrome with an automatically assigned CDP port on the page opened with `?probe=1`.
The runner reads Chrome's CDP port from its own profile and sends the page URL over standard input so the token is absent from the Chrome command line.
The operator's socket is only read, before and after, for the topology counts written beside the results.

Echo (PRD B8) is three trials of fifty `herdr pane send-text` markers.
`t0` is the CLI return timestamp and `t1` is the xterm write completion that first shows the marker in the parsed buffer, read through `window.__hideProbe`; the sample is `t1 - t0`, nearest-rank percentiles, the median of the three trial p95s against the S0 spike baseline p95 plus 5 ms.
The baseline is reused from the S0 report rather than re-measured, and `summarize.py` carries it as a named constant so a rerun does not quietly move it.

Frames (PRD B12) is one 120 s window with the pane printing a line every 8 ms while a `requestAnimationFrame` loop injected through CDP records every frame's `dt`; the result is the fraction of frames over 16.7 ms, the WebSocket frame count the page received during the window, and the pane tail that proves the driver ran.
This is a live driven pipeline, not the in-page replay the spike used: a replay mode would put spike code into the product, and the threshold is absolute, so the live run is the stricter measurement.

Run it as `HIDE_MEASURE_RUN_DIR=agents/runs/<slug>/measure/<attempt> bash scripts/web-shell-measure/run.sh` after `pnpm --dir web build` and `cargo build --release -p hided`.
For an unattended comparison, append `--isolated-headless`: Chrome has no native window and the runner records that operator topology observation was skipped, without contacting the operator socket.
Use that same browser mode for baseline and candidate, and report it with the results; a headless measurement does not prove native presentation.
`MEASURE_SCENARIO=areas2` or `areas3` shows two or three Agent tabs through the real area menu, each with one pane.
All shown panes receive the same line-every-8-ms driver; the measured pane also receives fifty echo markers during that load, recorded in `echo-driven-summary.json` separately from its idle echo trials.
`resources-idle.json` and `resources-driven.json` record twenty one-second CPU-time deltas and RSS sums for the owned hided, Herdr and Chrome process trees, independently of the echo and frame samples.
For an RSS comparison, append `--memory-series` after `--isolated-headless` to extend the driven window to ten minutes and collect eleven one-minute process-tree RSS samples in `memory-series.json`.
A first run opens on Main (PRD S6), so the harness uses the Projects sidebar to open its linked fixture checkout before measuring.
The run directory keeps `identity.txt` (head, dirty count, binary hash, Herdr and Chrome versions, load), `echo-*.json`, `frames.json`, both summaries and the owned PID status at cleanup.
The harness resolves that directory to an absolute path before starting child panes, so their private HOME, state, and checkout paths remain valid after the pane changes directory.
`MEASURE_SCENARIO=multi` is the S2 shape (PRD web-shell-pivot-s2 D-08): the measured pane shares its tab with four splits and four more tabs are shown once each so the core holds five attached tabs before the shell returns to the measured tab; `page.json` records the pane, split and tab counts, the attached pane ids and the live xterm instances the run started from (every attached pane keeps its instance parked while its tab is hidden, D-05).
`memory.py` samples resident memory twice, after the shape settled and after the driven window (`memory-settled.json`, `memory-after-frames.json`): the Chrome process tree's RSS sum, hided's RSS, the page's JS heap and its live instance count; the two samples say whether the parked instances grow the tab under load, which is the D-05 revisit trigger.
The driver and the marker still go to the one measured pane, so the other panes are idle shells with mounted xterm instances, and the gate is the same as the single-pane run.
A Chrome window opens on the desktop for the run; the loop throttles in an occluded or minimized window, so leave it visible and report the load recorded beside each trial.

## Projects and Overview cost contract

The shared input surface and empty-state renderer add no timers, tasks, I/O or core state.
Row hover/focus remains local to visible controls.
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
Dropping `Core` (`herdr-core/src/handle.rs`) stops producers and joins the last save outside the mutex before the process exits; forced process termination does not guarantee a pending save.
Standalone unit runtimes without a worker context retain synchronous persistence outside any shared runtime mutex.

A pin is one more key in the same sort and one more exclusion in the same fold pass; `workspace_pin_set` re-sorts the projected list in place and persists through the existing off-lock save, and the removal counts ride the checkout summary pass rather than a second visit of the panes.
Closing a project's panes for `Remove project…` runs on the same worker pattern as worktree deletion, outside the mutex, with one in-flight close per project.
Regression owners are `projects_follow_authoritative_activity_and_identical_snapshots_settle`, `overview_tracks_live_checkout_panes_and_drops_retired_lineage`, `pinned_projects_lead_their_device_in_activity_order`, `pinning_a_registration_reorders_the_row_and_persists_the_flag`, and the `removing_a_registration_*` and `removing_registration_*` tests.
Manual acceptance in the desktop app uses many private projects, Search and disclosure, live pane retirement/movement, pin and unpin, and registration removal with and without open panes.
Measure baseline and candidate idle/driven work separately with the same project/pane count; tests alone do not prove desktop responsiveness.

### Project worktrees, disk and cleanup

The Overview reads nothing of its own: every group header and stat cell is derived from the worktree catalog the sidebar already reads, and opening or closing the Overview adds no Git command.
`behind_upstream` rides the same `rev-list --left-right --count @{u}...HEAD` call that already counted unpushed commits, so a fetched-side count costs no extra process, and `created_at_unix_ms` is one `stat` of the worktree's gitdir in the same background pass off the mutex.
The catalog pass is bounded by the worktree count; a project with many worktrees pays one status, one rev-list and one stat per worktree per change, never per tick or per agent update.
A change is scoped to its own repository: each project carries its own freshness key (its git directory stamps and its working-tree sample), so a commit in one registered project re-reads that project alone and every other project is answered from the worker's last read; `a_commit_in_one_project_does_not_rerun_status_in_another` owns this.
A finished worktree removal follows the same scope: its row leaves the catalog under the lock with no Git call, the coordinator rebuilds the rows on its next wake, and the reader re-reads only the removed worktree's repository; `a_finished_removal_drops_its_row_at_once_and_an_older_read_cannot_bring_it_back` owns this.
Every `git` the catalog runs is bounded by `GIT_DEADLINE` (15 s) and drained off-thread past the pipe buffer; a repository that outruns it reports its status unavailable and a `git.deadline_exceeded` diagnostic rather than holding the other projects' answer, which a status over evicted iCloud files once did for minutes.
Group ordering, chips and search are pure functions of the accepted snapshot; agent status updates redraw rows and never recompute the catalog.
List rows use the existing lazy-loading and search keyboard patterns.

Disk reuses `DiskReader` (`herdr-core/src/disk.rs`), triggered by opening Git/Overview or explicit refresh, with one inflight read and coalesced pending input.
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

Regression owners include `overview_open_section_focuses_the_checkout_and_switches_the_panel_in_one_event`, `agent_start_in_checkout_reports_through_the_task_operation_slot`, `behind_upstream_is_absent_without_an_upstream_and_counts_the_fetched_side`, `linked_worktrees_carry_their_creation_time_and_the_main_worktree_none`, disk filesystem fixtures and cleanup filesystem fixtures.
Manual acceptance in the desktop app additionally covers row click versus header click versus the `N files` chip, narrow Korean/English wrapping of branch names and tasks, `…`/`?` cells, and cleanup review/cancel/exclusion/success/stale refusal in private fixtures only.

## Project Memory cost contract

Opening Sessions starts one bounded background catalog read for the focused local Project; filtering, searching, and switching Sessions/Memory modes operate on the retained snapshot and start no subprocess.
The coordinator checks for due Memory work no more than once every five seconds, keeps one poll in flight, and schedules analysis only after a source has complete unread bytes and its size and modification time have remained unchanged for sixty seconds.
An unchanged poll publishes nothing.
Session file discovery, incremental reads, SQLite work, redaction, provider requests, hook-config writes, and JSON serialization stay outside `Mutex<Runtime>`.
One incremental transcript poll reads at most 1 MiB, one retained JSONL line is capped at 256 KiB, and catalog or archive detail parsing rejects a complete session above 64 MiB.
One Project/session/content-hash intent is idempotent, and background input is capped at 64 KiB, one inflight request, thirty requests per minute, and 10,000 active items per Project.
Crossing a cap is an actionable state rather than an enlarged queue or automatic deletion.

The web shell's Project Sessions (PRD S8) adds no work until a Project is named: the `project_sessions` delta section is one absent check per publication.
Naming a Project, arriving at its screen, reconnecting or pressing Retry is one bounded catalog read on a worker; a request that arrives during a read coalesces into one more read, so pending work is at most one read, and opening a session is one bounded detail read the same way.
While a Project is named, each publication compares the section by value under the lock, O(rows) with the open transcript behind a shared pointer compared in O(1), and resends it only when it changed; a re-read that finds the same conversation keeps that pointer, and the worker, not the lock, compares the two.
The core keeps each Project's last rows so a session whose file went away stays listed, and hands them to the next read's worker by pointer; the worker checks the file of each row the catalog no longer lists, so the lock does no file I/O, and what is kept grows only with the sessions deleted while the daemon runs.
Provider filtering and search run in the page over the retained rows and send nothing.
Neither read schedules Memory work or touches the due-work poll.
Regression owners are `runtime::tests::project_sessions`, `web/src/sessions.test.ts` and `web/e2e/s8.spec.ts`.

`UserPromptSubmit` is a separate high-frequency boundary.
It accepts at most 256 KiB of hook input, opens the app database read-only, checks schema and active-projection integrity, requests at most sixty local FTS candidates, and returns at most three whole items and 600 estimated tokens.
It has a 100 ms hard deadline and starts no model, embedding, transcript scan, child process, network request, or database write.
Missing, locked, corrupt, stale, unresolved, or over-deadline inputs exit successfully with empty context so prompt submission continues.
`SessionStart` reads the precomputed Project capsule under the same read-only and fail-open ownership, with at most five whole items and 600 tokens.
If the first prompt arrives before that durable receipt is projected, the hook omits Memory for that prompt and retries on the next prompt; it never guesses the delivered set or creates a receipt sidecar or other second store.

Regression owners are the `hide-project` identity tests, `hide-session` provider-neutral catalog and cursor tests, `hide-memory` Project-isolation, lifecycle, convergence, FTS transaction, redaction, ranking and budget tests, `hide-agent-hooks` fixture/config/fail-open tests, and core atomic-event and editor-preview tests.
Manual acceptance in the desktop app uses one exact worktree-local `hided` PID and window against a private Herdr server and private app state.
Record hook idle and driven timing separately, including sample count and failures, and record provider request count, queue/inflight bounds, child descendants, and RSS separately from the prompt path.
An automated deadline test proves bounded return under its fixture conditions; it does not prove every storage device or interaction remains below 100 ms.

### Project Home projection and issue reads

The shell memoizes the board by navigation snapshot revision, project identity and connection state.
A body re-evaluation on unchanged input does not regroup cards or rebuild lineage.
The core issue projection uses accepted catalog and metadata only; it schedules GitHub work on a changed selected reference, board open, project selection or explicit refresh, never a timer.
The GitHub reader keeps its existing single worker and per-project generation cache.
At most 200 linked identities and backlog entries are retained per project; one extra list result reports overflow.
Closed and cross-repository identities are resolved in one bounded query.
Manual writes use the existing task-operation slot and a terminating worker; cleanup shares the bounded purpose mirror queue.
Manual acceptance in the desktop app includes empty-checkout entry, overlay dismissal, both groupings, issue linking, stale facts, narrow widths and mixed Korean/English labels.
`runtime::tests::issues` owns projection memoization, stage priority, deduplication, issue precedence and transition-only refresh regressions.

The Overview's tiles, lanes and lineages (PRD overview-lenses-tiles-agents B27, B32) are pure functions of the snapshot the page already holds (`web/src/overviewLens.ts`), memoized on the projects, agents and devices they read; one pass over the scope's agents buckets them and a delegation's crossing columns are found per lane from the spans, so the work grows with agents times lanes, not with snapshot frames.
Opening the Overview adds one `sessions_refresh` and nothing per frame; hover, focus and the half-second popovers are local component state that publishes nothing, and the delegation lines are measured by a ResizeObserver on the board, not per snapshot.
`web/src/overviewLens.test.ts` owns the order and column rules, and `web/e2e/overview.spec.ts` counts the client events during hovers.
The Issues board (PRD overview-lenses-issues B22) is the same kind of pure function (`buildTasks` in `web/src/projectBoard.ts`), one pass over the checkouts and the tasks; the filter is a pass over its cards.
The issue panel reads its issue once when it opens and on `재시도`, one `issue_detail_request` whose `gh issue view` runs on a worker off `Mutex<Runtime>`, and a later request replaces the slot so only the newest answer lands.
The preview reads an issue at most once per page and never while a read is in flight (`previewRead` in `web/src/issueDetails.ts`); answers are cached in the shell, at most 200 issues, so a card's labels and a reopened panel draw from memory while the next read runs.
Hover, focus and rest on a card send nothing else; `web/e2e/overview.spec.ts` counts the client events on a card's hover and on repeated previews.
The PRs view (PRD overview-lenses-prs B24) is `buildPullRequests` in `web/src/projectBoard.ts`, one pass over the project's pull requests with a pane lookup per row, memoized on the project; the core sends only the pull requests the view shows (the open ones and the merged ones D-52 keeps), cut from the `gh pr list` answer it already holds, so the snapshot grows with that bounded list and the view adds no read.
Its only reads are a pull request's body and feedback when 맡기기 or 새 이슈 만들기 opens (one `pr_feedback_read`, `gh pr view` on a worker), and its only writes follow a confirmation; hover, focus, unfolding and the half-second cards are screen state, and `web/e2e/overview-prs.spec.ts` counts the client events across them.
