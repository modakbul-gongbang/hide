# Agent delivery and inactivity watches

Hide's core owns agent registration, spawning and lineage, the single mailbox and inactivity watches for local and connected device agents.
The operating contracts are [PRD A](../agents/prd/hcoord-delivery-v2/prd.md) and its final coordination and retirement decisions in the local `agents/prd/hcoord-retire/prd.md` contract.
Human Inbox UI, relay/escalate, authority proof and automatic draft clearing remain follow-up work.

## Commands and caller identity

These commands need the running daemon and a current agent pane in a registered checkout; they work without an open renderer.
The daemon binds the caller through the existing Workspace credential boundary and resolves the actual pane, provider and native-session identity from Herdr.
A pane hint cannot replace that binding, and an absent, ambiguous or changed occupant returns an explicit error.
Each queued command revalidates both its original capability caller and the agent pane against the prepared Workspace and checkout context before applying or saving; a moved checkout or newly narrower caller binding returns `caller_context_changed`.
Mailbox callers and new recipients require a positive native-session binding; a missing binding returns `native_identity_required`.
Two missing native references in the same pane never authorize retained mail.
Target names and pane IDs resolve against the daemon's current observations.
A connected device recipient uses the existing reverse-forwarded Workspace bridge; the capability fixes its pane, sender and kind, and the only ledger remains on the controlling daemon.
A disconnected device hook finishes within its two-second budget with no letters, leaving them pending in that ledger.

```sh
hide request send child-name --intent task-question-1 --body 'Please report the check result.'
hide request reply letter-1 --intent task-answer-1 --body 'The check passed.'
hide request show letter-1
hide request ack letter-1
hide request cancel letter-1
hide inbox
hide watch start child-name
hide watch list
hide watch stop watch-2
hide request send parent-name --kind report --intent task-complete-1 --body 'The check passed.'
```

The recipient can acknowledge or reply; the sender can cancel.
Acknowledgement changes the receipt state, and a reply separately closes the original request's answer wait.
Retry the same intent after an interrupted call: the same sender identity and intent return the existing letter during its retention period, including after cancellation or delivery.
Use a new intent for a new letter.
The envelope identifies the sender and letter kind; it is not a session-level authority or anti-forgery proof.
The current first-line format is `Hide letter <id> from <name> (<agent>) [<kind>]`, with independent writer and transcript-reader examples in [delivery-envelope.json](../contracts/delivery-envelope.json).
A batch keeps the first letter's sender attribution in the request view; neither later headers nor older delivery formats select a sender.

## Safe intake and manual fallback

A doorbell carries only a short instruction to read `hide inbox`.
It requires a current idle/done occupant, 30 seconds since Hide last routed a key to that pane, positive native readiness, a positively styled empty composer, and a recognized footer structure.
Working agents, drafts, uncertain menus and unknown layouts leave the letter pending.
The adapter never copies, clears or restores a draft.
Its bounded visible ANSI read preserves the styling that distinguishes a placeholder from identical typed text.
A changed TUI that cannot be classified safely falls back to manual intake.
Each letter has at most three durable doorbell reservations, including successful input and attempts interrupted before confirmation.
The reservation is persisted before input, and the adapter repeats the native/composer inspection after the persistence wait.
A crash or changed composer after reservation may consume an attempt while leaving the letter pending.
Legacy records with a successful bell but no total count conservatively have no automatic attempts left; manual and prompt-hook intake remain available.

The next `UserPromptSubmit` hook pulls the oldest pending letters, emits their context, flushes stdout, then confirms those IDs.
Transport arrival and the doorbell alone do not confirm intake.
Interruption before confirmation can repeat the same letter ID; confirmed letters do not appear again in hook context.
The hook emits at most five letters and 8 KiB of context, with a remaining-count line and `hide inbox` guidance when more are pending.
A large letter is truncated at a UTF-8 boundary and includes its ID and `hide request show` command for the complete body.
The installed prompt hook supervises one guarded internal operation with a 1.85-second deadline inside the two-second caller budget, including Memory, filesystem, output and diagnostic work.
The internal operation requires positive owner proof and inherits the runtime payload and output streams; it confirms only after its output is successfully flushed.
Failure leaves unconfirmed letters pending and attempts a rate-limited private diagnostic inside that same budget.
A blocked diagnostic store or output stream can prevent the diagnostic from finishing; the outer timeout performs no filesystem or output tail that could hold submission open.
CLI output collection uses the canonical platform capture, with a 64 KiB bound and no reader thread or blocking join after timeout.
Cleanup uncertainty is a separate private diagnostic field and never authorizes confirmation.
Manual `hide inbox` and `hide request show` remain available when the hook is missing or fails.

