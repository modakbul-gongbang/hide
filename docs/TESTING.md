# Testing

This guide answers the question asked while a test is being written: which test to write, how to build its fixture, and how to keep it from becoming flaky.
It owns the layer choice, fixture construction, the writing rules that keep a test deterministic, the flaky policy, and the review checklist for a pull request that adds or changes a test.
Running a check is not here: [VERIFICATION.md](VERIFICATION.md) owns which check proves a claim, the traps before an e2e run, native QA and device checks, and [CONTRIBUTING.md](../CONTRIBUTING.md#ci-gates) lists the gates.
Runtime isolation and its variables live in [PERFORMANCE_TESTING.md](PERFORMANCE_TESTING.md#3-isolate-runtime-state-before-making-fixtures).
Where a test file goes inside a crate or package is in that directory's `AGENTS.md`.

## Choose what to test and where

A test is bought twice: once when it is written and again every time the implementation moves underneath it.
It earns its place when a plausible bug would make it fail and a change that keeps the behavior would not.

- Assert what a caller observes: the returned state, the serialized output, the snapshot the shell receives, the bytes a pane receives, the error a caller can branch on.
  An assertion that a function was called, or on internal call counts, stays green while the feature is broken and breaks when the code is rearranged.
- Take the expected answer from outside the implementation: the requirement or approved PRD, [UI_BEHAVIOR.md](UI_BEHAVIOR.md), [status-model.md](status-model.md), the Herdr references and `contracts/`.
  A test written by reading the code records what the code does, defects included.
- Exercise private helpers through the owning module's stable behavior whenever that boundary can express the case; a helper exported only so a test can reach it means the test is on the wrong boundary.
- Cover the unavailable, malformed and refused states alongside the successful one when the type can produce them, and the first value on each side of a rule's boundary.
- A test double stands in only for a boundary this repository does not own and implements the same trait or interface as production.
  Herdr is not faked: integration tests and e2e specs run the pinned binary as a private server, and a test that needs to control Herdr's timing does it at the socket (see [The test decides the order](#the-test-decides-the-order)).
- Change an expectation or fixture only when the requirement changed or the test misrepresented an unchanged requirement, and say which.
  Copying the new implementation's result into the expectation is not a reason.
- Before trusting a new test, predict which assertion fails for a realistic planted defect (a moved boundary, a dropped condition, a failure handled as success), make the change, and confirm the prediction; a test that stays green is thin.

Pick the cheapest layer that can observe the result.

| Layer | Use it for | Where |
| --- | --- | --- |
| Unit | A decision that takes values and returns a result: policy, parsing, serialization, layout math, a rejected transition | Beside the owning module: `#[cfg(test)] mod tests` in Rust, `*.test.ts` or `*.test.tsx` in `web/` and `desktop/` |
| Core runtime and crate boundary | State the core owns, reached through a dispatched event and the snapshot it publishes; a Herdr fixture; the hided wire; an OS contract | `herdr-core/src/runtime/tests/` and the other suites the core's modules include, `hided/tests/`, `hide-platform/tests/` |
| End to end | A flow that crosses a process boundary a user depends on: browser to hided to the core to Herdr to the PTY, or the desktop window and its native integration | `web/e2e/`, `desktop/e2e/` |

An e2e spec is for a flow that crosses a boundary, not for a rule a unit or core test can state.
Each spec starts its own Herdr, hided and browser, so a rule restated end to end costs runner minutes on every pull request and fails for reasons that have nothing to do with the rule.
Keep one representative journey per user-visible flow; when a long spec carries an independent contract, split that contract into a small spec that still runs against the real pinned Herdr and hided rather than adding steps to the journey.

## Wait for state, not time

- Never wait a fixed time: no `page.waitForTimeout`, `sleep`, or `setTimeout` used as a delay in a new test.
  Wait for the state the next step needs: a diagnostic `kind` with its subject ID, a snapshot field, a Herdr answer, the bytes in a pane's input log.
- Wait for the readiness the next step actually depends on.
  A healthy HTTP answer is not a subscribed socket, a delivered first snapshot, or a terminal that accepts input.
- A poll only observes.
  A click, focus, creation or resend is one explicit action outside the poll; an action repeated inside `toPass` or `expect.poll` turns one intent into several and can satisfy the assertion with the wrong one.
- Prefer subject IDs, request identity and generation or revision numbers over wall-clock comparisons and the last diagnostic string.
- A test's own waiting must not compete with a deadline the product enforces.
  While the test holds a request the product will time out, run nothing slow between holding and releasing it, and give every observation poll in that window explicit `intervals`.
  Playwright's default poll backoff grows to a second between attempts, which is time taken from the product's deadline.

`web/e2e/pane-focus-ordering.spec.ts`'s rapid-click test showed the cost.
It holds the first focus request, whose transport deadline is five seconds, while it sends forty clicks.
Between holding and releasing, it detached a CDP session (about 2.1 seconds on a loaded macOS runner) and waited out an `expect.poll` default backoff (about 0.9 seconds), so the held request was released only at about 4.4 seconds and the test failed intermittently on macOS CI.
The cause was found by timestamping each stage against the first request, not by rerunning it.

## The test decides the order

When the behavior depends on the order of two events, the test fixes that order; it does not hope the scheduler produces it.

- Hold one side at a real boundary and release it after the competing event has been observed.
  `focusGate` in `web/e2e/pane-focus-ordering.spec.ts` is the reference: a proxy on the private Herdr socket that holds one `pane.focus` request, forwards everything else, and lets the test release it, so every answer still comes from the pinned Herdr.
- Before releasing, assert the barrier the race needs, such as the number of accepted requests, so a stale snapshot or an earlier diagnostic cannot satisfy it.
- After releasing, assert the outcome and the boundary's own record (`gate.requests`, `gate.maximum()`), which show the order the product actually saw.
- A race the test cannot order proves nothing either way; find the boundary to gate before writing the assertion.

## Writing a fixture

- Set each fixture's `HCOORD_HOME` to a private folder such as `HOME/.hcoord`, so its daemon gets a launchd label of its own; hcoord gives the plain `com.hcoord.daemon` label only to the account's default `~/.hide/hcoord`, read from the user database, but a fixture that names its own home leaves no doubt.
- A test of a one-time move (`hide connect` moving the state folder, `hcoord home adopt`, the kit's hcoord part) stops only a daemon the test started in its own private folder, and injects launchctl and the label (`plugins/hcoord/test/unit/hcoord-home.test.mjs`); launchd domains are per account, so a real `launchctl` call with the default label reaches the operator's coordinator whatever `HOME` says.
  The desktop fixture refuses a mismatched coordinator before launching a candidate.
- Copy the whole isolation environment from `web/e2e/herdr-fixture.ts` and `desktop/e2e/fixture.ts`, never a subset; [PERFORMANCE_TESTING.md](PERFORMANCE_TESTING.md#3-isolate-runtime-state-before-making-fixtures) lists every variable and why.
- Register every process a fixture starts with `ownUntilWorkerExit` from `web/e2e/worker-owned.ts`, so synchronous cleanup runs on Node-managed worker exit even when a test's `finally` was skipped.
  Failed cleanup stays registered for recovery instead of being disowned.
  When setup and cleanup both fail, `cleanupAfterFailure` preserves the setup message and stack as the reported identity and keeps the cleanup error in `cause`, which Playwright serializes.
  `stopFixtureProcess` confirms the private Unix process group or identity-checked Windows descendants have exited before Herdr/hided roots are removed.
  Windows provider executable locks are released by ending their owned processes, with no EBUSY ignore or deletion retry.
  Exit-unconfirmed cleanup fails and retains the root.
  `desktop/e2e/platform-cleanup.unit.ts` checks both-error, cleanup-only and actual executable ownership controls on every OS.

  A `spawn` with no `error` listener is such a death: when `target/debug/hided` was missing, each test killed its worker and left its private Herdr server running under launchd.
  The exit callback cannot run after SIGKILL, an OOM kill or host loss; these require separate recovery and are not proven by a `process.exit()` regression.
- `desktop/e2e/fixture.ts` owns each `isolate` home through both automatic test teardown and worker exit, with at most sixteen unclosed homes per worker.
  Each home records at most sixteen live or pending candidate launches; a launch over that cap fails before starting another process.
  Automatic teardown closes candidate apps, then each home's cleanup checks its recorded process handles for confirmed exit before stopping the private hided, unloading only the hashed hcoord labels for that home's legacy and adopted directories, and confirming each label absent before deleting the home.
  An unconfirmed candidate exit, stop, launchctl query or unload failure fails teardown and retains the home for recovery; the error names the retained path and recovery action.
  When candidate exit is unconfirmed, close only the recorded owned candidate, confirm its exit, then call that fixture's `cleanup()` again.
  `desktop/e2e/fixture-cleanup.unit.ts` injects Electron close failures at the external boundary and checks real home retention, confirmed-exit deletion and recovery without starting native processes.
  `desktop/e2e/lifecycle.spec.ts` checks running and manually stopped services, failed tests, Node-managed worker exit via `process.exit(23)`, and retained state after an unload failure against private homes in real launchd.
  Standalone hcoord fixtures must use `hcoord daemon uninstall --json` with their original `HOME` and `HCOORD_HOME` before removing those paths; `daemon stop` alone keeps the job registered.
- Set `terminal.default_shell` in the private Herdr config, because Windows Herdr does not select its shell from `SHELL`.
  The fixture uses `/bin/zsh` with its private `.zshrc` on Unix and the native `ComSpec` cmd shell with a controlled `PROMPT` on Windows.
  A missing native shell fails fixture setup before starting the server.
  Before each initial agent start, the fixture waits for the prompt and the shell to hold the foreground within the existing ten-second setup bound.
  On Windows, the pinned Herdr can report the shell as foreground while a non-agent child remains, so one bounded, noninteractive PowerShell read also waits for that shell to have no children; a missing shell or failed read fails setup.
  The fixture sends each `agent.start` once and retains the original input and PTY-log assertions.
- Use `fixtureHomeEnv` from `web/e2e/platform-fixture.ts` to move `HOME`, provider config homes and, on Windows, `USERPROFILE`, `APPDATA` and `LOCALAPPDATA` together into the private fixture.
  The fixtures run a compiled `claude` shim instead, which proves the pipeline and not an agent or physical IME.
  On Windows the interactive shim writes its readiness line after console setup; provider/auth replies return before that line, and input logs record only received input.
  The native compiler is `cc` on Unix and `clang.exe` on Windows; its owned child has a 20-second bound and a compiler failure fails setup.
  The private Herdr server, panes and `hided` use `fixturePath`: the shim directory followed by system-tool directories, including `/usr/sbin` for `lsof` on Unix and native Windows and Git directories on Windows.
  `fixtureExecutable` supplies native `.exe` names and `fixtureToolPath` uses the native path delimiter while refusing missing or non-absolute Windows system-root or program-files values.
  Catalog discovery probes every provider, so appending the host's `PATH` also reaches its installed CLIs even when the test selects Claude.
  A spec adding a GitHub or Git shim prepends it to `fixturePath`; explicit `extraEnv.PATH` remains authoritative.
  A claim about a real agent session needs the real CLI inside an otherwise isolated fixture: a shim on the pane's `PATH` that runs the CLI under the operator's `HOME`, while the server keeps its private one.
- A fixture makes agent labels the way the product does, through the core and not through tokens: `labelAgent` in `web/e2e/herdr-fixture.ts` gives a pane a new Claude session and writes a synthetic transcript (`writeFixtureTranscript`) the core's label worker reads, and the fixture `claude` shim acts as the provider that answers the analysis with the label the spec asked for.
  The same session is read again only when the agent's state changes: a spec that needs a different label either gives the pane a new session, or, for a pane declared as another's child, appends a turn to its own session (`continueFixtureTranscript`) and changes its state, because a relationship holds only for the sessions it was written for.
  A spec drives status through Herdr's own `agent_status` with `setFixtureLifecycle` and `finishFixtureTurn` (which needs `elsewhereTab`, so the turn ends unseen), because nothing else produces working, blocked or done.
  No spec writes a label or status pane token; the lineage tokens `declareParent` writes (`parent_pane`, `child_session`, `parent_session`) are the only ones a fixture reports, so a spawned child is labelled before its parent is declared (`spawnAgent` with a label).
  `desktop/e2e/session-labels.spec.ts` is the reference for the session boundary.

### Operating-system differences belong to one fixture helper

`hide-platform` owns what differs between systems in the product; the e2e fixtures need the same for their own resources: the endpoint they listen on, how they spell a path the core compares, the programs they copy and run, and how they clean up.
That belongs in one shared fixture helper under `web/e2e/`, which the specs and both fixtures call, not in a `process.platform` branch per spec.
`web/e2e/platform-fixture.ts` owns native executable names, isolated home variables, system-tool paths and confirmed process-tree exit.
Put a new difference in that shared owner when it fits, or beside the fixture that owns the resource, rather than in a spec.
The first Windows runs of the web e2e fixtures found three differences the helper has to own:

- An endpoint is a named pipe on Windows: a fixture that would use the socket path `P` on Unix uses `\\.\pipe\P`, because listening on a Unix socket path there fails with `EACCES`.
- A key the core derives from a path is computed from the wire spelling the core uses (`/` between names, `hide-platform`'s `path`), never from the native spelling.
  `web/e2e/s2.spec.ts` hashes the checkout folder to find its owner workspace; hashed in the Windows spelling, the key named a different workspace than the one the core chose, and the new tab the test expected appeared elsewhere.
- A program just copied or just exited can still be locked on Windows, so deleting it can fail with `EBUSY`.
  Confirm the process that ran it has exited before removing it; a deletion retry is not that confirmation.

### Cleanup never hides the first failure

- Keep the original setup or assertion error as the test's failure, and report a cleanup error beside it with the step and resource it concerns.
  On Windows, a failed Herdr fixture start was followed by a cleanup `rmSync` that threw `EBUSY`, and the cleanup error replaced the start failure in the report.
- A cleanup failure with no earlier error still fails the test; a fixture that cannot confirm its own cleanup leaves state the next test inherits.
- Release every process a fixture started on every exit path, including a failed start: register it with `ownUntilWorkerExit` before the first step that can throw.
- Delete a fixture's root only after every process that used it has confirmed exit; when exit cannot be confirmed, keep the root and name it in the error, as `desktop/e2e/fixture.ts` does.
  `cleanupAfterFailure` keeps the original message and stack as the primary identity and the cleanup error in `cause`, which Playwright serializes.
  `scripts/tests/test_playwright_contracts.py` checks the actual JSON reporter and failure ledger, including cleanup-only failure.
  An `AggregateError.errors` array alone is insufficient because Playwright does not serialize it.

### Interrupted execution retains its partial results

`scripts/ci-reporter.ts` records scheduled identities at suite start and atomically replaces its bounded ledger at every test start and completion.
Completed results keep their original error and signature; an interrupted in-flight test or an unstarted identity cannot count as passed.
A hard process death leaves `partial-or-unknown` with the last completed results and the in-flight/scheduled identities, even when Playwright never reaches `onEnd`.
The real-process controls in `scripts/tests/test_playwright_contracts.py` terminate Playwright with SIGKILL and SIGINT after one completed test and during the next test.
The ledger caps remain 20,000 rows and 16 MiB; overflow fails the caller and retains the previous complete JSON.

## Flaky tests

A test is flaky when it both fails and passes on the same SHA; Playwright retries are off, so this shows up as a rerun or a nightly that disagrees with the pull request.
A failure on a different SHA is not evidence of flakiness by itself, and the same title can carry two different failures.

- Record each failure in the issue that tracks the test: the run, job and attempt link, the tested SHA, the system, the failing assertion and its message, and the result of the first run and of each rerun.
  Group failures by assertion signature, not by title, so two causes under one name are not read as one.
- A web or desktop e2e test under investigation leaves an ordinary required suite only through `contracts/ci-quarantine.json`.
  Each registry entry binds its exact file, title, OS and assertion signature to an issue, evidence, owner, registration and expiration, alternative coverage and return criteria.
  Registration lasts at most seven days; missing metadata, expiration and unregistered `@flaky` declarations fail `python3 scripts/ci-quarantine.py check` in the required policy lane.
  The source keeps its `@flaky` issue annotation, while Playwright `--test-list-invert` excludes only the exact registered file/title on its registered OS.
  An unregistered same-title test in another file, or a longer title, still runs.
  `advisory.yml` runs the relevant registered scenarios in a separate workflow on related PRs and daily; its failure stays visible without delaying ordinary `verify`.
  A PR changing a registered scenario's behavior, test or shared fixture selects `quarantine-fixes`, which runs the exact registered scenarios as required checks.
  `ci-quarantine.py results` requires a result for every selected identity, and a required scenario must pass.
  Missing, skipped or ambiguous results fail the scenario lane; a new signature remains a distinct failure in the ledger.
  Main and nightly full suites include the registered tests in their normal blocking shards.
- Quarantine is for a cause under investigation, not for a test nobody means to fix; removing the tag is part of the fix.
- Quarantine does not decide the cause.
  A failure that shows lost or misrouted input, a broken OS contract, or a missing tab the user asked for is a product defect candidate, and is investigated as one rather than tagged and left.
- Do not make a flaky test pass by raising a timeout or deadline, adding retries, changing the number of clicks or keys, lowering an expected count, resending an action, accepting a partial string, or skipping it.
  Each of these hides the order or readiness problem the failure was reporting; fix the wait or the gate, or fix the product.

The registry policy and result checks enforce metadata, exact selection and required outcomes in CI.
Review still decides whether the evidence supports quarantine and whether the original cause is fixed:

- A pull request that adds `@flaky` links an open issue holding at least one recorded failure, and says what would end the quarantine.
- A pull request that touches a quarantined test, or closes its issue, either removes the tag with evidence the cause is fixed or records in the issue why the quarantine stays.

## Reviewing a pull request that adds or changes a test

- [ ] The test asserts a result a caller observes, and the pull request names a plausible bug it would catch.
- [ ] The expected answer comes from a requirement or contract outside the implementation; a changed expectation names that source.
- [ ] It sits at the cheapest layer that can observe the result; an e2e spec covers a flow that crosses a boundary.
- [ ] No fixed waits; every wait names the state it waits for, and polls only observe.
- [ ] Nothing slow runs while the test holds a request the product will time out, and polls in that window set `intervals`.
- [ ] An order the result depends on is fixed by a gate at a real boundary, with the barrier asserted before release.
- [ ] The fixture copies the whole isolation environment, owns every process it starts on every exit path, and reports cleanup failures without replacing the original error.
- [ ] A system difference lives beside the fixture that owns the resource, not in the spec.
- [ ] No timeout, retry, count or skip was changed to make the test pass; a new `@flaky` tag meets the policy above.
