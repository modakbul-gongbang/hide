# Build output and worktrees

This document owns where build output goes and why: every build inside the worktree that asked for it, the release binaries at their fixed path, one Rust version named by the repository, and the machine's toolchain reused rather than reinstalled.
The scripts named here are the executable authority; `scripts/tests/test_toolchain_reuse.py`, `test_rust_toolchain_pin.py`, `test_ci_gate_portability.py` and `test_verification_builds.py` assert the parts a workflow depends on.

## One rule: build output lives in the worktree

Cargo writes to `target/`, the web shell to `web/node_modules/` and `web/dist/`, and the desktop app to `desktop/node_modules/`, `desktop/dist/` and `desktop/out/`, all at their default locations inside the checkout and all ignored by Git.
Nothing a checkout builds is written anywhere else, so `git worktree remove` is the whole cleanup, and no cache can outlive the work that produced it.

Two facts make the default location the only correct one.

Cargo names a workspace member's artifacts by its path relative to the workspace root, so two checkouts sharing one target directory read each other's build as fresh and run the other checkout's test binary.
Cargo also holds an exclusive lock on the directory, so sharing serializes the parallel builds the worktree layout exists to allow.

The release binaries are `target/release/{hided,hide,hide-host-helper,hide-agent-hooks}`, and `desktop/scripts/package.mjs` reads every one of them from that fixed relative path.
Redirecting a release build with `CARGO_TARGET_DIR`, `--target-dir` or `--build-path` leaves the packager reading a path nothing wrote.
`scripts/verify-cargo.sh` therefore sets `CARGO_TARGET_DIR` to the worktree's own `target/` on every invocation, whatever the caller carried.

The cost of this layout is one full build cache per worktree, which is why a worktree is removed when its branch lands rather than kept around.

A release `hided` carries `web/dist` inside the binary: `hided/build.rs` embeds every file under it when the profile is `release` and fails the build when `web/dist/index.html` is missing, so a release build is always `pnpm --dir web build` first, then `cargo build --release --locked -p hided --bins -p hide-host --bin hide-host-helper -p hide-agent-hooks --bin hide-agent-hooks` (`scripts/verify-cargo.sh release`).
`desktop/scripts/package.mjs` performs that build before staging the release binaries into the app bundle, so a packaged app's SessionStart probe has a matching `hide-agent-hooks` executable even without a separate CLI on `PATH`.
A debug `hided` embeds nothing and reads `web/dist` from disk at run time (`HIDED_UI_DIR` overrides the lookup), so a rebuilt web shell shows up without a cargo rebuild.
The web measurement (`MEASURE_SCENARIO=multi HIDE_MEASURE_RUN_DIR=agents/runs/<slug>/measure/<attempt> bash scripts/web-shell-measure/run.sh`, `docs/PERFORMANCE_TESTING.md`) needs that release `hided` at `target/release/hided` inside the worktree it measures; it is never redirected, for the same reason as the release binaries.
On 2026-09-09 one abandoned worktree held 5.3 GB, over half of the 10 GB across all eight.
An earlier design sent test builds to `/tmp/hide-verify-<uid>/<hash>/`, keyed by checkout path so a run judging the tree would not dirty it; the reason had lapsed once the harness scored only tracked files, and the caches it left behind reached 7.5 GB with no worktree owning them.
Hide's own merged-worktree cleanup had grown a step that forked `bash` to compute and delete that path, which this layout makes unnecessary.

`[profile.dev] incremental = false` in the workspace manifest is deliberate, not a leftover.
An agent worktree is built a few times and discarded, which never repays an incremental cache; what it does instead is grow one per worktree, and those had reached 1.5 GB.
Debug output is what makes a stale worktree expensive, because nothing strips it: a debug `hided` measures around 115 MB against 24 MB for the release binary.
The workspace's own crates build with `debug = "line-tables-only"` (one `[profile.dev.package.<crate>]` entry per member): a panic's backtrace keeps its files and lines, and the rest of their debug info, which is most of what their build writes and links, is left out.
Dependencies keep the default, so changing it would not invalidate the dependency builds CI restores from its cache.

## The toolchain is never copied

`scripts/toolchain-env.sh` is sourced by every script that calls cargo, and it resolves `CARGO_HOME` and `RUSTUP_HOME` from the cargo shim's own location so an isolated HOME reuses the machine's installed toolchain.

