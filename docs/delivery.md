# Agent delivery and inactivity watches

Hide's core owns agent registration, spawning and lineage, the single mailbox and inactivity watches for local and connected device agents.
This guide is the public operating contract.
Approved implementation contracts and run state stay in the private local harness.
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
Acknowledgement marks a letter as manually checked without changing its durable `hook_confirmed` intake receipt; a reply separately closes the original request's answer wait.
Retry the same intent after an interrupted call: the same sender identity and intent return the existing letter during its retention period, including after cancellation or delivery.
Use a new intent for a new letter.
The envelope identifies the sender and letter kind; it is not a session-level authority or anti-forgery proof.
The current first-line format is `Hide letter <id> from <name> (<agent>) [<kind>]`, with independent writer and transcript-reader examples in [delivery-envelope.json](../contracts/delivery-envelope.json).
A batch keeps the first letter's sender attribution in the request view; neither later headers nor older delivery formats select a sender.

## Safe intake and manual fallback

A doorbell carries only a short instruction to read `hide inbox`.
It is typed when every one of these hide-owned facts holds, and the verdict reads no terminal screen, so a redrawn footer or a user's statusline cannot change it:

- Herdr reports the pane `idle` or `done`, and has for 30 seconds.
- Hide has routed no key to the pane for 30 seconds.
- No input hide routed is newer than the later of the pane's last submission and the last time it entered `working`.
  Typed keys, a pasted attachment, a phone key and the keys of an agent's own find box all count as input, and none of them, Enter included, counts as a submission.
  A submission is a prompt hook that ran with the pane's own native session id, which the hook passes to `hide inbox --hook --session`, or a phone reply.
  A pane that entered `working` was submitted to as well, which is how an answered menu or a custom slash command that starts a turn clears the hold.
  A key after both is an unsent draft, an Esc-restored prompt or a recalled input; a pane held by one stays held until the next real prompt.
- The pane still hosts the native session the letter was written for, and the agent kind is one the bell targets.

Herdr's `blocked` status is the only menu guard, so only kinds whose permission and selection menus were observed to read `blocked` are bell targets.
Today those are Claude Code and Codex.
Gemini, Grok and Cursor are not targets because their menus were not observed (no logged-in CLI was available for the check); OpenCode, Pi and every other kind keep today's behavior, with letters read through `hide inbox` or a prompt hook.
Herdr 0.9.1 reads a built-in slash picker such as `/model` or `/resume` as `done`, not `blocked`, for both targets, and a bell typed into an open picker is accepted by it.
Hide therefore does not take the Enter that opens a picker for a submission: the pane holds as a draft, and stays held after `/clear`, `/model` or `/help` until the next real prompt runs its hook.
The same holds after an Esc or Ctrl-C that interrupts a turn, since no prompt hook runs for it.
Three residuals remain: a turn the operator did not start (a scheduled wake or a finished subagent) moves the pane to `working` and clears a half-typed draft, a prompt queued while the pane works leaves it held after its turn until the next prompt submitted from rest (its hook runs while `working` and clears nothing), and a pane restarted with hided or whose observation is dropped from a snapshot starts with no draft known.
A letter that cannot be belled waits, and the reason (`working`, `blocked`, `draft`, `quiet_period`, `kind_not_belled`, `session_changed`, `pane_unavailable`, `status_not_at_rest`, `changed_before_input`) is logged once per change with the letter and pane ids, never with its body and never on screen.
After a hided restart every pane starts with no key known and a 30 second grace; a draft typed before the restart cannot be known.
The adapter never copies, clears or restores a draft.
Each letter has at most three durable doorbell reservations, including successful input and attempts interrupted before confirmation.
The reservation is persisted before input, and the adapter repeats the native inspection (`agent.get`: kind, session, status, sequence) after the persistence wait.
A crash or changed pane after reservation may consume an attempt while leaving the letter pending.
Legacy records with a successful bell but no total count conservatively have no automatic attempts left; manual and prompt-hook intake remain available.