The pinned Herdr API has no atomic composer guard.
Hide checks the occupant and composer before and after the durable reservation and checks its memory state immediately before the off-lock pane write, but direct external Herdr/TUI input can race that final write.
That residual limit is the approved D-18 boundary; external input is not represented as a Hide key event.

## Persistence, clocks and limits

The mailbox lives at the state directory's `delivery-ledger.json`, named by `hide_kit::layout::delivery_ledger`.
An admitted mutation is atomically persisted as a private file before the daemon publishes it or returns success.
Startup establishes the existing trusted ancestor's directory barriers, creates and syncs at most 64 private descendants, validates the actual ledger and reestablishes its file barrier before admitting effects.
An uncertain replacement disables further delivery effects until a validated restart; corrupt bytes and incomplete directory chains remain available for recovery.
The canonical platform boundary currently refuses Windows directory-chain establishment with `Unsupported`; that is an open cross-OS requirement, and delivery does not become ready through a legacy no-op.
A corrupt, oversized or unsafe existing ledger is preserved, and commands return `ledger_unavailable` with a diagnostic instead of starting an empty ledger.
Capacity errors retain existing letters and watches.

| Resource or clock | Bound |
| --- | --- |
| Pending delivery deadline | 60 minutes, then `undelivered`; visible through CLI |
| Hide-key quiet period | 30 seconds |
| Doorbell reservations per letter | Three total, persisted across restart |
| First inactivity warning | 20 minutes without activity |
| Second inactivity warning | First-warning time plus 60 minutes, at most two warnings per episode |
| Unanswered parent warning notification | First-warning time plus 60 minutes, once per native target and inactivity episode |
| Intent retention and finished-letter cleanup | 30 days; open letters remain |
| Open / retained letters | 1024 / 5000 |
| Watches | 32 |
| Letter body / ledger file | 16 KiB / 16 MiB |
| Hook batch / context / total deadline | Five letters / 8 KiB / two seconds |

The delivery deadline and watch clocks are distinct.
There is no transition to `expired` in this contract; an undelivered letter uses the existing human notification paths without creating another letter or UI banner.
First-warning time and count persist across daemon restarts; activity resets both.

## Watch activity and completion

The registered observer receives inactivity warning letters; `hide watch assign` changes that observer without a polling interval flag.
The current observer can assign a new observer; an attested new observer can accept a handover with explicit `--approval <text>` and the current `--expected-generation <n>`.
Approval is nonblank text of at most 256 bytes without control characters, and does not replace the command's native caller binding.
For that acceptance, the requested observer must be the actual caller; `--actor` cannot select a different caller, and a missing or stale generation refuses the handover.
Activity is the later of Herdr's status-transition time and the confirmed native session file's modification time.
A local read and the device helper's `session_activity` use the same session ownership and root-confinement checks.
Session lookup has one total budget of 10000 directory entries, including skipped extensions and unmatched names, 64 visited directories and 8 MiB of retained path bytes.
Crossing a limit returns the path-free `session_capacity` outcome; the watch uses its existing status fallback and records the failure.
Below those limits, reported identity and provider ownership take precedence over cwd fallback, and each tick samples fresh metadata.
The helper returns only modification time and file size; conversation text, paths and native IDs do not appear in the activity answer.
The helper acquires the platform's `OwnerWatch` before reading arguments, stdin or files and retains it through shutdown.
A protected launch ends with its owner even while stdin stays open; a standalone launch without ownership metadata keeps its existing contract.
A missing reference or failed helper read falls back to the current status-transition evidence, includes the reason in the digest and does not suppress warnings.
That status must still be attributable to the watch's original native binding.
An uncertain acquisition or loss of native identity preserves the original watch and clocks without rebinding or accepting new file metadata.
Its digest reports `projection_unavailable` and an unavailable current status; two missing references do not prove an unchanged execution.
Proven original-target absence or a different positive native identity ends the watch.
There is no cached response or retry during the same tick.
Three consecutive read failures produce a rate-limited diagnostic.

