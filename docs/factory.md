# Software Factory

The Software Factory takes Tasks whose requirements are clear, runs them through worker agents in dependency order, verifies and merges them, and hands a person only what needs a person.
This guide owns the Factory's engine, its store, its roles, its commands and the read model a screen draws.
`hide-factory/`, the host thread in `herdr-core/src/factory.rs` and `herdr-core/src/runtime/factory.rs`, and the `hide factory` route in `hided/` are its executable authority.
[delivery.md](delivery.md) owns the mailbox the Factory writes through, [AI_PROVIDERS.md](AI_PROVIDERS.md) owns the provider boundary its judgments use, and [ARCHITECTURE.md](ARCHITECTURE.md#the-factory-host) owns the thread and lock rules.

Three deciders work in a fixed order, and a decision that ends at a lower one never rises.
Code runs the rules and enforces every boundary: states, order, slots, merges, reverts, roles and caps.
A tool-less judgment may only make work slower or safer: it adds a question, a dependency or a flag, and never removes, widens or approves.
A person sets intent, loosens a gate, widens a scope and decides what cannot be undone.

## What a Factory is and who owns what

A Factory is one project's work queue.
A project has at most one Factory, named by the project's canonical primary checkout path; its id is `f-` and the first ten hex digits of that path's SHA-256.
A Factory whose project has a `github.com` origin takes its Tasks from GitHub issues and merges pull requests (a GitHub Factory).
Any other Factory keeps its issues in the core's local issue store as `L-<number>` and merges into the local default branch (a local Factory).
A GitHub Factory acts as the login `gh` is signed in as, and only after a person approved that account, the repository and the list of reads and writes `init` shows; the Factory keeps that approval as `github_approval`, and a Factory without one refuses every GitHub write (`github.approval`) before calling `gh` or pushing.
Closing a Factory keeps its records, and creating a Factory for the same project again brings them back.

| Owner | What it owns |
| --- | --- |
| `hide-factory` crate | Values (`model.rs`), the store (`store.rs`), the dependency graph (`dag.rs`), roles and permissions (`role.rs`), the command contract and its printing (`command.rs`), the judgment features (`judgment.rs`), the read model (`summary.rs`), the adapter traits (`adapters.rs`), the engine (`engine.rs`) and the real git, `gh` and verify adapters (`project.rs`, `exec.rs`, `verify.rs`). It does not know the runtime or Herdr. |
| Host thread in the core | `herdr-core/src/factory.rs`: the engine thread, its request queue, the worker port (spawn, letters, sleep, wake, worktree removal), the judgment thread and the notifier. `runtime/factory.rs` holds the few reads and requests that take the runtime lock for owned data. |
| `hided` | The pane-capability `factory` request in `server.rs` (`ScopedRequest::Factory`) and the `hide factory` parser and printer in `factory_cli.rs`. |
| `hide-kit` | `hide_kit::layout::{factory_store, factory_files}` name the store and its files folder. |
| `hide-agent-hooks` | The session guidance sentence that sends agents to `hide factory add`. |

A command travels as one scoped request.
`hide factory ...` sends a `factory` request over the pane-capability socket, which needs no open renderer and refuses a caller from a connected device with `factory_local_only`.
The request is at most 256 KiB and is answered within 100 seconds.
The core checks the caller as delivery does, so a moved checkout returns `caller_context_changed`, and queues the command for the engine thread.
The queue holds 32 commands; a full queue answers `factory_busy` and a missed answer window answers `factory_timeout`.
The engine decides the caller's role, runs the command, saves the change, and answers.
A worker's `ask`, `block`, `propose` and `done` take the letter path described under [The mailbox recipient](#the-mailbox-recipient-factoryid).

## The store

The source of truth for every Factory on the machine is one SQLite file, `factory.sqlite3`, in the state folder, with a private folder `factory-files/` beside it.
The file runs in WAL mode with `synchronous=NORMAL`, foreign keys on and a two-second busy timeout.
Both are private to the user: the file is mode 0600 and the folder 0700 on Unix.

| Table | Holds |
| --- | --- |
| `factories` | One JSON record per Factory, with its project, closed flag and creation time. |
| `tasks` | One JSON record per Task, with indexed state, issue, priority and times. The record carries the card, questions, discoveries, decisions, attempts, worker, pull request and gates. |
| `dependencies` | One row per dependency edge, rewritten with its Task. |
| `events` | Ids-and-stage records of transitions, external calls and failures, never card text or secrets. |
| `records` | Each judgment's input and output and each letter's body to or from a worker, kept with its Task and cut at 256 KiB (D-58). |
| `meta` | The machine-wide worker limit and the ids of the last 4,096 applied letters. |

`PRAGMA user_version` carries the schema version, now 1.
A store written by a newer build is refused rather than read: the engine does not start, the host logs `store.open_failed`, and commands answer `factory_unavailable`.
Fields added later load from older rows through serde defaults.
The engine loads the whole store at start and saves each change before the command that made it answers, so a restart resumes Factories, Tasks, questions, workers and in-flight work.
A restart starts verification again from the recorded attempt and asks a pending review again.
A store write that fails does not stop the engine: it is counted, `hide factory status` says how many failed since start, and the host writes each one to the diagnostic log (`store.write_failed`, with the Factory, Task and stage).
A main recovery the restart cut short is not guessed again, since which merge it was finding or reverting lived only in the old process: it goes to a person with the same choices as a recovery that could not decide.
The engine opens the store when a store file already exists at start, or on the first command, so a machine that never created a Factory opens nothing.

`factory-files/<factory id>/<task id>/` holds each attached PRD as `<sha256>.<extension>`, a read-only copy of at most 4 MiB, and `factory-files/logs/` holds one `<run id>.log` per local verification run.
A run's log keeps only its last 1 MiB when the run ends; a run that writes more than 256 MiB is ended and fails as "output over the log cap".

The Factory deletes no Task, question, decision, discovery, attempt or attachment.
It removes a worker's worktree and pane when the Task is done and main verification has passed, when a cancelled Task or one an outside pull request took passes its keep period, and, only under disk pressure, for finished Tasks.
An outside pull request that takes a Task stops its worker and starts the keep period at that moment; the worktree waits the period out even when that pull request merges and the Task is done, because the worker's own work was never merged.
The removal runs from the repository's main checkout through the host's guarded worktree removal.
A finished Task keeps its branch; a cancelled or outside Task loses its local branch with its worktree, and the Factory never deletes a remote branch.
A finished Task's worktree goes only when it is clean: uncommitted or untracked leftovers keep it, and a notice names the folder for a person to look at. Only a cancelled or outside Task past its keep period, or a start abandoned on its way, has what is left in its worktree discarded.
A finished Task folds out of the board's Done column after 3 days and out of every list after 90 days; its page still opens.
Each Factory keeps its newest 20,000 events and drops older ones.
Judgment inputs and answers and letter bodies are kept in `records` and never deleted automatically; `Engine::records` reads a Task's, and the core's diagnostic log records each judgment's feature, ids, outcome and duration.

## Tasks, states and the DAG

A Task is the Factory's unit of execution.
It has a Factory-local id `T-<number>` that is never reused, and, once it is Ready, an issue: `#<number>` on GitHub or `L-<number>` locally.
A Task is shown by its issue number when it has one and by its Task id before.
A command names a Task by Task id, issue, or `<factory id>/<task id>`; a reference that matches Tasks of two Factories answers `task_ambiguous`.

The card has a title (one line, at most 200 characters), a goal, at least one completion criterion, out-of-scope items, open decisions, dependencies (`--after`) and external waits (`--external`).
A person sets four more fields on it: direct review, priority (-100 to 100), merge mode and runtime, and an empty field takes the Factory's default.
Text fields are capped at 4,000 characters, lists at 30 items, and a Factory at 5,000 Tasks.
A card that misses a field, has no criterion, names a dependency that does not exist or is cancelled, or closes a loop is refused at once with a `card_invalid` answer that lists each field and problem.
The producer writes the card; the review may add questions, dependencies and flags and never edits the card.

### States

| State | Label | Meaning | Slot | Board column |
| --- | --- | --- | --- | --- |
| `drafting` | 정리 중 | Open questions remain or the review has not answered. | no | drafting |
| `waiting` | 대기 | Ready; waits for predecessors, a slot or the environment. | no | waiting |
| `running` | 실행 중 | A worker holds a slot. | yes | running |
| `paused` | 일시정지 | A person paused it; the worker sleeps. | no | running |
| `blocked` | 막힘 | Waits for an answer or a predecessor; the worker sleeps. | no | running |
| `verifying` | 검증 중 | The worker reported done; checks, verification and the pre-merge steps run. | no | running |
| `merge_waiting` | 머지 대기 | A person merges, requests changes or cancels. | no | running |
| `landed` | 머지됨 | Merged; main verification has not answered yet. | no | running |
| `done` | 완료 | Main verification passed or none exists, or an outside pull request merged. | no | done |
| `stopped` | 멈춤 | The worker went quiet, a start was refused, or a limit was reached. | no | running |
| `relanding` | 다시 올리기 | The merge was reverted; the Task runs again on the latest main. | yes | running |
| `outside` | 밖에서 진행 중 | An outside pull request closes its issue. | no | running |
| `cancelled` | 취소 | Kept for the keep period, then purged. | no | off the board |

The main line is `drafting`, `waiting`, `running`, `verifying`, `merge_waiting` when a person merges, `landed`, `done`.
`blocked`, `paused` and `stopped` branch off `running`, and `relanding` returns to `verifying` through the worker.
A Task stops with one of these reasons: no report, stalled, verification failed three times, new-Task cap, the same environment failure repeated, or a refused worker start.

A person's actions depend on the state, and any other action is refused with `action_not_allowed_in_state`, the current state and the allowed list.
An open question adds `answer` to any state's list.

| State | Actions |
| --- | --- |
| `drafting` | `edit` (add again), `cancel` |
| `waiting` | `priority`, `dep-remove`, `cancel` |
| `running` | `pause`, `cancel` |
| `paused` | `resume`, `cancel` |
| `blocked` | `answer` only |
| `verifying` | `cancel` |
| `merge_waiting` | `merge`, `request-changes`, `cancel` |
| `stopped` | `retry`, `cancel` |
| `outside`, and `cancelled` inside its keep period | `revive` |
| `landed`, `done`, `relanding`, and `cancelled` after the keep period | none |

### When a Task may start

A `waiting` or `relanding` Task starts when all of these hold.
Its Factory is open.
Every Task in its `depends_on` is `landed` or `done`: a predecessor must be merged, and a missing one counts as unmerged.
The environment holds no start (see [Environment](#environment)).
A slot is free.
External waits are shown on the Task and never decide a start.
A Task that waits holds back only the Tasks below it in the chain; every other Task keeps running.

Slots are counted per machine, across every Factory.
The machine-wide limit is `max_workers`, 5 by default and between 1 and 64, and it counts Tasks in `running` or `relanding` plus workers still starting.
Free slots go first to `relanding` Tasks, which already hold theirs, then by priority from high to low, then by the older creation time, then by Task number.
A priority change or a removed dependency applies at the next tick.
A person's dependency removal is the only way to loosen an order, and a worker may only add a dependency to its own Task.
Adding a dependency to a running Task whose predecessor is unmerged puts the Task to sleep as `blocked`; it waits for a slot again when the predecessor merges.

## Roles

Each command runs with a role the engine decides from its own store, never from the caller's claim or a file.
A caller is a worker when its pane is the pane of a Task's worker, or when its working folder is inside a Task's worktree.
A `cancelled` or `done` Task's worker stays a worker while its worktree and pane stay, so its pane never gains an operator's commands, and the Task's state refuses its reports; a live Task wins when a pane or folder was reused.
Once a Task's worktree is removed it binds no pane or folder, because Herdr can give a closed pane id to the operator's next pane.
An agent a worker spawned acts as that worker: the host follows the caller's spawn lineage in the delivery ledger (up to 16 agents up) and binds an ancestor by its agent id or by the pane it was registered on, and another pane a pane-bound caller names besides its own only ever makes it a worker, never an operator.
A lineage the host cannot read to its root (the ledger unreadable, a missing record, a loop, more than 16 agents) may hide a worker above, so that caller can only read; anything else is refused with `lineage_unknown`.
The walk starts from the newest record on the caller's pane even when that record has ended, because an agent can end its own record and keep running, and from that same agent's newest record that names a parent when it registered again without one; an ended ancestor lends no pane to bind by, because Herdr reuses pane ids. A record ends through `hide agent end`, and by itself when Herdr no longer has its pane ([delivery.md](delivery.md#agent-registration-and-spawning)), so an ancestor whose pane closed lends no pane id from then on; until the core has read that host's panes again it still does. A caller below a Factory's own agent that no Task holds as its worker (a start abandoned on its way, or a shell on a reused pane id) can only read; anything else is refused with `factory_agent_unbound`.
Every other caller is an operator, recorded by pane id, or `checkout` when the caller has no pane.
A checkout-bound caller (a plain terminal, or a tool shell in Codex's shared app-server daemon) is the operator acting without a pane: the pane it names is never read as its identity or lineage, and only its checkout folder can make it a worker.
The engine itself acts as a third role for deadlines and timers and is never a command caller.
A capability file holds only a token, so editing it cannot change a role.

| Permission | Commands | Worker | Operator |
| --- | --- | --- | --- |
| Read | `status`, `show`, `inbox`, `config` without `--set` | yes | yes |
| Report | `ask`, `block`, `propose`, `done`, `decide` | its own Task | refused |
| Add a dependency | `dep add` | its own Task | yes |
| Intake | `init`, `add`, `check` | refused | yes |
| Answer | `answer` | refused | yes |
| Loosen | `dep remove`, `priority` | refused | yes |
| Merge | `merge`, `request-changes` | refused | yes |
| Control | `pause`, `resume`, `retry`, `cancel`, `revive`, `close` | refused | yes |
| Configure | `config --set` | refused | yes |

A refusal answers `role_not_allowed` with the role and the verb.
A Task keeps at most 500 questions, decisions and discoveries together; past that a worker's report is refused with `report_limit`, except `done`, which still finishes the Task without storing its summary, and `block`, which still stops it for a person.
A worker's `show` reaches its own Factory only.
An operator relays a person's words, and every answer and decision records the relaying pane as `relayed_by`.
Code cannot tell whether an operator pane's agent acted on a person's words, so it records who relayed and does not block.

## The mailbox recipient `factory:<id>`

Each open Factory is a code-owned recipient named `factory:<factory id>` in the delivery ledger.
[delivery.md](delivery.md#the-code-owned-recipient-factoryid) owns the ledger rules; this is how the Factory uses them.

- Only the engine sends as the Factory. The host holds that authority, and the ledger accepts it only while the Factory exists and is open.
- A pane may send to `factory:<id>` and may name it as a watch observer, and it can address the Factory by that name only. A pane or agent can never register under a name or pane that starts with `factory:`.
- The engine reads its letters from the ledger on each tick, up to 16 at a time, applies each once, and confirms it. A request or block letter also gets a reply carrying the engine's JSON answer. No composer input or doorbell is involved.
- The letter's sender must be a live worker of that Factory, or the letter is refused with `sender_not_a_worker` and logged.
- A worker's CLI report is a typed body `{"factory": <command>}` sent under an intent made from the pane, the Task's current state time and the body, so a retry in the same state applies once and the same report after the Task moved (a `done` again once verification sent it back) is a new letter. A harness that follows the letter protocol only is read as a question (a `request` or `block` letter becomes a blocking question with a 24 hour deadline, taking a `Recommendation:`, `Suggestion:`, `추천:` or `제안:` line as the suggestion) or as a completion (a `report` letter becomes `done`).
- A watch whose observer is the Factory warns after the Factory's `stall_minutes` (30 by default) instead of 20; the host hands each open Factory's window to the watch readings, so a change applies at the next reading. The engine turns the first warning into a stalled stop; it does not wait for the second warning or for the human notice, which the ledger skips for a code-owned recipient.
- The Factory writes to a worker with `report` letters, and starts a watch on the worker again each time it does, because a report to its parent ends the sender's watch.

## The worker lifecycle

A worker starts when the Task may start.
The engine asks the core to spawn it through coordination, the same path as `hide agent spawn`, under the Factory's own agent record as parent.
The spawn runs on the host's starter thread, one at a time with at most 16 waiting, because it waits for Herdr to make the worktree and start the agent: the engine keeps the Task's slot, answers commands and worker reports meanwhile, and takes the started worker on the next tick. A Task cancelled or paused while its worker starts on the starter has that worker ended when it arrives, and a worktree that start made removed; its next start is a new attempt with its own spawn intent. A Task cancelled while its agent had not shown its session yet keeps its attempt, so a revive asks the same spawn again and gets the pane and worktree it already made. A Task relanding after a revert keeps its restart while it waits. A restart of a gone worker waits on the starter the same way and holds its slot meanwhile, and a spawn lock another spawn holds is asked again in two seconds. When the host stops, starts still queued are dropped, not run.
The record is registered when needed and converges on the existing one.

- **Name, branch, intent.** The worker is named `factory-<project name>-<task id>` and works on `factory/<issue number>-<slug>` (`factory/l<number>-<slug>` for a local issue, `factory/<task id>` without a slug). The spawn intent is `factory-<factory id>-<task id>`, and each earlier refused start adds `-a<n>`, so a refused start never repeats a spawn that already failed and a retry of the same attempt converges.
- **Runtime.** The Task's runtime, else the Factory's `default_runtime`. The arguments in `worker_args` for that runtime go to the agent before the prompt.
- **First prompt.** One argument, at most 6 KiB, cut at a character boundary with a pointer to `hide factory show <task>` for the rest. It holds the Task, goal, criteria, out-of-scope items, the read-only absolute path of each attachment, the harness instruction when one is set, and the reporting rules: commit on the branch, never push or merge to the default branch, finish with `hide factory done`, ask with a default or block, report discoveries, and a turn that ends without a report stops the Task. The prompt text is written in Korean. When a `hide` program sits beside the daemon, the prompt names its absolute path and says every `hide` in the rules means that program, so a worker never reaches an older copy on its `PATH`.
- **Lineage and watch.** The spawn writes lineage and starts a watch whose observer is the Factory.

A worker whose pane exists but whose agent has not shown a session yet is still starting.
It holds its slot, the Task stays `waiting`, and the same spawn is asked again every 30 seconds.
After 10 minutes a notice asks the person to look at the pane, where a trust or login prompt may be waiting.
A Codex worker waits for this Mac's install kit to have read the machine since launch: the first start asks the kit to read and reports the worker as still starting.

A start the runtime refuses, other than for an environment signal, stops the Task with the reason "worker start failed" and the runtime's reason (cut at 300 characters) beside it.
A person fixes the cause and retries; the next attempt uses a new spawn intent.
A start that fails with an environment signal leaves the Task `waiting` and is handled by the environment rules.

A Task's worker is put to sleep when the Task blocks, pauses, goes to `verifying`, or waits for a slot, and woken when it runs again.
Sleep goes through agent sleep and is deferred until the agent's turn ends; a worker that never slept is sent the message instead.
Stopping a worker, when its Task is cancelled or an outside pull request takes it over, ends its ledger record at once and puts its agent to sleep the same way, so the pane and session stay for `revive`.
A wake restarts the agent in the same pane and session.
Letters for a woken worker are held, at most 16, until its agent is back, and are dropped to the diagnostic log after 10 minutes.
A retried Task, or a worker whose pane is gone, spawns again in the same worktree and session with a fresh intent.
A worker that is `gone` is detected from its pane, with a three-minute grace after its start.

A worker reports through `hide factory` and gets its answer in the same call.
`ask` needs a suggestion, a default action and a deadline of 1 to 720 hours, and the worker continues with the default; `block` has no default, so the Task blocks, releases its slot and sleeps.
`done` answers at once, goes to `verifying`, and puts the worker to sleep.
On a GitHub Factory the next tick pushes the worktree's commits to the Task branch (with a lease, so a rebase goes through), then finds the open pull request for that branch or opens one, and only then reads CI on the pushed commit; every report pushes, so a fix after a failed check reaches the same pull request, and a merged pull request from before a revert is never reused.
A Task branch the remote deleted, as automatic branch deletion does after a merge, is pushed again rather than refused by a stale lease.
Only a pull request from the repository's own branch is a Task's or a revert's; a fork's pull request on the same branch name is never adopted.
A push or pull request refused twice in a row with no environment signal in between (a protected branch, a hook; git's transport errors such as a 5xx answer or a timeout count as the network's, while a missing key, a gone repository or a hung-up remote reach a person) stops the Task as "push 거절됨" with the reason; a person fixes the cause and retries. One with an environment signal is asked again every minute.
A failed push or pull request is tried again a minute later.
`decide` records a decision.
A worker whose agent rests for the no-report window (`no_report_minutes`, 2) after a turn that reported nothing stops the Task as "no report"; a worker that goes quiet for the stall window (`stall_minutes`, 30) stops it as "stalled".

## Intake review and judgments

`hide factory add` checks the card's shape first, in milliseconds, then asks for an independent review.
The review sees only the card, its attachment, the repository's file names and guide, and the Factory's other Tasks, never the producer's conversation.
`add` waits up to 90 seconds and answers one of:

- `ready`: no question is open, so the Task is Ready.
- `needs_answers`: the questions to answer, each with a suggestion.
- `split`: the review proposes two or more pieces, each with a goal, criteria and the earlier pieces it follows.
- `pending`: the review has not finished, or could not run; the result goes later to the pane that added the Task when that pane still exists.

Adding again with `--task <id>` or the same issue updates the same Task and reviews it again; the same content changes nothing.
A re-add on a Task that already runs does not rewrite its card: it adds a scope-change question that a person approves or rejects.
A re-add adds dependencies but never drops one the review or a person added, so a producer that resends the card it knows changes nothing; removing a dependency is a person's `dep remove`.
A re-add that leaves out criteria, out-of-scope items, open decisions or external waits keeps the card's.
`dep add` names a Task of the same Factory as the Task it changes, and a watch warning names a Task of the Factory it watched; a warning naming none is only logged.
The producer's open decisions become intake questions, and a Task is Ready only when the review is done and no question besides a notice is open.
Ready creates the issue: a GitHub Task gets an issue labelled `factory` whose body carries a hidden Task marker, the goal, the criteria and the out-of-scope items, and a local Task gets an `L-<number>`.
A Task added from an existing issue gets the label instead.
The marker lets a retry find the issue it already created.
A split that a person accepts turns the original Task into the first piece, keeping its issue, and drafts the rest as new Tasks that depend on the pieces they follow; choosing to proceed clears the split and the Task becomes Ready when nothing else is open.
A Task is never split or made Ready by silence.

Every judgment is a tool-less, one-shot call whose input the code bundles and cuts to 48 KiB, whose answer must match a `factory.v1` schema, and whose deadline is 180 seconds.

| Feature id | Asked | Input | The answer may |
| --- | --- | --- | --- |
| `factory_intake_review` | On add and re-add, an edited issue body, a label-path card, a split piece, an approved proposal | The card, its attachment (24 KiB), up to 100 other Tasks, up to 400 file names, the repository guide (8 KiB), and the description of an autonomy scope the Task claims | Add questions, dependencies on listed Tasks, a split, and flags, and say whether the card fits the claimed scope |
| `factory_drift` | After `done` | The card, the diff against main (24 KiB), the decisions | Pass, or add questions, and flags |
| `factory_check` | At intake or after `done` for each natural-language check | The instruction, the card, the diff | The same as drift |
| `factory_watch` | See [The watch](#the-watch) | The Factory's board summary | Warnings, each with an optional action |
| `factory_env_diagnosis` | See [Environment](#environment) | The collected facts and the closed action list | One action from the list, or an exact command with its impact |

Questions that a drift or check judgment adds always carry a default action, taking the suggestion when the answer has none, so a check can only slow a Task down.
A drift question keeps the Task in `verifying`, does not wake the worker, and holds auto merge until it is answered or its deadline passes; an answer that differs from the default wakes the worker to apply it.
A judgment that cannot run is never skipped and never read as a pass.
A failed review marks the Task and puts a retry-or-cancel question in the inbox, and `add` answers `pending`.
While Hide AI is off or no agent is chosen to run it, every judgment fails as `disabled`, and the question says to turn it on in Settings › Hide AI.
A failed drift or check sends the Task to `merge_waiting` for a person, and so does a check that answers `pass: false` with nothing to ask or flag, or one that cannot read the Task's diff.
A judgment that answers after its Task was cancelled, taken outside or finished is dropped and logged as `judgment.dropped`.
A failed watch or diagnosis changes no Task and is logged.

The Factory has its own judgment queue on its own router (see [AI_PROVIDERS.md](AI_PROVIDERS.md#the-factorys-judgments)): one request in flight, intake reviews before every other judgment, and 16 waiting judgments per Factory.
A judgment submitted to a full queue fails like a provider failure and is escalated the way above.
Natural-language checks are added with `hide factory check --at intake|after-done|periodic`.
A `periodic` check runs over each running Task's card whenever the watch interval comes due; like an after-done check it only adds questions or marks, it never holds a merge, and one that cannot be queued is logged and asked again at the next interval.

## Verification

A Factory chooses one verification, or none.
`ci` reads the required check runs of the Task's pull request and runs nothing itself.
`verify` runs a bundle of shell commands in the Task's worktree, all of which must exit 0.
With none, auto merge is unavailable, and a Task goes from `done` straight to `merge_waiting`.

The Factory never reruns a verification: one failure is a failure, and the worker fixes and reports `done` again.
`n/3` counts those failures per Task, the limit is 3, and the third failure stops the Task as "verification failed three times" with the failing check and the worker's last summary.
A failure wakes the worker with the check name, the log path or CI link, and the count.
Environment failures and merge conflicts do not count.
`retry` resets the count.

**CI.** The check runs of the worktree's head commit are read on each tick with `gh api`.
Every named check must have a completed run, and only named checks decide; a commit with no run of a named check yet is pending, never passed.
`--ci` with no names takes the default branch's required checks from its protection, and `init` and `config ci=` refuse a Factory that would name no check (`ci_checks_required`).
A completed run passes on `success` or `neutral`, decides nothing on `skipped`, `cancelled` or `stale` (still pending), and fails on any other conclusion with the run's link.
A GitHub error that carries an environment signal is the environment's; any other read error stays pending.

**Verify bundle.** Bundles run one at a time on the machine, in a queue of at most 256, each command through the shell with its output in the run's log.
The cap, `verify_timeout_minutes` (60 by default), applies to the whole bundle from its first command, and the command running when it passes fails the run.
A command that exits 137, prints "no space left on device" in its last 64 KiB, or dies with no exit code after printing "killed" is an environment failure; every other non-zero exit is the Task's.
Each run belongs to the engine, which ends its whole process tree when the Task is cancelled or the core stops.

**Before a merge.** When the verification passed and no question holds the Task, the Factory checks the merge in seconds, for a verify or local Factory, and never reruns CI.
`git merge-tree` against the current default branch names conflicting files; a conflict sends the worker back to `running` with a rebase message and does not count as a failure.
An optional `quick_check` command then runs on the Task worktree; its failure counts as a verification failure.
Paths matching `risk_paths` add a gate.
A verify Factory then runs its bundle once more on the Task with the latest default branch merged in, and a failure there counts as a verification failure.

## Merge

A Task's merge mode is its own field, else the Factory's.
In `auto` mode a verified Task that meets no gate is merged by the Factory.
In `manual` mode it goes to `merge_waiting` and a person merges.
Merge mode is always `manual` for a Factory with no verification.

A gate sends an `auto` Task to `merge_waiting` and appears in the Task page and inbox.

| Gate | Set when |
| --- | --- |
| `review_directly` | The card asked for direct review. |
| `approved_scope_change` | A person approved a scope change on the Task. |
| `breaking_change` | The worker reported `--breaking`, or a check flagged a breaking or public-contract change. |
| `no_verification` | The Factory has no verification. |
| `risk_path` | A changed path matches `risk_paths` (empty by default). |
| `manual_mode` | The Task's mode is `manual`. |
| `open_question` | A question other than a default, scope-change or notice question is open. |
| `check_failed` | A judgment could not run. |
| `autonomy_diff` | An autonomy Task changed more than 200 lines. |
| `dirty_main` | A local merge finds the main checkout uncommitted or off the default branch. |
| `merge_refused` | GitHub or git refused the merge for a reason that is not the environment's, such as a branch policy; the reason is in the Task's events and in the answer to a person's `merge`. |

A review by an agent alone never merges.
A person's `merge` runs merge-tree and the quick check again on the latest main, then the same merge at once; a conflict sends the worker to rebase and a failed quick check counts as a verification failure.
A refused merge is tried once and then waits for a person, never once per tick; only an environment signal, or a merge GitHub answered before naming its commit, is asked again, at the GitHub back-off for a rate-limit, server or network signal (a minute for any other signal) and every 30 seconds for an unnamed commit, which is only read and never merged again: a pull request that merged lands whatever gate appeared since, because nothing can undo it, and one that did not merge goes through every check again before another merge; a commit still unnamed after 10 minutes waits for a person as a refused merge.
While a merge's commit is unnamed, a red run on a main head the Factory has not recorded is read as pending, not as an outside push.
`request-changes --comment` returns the Task to `running`, clears its gates and sends the worker the comment.

**GitHub.** The merge is `gh pr merge` with the Factory's `merge_method` and `--match-head-commit` set to the commit the Task-stage verification passed on, so a push after it makes GitHub refuse the merge; a Factory without verification pins the head a person reviewed.
A verified Factory with no passed commit refuses to merge, a pull request already merged returns its merge commit, and a merge GitHub answered before naming its commit is read again rather than recorded empty.
`init` picks the first merge method the repository allows, in the order merge, squash, rebase, and `config` changes it.
**Local.** The Factory merges the verified commit, not the branch's current tip, into the project's primary checkout with `git merge --no-ff`, only while that checkout is on the default branch with no uncommitted tracked changes; a merge that fails is aborted, so the checkout is never left mid-merge.
Otherwise the Task stays in `merge_waiting` with `dirty_main` until it is clean.

A merge records its commit, and the Task becomes `landed`.
Main verification is the chosen verification on that commit: the push run's check runs for `ci`, or the bundle run in `<project>.worktrees/factory-main`, a detached worktree the Factory creates beside the project's worktrees, for `verify`.
A green result, or no main verification, makes the Task `done` and removes its worker, pane and worktree.

A red result breaks main and stops auto merge; manual merges still run.
While main is broken an `auto` Task that passed verification waits in `verifying` without merge-tree or quick-check reads, unless a person's gate sends it to `merge_waiting`; main is read again every 30 seconds.
If the broken commit is among the Factory's own merges since the last green, the Factory finds the first failing one, asking again for runs that were skipped or cancelled, and reverts that merge alone.
On GitHub the revert is a pull request from `factory/revert-<task id>`, which the Factory force-pushes, checks and merges with `--merge`; locally it is a revert commit made in `factory-main` and fast-forwarded into the primary checkout, only while that checkout is on the default branch and clean, as for a Task merge.
A revert that conflicts is aborted in `factory-main`, so that worktree stays usable for main verification and the next revert.
A revert whose verification passes is merged on its own, and the original Task becomes `relanding`: it runs again in the same worktree and session on the latest main, with a new pull request.
When the failing commit is not the Factory's, nothing is reverted: the Factory drafts a fix Task for a person to confirm and stops auto merge.
Auto merge resumes when the head of main verifies green again.
A recovery that cannot name one merge, cannot make or merge the revert, or finds the revert red does not repeat itself: it stays stopped and puts the failed run's link, the candidate merges and these choices in the inbox: retry the revert, revert a named merge, or resume auto merge after a person fixed main.

## Outside work

The person wins outside the Factory, and the Factory follows.
Each open Factory reads GitHub every `outside_read_minutes` (2 by default): up to 200 issues labelled `factory` and 100 pull requests, or, locally, its local issues.
A failed read backs off 1, 2, 5, 15 and then 30 minutes on a rate-limit, server or network signal; three failures in a row mark the Factory's read stale.
A held issue missing from that list is looked up by number, at most 20 per read and continuing where the last read stopped; one that cannot be read now keeps its Task, and a deleted one counts as its label removed.

| What the Factory sees | What it does |
| --- | --- |
| A pull request the Factory did not open closes a Task's issue, and it is open | A pull request one of the Factory's own Tasks opened never counts, whatever its worker wrote; the decisions a worker records sit in a code block in its pull request body, so a "Fixes #N" there stays text. GitHub can still close another Task's issue from a closing keyword a worker put in a commit message or in a card it proposed, when that merge lands; the other Task is then cancelled and kept for revive. A pull request from a fork counts only once it is merged, since anyone can open one. Otherwise the Task becomes `outside`. A running worker is stopped, its worktree stays for the keep period, and a notice offers `revive`. |
| That pull request merges | The Task is `done`, and a dependent Task's predecessor counts as merged. A stopped worker's worktree still waits out its keep period. |
| The issue closes with no pull request, or the `factory` label is removed | The Task is cancelled with a notice, kept for the keep period. |
| A person edits the issue body | A Task before its start goes back to `drafting` and is reviewed again. A running Task gets a scope-change question. The body is compared with the one the Factory wrote, ignoring line endings and surrounding space, so the first edit counts. |
| A person labels an issue the Factory does not hold | A new Task is drafted from the issue, and a person confirms its card in the inbox before it is Ready. The body is not edited. |
| A finished Task's issue reopens | A notice only. |
| A push to main the Factory did not make | Main verification runs; while its checks are still running the head is read again every 30 seconds, and a red result takes the outside-push path above. |

A pull request or issue the Factory does not hold is ignored.
Cancelling a Task closes its open Factory pull request, and reviving reopens it.

## Questions, discoveries and autonomy

A question belongs to a Task card and is answered with `hide factory answer <task> [--question <id>] --choose suggestion|default|<choice>` or `--text`.
Every question has a suggestion, and a worker question has a deadline and may have a default action.
Its kind decides what an answer does.

| Kind | Raised by | An answer |
| --- | --- | --- |
| `intake` | The review, a check, a producer's open decision | Clears it; the Task is Ready when nothing else is open. |
| `split` | The review | `split` splits the Task, `proceed` clears it. |
| `default` | A worker `ask`, a drift or check finding | The default keeps going; a different answer wakes the worker. After the deadline the default is the answer. |
| `blocking` | A worker `block` | Wakes the worker through a reply; the Task waits for a slot and resumes its session. No deadline decides it. |
| `scope_change` | A worker, a re-add, an edited issue | `approve` records approval and adds a gate, and a re-added card and PRD become the Task's, sent to the worker in the reply; `reject` keeps the scope. After the deadline it is rejected. A newer re-add replaces a pending one. |
| `new_task_cap` | The Task reached the new-Task limit | `continue` lets it create more; any other choice leaves it stopped with a notice. |
| `proposed_task` | A prerequisite proposal outside autonomy | `approve` drafts the Task, and the proposer depends on it and gives its slot back until it lands. |
| `action` | A stop, a failed review, a main break | The listed choice runs: `retry`, `retry-review`, `cancel`, `resume-auto`, `retry-revert`, `revert <task>`. |
| `confirm_card` | A label-path card, a fix-Task draft | `confirm` lets the Task become Ready. |
| `proposal` | An environment diagnosis | Records the answer; a person runs the command. |
| `notice` | Anything a person should only see | `ok` removes it; answering it on a finished Task also clears its unread mark. |

A question with a default lets the worker continue, and the Task waits only at the end of verification for the answer or the deadline.
A blocking question releases the slot and stays open however long it waits; the inbox shows how many days.
A worker may report five classes of discovery with `hide factory propose --class`:

| Class | Effect |
| --- | --- |
| `in-scope` | A recorded fix inside the card. |
| `decision` | A recorded decision the agent made, reversible and inside the criteria. |
| `scope-change` | A scope-change question; the worker continues inside the current scope. |
| `prerequisite` | A new Task proposal. |
| `unrelated` | A notice in the inbox. |

`--reclassify <discovery>` may only move a discovery toward a person, in the order `in-scope`, `decision`, `prerequisite`, `scope-change` and `unrelated` (the last two being equal); the reverse answers `reclassify_away_from_person`.
A prerequisite needs a card.
A Task that was itself proposed or started by autonomy may not propose a Task (`proposal_depth_exceeded`), and each Task may propose `new_task_limit` Tasks (3 by default) before it stops with the "new-Task cap" question.
A proposal that names an enabled `--autonomy` scope drafts the Task now and makes the proposer depend on it and sleep as `blocked`; any other proposal is a question that a person approves before the Task is drafted, and approving a prerequisite makes the proposer wait on it the same way.
The worker's claim is not the fit: the intake review reads the scope's description and must answer that the card fits it, or the Task loses its autonomy and stays `drafting` with a question a person answers before it runs.

Autonomy scopes are the kinds of Task a worker may start without a person.
The presets are `flaky_test`, `dependency_patch`, `lint_format` and `docs_links`, all off, and a person may add scopes by name.
An autonomy Task is reviewed like any other, may not propose Tasks, and gates to a person when it changes more than 200 lines.

## Environment

An environment problem is not a Task's failure and is never counted against it.

- **Before a start.** Starts hold while free disk at the first waiting Task's project is under `disk_floor_gb` (20 by default), or while macOS reports critical memory pressure. Warn pressure starts normally, and other systems report normal. The held Tasks show the reason and are checked again after a minute or when a worker stops; without a hold the machine is read on a tick where a start can happen (a free slot and a runtime not at its usage limit) and otherwise once a minute, so a backlog waiting on full slots is not read every tick.
- **At a failure.** Only a structured signal makes a failure the environment's: no space left on the device, exit 137, a GitHub 401, 403, 429 or 5xx answer, a network error, or a Herdr connection error. The adapters read the GitHub and git signals from the command's exit code and its stderr text. Everything else is the Task's. A runtime's usage limit comes from the core's provider usage rows (the toolbar's Weekly Usage reader): a main row or bucket at 100 percent with a reset ahead parks that runtime until the reset, new starts of an unpinned Task use the other runtime, and a worker that ends its turn without a report while its runtime is limited waits for a slot instead of stopping. The rows refresh every 5 minutes only while a window shows them, so a daemon with no window open learns of a limit late.
- **Code handles each signal first.** Disk full holds starts and removes the worktrees of finished Tasks and cancelled Tasks past their keep period. Out-of-memory and Herdr connection errors send the Task back to `waiting`. A rate-limit, server or network error backs the read off as above. A 401 or 403 puts a notice in the inbox once, naming `gh auth login` or `gh auth refresh -s <scope>`, and the Factory does not perform that action or retry it.
- **Cascade.** Three different Tasks failing the same check or command within 30 minutes are read as the environment: each failure is taken back, the Tasks go to `waiting`, and new starts halt for 30 minutes.
- **If it does not clear.** A problem that outlasts 30 minutes asks `factory_env_diagnosis` once for a cause and either one action from the closed list or an exact command with its impact.

The closed recovery list is the only set of actions the Factory runs without a person, and `config --set recovery=<action>=on` turns each on; all are off. Each action touches only the diagnosed Factory's Tasks, except the start hold and the runtime switch, which belong to the machine.

| Action | Does |
| --- | --- |
| `remove_finished_worktrees` | Removes the worktree of the Factory's finished Tasks and of its cancelled Tasks past their keep period. |
| `restart_worker` | A Task stopped by a refused start, a repeated environment failure, no report or a stall goes back to `waiting`, and its worker starts again in the same worktree and session. |
| `sleep_wake_worker` | A running worker waiting on input is asked to sleep and woken in the same session with a note to carry on. Herdr does not put an agent that waits for a person to sleep, so for such an agent the note waits for its next prompt. |
| `switch_runtime` | New starts of unpinned Tasks leave the Factory's default runtime for an hour, while the other runtime is not limited; the runtime is the machine's, so this holds for every Factory. |
| `retry_reads_and_reconnect` | Clears the Factory's read back-off and the machine's start hold. |

A diagnosis naming an action that is off becomes a proposal that the Factory runs once a person answers `approve`; any other command becomes a proposal with the exact command and impact, which a person runs themselves or dismisses.
Logging in, deleting outside the Factory and installing tools are only ever proposals.
A Task that alone repeats an environment failure three times is the Task's: it stops as "same environment failure repeated".

## The watch

The watch reads a Factory's board for what a person cannot already see in the inbox, such as work that stopped moving, a chain held by one wait, or a pattern of failures.
It runs when a Task finishes, when main breaks, when a Task reaches its new-Task limit, and every `watch_interval_minutes` (30 by default, at least 5) for a Factory that has Tasks.
It asks `factory_watch` with the Factory's board summary.
A warning without a proposed action goes to the diagnostic log only.
A warning with an action becomes a notice on the Task it names, or on the Factory's last Task, and counts against `watch_daily_limit` (5) per Factory per UTC day; a warning past the limit is logged as capped.
A warning about a Task that already waits on a person is logged and not raised.
A watch that is slow or fails changes no Task.

## Configuration

`hide factory config [--project <path>]` prints the Factory's settings and the machine's worker limit, and `--set <key>=<value>` changes them for the next decision.
Only an operator may set values, and an invalid key or value answers `config_invalid`.
A Factory's `merge_mode` cannot be `auto` while it has no verification (`auto_needs_verification`).

| Key | Value | Default |
| --- | --- | --- |
| `merge_mode` | `auto` or `manual` | `manual` until `init` chose a verification, then `auto` |
| `merge_method` | `merge`, `squash` or `rebase` | The first the repository allows (merge, squash, rebase) |
| `verify` | Commands joined by `&&&`; empty clears | none |
| `ci` | Comma-separated check names | none |
| `no_verification` | Any value | none |
| `quick_check` | A shell command; empty clears | none |
| `max_workers` | 1 to 64, machine-wide | 5 |
| `question_deadline_hours` | Hours | 24 |
| `stall_minutes` | Minutes, at least 1: the quiet window of each worker's watch | 30 |
| `no_report_minutes` | Minutes, at least 1, after a turn ends without a report | 2 |
| `watch_interval_minutes` | Minutes, at least 5 | 30 |
| `watch_daily_limit` | Count | 5 |
| `outside_read_minutes` | Minutes, at least 1 | 2 |
| `cancel_keep_days` | Days | 7 |
| `done_fold_days` | Days | 3 |
| `archive_fold_days` | Days before a finished Task leaves the list | 90 |
| `new_task_limit` | Count | 3 |
| `verify_failure_limit` | Count, at least 1 | 3 |
| `autonomy_diff_limit` | Changed lines an autonomous Task may merge alone | 200 |
| `verify_timeout_minutes` | Minutes per bundle run, at least 1 | 60 |
| `disk_floor_gb` | Gigabytes | 20 |
| `default_runtime` | `claude` or `codex` | Claude Code when `init` finds it on the path, else Codex when it finds that, else `claude` |
| `harness` | `<name>:<instructions>`; empty clears | none |
| `autonomy` | `<scope>=on` or `off` | all off |
| `recovery` | `<action>=on` or `off` | all off |
| `worker_args` | `<runtime>=<arguments>`, split on spaces; empty clears | none |
| `risk_paths` | Comma-separated: `dir/**`, `*.ext`, or an exact path or folder | none |
| `prd_in_issue` | `on` or `off` | off |
| `macos_notifications` | `on` or `off` | off |

`max_workers` is the machine's, not the Factory's, and any Factory's `config` sets the same value.
`worker_args` holds the whole argument list for one runtime, such as a permission mode the operator chose, and an empty list removes it.
`harness` places the named instruction in each worker's first prompt; gates never trust a harness's own claims, and a harness's own checks belong in `verify`.

## The `hide factory` command

```
hide factory init <project> [--ci [<check>...]] [--verify <command>]... [--no-verification] [--merge auto|manual] [--confirm]
hide factory add [--task <id>|<issue>] --title <t> --goal <g> --criterion <c>... [--out-of-scope <s>]... [--open <decision>]... [--after <task>]... [--external <ref>]... [--prd <path>] [--review-directly] [--priority <n>] [--merge auto|manual] [--runtime claude|codex] [--project <path>]
hide factory status [--project <path>]
hide factory show <task>
hide factory inbox
hide factory answer <task> [--question <id>] [--choose suggestion|default|<choice>] [--text <answer>]
hide factory ask --question <text> --suggestion <text> --default <action> [--deadline-hours <n>]
hide factory block --question <text> --suggestion <text> [--deadline-hours <n>]
hide factory propose --class in-scope|decision|scope-change|prerequisite|unrelated --text <text> [--title <t> --goal <g> --criterion <c>...] [--autonomy <scope>] [--reclassify <discovery>]
hide factory done [--summary <text>] [--breaking]
hide factory decide --text <decision>
hide factory config [--project <path>] [--set <key>=<value>]...
hide factory priority <task> <n>
hide factory dep add|remove <task> --on <task>
hide factory pause|resume|retry|merge|cancel|revive <task>
hide factory request-changes <task> --comment <text>
hide factory check --at intake|after-done|periodic --instruction <text> [--project <path>]
hide factory close [--project <path>]
Add --json to print the answer as JSON.
```

| Command | Does |
| --- | --- |
| `init` | Without `--confirm`, shows the source, the verification candidates the code detected (the branch's required checks and verify commands read from `Cargo.toml`, `package.json`, `Makefile` or `pyproject.toml`), the merge mode and, on GitHub, the login `gh api user` names, the repository, and everything the Factory reads and writes there, and writes nothing. With `--confirm` and a verification choice, creates the Factory, records that approval (account, repository, time) in its history as `github.approved`, and, on GitHub, creates the `factory` label. A closed Factory is reopened with `--confirm`. A project that already has an open Factory answers with it. |
| `add` | Creates a Task or updates the named one. A positional issue reads its title and body for missing fields. `--prd` copies the file into the store. |
| `status`, `show`, `inbox` | Read the board, one Task page, and the inbox. |
| `answer` | Answers a question and records who relayed it. |
| `ask`, `block`, `propose`, `done`, `decide` | A worker's reports on its own Task. |
| `config`, `check` | Reads and sets settings, and adds a natural-language check. A check cannot be removed once added. |
| `priority`, `dep`, `pause`, `resume`, `retry`, `merge`, `request-changes`, `cancel`, `revive`, `close` | A person's actions. `close` needs no Task in the board's running column. |

Without `--project`, `add`, `config`, `check` and `close` use the only open Factory, and answer `factory_ambiguous` when there are several.
A project path is made absolute by the CLI.
`--json` prints the engine's answer as one JSON object, and without it the answer is printed for a person.
Every answer carries `ok`.
A refusal prints `refused: <reason>`, the current state and allowed actions or the invalid fields when it has them, and `next: <action>`, and exits non-zero; its JSON is `{"ok": false, "reason", "next_action", "detail"}`.
`add` answers `{result, task, questions}`, `init` a preview (its `github` is `{account, repo, reads, writes}` or `null` for a local project), `created` or `existing` object, `show` `{task: <TaskDetail>}`, `inbox` `{count, items}`, `status` a `FactorySummary`, and `config` `{config, machine}`.
Other actions answer `{message, task}` with the Task's id, display id and new state.
Common refusal reasons are `role_not_allowed`, `task_not_found`, `task_ambiguous`, `factory_not_found`, `factory_ambiguous`, `factory_closed`, `card_invalid`, `action_not_allowed_in_state`, `config_invalid`, `auto_needs_verification`, `github_login_required`, `github_permission_missing`, `main_dirty` and `revive_expired`.

## The read model

`FactorySummary` and `TaskDetail` are the contract for any screen of the Factory, and `status`, `inbox` and `show --json` print them as they are.
Every number in them is derived in `hide-factory/src/summary.rs` from the stored Tasks, and a screen computes nothing.
Fields are added and never renamed or removed.
The engine's sentences in `state_label`, `waiting_for`, `result`, `remaining`, `gates` and `stop` have a code beside them, so a screen can say them in any language; the codes are listed under [Codes](#codes) and pinned by `every_summary_code_is_pinned` in `summary.rs`.
An inbox item's `text` carries only its `kind`, and a question's own text, a split's piece count, a stop's detail and `TaskDetail.verification` have no code yet.

**`FactorySummary`**

| Field | Meaning |
| --- | --- |
| `my_turn` | The one person-facing number: the open inbox items across all Factories. |
| `factories` | One `FactoryView` per Factory. |
| `inbox` | Every `InboxItem`, in the inbox order below. |

**`FactoryView`**

| Field | Meaning |
| --- | --- |
| `id`, `project`, `project_name` | The Factory's id, canonical path and short name. |
| `source` | `github` or `local`. |
| `verification` | `ci`, `verify` or `none`. |
| `closed` | Whether the Factory is closed. |
| `flow` | `drafting`, `waiting`, `running` and `done_today` counts; `done_today` counts by the machine's local day, read through the clock port's UTC offset. |
| `my_turn` | This Factory's inbox items. |
| `columns` | The four board columns, each with its Task cards in order. |
| `cancelled` | Cancelled Tasks, off the board. |
| `graph` | Nodes, reduced edges and unrelated Tasks. |
| `dependencies` | Every edge as (predecessor, Task); the graph's edges are a reduction of these. |
| `outside_read_at` | When outside work was last read. |
| `stale` | Three outside reads failed in a row. |
| `main_broken` | Auto merge is stopped by a broken main. |
| `auto_merge_available` | The Factory has a verification. |
| `merge_mode` | `auto` or `manual`, `manual` when there is no verification. |

**`CardView`**

| Field | Meaning |
| --- | --- |
| `task`, `display_id` | The Task id, and the issue number once Ready. |
| `column` | `drafting`, `waiting`, `running`, `done` or none. |
| `title` | The card title. |
| `state`, `state_label` | The state id and its label, with the stop reason for a stopped Task. |
| `needs_person` | The Task is blocked, stopped, `merge_waiting`, or has an open question that is not a notice. |
| `waiting_for` | For a waiting Task, the predecessors by display id, the environment hold, or `slot`; for a blocked one, `answer` or `predecessor`. |
| `waiting_code` | The same as a `WaitingFor` code. |
| `waiting_on` | For `predecessors` on a waiting Task, the display ids of the predecessors it waits on, for a waiting or blocked Task. |
| `env_hold` | For `environment`, the `EnvHold` code. |
| `stop` | For a stopped Task, the `StopReason` code behind `state_label`. |
| `priority` | The Task's priority. |
| `since` | When it entered its state. |
| `unread` | A completion nobody has looked at: an operator's `show` of the Task, or answering a notice on it, clears it. |
| `folded` | A completion older than 3 days, or a purged cancelled Task. |
| `archived` | A finished Task older than 90 days. |
| `failures` | Verification failures counted against it. |
| `external` | External waits. |
| `revive_until` | When a cancelled Task can no longer be revived. |
| `worker_pane` | The worker's pane while the Task is not finished. |

**`InboxItem`**

| Field | Meaning |
| --- | --- |
| `group` | `answer`, `merge`, `stopped` or `notice`. |
| `kind` | The question kind, `merge` or `stopped`. |
| `rank` | The order key: 0 blocking question, 1 other answers, 2 merge, 3 stopped and action or proposal questions, 4 notices. |
| `factory`, `task`, `display_id`, `title`, `project` | Where it belongs. |
| `question` | The question id, when the item is a question. |
| `text` | Why it is the person's turn. |
| `suggestion` | The preselected answer. |
| `result` | What sending the preselected answer does. |
| `result_code` | The same as a `ResultCode`. |
| `unblocks` | For a blocking question or a merge, the display ids of the waiting Tasks that depend on this one. |
| `gates` | For a merge item, the `Gate` codes it waits on. |
| `stop` | For an item of a stopped Task, the `StopReason` code. |
| `default_action` | What the worker does meanwhile. |
| `choices` | The listed answers. |
| `deadline`, `remaining` | The deadline, and a short phrase for it or for how many days a blocking question has waited. |
| `remaining_hours` | Whole hours left before the deadline, rounded up, 0 once it passed; none for a blocking question. |
| `waiting_since`, `waiting_days` | When it started waiting, and whole days for a blocking question. |

The inbox lists each open question, each `merge_waiting` Task, and each stopped Task that has no action question, in order of rank and, within a rank, the item waiting longest first.
That puts blocking questions first, then other answers, merge waits, stops and notices, so what has waited the longest to be unblocked is at the top.
Cancelling a Task answers its open questions as `cancel`, so a cancelled Task lists none and no deadline applies a default to it.

A Task's board column comes from its state: `drafting`, `waiting` and `done` have their own, every other state except `cancelled` is in `running`, and `cancelled` is off the board.
Inside a column the cards a person must look at (blocked, stopped, merge waiting, or holding an open question) come first, oldest first, then the rest by priority and age.
`flow` counts a Task in `done` only on the day it finished.

**`TaskDetail`**

| Field | Meaning |
| --- | --- |
| `card` | The Task's `CardView`. |
| `factory`, `project` | The Factory's id and path. |
| `goal`, `criteria`, `out_of_scope` | The card text. |
| `before`, `after` | The chain: predecessors, and the Tasks that wait on this one. |
| `attachments` | Each PRD copy with its path, hash, version and original path. |
| `pr` | The pull request: number, URL, head, whether the Factory opened it, whether it is open. |
| `verification` | `n/3`, or "검증 없음" without verification. |
| `attempts` | A list of `AttemptView`. |
| `decisions` | The decision records with who made them. |
| `questions` | Every question with its answer and who relayed it. |
| `discoveries` | The discoveries with their class. |
| `gates` | The reasons a merge waits for a person. |
| `gate_codes` | The same as `Gate` codes. |
| `allowed` | The actions the state allows. |
| `stop` | The stop reason's label. |
| `stop_code` | The same as a `StopReason` code. |
| `merge_sha` | The merge commit. |
| `worker_name`, `worktree`, `branch` | The worker's name, folder and branch. |

**`AttemptView`**

| Field | Meaning |
| --- | --- |
| `number` | The attempt, one more than the Task's failures when it started. |
| `stage` | `task` (after `done`) or `pre_merge`. |
| `started_at` | When it started. |
| `outcome` | `passed`, `failed`, `environment` or `running`. |
| `check` | The failing check or command. |
| `link` | The CI link or the log path. |
| `log_tail` | The last 4 KiB of a local log. |

### Codes

| Code | Values |
| --- | --- |
| `WaitingFor` | `predecessors`, `slot`, `environment`, `answer` |
| `EnvHold` | `disk_floor` (free disk below the floor), `disk_full` (a command found no space), `memory_critical` |
| `StopReason` | `no_report`, `stalled`, `verify_failed`, `new_task_cap`, `environment_repeated`, `worker_start`, `publish_refused` |
| `Gate` | `review_directly`, `approved_scope_change`, `breaking_change`, `no_verification`, `risk_path`, `manual_mode`, `open_question`, `check_failed`, `autonomy_diff`, `dirty_main`, `merge_refused` |
| `ResultCode` | `wake_worker`, `apply_or_merge`, `ready`, `split`, `drafting`, `new_task_cap_choice`, `run_action`, `acknowledge`, `merge`, `restart_worker` |

## Performance boundaries

The engine runs on its own thread, `herdr-core-factory`, and the judgments run on a second, `herdr-core-factory-judge`.
Dropping the core stops the engine first, then ends the verify runs, the judgment in flight and any `git` or `gh` call under way, so nothing the Factory started outlives the daemon.
The engine takes the runtime lock only to read owned data (a delivery projection, a worker's pane state, the snapshot's agent rows) or to hand the core a request (a sleep, a wake, a kit read), and does no file, network or subprocess work while holding it.
Every `git` and `gh` call runs on the engine thread with a 120 second deadline, and a verify bundle runs in an owned process tree the engine polls, so a long run never waits on the thread.
The Factory adds two snapshot sections, `factory` and `factory_task`, which carry the summary and the open Task page only when they change (ARCHITECTURE.md, The Factory host), and a Task changes no terminal, tab or scroll path.

A machine without a Factory runs the thread's two-second wait and nothing else.
With Factories, each two-second tick does bounded work:

- It reads the delivery projection once, applies at most 16 letters, and confirms them.
- It applies finished judgments and polls each verifying Task: a CI Factory makes one `gh api` read per verifying Task per tick, and a verify Factory polls the runner with no subprocess.
- It checks each running Task's worker once and each `landed` Task's main verification once.
- It reads outside work once per `outside_read_minutes` for each Factory, with back-off, and reads the default branch head only when that read happens or main is broken.
- It walks each Factory's Tasks a few times to move them, with one external call at most per Task step.
- It settles pending sleeps and wakes and republishes the open Factories as recipients.

The work is bounded: 32 queued commands, 16 waiting judgments per Factory, one judgment in flight, 256 queued verify runs and one running, 16 held letters per woken worker, 4,096 remembered letters, 20,000 events and 5,000 Tasks per Factory, a 256 KiB kept record, a 6 KiB prompt, a 48 KiB judgment input, a 256 KiB request and a 4 MiB attachment.
Crossing a cap is a reported failure and never a larger number.
A judgment and a worker start are the only places the engine waits on a provider or on Herdr, and each retries on a timer and never on a loop.
