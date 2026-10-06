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
| `meta` | The machine-wide worker limit and the ids of the last 4,096 applied letters. |

`PRAGMA user_version` carries the schema version, now 1.
A store written by a newer build is refused rather than read: the engine does not start, the host logs `store.open_failed`, and commands answer `factory_unavailable`.
Fields added later load from older rows through serde defaults.
The engine loads the whole store at start and saves each change before the command that made it answers, so a restart resumes Factories, Tasks, questions, workers and in-flight work.
A restart starts verification again from the recorded attempt and asks a pending review again.
The engine opens the store when a store file already exists at start, or on the first command, so a machine that never created a Factory opens nothing.

`factory-files/<factory id>/<task id>/` holds each attached PRD as `<sha256>.<extension>`, a read-only copy of at most 4 MiB, and `factory-files/logs/` holds one `<run id>.log` per local verification run.
A run's log keeps only its last 1 MiB when the run ends.

The Factory deletes no Task, question, decision, discovery, attempt or attachment.
It removes a worker's worktree and pane when the Task is done and main verification has passed, when a cancelled Task passes its keep period, and, only under disk pressure, for finished Tasks.
The worktree removal leaves the Task's branch in place, and the Factory never deletes a remote branch.
A finished Task folds out of the board's Done column after 3 days and out of every list after 90 days; its page still opens.
Each Factory keeps its newest 20,000 events and drops older ones.
Judgment inputs and answers are not stored; the questions and flags they produce are, and the core's diagnostic log records each judgment's feature, ids, outcome and duration.

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
A caller is a worker when its pane is the pane of a live Task's worker, or when its working folder is inside a live Task's worktree; a `cancelled` or `done` Task has no worker.
Every other caller is an operator, recorded by pane id, or `checkout` when the caller has no pane.
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
- A worker's CLI report is a typed body `{"factory": <command>}` sent under an intent made from the pane and the body, so a retry applies once. A harness that follows the letter protocol only is read as a question (a `request` or `block` letter becomes a blocking question with a 24 hour deadline, taking a `Recommendation:`, `Suggestion:`, `추천:` or `제안:` line as the suggestion) or as a completion (a `report` letter becomes `done`).
- A watch whose observer is the Factory warns after the Factory's `stall_minutes` (30 by default) instead of 20; the host hands each open Factory's window to the watch readings, so a change applies at the next reading. The engine turns the first warning into a stalled stop; it does not wait for the second warning or for the human notice, which the ledger skips for a code-owned recipient.
- The Factory writes to a worker with `report` letters, and starts a watch on the worker again each time it does, because a report to its parent ends the sender's watch.

## The worker lifecycle

A worker starts when the Task may start.
The engine asks the core to spawn it through coordination, the same path as `hide agent spawn`, under the Factory's own agent record as parent.
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
A wake restarts the agent in the same pane and session.
Letters for a woken worker are held, at most 16, until its agent is back, and are dropped to the diagnostic log after 10 minutes.
A retried Task, or a worker whose pane is gone, spawns again in the same worktree and session with a fresh intent.
A worker that is `gone` is detected from its pane, with a three-minute grace after its start.

A worker reports through `hide factory` and gets its answer in the same call.
`ask` needs a suggestion, a default action and a deadline of 1 to 720 hours, and the worker continues with the default; `block` has no default, so the Task blocks, releases its slot and sleeps.
`done` opens a pull request on a GitHub Factory when none exists, goes to `verifying`, and puts the worker to sleep.
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
The producer's open decisions become intake questions, and a Task is Ready only when the review is done and no question besides a notice is open.
Ready creates the issue: a GitHub Task gets an issue labelled `factory` whose body carries a hidden Task marker, the goal, the criteria and the out-of-scope items, and a local Task gets an `L-<number>`.
A Task added from an existing issue gets the label instead.
The marker lets a retry find the issue it already created.
A split that a person accepts turns the original Task into the first piece, keeping its issue, and drafts the rest as new Tasks that depend on the pieces they follow; choosing to proceed clears the split and the Task becomes Ready when nothing else is open.
A Task is never split or made Ready by silence.

Every judgment is a tool-less, one-shot call whose input the code bundles and cuts to 48 KiB, whose answer must match a `factory.v1` schema, and whose deadline is 180 seconds.

