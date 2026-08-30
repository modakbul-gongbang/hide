# Pre-push secret and personal-data scan

Date: 2026-08-30.

This is a scan-only record for the future public-push approval gate.

No repository push, remote creation, release publication, or source cleanup was performed.

## Scope and method

The scan covered the 975 tracked files at HEAD `d021e73` and the 110 reachable commits across the local refs returned by `git rev-list --all`.

The working tree was clean before the scan, so there were no untracked implementation files awaiting publication.

`gitleaks`, `trufflehog`, `detect-secrets`, and `semgrep` were not installed on this machine.

The fallback scan used `git grep -n -I -E` over the current tracked tree and every reachable commit tree, with reports written only under `/tmp/hide-rebrand-scans-20260830`.

The credential patterns included PEM private-key headers, AWS access-key prefixes, GitHub, Slack, npm, Google, Stripe, OpenAI, and Anthropic token shapes, and bearer-token values.

The assignment scan looked for secret-like key names such as `api_key`, `client_secret`, `access_token`, `password`, and `secret` next to an assignment operator.

The personal-data scan looked for email addresses, phone-number-shaped strings, and absolute `/Users/<name>` paths.

## Results

| Check | Current HEAD | Reachable history | Interpretation |
| --- | ---: | ---: | --- |
| Credential-shaped token and private-key patterns | 0 lines | 0 lines | No credential-shaped value was detected by the checked patterns. |
| Secret-like assignment with a quoted value | 0 lines | 0 lines | No quoted secret value was detected. |
| Broad secret-name assignment | 1 line in 1 file | 199 repeated lines in 3 files | The current hit is an `OPENROUTER_API_KEY` environment-variable name in an interview log with no value. Historical hits are that same name plus retired synthetic redaction fixtures such as `xyz` and `secret2`, not credentials. |
| Precise email address pattern | 1 line in 1 file | 22 repeated lines in 1 file | The only hit is the synthetic `example.invalid` test address in the workspace fixture, repeated across commits. No personal email address was detected. |
| Absolute local home paths | 346 lines in 124 files | 21,749 repeated lines in 127 files | These are the material pre-push finding: verification logs, screenshots' metadata records, old architecture notes, and fixture evidence contain developer-local paths. They should be scrubbed or deliberately excluded before a public repository is approved. |
| Phone-number-shaped pattern | 0 high-confidence values | 0 high-confidence values | The broad fallback regex produced only system crash-sample address fragments after review, not phone numbers. |

## Approval summary

The credential scan is clear for the tested token classes.

The repository is not yet clean from a personal-data perspective because tracked evidence contains many absolute local paths.

The scan result is therefore a review input, not a public-push approval.

Before any public push, the path-bearing evidence and any other human-readable personal metadata must be scrubbed, removed from the public history, or explicitly accepted by the user after reviewing the exact files.

No raw scan report containing possible sensitive values was committed.

The Sasu AC16 state remains pending because the public push and its human approval are intentionally outstanding.
