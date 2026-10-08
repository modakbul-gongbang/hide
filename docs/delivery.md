# Agent delivery and inactivity watches

Hide's core owns agent registration, spawning and lineage, the single mailbox and inactivity watches for local and connected device agents.
This guide is the public operating contract.
Approved implementation contracts and run state stay in the private local harness.
Human Inbox UI, relay/escalate, authority proof and automatic draft clearing remain follow-up work.

## Commands and caller identity

These commands, the `hide agent` commands among them, need the running daemon and a current agent pane in a registered checkout; they work without an open renderer.
The daemon binds the caller through the existing Workspace credential boundary and resolves the actual pane, provider and native-session identity from Herdr.
Only a pane-bound caller is accepted: a checkout-bound caller (a plain terminal, or a tool shell inside Codex's shared app-server daemon) is refused `agent_pane_required` whatever pane it names, and its next step is to run the command inside the agent's own pane, or to run that Codex without the shared daemon (`--no-daemon`).
A pane hint cannot replace that binding: a pane-bound caller whose hint names another pane is refused `caller_identity_conflict`, and an absent, ambiguous or changed occupant returns an explicit error.
`hide factory` is the exception ([factory.md](factory.md)): a checkout-bound caller acts as the operator without a pane, and its pane hint is never read as identity or lineage.
Each queued command revalidates the caller's pane against the prepared Workspace and checkout context before applying or saving; a moved checkout returns `caller_context_changed`.
Mailbox callers and new recipients require a positive native-session binding; a missing binding returns `native_identity_required`.
Two missing native references in the same pane never authorize retained mail.
Target names and pane IDs resolve against the daemon's current observations.
A connected device recipient uses its node's pane service over that device's existing SSH link; the credential fixes its device, link, pane, sender and kind, and losing the link revokes that credential immediately.
The only ledger remains on the controlling daemon.
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
A `request` or `block` letter waits for its answer until one of four things ends the wait: the recipient replies, the sender cancels, either side's registration ends, or 24 hours pass since it was sent ([Persistence, clocks and limits](#persistence-clocks-and-limits)).
When the last two end it, the letter keeps its `state`, `waiting_answer` turns `false`, `finished_at_unix_ms` is set, and `answer_wait_ended` says why: `party_ended` or `deadline`; it is `null` on every other letter.
The sender reads it in the answer of `hide request show` (and in every other answer that carries the letter); `hide inbox` lists the same letters as before and does not announce a wait that ended.
A letter that ended its wait without an answer no longer counts against the open-letter limit, and the recipient can still reply to it by the same rules.
It is still a retained letter: it leaves the 5000-letter total only after the 30-day cleanup counted from the time its wait ended.
Acknowledgement is the recipient's receipt for every agent kind: only the recipient's own pane and native session can acknowledge (`hide request ack` from a pane-bound caller is the only path, with no operator or helper path), so it records `hook_confirmed: true` and ends a matching report watch; a reply separately closes the original request's answer wait.
Retry the same intent after an interrupted call: the same sender identity and intent return the existing letter during its retention period, including after cancellation or delivery.
Use a new intent for a new letter.
The envelope identifies the sender and letter kind; it is not a session-level authority or anti-forgery proof.
The current first-line format is `Hide letter <id> from <name> (<agent>) [<kind>]`, with independent writer and transcript-reader examples in [delivery-envelope.json](../contracts/delivery-envelope.json).
A batch keeps the first letter's sender attribution in the request view; neither later headers nor older delivery formats select a sender.

### The contract a calling tool checks

A tool that runs these commands from a program (sasu, a Factory engine) checks the installed `hide` before it calls one.
`hide version --json` answers `{"version", "commit", "contract"}`: the version the build reports, the commit it was built from (`null` for a debug build given none, or a build from a tree with no Git), and the `sha256:` digest of the command contract; neither it nor `hide contract --json` needs a daemon or a pane.
`hide contract --json` answers that contract:

- `format`: the document's own format, now 1.
- `commands`: each command's words, its positional `arguments` with their value types, `repeats_at_most` for a last argument that repeats, its `options` (each with `name`, `value` type or `null` for a switch, `required`, and the options it `requires`), what `rest` passes through after `--`, the names of the `answers` it can return, and, where the contract declares them, the `refusals` its own rule answers with; those are not every failure, since every command can also fail for the daemon, the pane or the ledger.
- `value_types`: what each value type admits (`text`, `key`, `body`, `approval`, `unsigned`, `one_of` with its `values`).
- `envelopes`: for each topic, where the printed line carries `ok`, the `answer` and the failure `code`; a failure also exits non-zero.
- `answers`: the JSON Schema of each answer, and `digest`, the `sha256:` of the document without it, its keys sorted and written compactly.

A caller compares the digest it was written against, or the commands and options it uses, and refuses a mismatch before running anything.
The CLI admits only the commands and flags the contract names (`hided/src/cli_contract.rs`), its tests hold each parser to the table, and the answer schemas come from the core's answer types (`herdr-core/src/delivery/answer.rs`), so a changed command name, option or answer field changes the exported contract.
`contracts/hide-cli.json` is the committed copy, and a test fails until it is regenerated with `target/debug/hide contract --json | python3 -m json.tool > contracts/hide-cli.json`.

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
- For an agent whose session read reports its turns (today Codex), that read, made for Herdr's current state, says nothing waits for the operator (see [A menu Herdr reads as a stop](#a-menu-herdr-reads-as-a-stop)).
- Herdr does not report the agent as starting or not ready.
  Herdr reports readiness only for an agent it launched itself (`herdr agent start`, which `hide agent spawn` uses): `launch_pending: true` while that start settles or is blocked, then `interactive_ready: true`.
  An agent the operator started by typing its program in a Herdr shell, the usual way a lead is started, carries neither flag; its readiness is unreported, not refused, and the facts above decide it.
  Only `launch_pending: true` or an explicit `interactive_ready: false` holds the letter.

Herdr's `blocked` status guards every permission and selection menu Herdr reads as `blocked`, so only kinds whose menus were observed to read `blocked` are bell targets.
Today those are Claude Code and Codex.
The shared agent adapter declares bell eligibility separately from prompt intake, spawn refusal and session format; its contract rejects a bell without a prompt hook, and adding prompt intake alone never widens the bell targets.
Claude Code's plan approval reads `blocked`; Codex's does not, and the session read guards it instead (next section).
Gemini, Grok and Cursor are not targets because their menus were not observed (no logged-in CLI was available for the check); OpenCode, Pi and every other kind keep today's behavior, with letters read through `hide inbox` or a prompt hook.
Herdr 0.9.1 reads a built-in slash picker such as `/model` or `/resume` as `done`, not `blocked`, for both targets, and a bell typed into an open picker is accepted by it.
Hide therefore does not take the Enter that opens a picker for a submission: the pane holds as a draft, and stays held after `/clear`, `/model` or `/help` until the next real prompt runs its hook.
The same holds after an Esc or Ctrl-C that interrupts a turn, since no prompt hook runs for it.
Three residuals remain: a turn the operator did not start (a scheduled wake or a finished subagent) moves the pane to `working` and clears a half-typed draft, a prompt queued while the pane works leaves it held after its turn until the next prompt submitted from rest (its hook runs while `working` and clears nothing), and a pane restarted with hided or whose observation is dropped from a snapshot starts with no draft known.
A letter that cannot be belled waits, and the reason is logged as `doorbell.held` once per change with the letter and pane ids, never with its body and never on screen.
The verdict's reasons are `working`, `blocked`, `awaiting_operator`, `session_unread`, `draft`, `quiet_period`, `kind_not_belled`, `session_changed`, `pane_unavailable` and `status_not_at_rest`.
Between the verdict and the input the doorbell asks Herdr again and checks its own memory last, and each refusal there has its own reason: `launch_pending` and `not_ready` (Herdr's readiness), `identity_changed` (Herdr's agent in the pane has another name, kind or native session), `sequence_moved` (Herdr's status or state sequence moved), `input_after_verdict` (hide routed input to the pane) and `letter_changed` (the letter was confirmed, cancelled, expired or reserved meanwhile).
A letter refused there is tried again in the same pane episode after 5 seconds, then 10, 20, 40 and 80, then every two minutes, because readiness and the letter are facts the pane episode does not carry; a change of the pane's status, sequence, input or session tries it at once.
A failed Herdr call is not retried on that schedule, since the bell may already have been typed; it waits for the pane to move.
A letter still held when its deadline passes is logged once as `doorbell.expired` with the reason it last waited for.
After a hided restart every pane starts with no key known and a 30 second grace; a draft typed before the restart cannot be known.
The adapter never copies, clears or restores a draft.
Each letter has at most three durable doorbell reservations, including successful input and attempts interrupted before confirmation.
The reservation is persisted before input, and the adapter repeats the native inspection (`agent.get`: readiness, kind, session, status, sequence) after the persistence wait.
A refusal before the reservation spends none, so a letter Herdr keeps refusing is never charged an attempt.
A crash or changed pane after reservation may consume an attempt while leaving the letter pending.
Legacy records with a successful bell but no total count conservatively have no automatic attempts left; manual and prompt-hook intake remain available.

The `UserPromptSubmit` hook of a bell target reads the submitted prompt from its input payload and asks for letters in one of two ways.
When the prompt is exactly the bell (`hide inbox --hook --bell`), it pulls the pending letters, oldest first, and after them the letters an earlier build acknowledged without a receipt (`hook_confirmed: false`), which the store's expiry pass before every request drops once their deadline passes, so such a backlog never displaces the letter the bell rang for; it emits their context, flushes stdout, then confirms those IDs.
For any other prompt, the operator's own included (`hide inbox --hook`), it adds at most one line, `Hide 편지 N통 대기 중, 이 턴이 끝난 뒤 전달`, counting the letters a bell will still bring, and confirms nothing; the prompt text is never changed and no letter body reaches an operator's turn.
A letter whose three bells are spent stays pending for `hide inbox` and expires undelivered.
A payload that is truncated, unreadable or not read within 0.5 seconds counts as an operator prompt.
Both pulls carry the payload's `session_id`; the core takes the pull as proof the pane's composer was sent only when that session is the pane's own native session, so a stray `hide inbox --hook` from another tool in the pane clears no draft.
The session id is not a secret, so this guards against accidents and not against a hostile process in the pane (external input, D-18).
A hook that runs while Herdr already reports the pane `working` is a queued prompt being taken up and clears nothing, and an id longer than 256 bytes or holding a control character is refused before it is hashed.
A device kit older than the local app sends `hide inbox --hook` without `--bell`, so its bell turn gets only the count line and the letter stays pending until the kit is updated; keep the kit and the app on the same build.
A prompt hook that runs inside an agent with no prompt hook of its own (Grok or OpenCode loading Claude Code's hook) receives nothing and confirms nothing.
An agent with no prompt hook reads letters with `hide inbox`, which shows an `ack_command` for each, and `hide request ack` is its receipt, as it is for an agent with one: it records `hook_confirmed` and ends a matching report watch, as the flushed hook confirmation does.
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

### A menu Herdr reads as a stop

Codex's plan approval, "Implement this plan?", reads to Herdr 0.9.3 as an ordinary stop rather than `blocked`, and a bell's Enter there picks its first item, "Yes, implement this plan", so a letter would start a plan the operator never approved (measured 2026-10-07, three runs of three, with the menu read as `done`).
What Herdr reads depends on its Codex detection manifest: the current one (2026.10.01.1, fetched while Herdr's manifest updates are on) reads the menu `idle`, and the one bundled with 0.9.3 (2026.09.23.1, which the e2e fixture runs with updates off) reads it `unknown`, which the status guard already holds (measured 2026-10-07 with Codex CLI 0.160.1 on an isolated server).
Hide reads that wait from the Codex session file instead of the screen (PRD codex-plan-approval-hold).
The label worker's session read, which already runs on each change of the pane's Herdr state, on its own thread and from where it last stopped, folds Codex's turn records into an agent-neutral turn tracker (`hide-session/src/turns.rs`): `task_started` with its `collaboration_mode_kind`, the `Plan` item a plan-mode turn proposes (or its `<proposed_plan>` reply), `task_complete`, `turn_aborted`, and each person's message.
A plan-mode turn that proposed a plan and finished, with no later turn or message from a person, waits for the operator's approval; the next `task_started` or a person's message ends the wait.
Records the parser does not recognise are not known rather than "nothing waits": a `task_started` whose mode is missing or not one it knows, a plan proposed in a turn whose mode says it runs none, and a person's messages with no turn record at all.
The rule and its basis (codex-cli 0.160.1) are written beside the Codex parser, and nothing outside it names Codex.
The answer is kept with the Herdr `state_change_seq` the read was asked under, and it holds for that state only.
Herdr can read the agent at rest before Codex writes how the turn ended; that state's read then does not settle a plan-mode turn, so the bell holds and the row shows no wait until Herdr's next state is read, because this rule adds no session reads of its own (PRD codex-plan-approval-hold B10).
While it says a plan waits, the bell holds with `awaiting_operator`.
For an agent whose read reports turns, a state that no read has settled holds the bell too, with `session_unread`: a session file not found (Codex is looked up in its seven newest day folders), a failed read, a device helper that predates the field, a read still behind Herdr's newest state, or a daemon that does not hold the label generator lock and so reads nothing.
That lock is one per Herdr server and taken with `flock` (`herdr-core/src/labels/generator.rs`): the operator's single daemon lacks it only while another hided follows the same Herdr server (a candidate or development daemon pointed at the operator's socket, even with a private HOME), or, after a restart that overlapped the old daemon, until its next attempt, at most thirty seconds after the old one exits; the kernel frees a crashed holder's lock.
Unknown is never read as "nothing waits".
An agent whose read reports no turns (Claude Code and every other kind) is belled exactly as before.
"No, stay in Plan mode" writes nothing to the session (measured with Codex CLI 0.160.1), so the wait, the hold and the row in Needs You last until Codex's next turn starts or a person's message is written.
The first start of a build with this rule resumes a stored session at its last person's message, after that turn's `task_started`, so a plan already waiting then is not known until Herdr's next state; that start holds the bell rather than ringing it.
The rule reads the session Herdr reports for the pane, so a pane reported with another session is judged by that session.
It guards against hide's own bell, not against a process of the same account, which could append records to the session file or type into the pane through Herdr directly.

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
| First inactivity warning | 20 minutes without activity; the Factory's stall window (30 minutes by default) when the observer is a Factory |
| Second inactivity warning | First-warning time plus 60 minutes, at most two warnings per episode |
| Unanswered parent warning notification | First-warning time plus 60 minutes, once per native target and inactivity episode |
| Answer wait of a `request` or `block` | 24 hours from the send, then `answer_wait_ended: "deadline"`; an end of either registration ends it at once as `party_ended` for a letter the recipient took in |
| Intent retention and finished-letter cleanup | 30 days; open letters remain |
| Open / retained letters | 1024 / 5000; a letter is open while it awaits intake or a reply |
| Watches | 32 |
| Letter body / ledger file | 16 KiB / 16 MiB |
| Hook batch / context / total deadline | Five letters / 8 KiB / two seconds |

Automatic doorbells apply only to `pending` letters, and the deadline turns only a `pending` letter `undelivered`, while watch clocks remain distinct.
A letter an earlier build acknowledged without a receipt stops awaiting intake at the same 60-minute deadline, on the store's first pass after it (within a second of startup for an existing backlog): the hook no longer hands it over, its receipt reads `null` like the legacy acknowledged records below, and it no longer counts against the open-letter limit unless it still awaits a reply.
Legacy records with missing or null `hook_confirmed` prove intake only in `delivered` state; older `acknowledged` records remain unknown, excluded from pull and subject to their previous closed-state retention rules unless still awaiting a reply.
The 24-hour answer deadline is judged in the store's same maintenance pass, which runs when some letter or registration needs it: a wait past its deadline counts as such work, so no new thread or timer exists.
A registration ending leaves a letter still awaiting intake alone: it follows the 60-minute delivery deadline above, and only a letter the recipient took in has its answer wait ended.
The first pass of a build that has this rule closes every wait older than 24 hours in the ledger it finds and leaves the younger ones; the ledger stays at version 1, and an older build ignores the new field and drops it at its next save.
Each closed wait logs one `delivery` diagnostic `answer_wait.ended` with the letter id, both agents' names and panes and the reason, never the body, and nothing reaches the screen.
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
The first receipt, an acknowledgement or the actual post-flush confirmation, records `hook_confirmed: true` and ends a matching sender-parent watch for `hide request send --kind report`; a confirmation still does after cancellation or the delivery deadline, and replay after restart preserves a watch explicitly started after that receipt.
A report from an unwatched sender is an ordinary letter; the parent can restart a watch explicitly.

## The code-owned recipient `factory:<id>`

Each open Factory is a code-owned recipient named `factory:<factory id>`, and [factory.md](factory.md#the-mailbox-recipient-factoryid) owns how it is used.
Only the Factory's engine sends as it, and the ledger accepts that authority only while the Factory exists and is open.
A pane or agent may send to it and may name it as a watch observer, and only while the Factory exists and is open; a pane or agent name that starts with `factory:` is refused at registration.
A letter to it is never pasted into a composer and never rings a doorbell: the engine reads it, confirms it and, for a request or block, replies with its answer.
The human-notice claim skips a letter whose recipient is code-owned.
A watch observed by a Factory warns after that Factory's stall window (`stall_minutes`, 30 by default) rather than 20, and the engine acts on the first warning.

## Agent registration and spawning

`hide agent register [--check]`, `list`, `show` and `end` preserve the caller surface used by dispatch and Fork.
`register` records the participant on the caller's own machine, which the pane capability names: `--machine` may be left out, and when given it must name that same machine or the call is refused with `machine_identity_conflict`, so a caller in a connected device's pane registers as that device.
`hide agent show here` answers the caller's own registration, read only, with no renderer: the one live record whose actor is the caller's attested pane, device and session, the same match as `--parent here` (`coordination::live_self`), refused when two match (`coordination::here`).
Only a pane-bound credential can ask, as for every delivery and agent command: a checkout-bound one is refused `agent_pane_required`, and a pane-bound one whose hint names another pane is refused `caller_identity_conflict`.
A caller with no such record is refused `participant_ended` when its record ended, `participant_session_changed` when its pane's live record belongs to another session (the pane's agent session changed), `ambiguous_participant` when two live records match, and `participant_unavailable` otherwise; a remote participant on a pane of the same name is never the local caller.
`hide agent spawn` requires `--name`, `--intent`, `--kind`, `--repo` and `--branch`, with optional `--parent`, `--path` and native arguments after `--`.
Choose responsibility when creating the agent; it cannot be transferred afterwards.
With `--parent here` or the caller's own id, the agent is delegated: its parent owns the work, lineage is written immediately and an automatic watch starts.
Without `--parent`, the work is handed off to the operator: the agent is an independent root, has no lineage edge to the spawner and starts no automatic watch.
Both modes create a separate tab, in an existing checkout or a new worktree, without changing the current screen or keyboard focus.
The no-focus request applies to opening the checkout's owner workspace as well as creating its tab; interactive workspace and tab creation keep their focus behavior.
`hide agent spawn --help` explains the modes without contacting a daemon; the removed `--no-watch` flag is refused before any effects.

Every `AgentView` includes `origin`, the spawner id for either mode and null for ordinary roots.
Delegation derives origin from parent; handoff stores origin separately, and it grants no parent, subtree-close or end authority.
A handed-off agent's identical self-registration or `--check` preserves its id and stored origin, without taking an origin input or changing responsibility.
A handed-off agent can end its own registration; its spawner cannot end it and receives `parent_authority_required`.
Ordinary letters and explicit watches remain available between independent agents.
The sidebar, graph, ancestor unread state, descendant badges and waiting-on-descendants rule read only responsibility through parent.
PR and issue panels retain the handed-off session through its own branch facts and show no delegation line from origin.

A completed spawn stores a durable receipt for its caller and intent, so retries return the same agent and preserve ended registrations and closed watches.
Changing only responsibility mode on the same intent returns `intent_conflict` before creating anything.
Only incomplete intents resume their recorded creation and registration steps; starting a new watch after completion requires explicit `hide watch start`.
Remote starts use Hide's existing device start path with the same caller, modes and focus rules.
The ledger stays at version 1: older records without mode load as delegation, missing origin defaults to null and legacy `no_watch` data is ignored without changing existing watches.
Factory work remains explicitly delegated and watched, and dispatch clients using `--parent here` retain that behavior.
The unsupported reconciliation/resume/session flags and relay, escalate, graph and events commands are absent.

A registration ends when Herdr no longer has its pane, so its name and the watches on it do not outlive the pane; the ended record stays in the ledger, like one `hide agent end` ended, and still counts against the 2048-registration limit.
The core reads that from its own session sync of the host's Herdr, never from a separate poll: a pane the in-sync replica listed and then stops listing was closed by Herdr or moved to another tab, which gives it a new id and already ends the watches on it, and a pane missing from the fresh `session.snapshot` of a connect is gone for every registration made before that snapshot was asked for, which is how registrations left by panes that closed while no Hide was running end on the first connect.
Nothing ends on uncertainty: a stream that lost events, a Herdr live handoff or restart and an unreachable Herdr publish no read until a fresh snapshot replaces it, a disconnected device publishes none, a sleeping or resumed agent keeps its pane, a registration on another Herdr socket of the same machine is not judged by this one's read, and a registration made after the snapshot was asked for waits until a read lists its pane.
The delivery store ends it with the watches on it and the answer waits of the letters it sent or received, as `hide agent end` does, and logs `agent.ended` once with the agent id, machine, pane and reason (`pane_left` or `pane_absent`); nothing reaches the screen.
Removing a device retires its cached pane read and rejects late reads from its retired coordinator; removal does not prove its panes gone or end any registration or watch.
Only the coordinator sharing the currently installed remote control connector may begin a pane read, publish delivery observations or replace the remote session and connection status.
Each check holds the runtime lock through its write, including a second check after projecting a remote session; replacing the coordinator for the same device ID rejects the old one's late snapshot and failure, while the current coordinator may bootstrap before its first connected status.

An agent started through Herdr directly bypasses Hide's responsibility record.
The Claude Code and Codex `PreToolUse` spawn guard refuses such a call in a registered checkout and offers two filled commands: delegation with `--parent here`, or operator handoff without it ([agent-hooks.md](agent-hooks.md#the-spawn-guard)).
The guard remains bypassable guidance; it does not change either mode's authority or execution path.
A launch chained behind another command is refused whole, and a launch inside `bash -c`, `$(...)`, a script or an alias is not seen, so a child started that way has no parent line, as before the guard.
A first spawn of a new Codex child can answer `native_identity_unavailable` until that Codex has bound its session after its first turn; running the same `hide agent spawn` again with the same intent converges on the same child.

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
A channel that cannot reach the operator is not tried, and a failed first channel falls back to the other; two failures record a diagnostic without retry.
The phone is skipped before any send when the push key is missing (`no_vapid`), the push mode is off (`mode_off`) or is app-closed-only while a desktop or web shell is connected (`app_open`), Mobile is off (`mobile_off`) or no paired phone has a push subscription (`no_subscription`); the first that applies is the reason, and a send that fails or finds every subscription gone is `send_failed` or `subscription_gone`.
Herdr is skipped only when this Mac has no Herdr socket (`no_socket`), because the pinned contract has no method that reads its toast setting; otherwise its answer decides, and `disabled`, `rate_limited`, `no_foreground_client` and `busy` are Herdr's own `reason` for a notification it did not show, with `call_failed` and `answer_unreadable` when it gave no usable answer.
When neither channel reached the operator, one `human.channels_failed` record in `Logs/core.jsonl` carries the notice (`letter_undelivered` or `observer_unconfirmed`), the letter id, `push` and `herdr`; it never carries a letter's text, a phone endpoint or a key.
The claim stays consumed, so the same letter or warning is never announced again, and nothing is drawn on screen: an operator with phone push off and Herdr's toast off learns of a held letter from that record only.
These cases add no Inbox screen or automatic escalation chain.

## Verification

Use the worktree-owned entrypoints described in [BUILD.md](BUILD.md#two-entrypoints).
Focused checks are:

```sh
bash scripts/verify-cargo.sh test-scoped -p herdr-core --lib delivery:: -- --nocapture
bash scripts/verify-cargo.sh test-scoped -p hide-session --lib session_activity -- --nocapture
bash scripts/verify-cargo.sh test-scoped -p hided --test it node_session_activity::
```

A filtered run must execute the expected named tests; zero selected tests is a failed check.
The helper executable tests use private homes and fixture transcripts, pair JSON-line responses by request ID, and cover activity success/refusal, privacy, kit coexistence, normal exit and abrupt owner loss.
Each fixture launch and capture shares an absolute five-second deadline and a combined 64 KiB output cap; provider sessions and the installed helper are never used.
The full Rust test and lint lanes still apply to the final committed head.
Actual Linux and Windows OS-contract runner results are required for the state-machine, ledger and activity portability claim; declaring a workflow does not prove it passed.
Real TUI delivery requires an isolated Herdr server, private HOME/state, precisely identified candidate processes and disposable provider sessions with observed native identity and registered lineage.
Register only an actual parent session and spawn its child through `hide agent spawn`; never seed a capability or coordination ledger.
Exercise idle/done delivery to a Claude pane with a statusline and to a Codex pane, working delay, a pending letter held while a permission, question or plan menu is open and delivered after it is answered, each target's menus read as Herdr `blocked` or, for Codex's plan approval, held as `awaiting_operator`, hook confirmation/restart, capacity/corruption, watch clocks/reset/exit/reply and helper privacy/fallback.
Label protocol fixtures separately from actual provider runtime observations.
Measure matched baseline/candidate input latency and idle/driven load through [PERFORMANCE_TESTING.md](PERFORMANCE_TESTING.md); a headless or socket-only check does not prove native presentation.
Own every fixture process with a deadline and teardown, preserve the operator's app/server/panes/hooks, and remove private authentication caches after the owned agents exit.
Run evidence stays under ignored `agents/runs/`; it is never committed with this guide.
