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
The latency summarizer's Python tests, browser-host Node tests, and fixture/replay commands do not become CI gates merely because this guide lists them.
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

Check the pinned runtime's path behavior when updating it.
The tested session layout stores sessions under `<XDG_CONFIG_HOME>/herdr/sessions/<HERDR_SESSION>`; changing `HERDR_CONFIG_PATH` alone does not isolate session data.
The client socket inserts `-client` before `.sock`; allow room for that suffix in the platform's Unix socket path limit.
Do not repurpose `HOME` or assume a private local socket disables SSH discovery.
Inspect remote registrations, automatic SSH connection attempts, and remote client state too; an unexpected remote connection is an isolation failure to resolve before interacting.
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
Core diagnostics are mirrored in the app state directory's `Logs/core.jsonl`, with one previous 1 MiB file; copy both into the run evidence before rotation loses the relevant window.

### Memory and renderer replay

For an RSS claim, take eleven one-minute samples across ten minutes per build and retain process lists, endpoints, range, and median.
Keep pane count, occlusion, warm-up, and workload comparable; memory pressure can lower RSS without an allocation improvement.
An isolated renderer replay can identify repeated row preparation, retained generations, and eviction spikes, but cannot establish native input latency.
Replay the same captured frames on both revisions, use optimized builds, and report cold draw, warm distribution, retained-entry peak, and bulk-release events separately.
A cache that stays bounded may remove periodic destruction spikes while leaving median/p95 unchanged or slightly higher; report that result as it is.

## 6. Preserve the architecture while fixing the cause

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
