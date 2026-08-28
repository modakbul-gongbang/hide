# T15 regression matrix

T15 uses `tools/t15-regression` to execute safe local checks and record every required verification mode in one machine-readable matrix.

Commands are argv-only and run from the worktree root with no shell interpolation.

Each row records its mode, required-for-Done flag, blockability, bounded duration, redacted output, and evidence references.

Rows that require a live provider, Browser pane, remote `mini`, native physical input, or fresh performance sampling remain explicit `BLOCKED` or `NOT_RUN` rows rather than being treated as passes.

The aggregate is `PASS` only when every required row passes.

Task completion and verification state are separate: T5 is complete by explicit user acceptance,
while its physical Option+F check is recorded as `user-accepted/verification-deferred` with
`machine_pass: false`. The native row may therefore remain `BLOCKED` without incorrectly describing
the task itself as open or claiming a machine PASS.

If a required row is blocked or not run, the aggregate is `PARTIAL`; if a required row fails, the aggregate is `FAIL`.

The output directory has an ownership marker containing the run ID, profile, manifest hash, and exact path.

An existing output with a different marker or an incomplete result is rejected and requires a new run identity.

## Non-invasive commands

```sh
./tools/t15-regression/t15-regression --root . --manifest tools/t15-regression/fixtures/manifest-v2.json --mode contract
./tools/t15-regression/t15-regression --root . --manifest tools/t15-regression/fixtures/manifest-v2.json --mode run
PYTHONPATH=tools/t15-regression python3 -m unittest discover -s tools/t15-regression/tests -v
```

The T15 runner does not send CGEvents, activate windows, close user processes, call OpenRouter, mutate `mini`, or delete fixture resources.
