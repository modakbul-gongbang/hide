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
| Deterministic regression tests and structural checks | Blank repaint buffers, cache bounds, incorrect state transitions, blocking work in forbidden paths | The repository invariant checks run on every PR and main push in `.github/workflows/pr.yml`, and the Rust suite on every main push and on each PR whose plan includes it (docs/TESTING.md, "Which lanes a pull request runs"); `design-contract.yml` adds static design checks |
| Automated end-to-end | Real window launch, attach to a private Herdr server, IPC between the desktop host and `hided`, occluded-window rendering, packaging mistakes | Playwright drives the web shell in a browser and the desktop app through `desktop/e2e/fixture.ts`; the web suite runs in the four Linux `web-e2e` shards and, for its `@platform` tests, in one Windows job, the desktop app in the macOS `desktop-e2e` job of `pr.yml`, each when the pull request's plan includes it, and `nightly.yml` calls `verify` with every lane and configures the `@platform` web tests on macOS and Windows, the full desktop suite on macOS and its `@platform` tests on Linux and Windows, with tracked flaky tests blocking nightly; Linux desktop uses Xvfb; `verify` requires the pull request's jobs |
| Controlled performance comparison | Warm/cold latency distributions, periodic stalls, CPU/lock contention, sustained RSS, and cost that grows with process uptime | Local matched baseline/candidate measurements (`scripts/web-shell-measure/run.sh`); no checked-in scheduled or required performance job |

The repository-invariant Python tests run in the `policy` lane of `pr.yml`, which every plan includes.
Fixture/replay commands under `scripts/web-shell-measure/` do not become CI gates merely because this guide lists them.
Check the workflow before claiming any of them runs automatically.
The per-OS schema/runtime contracts and the package jobs run in a pull request only when its plan names them; actual executed jobs, failures and skips establish a particular head's coverage.
PR macOS queue comparisons use actual executed jobs (exclude skipped reusable placeholders), recorded SHA/time windows and sample counts, and report run wall time and runner cost separately.
A before/after observational sample with different workloads or little concurrent queueing does not establish the concurrent-PR p90 target.
Hosted desktop automation and private hook fixtures do not establish physical IME, first-launch security prompts or real agent hook behavior.

### Maintenance and review policy

1. Every change runs the applicable CI regression gates; a rendering/performance bug gets a deterministic regression test when it can reproduce the observed failure economically.
2. Changes to terminal drawing, selection, input/scroll routing, snapshot delivery, geometry, focus, or attach lifecycle also require the affected `desktop/e2e/*.spec.ts` or `web/e2e/*.spec.ts` scenario before being called verified.
   When a scenario cannot be automated yet, exercise it manually against the packaged app and record that the check was manual.
3. Changes to caching, scheduling, snapshots, locking, or claims of improved speed/memory require the affected controlled baseline/candidate measurement (`scripts/web-shell-measure/run.sh`) as well.
   Apply it only when making a memory or latency claim; do not impose it on unrelated documentation changes.
4. The change author records those results in the PR's existing Evidence section, and the reviewer checks coverage and exclusions as well as CI.
5. Keep the regression test beside its component, reusable measurement tools in `scripts/`, procedure and comparison policy here, and each run's raw evidence under `agents/runs/`.
   Never turn a one-off trace or historical number into a hardcoded universal latency limit.

## Resident ticks, timers and watchers

Use this ledger when deciding whether a feature can reuse an owner instead of adding another permanent loop.
The source paths are the authority for cadence, admission and cleanup; a duration is a scheduling interval or deadline, never a measured latency guarantee.
The ledger covers core/daemon background schedules, the desktop host's discovery watch and the shared elapsed display clock.
Finite request deadlines, UI debounce/animation timers and the upstream Herdr server's internal schedules are outside this ledger.
An active-worker limit does not cap queue length, retained bytes or all work done by a tick.
The missing bounds and shutdown gaps below describe the current implementation; this documentation change does not add them.