Without it a verification runner pays for a whole toolchain and keeps it.
rustup reads `RUSTUP_HOME` with a default of `$HOME/.rustup`, and a runner HOME makes that an empty directory; rustup does not fail there, it downloads and installs into it and reports the fact as a warning while exiting 0.
That exit 0 is why two earlier workarounds never ran: each had diagnosed the missing toolchain correctly, and each guarded its recovery behind a cargo invocation failing.
The cost was 1.3 GB of `.rustup` plus 128 MB of `.cargo` per run, 9.1 GB across eleven run directories, duplicating a toolchain already on the machine.

## One toolchain version

`rust-toolchain.toml` at the repository root names the exact Rust version, with rustfmt and Clippy, and every build uses it: each CI job on Linux, macOS and Windows, and every workstation checkout.
rustup reads the file on the first `rustc` or `cargo` call and installs that version if it is missing, so no workflow has a toolchain step and no runner image's own Rust builds anything.
Before the file existed, CI used whatever stable the runner image carried; on 2026-10-06 the ubuntu image moved to a new stable that deprecated one method and added a Clippy finding, and code already on `main` failed `-D warnings` in every pull request that touched Rust, and in `main` itself, with no change in this repository.

The version is written only in that file; `scripts/tests/test_rust_toolchain_pin.py` fails when a workflow, script or document restates it, or when a script reaches a toolchain by its directory rather than through rustup, which would build with another version.
A verification run under its own HOME still sources `scripts/toolchain-env.sh`, so rustup looks for the pinned version in the machine's `~/.rustup`: when it is installed there, nothing is downloaded; when it is not, rustup installs it there once, never under the runner HOME.

Moving to a new version is a pull request that changes the file together with whatever the new compiler and Clippy ask of the code, proven by that pull request's CI; a finding is fixed, not silenced with `allow`.

## Two entrypoints

`scripts/verify-cargo.sh` and `scripts/verify-web.sh` are the only way a check script or the PRD harness builds this repository.
The harness runs a verify command with no shell, so an `ENV=value cargo ...` binding fails with ENOENT at verify time, when a sealed run can no longer be amended; a script is the only place that environment decision can live.
A check script calls these scripts rather than cargo or pnpm directly, so the target directory and toolchain decisions are made once.

| Binding | Command | Output and proof |
| --- | --- | --- |
| test | `bash scripts/verify-cargo.sh test` | `target/debug`; locked workspace tests |
| lint | `bash scripts/verify-cargo.sh lint` | `cargo fmt --check` then `cargo clippy -D warnings` over every target |
| release | `bash scripts/verify-cargo.sh release` | `target/release/{hided,hide,hide-host-helper,hide-agent-hooks}`, the binaries the desktop packager ships |
| cli | `bash scripts/verify-cargo.sh cli` | `target/debug/{hide,hided,hide-host-helper,hide-agent-hooks}` for isolated CLI, daemon, SessionStart and install kit checks |
| web | `bash scripts/verify-web.sh` | `pnpm install --frozen-lockfile`, then typecheck, lint, test and build for both `web` and `desktop`: `web/dist` and `desktop/dist` |