The `UserPromptSubmit` hook of a bell target reads the submitted prompt from its input payload and asks for letters in one of two ways.
When the prompt is exactly the bell (`hide inbox --hook --bell`), it pulls the oldest pending letters and newly acknowledged letters with `hook_confirmed: false`, which remain open for capacity and retention, emits their context, flushes stdout, then confirms those IDs.
For any other prompt, the operator's own included (`hide inbox --hook`), it adds at most one line, `Hide 편지 N통 대기 중, 이 턴이 끝난 뒤 전달`, counting the letters a bell will still bring, and confirms nothing; the prompt text is never changed and no letter body reaches an operator's turn.
A letter whose three bells are spent stays pending for `hide inbox` and expires undelivered.
A payload that is truncated, unreadable or not read within 0.5 seconds counts as an operator prompt.
Both pulls carry the payload's `session_id`; the core takes the pull as proof the pane's composer was sent only when that session is the pane's own native session, so a stray `hide inbox --hook` from another tool in the pane clears no draft.
The session id is not a secret, so this guards against accidents and not against a hostile process in the pane (external input, D-18).
A hook that runs while Herdr already reports the pane `working` is a queued prompt being taken up and clears nothing, and an id longer than 256 bytes or holding a control character is refused before it is hashed.
A device kit older than the local app sends `hide inbox --hook` without `--bell`, so its bell turn gets only the count line and the letter stays pending until the kit is updated; keep the kit and the app on the same build.
A prompt hook that runs inside an agent with no prompt hook of its own (Grok or OpenCode loading Claude Code's hook) receives nothing and confirms nothing.
An agent with no prompt hook reads letters with `hide inbox`, which shows an `ack_command` for each, and `hide request ack` is its receipt: it records `hook_confirmed` and ends a matching report watch, as the flushed hook confirmation does for the others.
Transport arrival and the doorbell alone do not confirm intake.
Interruption before confirmation can repeat the same letter ID; confirmed letters do not appear again in hook context.
The hook emits at most five letters and 8 KiB of context, with a remaining-count line and `hide inbox` guidance when more are pending.
A large letter is truncated at a UTF-8 boundary and includes its ID and `hide request show` command for the complete body.
The installed prompt hook supervises one guarded internal operation with a 1.85-second deadline inside the two-second caller budget, including Memory, filesystem, output and diagnostic work.
The internal operation requires positive owner proof and inherits the runtime payload and output streams; it confirms only after its output is successfully flushed.
Failure leaves actual intake unconfirmed and attempts a rate-limited private diagnostic inside that same budget.
A blocked diagnostic store or output stream can prevent the diagnostic from finishing; the outer timeout performs no filesystem or output tail that could hold submission open.
CLI output collection uses the canonical platform capture, with a 64 KiB bound and no reader thread or blocking join after timeout.
Cleanup uncertainty is a separate private diagnostic field and never authorizes confirmation.
Manual `hide inbox` and `hide request show` remain available when the hook is missing or fails.

The pinned Herdr API has no atomic composer guard.
Hide checks the occupant before and after the durable reservation and checks its memory state immediately before the off-lock pane write, but direct external Herdr/TUI input can race that final write.
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
| Quiet period | 30 seconds since hide last routed a key and since Herdr last changed the pane's status |
| Doorbell reservations per letter | Three total, persisted across restart |
| First inactivity warning | 20 minutes without activity; 30 minutes when the observer is a Factory |
| Second inactivity warning | First-warning time plus 60 minutes, at most two warnings per episode |
| Unanswered parent warning notification | First-warning time plus 60 minutes, once per native target and inactivity episode |
| Intent retention and finished-letter cleanup | 30 days; open letters remain |
| Open / retained letters | 1024 / 5000 |
| Watches | 32 |
| Letter body / ledger file | 16 KiB / 16 MiB |
| Hook batch / context / total deadline | Five letters / 8 KiB / two seconds |

Automatic expiry and doorbells apply only to `pending` letters, while watch clocks remain distinct.
Legacy records with missing or null `hook_confirmed` prove intake only in `delivered` state; older `acknowledged` records remain unknown, excluded from pull and subject to their previous closed-state retention rules unless still awaiting a reply.
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
The first actual post-flush confirmation records `hook_confirmed: true` and ends a matching sender-parent watch for `hide request send --kind report`, even after acknowledgement, cancellation or the delivery deadline; replay after restart preserves a watch explicitly started after that receipt.
A report from an unwatched sender is an ordinary letter; the parent can restart a watch explicitly.

## The code-owned recipient `factory:<id>`

Each open Factory is a code-owned recipient named `factory:<factory id>`, and [factory.md](factory.md#the-mailbox-recipient-factoryid) owns how it is used.
Only the Factory's engine sends as it, and the ledger accepts that authority only while the Factory exists and is open.
A pane or agent may send to it and may name it as a watch observer, and only while the Factory exists and is open; a pane or agent name that starts with `factory:` is refused at registration.
A letter to it is never pasted into a composer and never rings a doorbell: the engine reads it, confirms it and, for a request or block, replies with its answer.
The human-notice claim skips a letter whose recipient is code-owned.
A watch observed by a Factory warns after 30 minutes without activity rather than 20, and the engine acts on the first warning.

## Agent registration and spawning

`hide agent register [--check]`, `list`, `show` and `end` preserve the caller surface used by dispatch and Fork.
`register` records the participant on the caller's own machine, which the pane capability names: `--machine` may be left out, and when given it must name that same machine or the call is refused with `machine_identity_conflict`, so a caller in a connected device's pane registers as that device.
`hide agent spawn` accepts `--parent`, `--name`, `--intent`, `--kind`, `--repo`, `--branch`, optional `--path`, `--no-watch` and native arguments after `--`.
It creates the checkout when needed, the real child pane and agent, registers their relationship, writes lineage immediately and starts a watch unless `--no-watch` is present.
A completed spawn stores a durable receipt for its parent and intent, so retries return the same child and preserve ended registrations and closed watches.
Only incomplete intents resume their recorded creation and registration steps; starting a new watch after completion requires explicit `hide watch start`.
Remote starts use Hide's existing device start path.
The unsupported reconciliation/resume/session flags and relay, escalate, graph and events commands are absent.

The existing one-second `agent.list` refresh, also requested by native events, reconciles only panes whose four lineage tokens differ; startup and reconnect perform one full pass.
An unchanged native observation and append-only registration count skip planning; unrelated letter/watch writes and label/process/catalog publications do not start a lineage pass.
The native pane and positive session select the matching retained registration, including an ended one, independently of append order.
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
Exercise idle/done delivery to a Claude pane with a statusline and to a Codex pane, working delay, a pending letter held while a permission, question or plan menu is open and delivered after it is answered, each target's menus read as Herdr `blocked`, hook confirmation/restart, capacity/corruption, watch clocks/reset/exit/reply and helper privacy/fallback.
Label protocol fixtures separately from actual provider runtime observations.
Measure matched baseline/candidate input latency and idle/driven load through [PERFORMANCE_TESTING.md](PERFORMANCE_TESTING.md); a headless or socket-only check does not prove native presentation.
Own every fixture process with a deadline and teardown, preserve the operator's app/server/panes/hooks, and remove private authentication caches after the owned agents exit.
Run evidence stays under ignored `agents/runs/`; it is never committed with this guide.
