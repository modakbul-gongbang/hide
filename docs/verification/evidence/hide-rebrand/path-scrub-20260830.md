# Public-path and identity cleanup

Date: 2026-08-30.

Branch: `prd/hide-rebrand`.

The cleanup was performed after the pre-push scan found developer-local absolute paths in tracked evidence and documentation.

## Replacement policy

Paths inside this repository are written relative to the repository root, usually as `.` or a repository-relative link.

Paths to another local checkout use `~` notation so the public record does not reveal the local account name or home-directory layout.

Synthetic test and verification paths use `/private/tmp/hide-*` because they are outside the repository, reproducible, and do not identify a user or account layout.

The `/private/tmp` notation is intentionally retained and documented rather than replaced with `~`, because `~` is a shell convention and is not a valid expansion when a test passes a literal path to a library.

## Code and fixture review

`herdr-core/src/environment.rs` now derives the chromux installation path from `HOME` and keeps the path injectable in the environment contract test.

`herdr-core/src/bin/herdr-ide-fixture.rs` now derives the local Herdr executable from `HOME` and keeps the remote executable as `~/.local/bin/herdr` for remote shell expansion.

The remote and runtime Rust fixtures use `/private/tmp/hide-*` paths, and the T16 Python fixture uses `Path.home()` rather than a literal tilde string.

Those changes preserve the behavior at the executable boundary and are covered by the full Rust and Swift suites.

No file under `macos/Sources/HerdrMacOS/` was edited by this cleanup.

Two absolute home-path lines remain in `macos/Tests/HerdrMacOSTests/OperationalPolicyTests.swift` as a synthetic remote checkout fixture outside the owned documentation/evidence cleanup boundary.

That fixture is intentionally listed instead of silently changing a Swift test owned by the UI workstream.

## Current-tree rescan

The current tracked tree contains 981 files.

The repository-wide rescan found 2 absolute home-path lines in 1 file, both the synthetic fixture described above.

The owned documentation, evidence, PRD, and spike-record paths found 0 absolute home-path lines.

The rescan found 0 occurrences of the local account name and 0 occurrences of the personal email address.

Credential-shaped token and private-key patterns found 0 values.

The only secret-like name retained is a documented `OPENROUTER_API_KEY` environment-variable name without a value.

The only precise email-shaped value is one reserved `.invalid` synthetic fixture address, repeated in the source fixture and this record.

No high-confidence personal phone number was found after reviewing the system-address false positives from crash samples.

Five tracked files with a `.json` suffix are command-transcript evidence rather than JSON documents.

They begin with usage text, shell warnings, a Herdr preamble, or an empty command result, and all five were already invalid under `jq` at the pre-cleanup `a20d325` baseline.

They were retained as evidence and were not treated as runtime JSON fixtures.

The current-tree result is therefore 0 exposed personal identity values and 0 exposed credential values, with 2 intentional synthetic absolute-path lines remaining.

## Reachable-history boundary

The final reachable local history contains 122 commits.

The final history rescan across the 122 reachable commits finds 24,648 repeated absolute home-path lines across 129 historical paths and 29,630 occurrences of the old local account name.

No personal email address or credential-shaped token was found in reachable history.

These historical strings were not rewritten because history rewriting is a separate destructive delivery decision and the user has not approved it.

A first public push of this branch would send its reachable commit history as well as the current tree.

Therefore the current-tree cleanup is complete, but public-push approval still requires choosing a sanitized initial history or separately approving an explicit history rewrite after reviewing its recovery plan.
