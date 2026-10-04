# Agent delivery and inactivity watches

Hide's core owns the local mailbox and inactivity watches while the existing hcoord coordinator continues to own its requests, watches, spawning and lineage.
The operating contract is [PRD A](../agents/prd/hcoord-delivery-v2/prd.md).
Remote recipients, hcoord retirement, human Inbox UI and automatic draft clearing belong to later work.

## Commands and caller identity

These commands need the running daemon and a current agent pane in a registered checkout; they work without an open renderer.
The daemon binds the caller through the existing Workspace credential boundary and resolves the actual pane, provider and native-session identity from Herdr.
A pane hint cannot replace that binding, and an absent, ambiguous or changed occupant returns an explicit error.
Each queued command revalidates both its original capability caller and the agent pane against the prepared Workspace and checkout context before applying or saving; a moved checkout or newly narrower caller binding returns `caller_context_changed`.
Mailbox callers and new recipients require a positive native-session binding; a missing binding returns `native_identity_required`.
Two missing native references in the same pane never authorize retained mail.
Target names and pane IDs resolve against the daemon's current observations.
Sending to a remote recipient returns `remote_delivery_unsupported`.

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
```

The recipient can acknowledge or reply; the sender can cancel.
Acknowledgement changes the receipt state, and a reply separately closes the original request's answer wait.
Retry the same intent after an interrupted call: the same sender identity and intent return the existing letter during its retention period, including after cancellation or delivery.
Use a new intent for a new letter.
The envelope identifies the sender and letter kind; it is not a session-level authority or anti-forgery proof.

## Safe intake and manual fallback

A doorbell carries only a short instruction to read `hide inbox`.
It requires a current local idle/done occupant, 30 seconds since Hide last routed a key to that pane, positive native readiness, a positively styled empty composer, and a recognized footer structure.
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
The hook has one total two-second budget; failure succeeds without context, records a bounded private diagnostic and leaves the letter pending.
Manual `hide inbox` and `hide request show` remain available when the hook is missing or fails.

The pinned Herdr API has no atomic composer guard.
Hide checks the occupant and composer before and after the durable reservation and checks its memory state immediately before the off-lock pane write, but direct external Herdr/TUI input can race that final write.
That residual limit is the approved D-18 boundary; external input is not represented as a Hide key event.

## Persistence, clocks and limits

The mailbox lives at the state directory's `delivery-ledger.json`, named by `hide_kit::layout::delivery_ledger`, independently of hcoord's storage.
An admitted mutation is atomically persisted as a private file before the daemon publishes it or returns success.
A corrupt, oversized or unsafe existing ledger is preserved, and commands return `ledger_unavailable` with a diagnostic instead of starting an empty ledger.
Capacity errors retain existing letters and watches.

| Resource or clock | Bound |
| --- | --- |
| Pending delivery deadline | 10 minutes, then `undelivered`; visible through CLI |
| Hide-key quiet period | 30 seconds |
| Doorbell reservations per letter | Three total, persisted across restart |
| First inactivity warning | 20 minutes without activity |
| Second inactivity warning | First-warning time plus 60 minutes, at most two warnings per episode |
| Intent retention and finished-letter cleanup | 30 days; open letters remain |
| Open / retained letters | 1024 / 5000 |
| Watches | 32 |
| Letter body / ledger file | 16 KiB / 16 MiB |
| Hook batch / context / total deadline | Five letters / 8 KiB / two seconds |

The delivery deadline and watch clocks are distinct.
There is no transition to `expired` in this contract, and an undelivered letter does not generate a new notification letter or UI banner.
First-warning time and count persist across daemon restarts; activity resets both.

## Watch activity and hcoord coexistence

Only a local parent that explicitly starts a watch receives its warning letters.
Activity is the later of Herdr's status-transition time and the confirmed native session file's modification time.
A local read and the device helper's `session_activity` use the same session ownership and root-confinement checks.
Session lookup has one total budget of 10000 directory entries, including skipped extensions and unmatched names, 64 visited directories and 8 MiB of retained path bytes.
Crossing a limit returns the path-free `session_capacity` outcome; the watch uses its existing status fallback and records the failure.
Below those limits, reported identity and provider ownership take precedence over cwd fallback, and each tick samples fresh metadata.
The helper returns only modification time and file size; conversation text, paths and native IDs do not appear in the activity answer.
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
A done target remains watched until exit or explicit stop; there is no completion-report command in this phase.

At registration only, Hide runs one existing read-only hcoord watch-list call and, when active watches exist, at most one agent-list call within one combined two-second budget.
An exact current pane/session/machine/host-scope match rejects registration as `conflict`; unavailable or ambiguous proof records a stable diagnostic and allows registration.
Returned prompts, paths and arguments are not persisted or logged.
During PRD A, do not add an hcoord watch to a target already watched by Hide: the reverse registration direction is an operational rule, and two independent watches can otherwise produce two warning letters.
The helper protocol addition must inherit the preceding request-view contract; this work does not authorize replacing an installed helper or retiring hcoord.

## Verification

Use the worktree-owned entrypoints described in [BUILD.md](BUILD.md#two-entrypoints).
Focused library checks are:

```sh
bash scripts/verify-cargo.sh test-scoped -p herdr-core --lib delivery:: -- --nocapture
bash scripts/verify-cargo.sh test-scoped -p hide-session --lib session_activity -- --nocapture
```

A filtered run must execute the expected named tests; zero selected tests is a failed check.
The full Rust test and lint lanes still apply to the final committed head.
Actual Linux and Windows OS-contract runner results are required for the state-machine, ledger and activity portability claim; declaring a workflow does not prove it passed.
Real TUI delivery requires an isolated Herdr server, private HOME/state/HCOORD_HOME, precisely identified candidate processes and disposable provider sessions with observed native identity and hcoord lineage.
Register only an actual parent session and spawn its child through the existing hcoord path; never seed a capability or coordination ledger.
Exercise idle/done delivery, working delay, a draft identical to the placeholder, uncertain menus, hook confirmation/restart, capacity/corruption, watch clocks/reset/exit/reply and helper privacy/fallback.
Label protocol fixtures separately from actual provider runtime observations.
Measure matched baseline/candidate input latency and idle/driven load through [PERFORMANCE_TESTING.md](PERFORMANCE_TESTING.md); a headless or socket-only check does not prove native presentation.
Own every fixture process with a deadline and teardown, preserve the operator's app/server/panes/hooks, and remove private authentication caches after the owned agents exit.
Run evidence stays under ignored `agents/runs/`; it is never committed with this guide.