The minute tick admits at most one read per watched target, at most four remote reads at once, each with a five-second deadline.
Different parents watching one exact target share that tick's activity sample while keeping independent warning clocks and intent keys.
A warning rejected by capacity leaves other targets' exits and activity resets intact.
Session discovery, ownership reads and helper I/O run outside `Mutex<Runtime>`.
A tick also performs one bounded ledger encoding and bounded per-watch admission checks; it publishes only watch start, warning and end transitions.
A target exit or parent's explicit stop ends the watch.
A normal reply closes the request's answer wait and leaves the watch active.
A done target remains watched until exit, explicit stop or a completion report.
`hide request send --kind report` ends its sender's watch when delivery to the parent is confirmed, without waiting for acknowledgement.
A report from an unwatched sender is an ordinary letter; the parent can restart a watch explicitly.

## Agent registration and spawning

`hide agent register [--check]`, `list`, `show` and `end` preserve the caller surface used by dispatch and Fork.
`hide agent spawn` accepts `--parent`, `--name`, `--intent`, `--kind`, `--repo`, `--branch`, optional `--path`, `--no-watch` and native arguments after `--`.
It creates the checkout when needed, the real child pane and agent, registers their relationship, writes lineage immediately and starts a watch unless `--no-watch` is present.
Retrying the same intent resumes the existing child and repairs incomplete registration rather than creating another one.
Remote starts use Hide's existing device start path.
The unsupported reconciliation/resume/session flags and relay, escalate, graph and events commands are absent.

The existing one-second `agent.list` refresh, also requested by native events, reconciles only panes whose four lineage tokens differ; startup and reconnect perform one full pass.
An unchanged native observation and append-only registration count skip planning; unrelated letter/watch writes and label/process/catalog publications do not start a lineage pass.
A positive child-session replacement clears stale tokens; an absent native session or agent end leaves the pane's tokens in place, while unchanged readers reject obsolete session identities.
There is no separate lineage timer, subprocess on the input path or blocking work under `Mutex<Runtime>`.
The token readers and digest contract remain unchanged.

People receive notifications only for an unanswered parent inactivity warning and an overdue undelivered letter.
A first warning left unacknowledged, uncancelled and unreplied for 60 minutes triggers one human notification per native target and inactivity episode, shared across sibling watches and phone Web Push/Herdr channels.
The receipt lives on the existing warning letters and survives daemon restart and watch stop/restart; new target activity starts a new episode.
The second agent warning introduces no second human notification schedule.
An overdue letter's notification key is its ID plus the cause and sends once across both channels.
A failed first channel falls back to the other; two failures record a diagnostic without retry.
These cases add no Inbox screen or automatic escalation chain.

## Verification

Use the worktree-owned entrypoints described in [BUILD.md](BUILD.md#two-entrypoints).
Focused checks are:

```sh
bash scripts/verify-cargo.sh test-scoped -p herdr-core --lib delivery:: -- --nocapture
bash scripts/verify-cargo.sh test-scoped -p hide-session --lib session_activity -- --nocapture
bash scripts/verify-cargo.sh test-scoped -p hide-host --test session_activity
```

A filtered run must execute the expected named tests; zero selected tests is a failed check.
The helper executable tests use private homes and fixture transcripts, pair JSON-line responses by request ID, and cover activity success/refusal, privacy, kit coexistence, normal exit and abrupt owner loss.
Each fixture launch and capture shares an absolute five-second deadline and a combined 64 KiB output cap; provider sessions and the installed helper are never used.
The full Rust test and lint lanes still apply to the final committed head.
Actual Linux and Windows OS-contract runner results are required for the state-machine, ledger and activity portability claim; declaring a workflow does not prove it passed.
Real TUI delivery requires an isolated Herdr server, private HOME/state, precisely identified candidate processes and disposable provider sessions with observed native identity and registered lineage.
Register only an actual parent session and spawn its child through `hide agent spawn`; never seed a capability or coordination ledger.
Exercise idle/done delivery, working delay, a draft identical to the placeholder, uncertain menus, hook confirmation/restart, capacity/corruption, watch clocks/reset/exit/reply and helper privacy/fallback.
Label protocol fixtures separately from actual provider runtime observations.
Measure matched baseline/candidate input latency and idle/driven load through [PERFORMANCE_TESTING.md](PERFORMANCE_TESTING.md); a headless or socket-only check does not prove native presentation.
Own every fixture process with a deadline and teardown, preserve the operator's app/server/panes/hooks, and remove private authentication caches after the owned agents exit.
Run evidence stays under ignored `agents/runs/`; it is never committed with this guide.
