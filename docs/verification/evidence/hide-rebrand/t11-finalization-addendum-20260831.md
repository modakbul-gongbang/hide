# T11 finalization addendum

Date: 2026-08-31.

This addendum is the current state record for the T11 closeout.

The earlier continuation and scan-only documents are retained as time-ordered evidence of the state before the final public-push approval.

The user's later exact approval `a` is recorded in [t11-public-push-approval-20260831.md](t11-public-push-approval-20260831.md) and supersedes the earlier pre-approval statements for the delivery step.

The public branch was created from a separate orphan history and pushed without rewriting the local `main` or `prd/hide-rebrand` branches.

The local merged `main` contains this feature; its final merge SHA is recorded by
the repository state and in the final handoff.

The feature commit included in that merge is `214b262b6e3c578134d9807e1f4bb593beb44d61`.

The subsequent navigator-focus cleanup is `f979cfc` and is merged into local `main` at `dc8e0c6`.

After that merge, the release bundle was rebuilt with the Rust cache at `/tmp/hide-finisher-cargo` and the Swift scratch path `/tmp/hide-finisher-swift`, both outside the repository so verification rounds do not create and remove bulk build files in the judged tree.

The resulting bundle was installed at `/Applications/hide.app` after the prior installed copy was moved to a recoverable temporary backup.

The installed `HerdrMacOS` executable SHA-256 is `c39d84bcc245f1aeeae27f976d7ff2f75fcd363dc1cd111d367ed428df8a7aa8`.

The installed Herdr runtime is version `0.8.2` with the official SHA-256 `a5d4f4d504d8b309c91f811050559300faba31258425f53c50852fc96f6ae574`.

The installed bundle passes `codesign --verify --deep --strict`, Spotlight returns `/Applications/hide.app` for bundle identifier `me.grab.hide`, and exactly one installed `HerdrMacOS` process was observed after launch.

The final public release candidate remains `7c0ca7e0dad68f2d4204d36d9e82cd3f3265e615` at tag `v0.1.9`, with the draft-release details in [t11-public-release-20260831.md](t11-public-release-20260831.md).

The pane-less checkout behavior is an approved product deviation from the original empty-state wording.

The user changed it to: `앞으로는 페인이 있는 브랜치만 아니면 브랜치 기존에 보여주고 거기에 허드 페인이 없다면 아예 새로 거기서 띄워`.

The implemented result is that an existing checkout row remains visible regardless of pane count, and selecting a checkout without a Herdr pane creates a new tab and terminal pane in that checkout's cwd.

This deviation is intentional, preserves immediate workability, and is covered by the passing Swift regression `paneLessCheckoutSelectionRequestsAnAutomaticTerminal`.

The current local verification state is pending a fresh Sasu verify after this addendum, the approval artifact, and the runtime cleanup change are registered.