| Feature id | Asked | Input | The answer may |
| --- | --- | --- | --- |
| `factory_intake_review` | On add and re-add, an edited issue body, a label-path card, a split piece, an approved proposal | The card, its attachment (24 KiB), up to 100 other Tasks, up to 400 file names, the repository guide (8 KiB) | Add questions, dependencies on listed Tasks, a split, and flags |
| `factory_drift` | After `done` | The card, the diff against main (24 KiB), the decisions | Pass, or add questions, and flags |
| `factory_check` | At intake or after `done` for each natural-language check | The instruction, the card, the diff | The same as drift |
| `factory_watch` | See [The watch](#the-watch) | The Factory's board summary | Warnings, each with an optional action |
| `factory_env_diagnosis` | See [Environment](#environment) | The collected facts and the closed action list | One action from the list, or an exact command with its impact |

Questions that a drift or check judgment adds always carry a default action, taking the suggestion when the answer has none, so a check can only slow a Task down.
A drift question keeps the Task in `verifying`, does not wake the worker, and holds auto merge until it is answered or its deadline passes; an answer that differs from the default wakes the worker to apply it.
A judgment that cannot run is never skipped and never read as a pass.
A failed review marks the Task and puts a retry-or-cancel question in the inbox, and `add` answers `pending`.
A failed drift or check sends the Task to `merge_waiting` for a person.
A failed watch or diagnosis changes no Task and is logged.

The Factory has its own judgment queue on its own router (see [AI_PROVIDERS.md](AI_PROVIDERS.md#the-factorys-judgments)): one request in flight, intake reviews before every other judgment, and 16 waiting judgments per Factory.
A judgment submitted to a full queue fails like a provider failure and is escalated the way above.
Natural-language checks are added with `hide factory check --at intake|after-done|periodic`.
A `periodic` check is stored with the others, and no timer runs it yet.

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
With a list of names, every name must have a completed run; without one, every check run on the commit is read, and a commit with no check run yet reads as passed, so name the required checks in `--ci`.
A completed run passes on `success` or `neutral`, decides nothing on `skipped`, `cancelled` or `stale` (still pending), and fails on any other conclusion with the run's link.
A GitHub error that carries an environment signal is the environment's; any other read error stays pending.

**Verify bundle.** Bundles run one at a time on the machine, in a queue of at most 256, each command through the shell with its output in the run's log.
The cap, `verify_timeout_minutes` (60 by default), applies to each command, and a command past it fails the run.
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

A review by an agent alone never merges.
A person's `merge` runs the same merge at once; `request-changes --comment` returns the Task to `running`, clears its gates and sends the worker the comment.

**GitHub.** The merge is `gh pr merge` with the Factory's `merge_method` and `--match-head-commit` set to the pull request's head as read just before the merge, and a pull request already merged returns its merge commit.
`init` picks the first merge method the repository allows, in the order squash, merge, rebase, and `config` changes it.
**Local.** The Factory merges into the project's primary checkout with `git merge --no-ff`, only while that checkout is on the default branch with no uncommitted tracked changes.
Otherwise the Task stays in `merge_waiting` with `dirty_main` until it is clean.

A merge records its commit, and the Task becomes `landed`.
Main verification is the chosen verification on that commit: the push run's check runs for `ci`, or the bundle run in `<project>.worktrees/factory-main`, a detached worktree the Factory creates beside the project's worktrees, for `verify`.
A green result, or no main verification, makes the Task `done` and removes its worker, pane and worktree.

A red result breaks main and stops auto merge; manual merges still run.
If the broken commit is among the Factory's own merges since the last green, the Factory finds the first failing one, asking again for runs that were skipped or cancelled, and reverts that merge alone.
On GitHub the revert is a pull request from `factory/revert-<task id>`, which the Factory force-pushes, checks and merges with `--merge`; locally it is a revert commit made in `factory-main` and fast-forwarded into the primary checkout.
A revert whose verification passes is merged on its own, and the original Task becomes `relanding`: it runs again in the same worktree and session on the latest main, with a new pull request.
When the failing commit is not the Factory's, nothing is reverted: the Factory drafts a fix Task for a person to confirm and stops auto merge.
Auto merge resumes when the head of main verifies green again.
A recovery that cannot name one merge, cannot make or merge the revert, or finds the revert red does not repeat itself: it stays stopped and puts the failed run's link, the candidate merges and these choices in the inbox: retry the revert, revert a named merge, or resume auto merge after a person fixed main.

## Outside work

The person wins outside the Factory, and the Factory follows.
Each open Factory reads GitHub every `outside_read_minutes` (2 by default): up to 200 issues labelled `factory` and 100 pull requests, or, locally, its local issues.
A failed read backs off 1, 2, 5, 15 and then 30 minutes on a rate-limit, server or network signal; three failures in a row mark the Factory's read stale.

| What the Factory sees | What it does |
| --- | --- |
| A pull request the Factory did not open closes a Task's issue, and it is open | The Task becomes `outside`. A running worker is stopped, its worktree stays for the keep period, and a notice offers `revive`. |
| That pull request merges | The Task is `done`, and a dependent Task's predecessor counts as merged. |
| The issue closes with no pull request, or the `factory` label is removed | The Task is cancelled with a notice, kept for the keep period. |
| A person edits the issue body | A Task before its start goes back to `drafting` and is reviewed again. A running Task gets a scope-change question. |
| A person labels an issue the Factory does not hold | A new Task is drafted from the issue, and a person confirms its card in the inbox before it is Ready. The body is not edited. |
| A finished Task's issue reopens | A notice only. |
| A push to main the Factory did not make | Main verification runs, and a red result takes the outside-push path above. |

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
| `scope_change` | A worker, a re-add, an edited issue | `approve` records approval and adds a gate; `reject` keeps the scope. After the deadline it is rejected. |
| `new_task_cap` | The Task reached the new-Task limit | `continue` lets it create more; any other choice leaves it stopped with a notice. |
| `proposed_task` | A prerequisite proposal outside autonomy | `approve` drafts the Task. |
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
A proposal that names an enabled `--autonomy` scope drafts the Task now and makes the proposer depend on it and sleep as `blocked`; any other proposal is a question that a person approves before the Task is drafted, and approving it does not make the proposer wait.

Autonomy scopes are the kinds of Task a worker may start without a person.
The presets are `flaky_test`, `dependency_patch`, `lint_format` and `docs_links`, all off, and a person may add scopes by name.
An autonomy Task is reviewed like any other, may not propose Tasks, and gates to a person when it changes more than 200 lines.

## Environment

An environment problem is not a Task's failure and is never counted against it.

- **Before a start.** Starts hold while free disk at the first waiting Task's project is under `disk_floor_gb` (20 by default), or while macOS reports critical memory pressure. Warn pressure starts normally, and other systems report normal. The held Tasks show the reason and are checked again after a minute.
- **At a failure.** Only a structured signal makes a failure the environment's: no space left on the device, exit 137, a GitHub 401, 403, 429 or 5xx answer, a network error, or a Herdr connection error. The adapters read the GitHub and git signals from the command's exit code and its stderr text. Everything else is the Task's. A usage-limit signal parks a runtime until its reset time, one hour by default, and moves an unpinned Task to the other runtime, and the host adapters do not report one yet.
- **Code handles each signal first.** Disk full holds starts and removes the worktrees of finished Tasks and cancelled Tasks past their keep period. Out-of-memory and Herdr connection errors send the Task back to `waiting`. A rate-limit, server or network error backs the read off as above. A 401 or 403 puts a notice in the inbox once, naming `gh auth login` or `gh auth refresh -s <scope>`, and the Factory does not perform that action or retry it.
- **Cascade.** Three different Tasks failing the same check or command within 30 minutes are read as the environment: each failure is taken back, the Tasks go to `waiting`, and new starts halt for 30 minutes.
- **If it does not clear.** A problem that outlasts 30 minutes asks `factory_env_diagnosis` once for a cause and either one action from the closed list or an exact command with its impact.

The closed recovery list is the only set of actions the Factory runs without a person, and `recovery.<action>=on` turns each on; all are off.

| Action | Does |
| --- | --- |
| `remove_finished_worktrees` | Removes the worktree of a finished Task and of a cancelled Task past its keep period. |
| `restart_worker` | Starts a worker again in the same worktree and session. |
| `sleep_wake_worker` | Puts a worker to sleep or wakes it. |
| `switch_runtime` | Moves an unpinned Task to the other runtime. |
| `retry_reads_and_reconnect` | Clears read back-offs and the start hold. |

A diagnosis naming an action that is off, or any other command, becomes a proposal in the inbox with the exact command and impact, and a person runs or dismisses it.
Logging in, deleting outside the Factory and installing tools are only ever proposals.
A Task that alone repeats an environment failure three times is the Task's: it stops as "same environment failure repeated".

## The watch

The watch reads a Factory's board for what a person cannot already see in the inbox, such as work that stopped moving, a chain held by one wait, or a pattern of failures.
It runs when a Task finishes, when main breaks, and every `watch_interval_minutes` (30 by default, at least 5) for a Factory that has Tasks.
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
| `verify_timeout_minutes` | Minutes per command, at least 1 | 60 |
| `disk_floor_gb` | Gigabytes | 20 |
| `default_runtime` | `claude` or `codex` | `claude` |
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
| `init` | Without `--confirm`, shows the source, the verification candidates the code detected (the branch's required checks and verify commands read from `Cargo.toml`, `package.json`, `Makefile` or `pyproject.toml`), the merge mode and the GitHub writes it would make, and writes nothing. With `--confirm` and a verification choice, creates the Factory and, on GitHub, creates the `factory` label. A closed Factory is reopened with `--confirm`. A project that already has an open Factory answers with it. |
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
`add` answers `{result, task, questions}`, `init` a preview, `created` or `existing` object, `show` `{task: <TaskDetail>}`, `inbox` `{count, items}`, `status` a `FactorySummary`, and `config` `{config, machine}`.
Other actions answer `{message, task}` with the Task's id, display id and new state.
Common refusal reasons are `role_not_allowed`, `task_not_found`, `task_ambiguous`, `factory_not_found`, `factory_ambiguous`, `factory_closed`, `card_invalid`, `action_not_allowed_in_state`, `config_invalid`, `auto_needs_verification`, `github_login_required`, `github_permission_missing`, `main_dirty` and `revive_expired`.

## The read model

`FactorySummary` and `TaskDetail` are the contract for any screen of the Factory, and `status`, `inbox` and `show --json` print them as they are.
Every number in them is derived in `hide-factory/src/summary.rs` from the stored Tasks, and a screen computes nothing.
Fields are added and never renamed or removed.

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
| `flow` | `drafting`, `waiting`, `running` and `done_today` counts. |
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
| `priority` | The Task's priority. |
| `since` | When it entered its state. |
| `unread` | A completion nobody has looked at. |
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
| `default_action` | What the worker does meanwhile. |
| `choices` | The listed answers. |
| `deadline`, `remaining` | The deadline, and a short phrase for it or for how many days a blocking question has waited. |
| `waiting_since`, `waiting_days` | When it started waiting, and whole days for a blocking question. |

The inbox lists each open question, each `merge_waiting` Task, and each stopped Task that has no action question, in order of rank and, within a rank, the item waiting longest first.
That puts blocking questions first, then other answers, merge waits, stops and notices, so what has waited the longest to be unblocked is at the top.
A cancelled Task inside its keep period still lists its open questions.

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
| `allowed` | The actions the state allows. |
| `stop` | The stop reason's label. |
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

## Performance boundaries

The engine runs on its own thread, `herdr-core-factory`, and the judgments run on a second, `herdr-core-factory-judge`.
Dropping the core stops the engine first, then ends the verify runs, the judgment in flight and any `git` or `gh` call under way, so nothing the Factory started outlives the daemon.
The engine takes the runtime lock only to read owned data (a delivery projection, a worker's pane state, the snapshot's agent rows) or to hand the core a request (a sleep, a wake, a kit read), and does no file, network or subprocess work while holding it.
Every `git` and `gh` call runs on the engine thread with a 120 second deadline, and a verify bundle runs in an owned process tree the engine polls, so a long run never waits on the thread.
The Factory adds nothing to the snapshot wire, and a Task changes no terminal, tab or scroll path.

A machine without a Factory runs the thread's two-second wait and nothing else.
With Factories, each two-second tick does bounded work:

- It reads the delivery projection once, applies at most 16 letters, and confirms them.
- It applies finished judgments and polls each verifying Task: a CI Factory makes one `gh api` read per verifying Task per tick, and a verify Factory polls the runner with no subprocess.
- It checks each running Task's worker once and each `landed` Task's main verification once.
- It reads outside work once per `outside_read_minutes` for each Factory, with back-off, and reads the default branch head only when that read happens or main is broken.
- It walks each Factory's Tasks a few times to move them, with one external call at most per Task step.
- It settles pending sleeps and wakes and republishes the open Factories as recipients.

The work is bounded: 32 queued commands, 16 waiting judgments per Factory, one judgment in flight, 256 queued verify runs and one running, 16 held letters per woken worker, 4,096 remembered letters, 20,000 events and 5,000 Tasks per Factory, a 6 KiB prompt, a 48 KiB judgment input, a 256 KiB request and a 4 MiB attachment.
Crossing a cap is a reported failure and never a larger number.
A judgment and a worker start are the only places the engine waits on a provider or on Herdr, and each retries on a timer and never on a loop.