| Work and source owner | Cadence or trigger | Work per wake | Bound and known missing cap | Stop or release |
| --- | --- | --- | --- | --- |
| Session coordinator: `herdr-core/src/session_sync.rs`, `session_sync/coordinator.rs` | Operation tick 250 ms; `agent.list` 1 s; catalog due at 30 s; reconnect backoff 100 ms to 5 s. | Advance pending operations, compare telemetry and publish only changed state or a due catalog; due socket reads run off-lock. | One coordinator per followed server; active-tab recovery allows 8 reads; the coordinator `mpsc::channel` mailbox has no numeric capacity. | `SessionSyncHandle::drop` sends Stop and joins; coordinator exit stops its subscription; loss of the weak runtime owner also ends the loop. |
| Agent sleep: `runtime/agent_sleep.rs`, `agent_sleep.rs` | Minute decision on the local coordinator tick; committed visits update last look. | Inside the minute, a due-time comparison; when due, inspect pane/agent state, persist changed last-look stamps and schedule eligible ends. | At most 4 automatic ends in flight; the full decision scans the current agents and has no separate per-pass agent-count cap. | Decision polling ends with the coordinator; individual end/wake work is finite and separate from this timer. |
| Lineage reconciliation: `coordination/lineage.rs` | Existing one-second native agent refresh, also requested by native events, and startup/reconnect; no new timer or general replica-publication trigger. | Reuse the existing native-agent equality result and append-only registration count; settled observations with no successful write awaiting native confirmation only drain bounded completions and compare the count. A native change, new registration, failed write or outstanding successful write triggers O(registrations + agents) indexed planning and differing-token I/O outside Runtime; a read begun before that write completed cannot confirm it, while a newer read can repair missing tokens. Registered targets are selected across the supplied native list, with at most one patch per registered pane; letter/watch writes and label/process/catalog publications do not trigger planning. | One worker per followed server, one active and one pending batch; 2,048 patches per batch, 4,096 completions and 2,048 cached signatures; one-second API deadline per changed pane. | Drop raises its stop flag, closes admission and joins outside Runtime; cancellation skips the remaining patches after the current one-second API read. |
| Process names: `session_sync/process_info.rs`, `hide-herdr-client/src/lib.rs` | Focus, agent-state or subscription-generation change, or 30 s after a settled read. | Sequential `pane.process_info` reads on one off-lock worker; reject obsolete generations and publish only changed names. | At most 5 attached tabs' focused panes and one worker; each small response has a 1 s read deadline and 64 KiB frame cap. Connection setup depends on the connector; local Unix has no explicit setup deadline here. | Reader Drop cancels between batch reads and joins the current read off-lock; it does not interrupt an in-flight connection attempt or impose a platform-independent batch/join deadline. |
| Label follow-ups and generator retry: `labels/worker.rs`, `labels/generator.rs`, `labels/analyzer.rs` | Changed agent/session; one follow-up at 3 s, unavailable transcript retry at 15 s, standby lock retry at 30 s. | Check retained panes, schedule transcript reads and analyses, drain results and persist label records off-lock. The same read folds an agent's turn records (Codex) into a fixed-size turn tracker kept beside the checkpoint, and the delivery observation reads the overlay the publish already builds; no read, file or process is added for it (PRD codex-plan-approval-hold D-08). | One transcript read and one analysis submission per server; one analyzer executes per Core instance, shared by its followed servers; waiting panes and analyzer `mpsc` have no fixed numeric queue cap. | Coordinator drops its transcript reader and joins it; core analyzer shutdown cancels the provider, answers queued jobs and joins. |
| Capability readers: `session_sync/coordinator.rs`, `ports.rs`, `usage.rs`, `ai.rs` | UI-attached gate; Ports 5 s after completion; Usage initial 1 s, visible-window refresh 5 min or popover opening after 60 s; Background AI probes 30 s with 2 s changed-request spacing; hook diagnosis 1 s while Settings is observed. | Run due `lsof`, credential/network/transcript reads, provider probes or hook diagnosis outside Runtime; ingest changed answers. | Ports are synchronous on the coordinator, which waits on its node's `listening_ports` call with a 10 s deadline on each `lsof`; Claude usage and AI probes each use one `BackgroundRead`; no general aggregate response-byte cap covers this group. | Coordinator ends polling; Usage Drop cancels Claude; generic `BackgroundRead` Drop does not cancel or join an in-flight read. |
| Git facts watcher: `worktrees.rs`, `hide-host/src/git_watch.rs`, `hide-platform/src/watch.rs` | OS changes on the node, reported per repository at least once a second; drained on coordinator wakes; 300 ms quiet period; an ended watch restarts after 30 s. | The node filters Git facts before queuing and reports changed repositories; the core marks affected projects and requests one off-lock worktree read; overflow invalidates watched projects. | First 64 requested projects are watched by one `git_watch` call; 4,096 queued paths become an explicit overflow; one background read; this watch cap does not cap the full project request vector. | A changed watched set starts a new call and stops the old one; dropping the reader stops its call, whose watcher the node drops within one report; a generic in-flight read may finish after the driver ends. |
| Changes pump: `changes.rs` | Pump 250 ms; read on changed input or 2 s after settlement while Explorer, Changes or an active diff needs it. | Take the front checkout request, start one off-lock host read and ingest its scoped result. | One active `BackgroundRead`, current request coalesces changes; host-call deadline 30 s; no additional pump queue of requests. | Pump Drop sends Stop and joins the pump; its generic in-flight host read is not joined by `BackgroundRead`. |
| Kit pump: `kit.rs`, `runtime/kit.rs` | Pump 250 ms; launch/apply intent, or 5 s status refresh while Settings is observed. | Take one local kit job under the lock, run installation/status outside it and ingest the result; devices use their own workers. The agent search stats about 25 shell startup files per pass and starts the login shell (`$SHELL -ilc`, 10 s deadline) only on its first pass, after one of those files changed, or a minute after a failed ask; this Mac's shell answered in 1.2 to 1.9 s, and a re-read with nothing changed starts no process. | One local job at a time and one worker per device; device call deadline 180 s; no fixed global device-worker count cap in this pump. | Local pump Drop sets the child stop flag, sends Stop and joins; a device worker exits when work/connection/weak owner ends, without a retained join handle. |
| Terminal recovery: `terminal_recovery.rs` | 1 s clock; retry delays 5, 10, 20 and 30 s. | Try the Runtime lock, skip a busy lock and advance pending terminal recovery only. | One maintenance worker; at most 4 automatic retries per recovery; retries are not an additional unbounded command queue. | Maintenance Drop sends Stop and joins. |
| Project Memory due poll: `runtime/memory.rs`, `runtime/operations.rs` | 5 s due poll on operation ticks; a session must be unchanged for 60 s before analysis. | Check eligibility under the lock, then discover sessions/read SQLite/analyze off-lock and apply a scoped result. | One poll and one Memory operation at a time; input/storage limits belong to the [Memory cost contract](#project-memory-cost-contract). | Runtime Drop cancels the provider; poll and operation threads have no retained join handle, so cancellation is not a join guarantee. |
| Session search worker: `runtime/session_search.rs`, `hide-session/src/{search,search_read}.rs` | Condvar wake; 5 s wait with no indexing queue or 40 ms while indexing; refresh/retention due at 30 s. | Process control/query requests and bounded transcript indexing chunks outside Runtime. | One latest pending query, 8 queued controls, 2,000 session files per current-project indexing request; at most 8 chunks of 1 MiB per indexing turn, yielding after 20 ms between chunks. | Worker Drop marks stop, wakes and joins after draining already accepted controls. |
| Link record worker: `links/worker.rs`, `links/store.rs`, `runtime/links.rs` | Condvar wake on changed facts or a panel request; list changed session files every 15 s, every 3 s while a panel is open (60 s overlap, a 90-day backfill once); pane spans re-applied every 60 s; each ready device asked at most every 60 s, a failed one left for 60 s; a store that failed to open retried every 15 s; prune every 30 min; summary published at most every 2 s. | Upsert the handed facts, read one queued session file for one 20 ms turn then rest 30 ms, answer the open panel after a write, and publish `link_summaries` or `link_panel` only when the value changed; facts are fingerprinted under the lock, so an unchanged navigator hands nothing. No `git` process and no read under `Mutex<Runtime>`. | One worker per Core and one write connection; the mailbox holds the latest facts of each kind and one latest panel request; one session file read up to 1 MiB per turn, resumed from its cursor on the next; a device answers at most 8 reads per call with a 10 s deadline; the file is capped at 64 MiB (`links_store_full`); the panel carries 200 lines. A list page names at most 2,000 files (newest first, walking at most 100,000 entries); the next page is listed only once the queue has drained. | `LinkWorker::drop` (dropped in `Drop for Core` after the search worker) marks stop, wakes and joins; a turn in progress finishes its one read first. |
| Local Explorer watch: `hided/src/watch.rs` | 200 ms poll of the core's own node and 200 ms coalescing window, or target-change notification; 2 s after a failed poll. | Stamp the focused checkout's watched folders in one in-process `Call::Stamps` and broadcast one changed-folder frame per coalesced change; ignore an overtaken target. | 64 folders via the shared watch selection; one request at a time; 64 broadcast frames. | Clearing the target stops I/O, not the timer; the task ends with the daemon's async runtime. |
| Device Explorer watch: `hided/src/watch.rs` | 2 s or target-change notification; missed interval ticks are skipped. | Stamp the selected visible device Explorer's folders in one awaited helper request; ignore an overtaken target. | 64 folders via the shared watch selection; one request at a time; no queue of periodic helper reads. | Clearing the target stops I/O, not the timer; the task has no separate stop handle and ends with the daemon's async runtime. |
| Pane-capability sweep/bootstrap: `hided/src/pane_auth.rs` | Sweep 5 s; bootstrap on accepted connection. | Sweep expired/unregistered checkout references on a blocking worker and answer admitted bootstrap requests. | 64 capabilities, 8 queued accept slots and 8 active bootstraps; the accept thread can hold one additional accepted stream while waiting to send, and the kernel listener backlog is separate. Sweep workers are spawned without a concurrency cap or join handle. | Shutdown ends the serving loop; `CloseOnDrop` closes its listener; registry revoke/Drop removes references; a started sweep may still finish. |
| Remote workspace bridges: `hided/src/remote_bridge.rs` | 2 s reconciliation; failed-route retry 2 s doubling to 60 s. | Read desired routes off-lock, reuse live routes and start/stop SSH helper channels as routes change. | 8 routes; each owns its channel; the route cap bounds resident bridges, not all independent device-helper connections. | `RunningDaemon::stop`/Drop and supervisor Drop call `stop_all`, closing channels; workers are not synchronously joined there. |
| Browser-route reaper: `hided/src/browser_routes.rs` | 2 s. | Check retained route owners/load identities and release stale routes sequentially. | 12 routes, 4 route builds and 8 file requests per built route. | Shutdown exits the reaper and drains/closes its routes. |
| Daemon idle check: `hided/src/server.rs`, `env.rs` | 1 s check; Herdr probe no more often than 30 s with a 2 s request deadline. | With no clients, check Mobile keep-alive and Herdr reachability before advancing idle expiry. | One serial idle task; default expiry 600 s; keep-alive can disable it; no overlapping probe queue. | Idle expiry signals shutdown; normal server completion aborts the idle task; process/runtime exit also ends it. |
| Mobile reconciliation/retention: `hided/src/mobile/mod.rs`, `mobile/phones.rs`, `mobile/store.rs` | 3 s observation clock; reconcile while enabled Settings is watched, or every 60 s when enabled/removal owed; phone sweep hourly. | Serialize reconciliation and sweep phones not seen for seven days, except live connected phones; changes publish existing Mobile state. | Pairing admits at most 4 phones and refuses further pairing with `phone_limit`; persisted-file loading has no equivalent count or byte validation. One reconciliation executes under the lock, but spawned reconciliation calls waiting for it have no fixed admission cap or shared coalescing flag; sweep work follows the loaded phone vector. | Mobile shutdown signals its stopping channel and removes the owned serve entry after any reconciliation; crash cleanup waits for next startup. |
| Phone connection/detail: `hided/src/mobile/phone.rs`, `mobile/mod.rs` | Live ping 20 s; open pane detail refresh 1 s. | Ping/check silence and read the open detail's pane/conversation. | 2 live connections per phone, 8 queued detail frames, 16 KiB inbound frame, 10 s send bound and 45 s silent limit. | Socket close/revoke/eviction ends its loop; dropping the open detail aborts its detail task. |
| Desktop daemon watch: `desktop/src/main/host.ts`, `spawn.ts` | Health recheck 2 s after each response; lost-daemon status poll 3 s after completion. | One HTTP health read or attach-only `hide status`; discovery replaces the watch. | One watch timer and one CLI child; health deadline 1.5 s, status deadline 5 s; 2 health misses mark lost. | Host quit or new discovery clears the timer; quit kills its in-flight CLI child and leaves detached daemons alone. |
| GitHub project re-read and store: `runtime/projects.rs` (`reread_stale_github`, `read_sighted_pull_requests`), `labels/worker.rs` (`note_sighted`), `github.rs`, `github_store.rs` | UI-attached gate, like the other capability readers: no read and no clock comparison while no window is attached. Then a comparison on each coordinator wake; a project is asked for again 5 min after its last answer, and not while its previous ask is unanswered; one `gh` pass per changed request. A pull request a local session printed is compared on the labels path whether or not a window is attached, inside the lock section that already hands the worker the pull request times, and only when a read found one; its `gh` read still waits for the gate. | Compare the instants of at most 64 projects (`GithubClock`), advance the generation of the due ones, and let the single reader read only those; after an answer that changed, copy the snapshot under the lock and write it on the store thread. Per transcript read, the worker checks each sighting against the times it holds (no lock); per exchange that carries sightings, the runtime looks each up in the times and its remembered set, and scans the read projects' first pull request and the navigator's panes once per new address; each clock comparison also drops the remembered addresses that grew old and the waiting projects no longer read (at most 64 entries). | 64 projects, the one in front and the ones a screen named first (a count past it is logged as `projects.over_limit` when it changes); at most 32 sightings wait in the worker between two exchanges, the newest kept and the rest logged as `read.sighted_capped`; at most 64 owned addresses remembered for 15 minutes or until an answer holds them, one past it not read and logged as `read.sighted_over_limit`; one reader worker; one store thread with one pending slot, so a burst writes the latest; the file is capped at 32 MiB on read. | The store's Drop joins its thread, so a queued write lands before quit; the reader keeps its existing `BackgroundRead` ownership. |
| Factory host: `herdr-core/src/factory.rs`, `hide-factory/src/engine.rs`, `verify.rs`, `project.rs` | One thread per Core waits on its command channel for up to 2 s; a command is answered at once, and the engine ticks at most every 2 s. Without a Factory store the wake does nothing until the first `hide factory` command opens one. Due inside a tick: the outside world every `outside_read_minutes` (2), running CI checks on one commit again after 30 s, the watch and periodic checks every `watch_interval_minutes` (30). | Pump worker letters, apply finished judgments and verification runs, expire questions, probe each running worker, read the outside world when due, start Tasks into free slots, settle sleeps, deliver woken letters and publish the open Factories with their stall windows. After each tick or command it builds the screen's summary and, while a Task page is open, that page's detail, compares them with what it last handed over, and only on a difference takes the lock once and announces once, so an unchanged tick publishes no frame. Every subprocess (`git`, `gh`, verify commands) and every SQLite write runs on the host or verify thread; each runtime call takes `Mutex<Runtime>` for owned data only. | 32 queued commands, one judgment in flight with 16 waiting per Factory, one verify run with 256 queued, one worker start with 16 queued on the starter thread, 16 held letters per woken worker, 256 pending CI commits, 20,000 events and 5,000 Tasks per Factory, worker slots by `max_workers` (5) per machine; one open Task page per Core, the 16 newest answers to screen requests and 64-byte request ids, a longer id refused; kept judgment and letter records grow without a count cap (D-58, each cut at 256 KiB). | `FactoryHost::drop` shuts the engine thread down and joins it, and the engine thread closes the start queue and joins the starter after its running start, dropping the starts still queued; `JudgeThread::drop` cancels the provider and joins; a verify run's child is stopped through its stop flag. |
| Elapsed display clock: `web/src/components/elapsed.tsx` | One shared 1 s timer per window while subscribed. | Update the time and notify elapsed-span subscribers in memory; no core snapshot. | One timer; listener count has no fixed numeric cap and fan-out follows the mounted elapsed spans. | Last unsubscribe clears the timer; window destruction ends it. |

Worktree, GitHub and disk readers also run on changed explicit requests; those requests are not extra permanent timers (`worktrees.rs`, `github.rs`, `disk.rs`).
Their command, walk and storage bounds remain in the feature cost contracts below and must still be reviewed when changed.
Agent registration and spawn records share the durable delivery worker: 2,048 agent records, 4,096 spawn intents, 128 native arguments totaling 8 KiB, the 16 MiB ledger bound and a 64-slot admitted-operation queue.
The shared `BackgroundRead` has one in-flight request and observes the latest desired request at its next poll; only owners that explicitly cancel and call `join_pending` obtain that shutdown guarantee (`reader.rs`, `session_sync/process_info.rs`).

## The spawn guard hook on a shell call

`PreToolUse` with the `Bash` matcher runs `hide-agent-hooks` before every shell call a Claude Code or Codex agent makes, so it is a high-frequency path that every agent pays and no operator sees (PRD herdr-spawn-guard B13).
Per input it adds one process start: the helper reads the payload (at most 256 KiB, waiting at most 0.5 seconds), runs a byte test for `herdr` or `HERDR_BIN_PATH` over the whole payload (its `cwd` and transcript path too, so under a checkout whose path contains `herdr` every call also pays the JSON parse and the lexer, microseconds that start nothing), and exits with no output when neither is there, before it parses JSON, reads a file or starts a child.
A call that mentions `herdr` without launching an agent (`pane split`, `agent list`) pays one JSON parse and the shell lexer, and still starts nothing.
Only a launch in a Herdr pane starts a child: one `hide workspace bootstrap`, owned through `hide_platform::process` with the guard's 2.5 second deadline, a 16 KiB output cap, and an end of the child on success, failure and timeout alike.
The guard keeps no state, runs no timer or worker, takes no lock of the runtime, publishes nothing and fans out no notification; the daemon sees one `bootstrap` request per refused launch, the call `SessionStart` already makes.
The retained data is the refusal log, capped at 256 KiB with one rotation, and the throttled diagnostic store `delivery` already caps; crossing a cap rotates or suppresses, and never refuses a call.
A defect, a missing `hide` or a daemon that does not answer lets the shell call run, so the worst the guard can add to an ordinary call is its own bounded wait, never a refusal.

Measure it at the command boundary: the installed command line run as the runtime runs it (`sh -c`, the payload on stdin), wall time per call from the caller's side, against a process-spawn baseline, with the first calls discarded as warm-up (ten, and five for the real `hide` row).
Report p50, p95 and the load average before and after, and report the idle and driven (launch) paths separately.
The harness is a small script that times the installed command through `sh -c` with the payload on stdin, kept in the run directory that introduced the guard; a release `hide-agent-hooks` on an Apple silicon Mac gave the following, with the machine's load average about 4, so these are not idle-machine floors:

| Call | Samples | p50 | p95 |
| --- | --- | --- | --- |
| baseline `sh -c ':'` (process start only) | 300 | 2.8 ms | 4.3 ms |
| Claude Code, ordinary call (`cargo test`) | 300 | 5.7 ms | 7.2 ms |
| Codex, ordinary call (`cargo test`) | 300 | 5.6 ms | 8.0 ms |
| Claude Code, mentions `herdr`, not a launch | 300 | 5.6 ms | 7.1 ms |
| Claude Code, launch refused (stand-in `hide`) | 100 | 8.9 ms | 12.4 ms |
| Claude Code, launch refused (real `hide` and daemon, from a Herdr pane; 25 calls, the first 5 not counted) | 20 | 10 ms | 12 ms |

An ordinary call therefore pays about 3 ms over the process-start baseline, and a refused launch about 10 ms in total.
These numbers prove the helper's cost at that boundary; they do not prove the agent's own time to a first token, and a runtime that serializes hooks on its own schedule can add more.
A non-`herdr` Bash call was also watched in both TUIs against a private Herdr server and showed no hook output and no visible delay.
Regression owners are `hide-agent-hooks/tests/spawn_guard.rs` (nothing spawned for a payload without `herdr`, a slow `hide` stays under the budget) and the parser tests in `src/spawn_guard.rs`.

## Resident work cost review

Every change adding resident work or touching a high-frequency path must explain its cost in the PR's Review section, even when it claims no performance improvement.
This is a review requirement, not a new CI benchmark or permission to change runtime behavior through documentation.
Apply engineering principle #14 by naming the resource's owner and cleanup on success, failure, cancellation, owner exit and partial startup.
Apply principle #15 by naming admission, pending-work and retained-data caps, the outcome when each cap is crossed, and every cap the implementation still lacks.

Before adding or changing an entry:

1. Name the input/tick, its cadence and source owner, and the existing reader or worker it can reuse.
2. Explain added work per input or tick, including idle and driven paths, lock acquisitions, scans, I/O and subprocesses.
3. Explain notification fan-out and the transition that publishes; unchanged observations must not create frames merely because a timer fired.
4. State the pending-work bound, retained-byte/count caps and overflow/refusal behavior; distinguish a timeout or active-worker limit from a queue cap.
5. State how work stops on every exit path and which admitted reads may finish after cancellation; detached work needs an explicit bounded lifetime.
6. Update this ledger and the [state/process ownership guide](ARCHITECTURE.md#state-placement-and-publication), then record applicable regression checks and any matched measurements in Evidence.

For example, a reader with one active job and one latest pending request has bounded pending work only if its request/answer sizes and owner count are bounded too.
If code lacks a bound, write that limitation in Review and the ledger; do not invent one or quietly alter behavior in a documentation-only change.
The [maintenance policy](#maintenance-and-review-policy) decides when actual app scenarios and controlled baseline/candidate measurements are required.
Unrelated documentation changes need focused documentation/invariant checks and must not claim measured performance parity.

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

`verify-cargo.sh release` builds the release `hided`, `hide`, `hide-host-helper` and `hide-agent-hooks` binaries; release `hided` embeds `web/dist`, so run `pnpm --dir web build` first.
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
| `HOME` | A private home under the run directory, including kit and retirement state |
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
Do not repurpose the shell's own `HOME`; pass a private HOME explicitly to fixture children, and do not assume a private local socket disables SSH discovery.
Inspect remote registrations, automatic SSH connection attempts, and remote client state too; an unexpected remote connection is an isolation failure to resolve before interacting.
SSH devices are registrations kept under the daemon's state directory (the operator's is `~/.hide/state`), so a run-owned `HIDE_STATE_DIR` is what keeps a scenario that does not exercise remote behavior from making an SSH connection attempt.
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

A `hided` with no window is a separate idle condition, and it is the daemon's resident state: it lives as long as its Herdr server answers (ARCHITECTURE.md, The daemon lives with its Herdr), so it runs while the app is closed.
With no web or desktop renderer attached the core's coordinator rests every reader that only feeds a window (foreground processes, provider usage, hook diagnosis, ports, worktrees, GitHub, disk and the Background AI probe), and the root follower reads and sends no screen snapshot while no client is connected, catching up with one read when the next client arrives; only label work runs, and it is bound to agent state changes (`herdr-core/src/labels/`).
Report it apart from a daemon with a window attached, with the agent count and workload recorded for each, and check that no snapshot is read or sent and none of those readers runs while no agent state changes.
A change to the coordinator's readers or to the root follower has to keep this: a new reader that feeds a screen belongs in the gated block of `run_coordinator`, and `ui_attached` is sent by the daemon only, never by a window.

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
  Extend `PrecomputedCatalog`, `CatalogCache`, and `PathIndex` (`herdr-core/src/session_sync.rs`, `herdr-core/src/workspace.rs`) rather than adding per-tick or per-tab git calls; stale precomputation keeps the accepted catalog.
  The coordinator asks the core's own node about every path a rebuild reads in one `Call::PathFacts` (at most `PATH_FACTS_LIMIT` paths per call) before it takes the lock, and reuses the answer for the same paths until the 30-second catalog refresh, so a burst of session updates over the same panes asks the node once.
- Keep the session-sync thread that applies Herdr's events free of blocking work (PRD instant-pane-topology D-13).
  A socket read that opens its own connection goes to the sync-read worker (`session_sync/sync_reads.rs`, one read of each kind in flight), a subprocess, a node read that runs one or an HTTP request to a `BackgroundRead` reader (`herdr-core/src/reader.rs`), and a file write that ends in an fsync to its store's own thread (the label store); the work's output and interval stay what they were.
  An event that arrives while one of them runs is applied at once, which the topology scenario's churning agents exercise.
- Keep a publish's cost independent of the agent, tab and checkout counts when the catalog's inputs did not move (D-18).
  The catalog is rebuilt only on changed registrations, spaces or worktrees or its refresh window, the purpose mirror syncs only when that catalog or the unconfirmed created purposes changed, and a path is resolved by the node only when the set of paths asked changes (above), never by the core per publish; an agent status change repeats none of them.
  Measure a change to this at both scales of the topology scenario, idle and with its agent churn, against a baseline: idle CPU and RSS must not grow, and a status-change publish must not double when the agents do.
- A drawn-ahead change adds one publish, a bounded line (`GEOMETRY_QUEUE_LIMIT`, `INPUT_HOLD_LIMIT_BYTES`) and one `pane_op.timing` record per operation, and nothing per frame or per tick.
- Announce once per burst and clear the `ChangeNotifier` (`herdr-core/src/handle.rs`) latch before taking the snapshot lock.
  Read-then-clear can swallow a concurrent change.
- Size snapshot traffic by changes: terminal sequence cursors, rarely-changing revisioned `rest`, and per-event scalars.
  An unused heartbeat timestamp can still dirty `rest` and resend the full navigator every second.
- Keep the keyboard path to the byte bridge.
  The one thing added per key is the operator-submit check (`labels::input::key_submits`, PRD overview-request-view D-19): a chunk over 64 bytes is skipped, a shorter one is decoded into a stack buffer and scanned; only a found submit looks up the agent row and pushes one entry behind the submit record's own lock, and nothing is published.
- Keep async operation records bounded by active intent and conflict scope; a tab's geometry operations wait in its line, one with Herdr and at most `GEOMETRY_QUEUE_LIMIT` behind it.
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

Published figures and their measurement boundaries live in the dated [performance observations](PERFORMANCE_RESULTS.md).
That report is an observed candidate result, not a new threshold or a replacement for the measurement contract here.

`scripts/web-shell-measure/run.sh` measures the product `hided` the way the S0 spike measured its prototype, so the numbers stay comparable to the S0 spike baseline.
It owns every process it starts: an isolated pinned Herdr server on a socket inside a run-specific mode-0700 directory under `/tmp` (`isolated-env.sh`, the same routing table as section 3), one linked Git checkout and workspace with a `stty -echo -icanon; cat` pane, the release `hided` with its embedded `web/dist`, and one Google Chrome with an automatically assigned CDP port on the page opened with `?probe=1`.
The runner reads Chrome's CDP port from its own profile and sends the page URL over standard input so the token is absent from the Chrome command line.
The operator's socket is only read, before and after, for the topology counts written beside the results.

Echo (PRD B8) is three trials of fifty `herdr pane send-text` markers.
`t0` is the CLI return timestamp and `t1` is the xterm write completion that first shows the marker in the parsed buffer, read through `window.__hideProbe`; the sample is `t1 - t0`, nearest-rank percentiles, the median of the three trial p95s against the S0 spike baseline p95 plus 5 ms.
The baseline is reused from the S0 report rather than re-measured, and `summarize.py` carries it as a named constant so a rerun does not quietly move it.

Frames (PRD B12) is one 120 s window with the pane printing a line every 8 ms while a `requestAnimationFrame` loop injected through CDP records every frame's `dt`; the result is the fraction of frames over 16.7 ms, the WebSocket frame count the page received during the window, and the pane tail that proves the driver ran.
This is a live driven pipeline, not the in-page replay the spike used: a replay mode would put spike code into the product, and the threshold is absolute, so the live run is the stricter measurement.

Run it as `HIDE_MEASURE_RUN_DIR=agents/runs/<slug>/measure/<attempt> bash scripts/web-shell-measure/run.sh` after `bash scripts/verify-web.sh web build` and `bash scripts/verify-cargo.sh release`.
For an unattended comparison, append `--isolated-headless`: Chrome has no native window and the runner records that operator topology observation was skipped, without contacting the operator socket.
Use that same browser mode for baseline and candidate, and report it with the results; a headless measurement does not prove native presentation.
`MEASURE_SCENARIO=areas2` or `areas3` shows two or three Agent tabs through the real area menu, each with one pane.
All shown panes receive the same line-every-8-ms driver; the measured pane also receives fifty echo markers during that load, recorded in `echo-driven-summary.json` separately from its idle echo trials.
`resources-idle.json` and `resources-driven.json` record twenty one-second CPU-time deltas and RSS sums for the owned hided, Herdr and Chrome process trees, independently of the echo and frame samples.
For an RSS comparison, append `--memory-series` after `--isolated-headless` to extend the driven window to ten minutes and collect eleven one-minute process-tree RSS samples in `memory-series.json`.
A first run opens on Main (PRD S6), so the harness uses the Projects sidebar to open its linked fixture checkout before measuring.
The fixture workspace reports the checkout's `hide_owner` token before startup so the core reuses the prepared pane rather than creating a different workspace.
The Projects control is a tab, so readiness reads `aria-selected`; opening the fixture is one click after its checkout row appears.
Cleanup removes the private socket's label-generator lock as well as its socket before removing the owned socket directory.
The run directory keeps `identity.txt` (head, dirty count, binary hash, Herdr and Chrome versions, load), `echo-*.json`, `frames.json`, both summaries and the owned PID status at cleanup.
The harness resolves that directory to an absolute path before starting child panes, so their private HOME, state, and checkout paths remain valid after the pane changes directory.
`MEASURE_SCENARIO=multi` is the S2 shape (PRD web-shell-pivot-s2 D-08): the measured pane shares its tab with four splits and four more tabs are shown once each so the core holds five attached tabs before the shell returns to the measured tab; `page.json` records the pane, split and tab counts, the attached pane ids and the live xterm instances the run started from (every attached pane keeps its instance parked while its tab is hidden, D-05).
`memory.py` samples resident memory twice, after the shape settled and after the driven window (`memory-settled.json`, `memory-after-frames.json`): the Chrome process tree's RSS sum, hided's RSS, the page's JS heap and its live instance count; the two samples say whether the parked instances grow the tab under load, which is the D-05 revisit trigger.
The driver and the marker still go to the one measured pane, so the other panes are idle shells with mounted xterm instances, and the gate is the same as the single-pane run.
A Chrome window opens on the desktop for the run; the loop throttles in an occluded or minimized window, so leave it visible and report the load recorded beside each trial.

### Pane topology latency

`MEASURE_SCENARIO=topology` measures how long a split, a zoom and unzoom, a pane close, a new tab and a tab switch take to reach the screen (PRD instant-pane-topology D-15, B23).
It adds a second tab to the measured workspace, and with `MEASURE_SCALE=operator` or `double` `scale.sh` first fills the private server to that scale (D-18): 43 workspaces, 62 tabs, 66 panes and 30 agents of which 5 print a line every 50 ms, or twice each (86, 124, 132, 60, 10); every other workspace is its own Git checkout, agents are reported with `herdr pane report-agent`, and while the run lasts one idle agent changes state every two seconds, the agent-status-only publish D-18 prices.
The run records `resources-idle.json` before anything moves, one echo trial under the agent churn (`echo-summary.json`, the typing half of B24), and `topology.mjs`'s rounds (`MEASURE_TOPOLOGY_ROUNDS`, 20 by default), then skips the frame window.
Keys are CDP `Input.dispatchKeyEvent` events, which Chrome delivers to the focused terminal as an ordinary renderer keydown, so each chord takes the web shell's own keydown path and its browser chord; a tab switch is a click on the tab.
Every time is on the page's clock from the chord's first keydown (or the click): `screen_ms` is the first animation frame whose DOM shows the change (the canvas's pane count, its zoom flag or the shown tab), and `frame_ms` the first frame after it in which the terminal the change is about shows it (text in a new pane, a new grid in a resized one).
`topology-summary.json` gives each operation's p50, p95 and max to screen and to frame.
`MEASURE_HIDED_BIN` points the run at another hided, such as a baseline built from the merge base, so baseline and candidate share the fixture; run both headless (`--isolated-headless`) under the same scale and report the load beside each.
Herdr's share and Hide's are read from the candidate's `pane_op.timing` lines in the run's `hide-state` diagnostic log (Drawn ahead of Herdr in [ARCHITECTURE.md](ARCHITECTURE.md#drawn-ahead-of-herdr)), which the baseline does not write.

## Scoped browser gateway discovery

Browser inventory area identity is projected in the existing changed-layout generation pass, one visit per area and display, with no additional notification or timer.
Each sync also compares borrowed identities for at most 256 saved Workspace layouts against the current connected checkout catalog, with no allocation when the scope is unchanged.
A scope transition advances that same generation and rebuilds the inventory; removed or disconnected checkouts retain their saved layouts but retain no native page authority.
Positive area scopes are collected in that same pass (at most 256 Workspaces times six areas), with no allocation or extra notification on unchanged generations.
Changed generations build one bounded map of the previous scopes to retain their incarnations; missing scopes receive the current generation on regrant, so coalesced revocations cannot preserve old capabilities.
Session and catalog mutation boundaries use that same borrowed comparison and immediately discard revoked scope incarnations; only a scope transition allocates a bounded checkout set and advances the generation, without a tree reconcile or another notification.
The daemon's gateway registration retains at most four app process identities; each discovery or action checks those bounded identities outside the Runtime mutex.
Discovery and browser actions share eight admission permits; exceeding the cap reports `browser_control_busy` rather than queuing more work.
Discovery does loopback HTTP outside the core owner thread, with no proxy or redirect, an eight-second deadline and a 16 KiB answer cap.
Browser creation reuses Workspace prepare/read/commit and its existing retry-record cap; checkout file reads remain outside the Runtime mutex.
Idle pages add no discovery work, and ordinary terminal input, snapshots and tab selection do not start gateway requests.

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

Each opening of a local Git project's Overview requests a background worktree and pull request read for that project; closing it adds no Git command.
The previous catalog remains visible while that read runs.
An accepted changed worktree answer announces its facts and loading completion after releasing the runtime mutex, even when the following catalog rebuild leaves pane topology unchanged; unchanged or rejected answers announce nothing.
The open-dialog regression in `web/e2e/worktree-delete.spec.ts` checks lock recovery, loading completion and repository-list A-to-B-to-A consent on the existing renderer connection.
`behind_upstream` rides the same `rev-list --left-right --count @{u}...HEAD` call that already counted unpushed commits, so a fetched-side count costs no extra process, and `created_at_unix_ms` is one `stat` of the worktree's gitdir in the same background pass off the mutex.
The catalog pass is bounded by the worktree count; a project with many worktrees pays one status, one rev-list and one stat per worktree per change, never per tick or per agent update.
A Git HEAD, index or ref change is scoped to its own repository: the OS watcher groups a burst into one generation change, so that project alone is re-read and every other project is answered from the worker's last read.
Idle repositories do not run Git commands or sample working-tree files; content-only edits are reflected when the Overview opens again.
`a_commit_in_one_project_does_not_rerun_status_in_another` and `idle_and_working_tree_edits_do_not_reread_but_manual_refresh_does` own these boundaries.
A finished worktree removal follows the same scope: its row leaves the catalog under the lock with no Git call, the coordinator rebuilds the rows on its next wake, and the reader re-reads only the removed worktree's repository; `a_finished_removal_drops_its_row_at_once_and_an_older_read_cannot_bring_it_back` owns this.
The linked-worktree facts pass also measures ignored repository boundaries off the runtime mutex once for each linked worktree in an accepted project read.
Each scan has hard caps of 2,000,000 steps, 30 seconds and 1,024 names.
The total project-read cost scales with its linked-worktree count; there is no shared project-wide scan step, time or name cap.
It skips Git metadata and directory links; a failed or capped scan publishes an unavailable fact rather than an empty list.
A confirmed deletion uses the existing single removal slot and one preflight worker before any pane close; the same host check runs again before guarded removal.
No scan is added to pane input, a snapshot tick, hover or an unchanged catalog read, and only phase transitions notify the shell.
Every `git` the catalog runs is bounded by `GIT_DEADLINE` (15 s) and drained off-thread past the pipe buffer; a repository that outruns it reports its status unavailable and a `git.deadline_exceeded` diagnostic rather than holding the other projects' answer, which a status over evicted iCloud files once did for minutes.
Group ordering, chips and search are pure functions of the accepted snapshot; agent status updates redraw rows and never recompute the catalog.
List rows use the existing lazy-loading and search keyboard patterns.

Disk reuses `DiskReader` (`herdr-core/src/disk.rs`), triggered by opening Git/Overview or explicit refresh, with one inflight read and coalesced pending input; the walk itself runs on the checkouts' node (`hide-host/src/disk.rs`, the `disk_usage` call), which reports each checkout as it finishes.
The filesystem walk counts `st_blocks * 512`, partitions nested checkout/shared-Git roots by longest ownership, and deduplicates `(device, inode)` across components.
It counts a symlink's own allocation without following it and rejects alias roots rather than escaping the declared boundary.
Each checkout root has its own bound of thirty seconds and one million visited/pending entries; a root that runs out or cannot be read has no total and remains visible beside a confirmed subtotal, and never takes another root's answer with it.
A whole read has its own cap too, ten million visited entries and five minutes, and only files with more than one link enter the `(device, inode)` set (capped at one million); the roots the read did not reach before that cap stay unavailable, each with a `disk.measure_failed` diagnostic carrying its checkout and reason code.
A project with some unmeasured checkouts shows the layer subtotals of the measured ones only, beside `confirmed_bytes`, and no total.
Each finished root is handed to the coordinator as soon as it is measured, laid over the previous answer, so a large project fills row by row; the whole answer replaces the partial ones when the read ends.
The same walk sorts a checkout's blocks by layer (`hide-host/src/disk_layers.rs`): the ignore rules (`hide_host::index::IgnoreRules`, the rules the file index uses, global excludes file included) name the candidate folders, and a signed `CACHEDIR.TAG` or a known ecosystem's marker file in the same parent vouches for a build cache or dependency folder; a candidate holding another repository, a link, or no proof is `other`.
Nothing here runs a `git` process or reads a file beyond a directory listing, the `.gitignore` of each source folder and one tag per candidate, and the free space of the volume comes from one `statvfs` per read.
This is allocated disk accounting, not physical reclaim estimation for APFS clones.
Opening or refreshing replaces the measurement; no timer, hover or per-row subprocess measures disk.

Cleanup uses the existing action worker context with one active review/removal, never the Runtime mutex, for Git, disk and fresh schema-decoded Herdr snapshots.
Cleanup protects both launch `cwd` and current `foreground_cwd` from the generated snapshot contract; it does not change navigation projection policy.
Review reads are non-mutating; confirmation rechecks each target immediately before `git worktree remove` without force.
Git and Herdr have no shared atomic filesystem transaction: a state change after the last Herdr check cannot be reserved against by this contract.
The UI therefore describes a fresh eligibility check rather than a permanent unused guarantee; Git independently refuses dirty or locked removal.
A stale, missing or failed check is caller-visible and never becomes permission to delete.
Completed intents are retained until dismissal; duplicate confirmation does no work, and retry through a fresh review excludes already removed targets.

The disk cleanup names a project (`cleanup_review {workspace_id}`) and shares that one lane: one active review or run per daemon, published on the reviewed project's snapshot, never under the Runtime mutex.
What is in use is read on the worker when the review opens and again when it is confirmed: the checkout agent summary the lock already holds, one `pane.process_info` per non-agent terminal pane of the project (capped at 256 panes; past it the read fails rather than guessing), and one `lsof` listener sample (each `lsof` stopped at ten seconds, which fails the read closed); a read that fails leaves the review with a `usage_error` and nothing selectable.
Every move is judged from a fresh read: the checkout's facts are copied again under the lock and its panes and ports are read again for each worktree and for each checkout's cells, after the folders were judged and right before they move, and a registered checkout the runtime has no facts about reads `unverified` and loses nothing.
The tracked-file check lists the whole index once and compares by case- and Unicode-normalized spelling, so no argument list can overflow and a folder the file system spells differently from the index is still found; the repository look shares one 20 million entry, 300 second budget across the run.
While a worker runs the daemon takes no dismissal, no second review and no confirmation, and the per-move reads copy only the checkouts' in-use facts under the lock, and a worker that unwinds publishes a terminal `failed` or `complete` snapshot and frees the lane.
The wire carries codes and sizes only: `result` is `removed`, `skipped` or `failed` with a `result_code` and `bytes`, and the English reasons stay in the diagnostic log.
The run waits only for the trash entries it created.
The in-use answer is published before the slower worktree eligibility checks (Git per worktree, GitHub only for a branch not already an ancestor of main), so build caches can be chosen while those run.
A confirmation moves each chosen folder into `<git-common-dir>/hide-removed` with one rename after judging it again from the files (still ignored, still vouched for, no link on its path, no repository inside, no tracked file from one `git ls-files -z` per checkout), then the existing trash sweep deletes and the run waits for the trash to empty before it reads the volume's free space; the core decides which worktree and folder may go and the checkouts' node does every read and move (`hide-host/src/cleanup.rs`: one `judge_folders` call per checkout that spends the run's shared nested-repository allowance, which the core carries from one checkout to the next, one `set_aside_folder` call per folder, one clean `worktree_remove_clean` per worktree and one `drain_trash` call); the confirmation is accepted once because it moves the phase from `review` to `removing` under the lock.
Regression owners are the `live::cleanup` tests for in-use reads, exclusion codes, cell rechecks (`a_folder_that_changed_after_the_measurement_is_kept_and_named`), repeat convergence and the single `ls-files`, and `a_cleanup_confirmation_only_selects_what_the_review_allows_and_runs_once`.

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
While a Project is named, immutable rows and the open transcript are shared pointers with O(1) unchanged comparisons and capture under the lock; the history is resent only when it changed; a re-read that finds the same conversation keeps that pointer, and the worker, not the lock, compares the two.
The core keeps each Project's last rows so a session whose file went away stays listed, and hands them to the next read's worker by pointer; the worker checks the file of each row the catalog no longer lists, so the lock does no file I/O, and what is kept grows only with the sessions deleted while the daemon runs.
Metadata filtering stays local; debounced body queries and provider changes send one scoped event to the owned search worker.
Progress/result transitions use the independent `session_search` section, so indexing never resends the full history, editor or navigator merely to change a counter.
Each source operation combines at most 1 MiB of transcript and hash reads, staging oversized structural parsing, newly consumed prefix hashes and append validation across durable turns.
An unchanged completed source stamp reads zero transcript bytes and performs no hash or SQLite write.
A worker turn handles at most eight chunks and yields between chunks after 20 ms, with SQLite progress deadlines of 500 ms for mutations and 150 ms for retrieval.
The 30-second refresh interval starts after a completed pass, and unchanged answer snapshots publish no notification.
Measure backfill, unchanged and append workloads separately: actual source bytes per operation and total, query latency, notification count/WS payload sizes, idle and driven CPU/RSS, and terminal input-to-write timing.
Include a 2,000-row named history while indexing and a broad query with hundreds of matches in one session; a progress deadline alone proves neither end-to-end latency nor responsiveness.
Regression owners additionally include `hide-session/tests/search.rs`, `runtime::tests::session_search`, `runtime::tests::snapshot_delta`, `web/src/store.test.ts`, and the server/session browser and native E2E flows.
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
The core issue projection uses accepted catalog and metadata only; it schedules GitHub work on a changed selected reference, board open, project selection or explicit refresh; the one timer it adds is the re-read of every local Git project in the ledger above, five minutes after a good answer and 30 seconds doubling to five minutes after a failed one.
The GitHub reader keeps its existing single worker and per-project generation cache.
A project's pull request read is two concurrent `gh pr list` calls under the same 15-second limit, so it adds one short-lived thread per read and no timer; a read asks for the checks of the open pull requests only, which took it from 11 - 14 seconds to about 5 on this repository (2026-10-05, same Mac and `gh`), and the cache keeps at most one check result per listed pull request.
Each read's `pull_requests.ok`, `pull_requests.empty` or `pull_requests.failed` diagnostic carries `duration_ms`, which is where a slow read is found afterwards.
Tying a pull request to a checkout compares the checkout's already-held HEAD with at most 200 listed pull requests under the lock, with no Git process or file read.
At most 200 linked identities and backlog entries are retained per project; one extra list result reports overflow.
Closed and cross-repository identities are resolved in one bounded query.
Manual writes use the existing task-operation slot and a terminating worker; cleanup shares the bounded purpose mirror queue.
Manual acceptance in the desktop app includes empty-checkout entry, overlay dismissal, both groupings, issue linking, stale facts, narrow widths and mixed Korean/English labels.
`runtime::tests::issues` owns projection memoization, stage priority, deduplication, issue precedence and transition-only refresh regressions.

The Overview's lens tabs (PRD overview-lenses-tiles-agents B27, B32) are pure functions of the snapshot the page already holds (`web/src/overviewLens.ts`), memoized on the projects, agents and devices they read; one pass over the scope's agents buckets them, so the work grows with agents, not with snapshot frames.
The Agents graph (PRD agents-graph-view B38, B39) is the same kind of pure function (`buildGraph` in `web/src/agentGraph.ts`): columns, bands and line routes are computed from row counts and the `--graph-*` tokens read once, never measured from the page, memoized on the projects, agents, folds, selected box and filter it reads.
A snapshot that does not change the graph causes no relayout and zero `requestAnimationFrame`: `graphTargets` yields an equal map, `web/src/graphMotion.ts` starts no timer or frame, and the canvas's `data-graph-revision` does not move, which is what an idle check reads.
The glide over 320 ms runs only when the model changes, one frame loop painting positions straight into the DOM without a React render, and the dashes flowing along a working line are stepped by a timer a few times a second (`Flow` in `graphMotion.ts`; a CSS animation of the same dashes cost about a tenth of a core natively, and about three percent even stepped in CSS, because it keeps the frame pipeline running at the display rate), so they are measured idle against driven: the same screen with the flow off against the screen with the flow on, load and workload recorded for each (PRD D-26: at most 1 percentage point more renderer CPU over a 20 second mean with 20 agents and 6 flowing lines, and the relayout of 20 agents within 8 ms), on the native desktop app, since a headless 60 Hz frame gate has failed on a blank page.
Opening the Overview adds one `sessions_refresh` and nothing per frame; hover, focus, filter changes and the half-second popovers are local component state that publishes nothing.
`web/src/agentGraph.test.ts` owns the column, band, routing, fold and filter rules, and `web/e2e/overview.spec.ts` counts the client events during hovers.
The Issues board (PRD overview-lenses-issues B22) is the same kind of pure function (`buildTasks` in `web/src/projectBoard.ts`), one pass over the checkouts and the tasks; the filter is a pass over its cards.
The issue panel reads its issue once when it opens and on `재시도`, one `issue_detail_request` whose `gh issue view` runs on a worker off `Mutex<Runtime>`, and a later request replaces the slot so only the newest answer lands.
The ⌘K GitHub search is one `github_search` event per query the operator commits, never per keystroke and never on a timer; its `gh search prs` and `gh search issues` calls (two calls however many projects, at most 20 `--repo`, 15 s each, after one `gh repo view` per project the reader has not read) run one after another on a single worker off `Mutex<Runtime>`, one search at a time with a newer request replacing the one waiting, and it adds only the `issue_work.search` slot (at most 20 pull requests and 20 issues) to the revisioned `rest` section; `runtime::tests::issues` owns the lifecycle and `github::tests` the allowlist and parser.
The preview reads an issue at most once per page and never while a read is in flight (`previewRead` in `web/src/issueDetails.ts`); answers are cached in the shell, at most 200 issues, so a card's labels and a reopened panel draw from memory while the next read runs.
Hover, focus and rest on a card send nothing else; `web/e2e/overview.spec.ts` counts the client events on a card's hover and on repeated previews.
The PRs view (PRD overview-lenses-prs B24) is `buildPullRequests` in `web/src/projectBoard.ts`, one pass over the project's pull requests with a pane lookup per row, memoized on the project; the core sends only the pull requests the view shows (the open ones and the merged ones D-52 keeps), cut from the `gh pr list` answer it already holds, so the snapshot grows with that bounded list and the view adds no read.
Its only reads are a pull request's body and feedback when 맡기기 or 새 이슈 만들기 opens (one `pr_feedback_read`, `gh pr view` on a worker), and its only writes follow a confirmation; hover, focus, unfolding and the half-second cards are screen state, and `web/e2e/overview-prs.spec.ts` counts the client events across them.


### Terminal path links

`web/src/terminalLinks.ts` reads at most three rows on either side of the hovered row and retains the existing maximum of sixteen joined spellings per token.
Each spelling offers its original path and at most five context interpretations, so a token has at most 192 logical cwd/root lookups, at most 160 beyond the original spellings.
The six stages preserve raw and symbol-only spelling, grammar-only removal from each with literal leading/closing punctuation, the cleaned literal location spelling, then the parsed location.
Identical text/target stages deduplicate.
These are finite punctuation, Korean grammar and location interpretations; arbitrary Hangul is never removed character by character.
The selected range maps UTF-16 slice boundaries onto the buffer's glyph cells, retaining both halves of a wide glyph and refusing a cut inside a combining cell.

`web/src/terminalLinkProvider.ts` spends one invocation's budget on originals before inferred spellings.
Deduplication and fresh cache hits precede admission: at most 512 new unique logical paths reach the desktop host, in batches of at most 64.
A budget overflow records `path_budget_exceeded` with unique lookup, cache-miss, limit and skipped counts, without output text or paths.
A failed or skipped higher-precedence candidate leaves that group unresolved instead of selecting an unproven shorter spelling.
The native host distinguishes confirmed `ENOENT`/`ENOTDIR` absence from other filesystem failures, which reject the batch and reach the existing count/reason diagnostic without paths.
Answers retain the existing ten-second TTL and 512-entry cache cap.
An invocation holds its own bounded answer set while the shared cache is pruned, so a cache eviction during resolution cannot change that invocation's result.
There is no new input, render, snapshot, notification or remote-filesystem work; URL and OSC 8 routing and native path authority are unchanged.

Regression owners are `web/src/terminalLinks.test.ts` (finite grammar, per-token bounds and cell ranges), `web/src/terminalLinkProvider.test.ts` (original precedence, missing/failed/over-budget checks, batching, cache and device boundaries), and `desktop/e2e/terminal-links.spec.ts` (real file opening and native path/link interaction).
For a matched dense-line comparison, record grid columns, token count and fixture spellings, then measure cold and warm hover-to-pointer latency separately with the same window and input sequence.
Count logical candidates, cache misses and IPC batches separately from actual native `realpath` and `stat` invocations; missing paths perform no successful stat, and cache hits perform neither.
With the existing `probe=1` QA seam, the provider records count-only `path_probe` diagnostics in the bounded diagnostic log.
Native IPC unique paths count admitted cache misses, not all logical candidates; a warm hover can have logical candidates while sending no host requests.
The original baseline has no count diagnostic, so record that limitation and cross-check its logical candidates with its original parser on the identical fixture cells.
Capture the exact candidate PID/window without activating it, and distinguish a renderer pointer/file-opening observation from an operating-system handler replaced by a recorder.
