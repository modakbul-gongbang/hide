# T11 finalization addendum

Date: 2026-08-31.

This addendum is the current state record for the T11 closeout.

The earlier continuation and scan-only documents are retained as time-ordered evidence of the state before the final public-push approval.

The user's later exact approval `a` is recorded in [t11-public-push-approval-20260831.md](t11-public-push-approval-20260831.md) and supersedes the earlier pre-approval statements for the delivery step.

The public branch was created from a separate orphan history and pushed without rewriting the local `main` or `prd/hide-rebrand` branches.

The local merged `main` currently points to `3c41165a56920bb8c999e1acc4309cc172c3cde3`.

The feature branch currently points to `27f0c25b20db47d69929a8c1d73672b2726a9218`.

The final public release candidate remains `7c0ca7e0dad68f2d4204d36d9e82cd3f3265e615` at tag `v0.1.9`, with the draft-release details in [t11-public-release-20260831.md](t11-public-release-20260831.md).

The pane-less checkout behavior is an approved product deviation from the original empty-state wording.

The user changed it to: `앞으로는 페인이 있는 브랜치만 아니면 브랜치 기존에 보여주고 거기에 허드 페인이 없다면 아예 새로 거기서 띄워`.

The implemented result is that an existing checkout row remains visible regardless of pane count, and selecting a checkout without a Herdr pane creates a new tab and terminal pane in that checkout's cwd.

This deviation is intentional, preserves immediate workability, and is covered by the passing Swift regression `paneLessCheckoutSelectionRequestsAnAutomaticTerminal`.

The current local verification state is pending a fresh Sasu verify after this addendum, the approval artifact, and the runtime cleanup change are registered.
