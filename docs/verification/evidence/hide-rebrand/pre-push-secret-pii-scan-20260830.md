# Pre-push secret and personal-data scan

Date: 2026-08-30.

This is a scan-only record for the future public-push approval gate.

No repository push, remote creation, or release publication was performed.

## Scope and method

The initial scan covered 975 tracked files at HEAD `d021e73` and 110 reachable commits across the local refs returned by `git rev-list --all`.

The initial working tree was clean before the scan, so there were no untracked implementation files awaiting publication.

`gitleaks`, `trufflehog`, `detect-secrets`, and `semgrep` were not installed on this machine.

The fallback scan used `git grep -n -I -E` over the current tracked tree and every reachable commit tree, with reports written only under `/tmp/hide-rebrand-scans-20260830`.

The credential patterns included PEM private-key headers, AWS access-key prefixes, GitHub, Slack, npm, Google, Stripe, OpenAI, and Anthropic token shapes, and bearer-token values.

The assignment scan looked for secret-like key names such as `api_key`, `client_secret`, `access_token`, `password`, and `secret` next to an assignment operator.

The personal-data scan looked for email addresses, phone-number-shaped strings, local account-name occurrences, and absolute local home-directory paths.

## Results

| Check | Current HEAD | Reachable history | Interpretation |
| --- | ---: | ---: | --- |
| Credential-shaped token and private-key patterns | 0 lines | 0 lines | No credential-shaped value was detected by the checked patterns. |
| Secret-like assignment with a quoted value | 0 lines | 0 lines | No quoted secret value was detected. |
| Broad secret-name assignment | 1 line in 1 file | 199 repeated lines in 3 files | The current hit is an `OPENROUTER_API_KEY` environment-variable name in an interview log with no value. Historical hits are that same name plus retired synthetic redaction fixtures such as `xyz` and `secret2`, not credentials. |
| Precise email address pattern | 1 line in 1 file | 22 repeated lines in 1 file | The only hit is one reserved `.invalid` synthetic test address in the workspace fixture. No personal email address was detected. |
| Absolute local home paths | 2 lines in 1 synthetic test fixture after cleanup | 24,301 repeated lines in 129 historical paths | Owned documentation and evidence are clean. The two current-tree lines remain only in a synthetic Swift test fixture outside the documentation/evidence cleanup boundary; old reachable history still contains pre-cleanup local paths. |
| Local account name | 0 lines after cleanup | 29,042 repeated lines | The current tree is redacted; old reachable history still contains the prior local identity and was not rewritten. |
| Phone-number-shaped pattern | 0 high-confidence values | 0 high-confidence values | The broad fallback regex produced only system crash-sample address fragments after review, not phone numbers. |

## Approval summary

The credential scan is clear for the tested token classes.

The current tree is clean of personal identity values and credential values under the checked patterns.

The scan result is therefore a review input, not a public-push approval.

## Post-cleanup rescan

After the path cleanup, the current tracked tree contains 981 files.

The current tree has 2 absolute home-path lines in the one synthetic Swift fixture listed above, while owned documentation and evidence have 0 such lines.

The current tree has 0 local account-name occurrences, 0 personal email occurrences, 0 credential-shaped values, and 0 high-confidence personal phone values.

The 122 reachable commits contain 24,648 repeated absolute home-path lines across 129 historical paths and 29,630 repeated occurrences of the old local account name.

The historical counts are repeated snapshot matches, not additional current-tree files.

Five tracked `.json`-suffixed evidence files are command transcripts, not JSON documents.

Each was already invalid under `jq` at the pre-cleanup `a20d325` baseline, so the path cleanup did not introduce a JSON-contract regression.

Before any public push, the remaining reachable-history paths and local identity strings must be removed through a sanitized initial history, handled by a separately approved history rewrite, or explicitly accepted by the user after reviewing the exact exposure.

No raw scan report containing possible sensitive values was committed.

The Sasu AC16 state remains pending because the public push and its human approval are intentionally outstanding.
