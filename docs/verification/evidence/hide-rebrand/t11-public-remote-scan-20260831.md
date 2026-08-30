# T11 public tree rescan

Date: 2026-08-31.

Repository: `https://github.com/modakbul-gongbang/hide`.

Ref checked: public default branch `main` at `7c0ca7e0dad68f2d4204d36d9e82cd3f3265e615`.

## Method

The public repository was cloned fresh with `git clone --depth 1 --branch main` into a temporary directory represented here as `<temporary-clone>`.

The temporary directory is outside the repository and is intentionally represented by a placeholder so this evidence remains reproducible without exposing a workstation home directory.

The scan used `git grep -n -I` over the checked-out tracked tree for local home paths, known account identifiers, personal email, credential-shaped token values, and secret-like assignments.

The scan also checked the intentionally synthetic remote fixture markers used by the Rust workspace test.

## Results

| Check | Result | Interpretation |
| --- | ---: | --- |
| macOS or Linux local home paths | 0 | No workstation home path is present in the current public tree. |
| Known local account identifiers | 0 | No local username, host alias, or workstation name is present in the current public tree. |
| Personal email address | 0 | No personal email address is present in the current public tree. |
| Credential-shaped token values | 0 | No checked GitHub, AWS, Slack, npm, Google, Stripe, OpenAI, Anthropic, bearer, or private-key value was detected. |
| Secret-like assignments with a value | 0 | No checked credential assignment contains a value. |
| Intentional synthetic fixture markers | 2 lines | The two lines are `hide@example.invalid` and `/private/tmp/hide-remote-repo` in the remote workspace test. |

The current public `main` tree contains 981 tracked files.

The two synthetic fixture lines are not user identity or credentials, and the `/private/tmp` spelling is required because the test passes that literal disposable path to the library rather than asking a shell to expand `~`.

## History boundary

The local development branches were not rewritten, deleted, or pushed.

The public branch was created from an orphan root and subsequent release corrections were pushed as ordinary non-force commits.

The public root commit metadata retains a personal author identity from the first staging commit because rewriting an already published ref was prohibited by the T11 safety rules.

The current-tree scan above therefore proves the content exposed by the default branch checkout, but it does not claim that every older public commit object or commit metadata record is free of historical identity strings.

No force push, ref deletion, or release publication was performed.