The Cargo `test` mode forwards trailing test arguments, so an explicitly configured live probe can run as `verify-cargo.sh test <test-name> -- --ignored` without bypassing worktree isolation or toolchain ownership.
The no-argument `test` mode remains the full locked workspace gate.
Focused delivery and session-activity filters and their nonzero-test prerequisite are listed in [delivery.md: Verification](delivery.md#verification).
Each compiler or test process's failure reaches the caller.

CI uses scoped Cargo modes `test-scoped`, `check`, `build` and `clippy` with trailing Cargo arguments, for example `bash scripts/verify-cargo.sh test-scoped -p hide-platform -p hide-herdr-client`.
Each adds `--locked`, reuses the installed toolchain, clears inherited Herdr and legacy coordination overrides, and fixes output to this worktree's `target/`.
Scoped modes accept at most 128 arguments and refuse `--target-dir`, `--manifest-path` and `--config`; an unknown mode exits 2.
The sealed `test`, `lint`, `release` and `cli` invocations keep their existing behavior.
`verify-web.sh install [--ignore-scripts]` locks dependency installation; `verify-web.sh <web|desktop> <typecheck|lint|test|build>` runs one package step.
`web e2e` runs Playwright against the web output already built; `desktop e2e` rebuilds the desktop host before Playwright.
Both forward the test arguments and their exit status, so a missing test filter fails the caller.
`playwright-install` installs Chromium for web or desktop; `desktop package` packages this runner's app.
An invalid package/action pair or more than 128 trailing arguments exits 2; the no-argument full web gate stays unchanged.
`test_ci_verification_entrypoints.py` checks this external command boundary without building or installing; it complements the real build tests below.

The build regression tests in `test_verification_builds.py` use a tiny real Cargo workspace, not compiler mocks.
They check that a caller's `CARGO_TARGET_DIR` cannot move the release binaries, that output stays in the checkout, warm build reuse, a core value change, a changed failing test, and compiler and prerequisite failure propagation.
They require macOS with Cargo.


The Windows/Linux package smoke uses an isolated HOME and Herdr socket to check the command from a fresh shell, both configured agent hooks and the completed retirement row.
It then uses a second complete package fixture with an executable overlay that changes the daemon hash, proving replacement, refreshed command and hook paths, same-build reuse and standalone refusal.
All test daemons and temporary files are cleaned up on failure as well as success.

## The desktop app

`desktop/` is a pnpm workspace member; `pnpm install` at the root installs it with the web shell.
Electron downloads its runtime into `desktop/node_modules/electron/dist/` on the first launch rather than at install, so a lane that only typechecks never fetches it.
CI acquires that lock-resolved runtime once with `bash scripts/verify-web.sh desktop electron-install` before desktop or packaged-app fixtures, using the dependency's own checksum-verifying installer within a five-minute step.
An acquisition failure blocks the suite at that prerequisite and retains the upstream error instead of retrying the download in each test.

| Command | Does |
| --- | --- |
| `pnpm --dir desktop dev` | Bundles `desktop/dist/` with esbuild and launches the app unpackaged; it finds this worktree's `target/{debug,release}/hide` itself |
| `pnpm --dir desktop typecheck`, `lint`, `test` | The desktop CI lane |
| `pnpm --dir desktop e2e` | Playwright `_electron` against a private hided and pinned Herdr; needs `web/dist`, `target/debug/hide` and `hided`, and the pinned `herdr` as the web e2e does |
| `pnpm --dir desktop package` | `desktop/scripts/package.mjs`: builds the release binaries and fetches the pinned Herdr, bundles `hided`, `hide`, `hide-agent-hooks`, `hide-host-helper-macos-<arch>` and `herdr` into `Contents/Resources`, ad-hoc signs `desktop/out/hide-darwin-<arch>/hide.app` (bundle id `me.grab.hide.desktop`), and archives it to `desktop/out/hide-v<version>-macos-<arch>.zip` beside a `.sha256` checksum; nothing is notarized or installed. On Windows x64 and Linux x64 the same command builds that system's package, unsigned: the folder `desktop/out/hide-win32-x64/` or `hide-linux-x64/` with the same binaries (`.exe` on Windows, with Herdr's `conpty/` beside `herdr.exe`) and `hide-host-helper-<windows\|linux>-x86_64` in its `resources/`, archived to `hide-v<version>-windows-x64.zip` (the system's `tar.exe`) or `hide-v<version>-linux-x64.tar.gz` beside a `.sha256`. Each system packages only itself, and a machine of another architecture than its pinned Herdr asset is refused |
| `node desktop/scripts/smoke-package.mjs <archive>` | On Windows or Linux only: checks a package against its `.sha256`, unpacks it into a temporary folder, and with a private home and state folder runs the bundled `herdr --version`, the device helper, `hide-agent-hooks doctor`, and `hide connect`, `/health`, the embedded shell, first-launch CLI and hooks, a command from a fresh shell, the completed retirement row, replacement by a second package fixture, refreshed paths, same-build reuse, standalone refusal and `hide stop`, then waits for each daemon pid it started to end (ten seconds, a named failure beyond that) before it removes the folder, with no removal retry; `.github/workflows/package.yml` runs it after each package. It refuses macOS, where first-launch kit behavior is covered by the isolated desktop suite |

The app attaches to whatever daemon the environment names: without `HIDE_STATE_DIR` it is the operator's own at `~/.hide/state`.
For QA, set `HIDE_STATE_DIR`, `HOME`, `HERDR_SOCKET_PATH` and `HIDE_DESKTOP_USER_DATA_DIR` to private paths, as `desktop/e2e/fixture.ts` does and refuses to launch without.
Use `web/e2e/platform-fixture.ts` for the native executable and tool paths and for Windows `USERPROFILE`, `APPDATA` and `LOCALAPPDATA` inside that private home.
The fixture C shim needs the runner's native compiler, `cc` on Unix or `clang.exe` on Windows, which Playwright's `globalSetup` runs once before any test (`web/e2e/shims/build.ts`); this is a fixture prerequisite, not an installed product requirement.
A packaged app does not need `hide` on `PATH`: it ships its own CLI and Herdr, and only falls back to a login-shell PATH search and the well-known install directories when its own bundled CLI is somehow missing (see `docs/ARCHITECTURE.md`, The desktop host).
macOS may refuse the unsigned app's first launch until it is opened once with Open from the context menu.

The desktop e2e runs beside the operator's own work on the same Mac (issue 232).
Every launch through `desktop/e2e/fixture.ts` passes `--hide-show-inactive`, so the host shows its window behind every other window without activating the app and a second launch never focuses it, and `--disable-backgrounding-occluded-windows`, so the window keeps painting behind the operator's and a capture by window id is current.
The fixture also preloads `desktop/e2e/focus-guard.cjs` into the app, which records into a per-test file each time this app becomes active or a window takes the keyboard, from launch to quit, and fails any test not tagged `@needs-focus` that recorded one; specs take `test` from the fixture, and `launch` refuses a test that did not.
The guard sees only this app, so a spec that could reach another program (a browser, Finder, the folder picker) stubs it, as the existing ones do.
A spec that needs the key window or native input (a page holding the keyboard, a pinch, a native drag) carries the `@needs-focus` tag (`NEEDS_FOCUS` in the fixture) and focuses the window itself, a browser page through `focusPage` once the host shows that page, which gives the page the keyboard only after the window's `focus` event says macOS made it the key window; `pnpm --dir desktop e2e --grep-invert @needs-focus` runs everything that leaves the operator's keyboard alone, and CI runs the whole suite.
The suite runs the focus tests after every other one (`desktop/playwright.config.ts` puts them in a project that depends on the rest): a focus test brings its app forward and quits it, and on a machine with no other app in front, a CI runner, macOS then activates the next app that opens, which failed every background test after the first focus test.

## Release asset gate

The release workflow accepts only a stable `vX.Y.Z` tag whose commit is already an ancestor of protected `main`.
It serializes runs of the same tag and waits for all three packaging jobs before `node scripts/release-draft.mjs <tag> <event-commit-sha> <directory>` prepares the draft.
The file-only entrypoint is `node scripts/check-release-assets.mjs <tag> <directory> [release-pages.json]`.
The directory must contain exactly that tag's macOS ARM64 ZIP, Windows x64 ZIP and Linux x64 TAR.GZ, and one SHA-256 sidecar per archive.
The gate rejects a missing target, extra or mixed-version files, symbolic links, empty archives, an incorrectly named sidecar, and a mismatched digest; archive hashing streams bytes rather than retaining each package in memory.
Before any release write, the writer checks that the current tag still resolves to the event commit; the workflow passes its contents-write token as `GH_TOKEN`.
A missing credential, moved tag, or failed initial API read blocks preparation before writes; package workflow artifacts remain available.
An API failure after an append may leave a partial or complete draft, including an upload whose success response was lost.
The writer stops further preparation and preserves accepted assets; a retry validates those bytes before skipping them.
The operator keeps immutable releases turned on in the repository's Settings; the workflow does not check it.

The authenticated release and asset inventories are fully paginated, with a 100-page and 16 MiB limit; a tag lookup alone cannot establish that a draft is absent.
Multiple releases for the same tag, an invalid inventory, or a published release block the run.
A new draft records its event commit in its body; an existing draft must have that exact provenance record.
Legacy drafts are not automatically reused.
The provenance record describes this controlled writer's origin; it is not a cryptographic attestation or a defense against administrative bypass.

The writer binds to that checked release ID, uploads only missing names, and skips an existing asset only when its uploaded state, size and SHA-256 digest match the local file.
It never changes an existing release's metadata, deletes an asset, or overwrites a name.
Mismatched assets, incomplete `starter` uploads and duplicate-name races stop for review; a retry resumes a matching partial draft without replacing completed bytes.
The final read must still show the complete unpublished draft and matching bytes.
[GitHub immutability](https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases) supplies, once the operator setting is on, the server protection if publication happens between a read and upload; a GET alone is not an atomic draft-state condition.
The operational contract excludes concurrent owner-policy disabling, manual changes to the draft, and tag moves during preparation and publication.
Repeated tag reads do not make prepublication provenance atomic; the maintainer must confirm that the tag and draft source record still identify the tested commit before publishing.
Each request has a 30-second bound, JSON responses are limited to 8 MiB, and the workflow job has a ten-minute bound.

This gate proves the asset set and digests, not installation, terminal input/output, native first launch, signing or notarization.
The Windows and Linux package smoke checks cover bundled tools, daemon startup, the embedded shell and daemon shutdown; actual desktop and terminal checks on each supported system still need their own evidence before a maintainer publishes the draft.
The release workflow prepares a draft only and never publishes it automatically.
Its source guard does not establish that the repository setting is on; verify it before release work.
