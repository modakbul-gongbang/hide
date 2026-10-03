<p align="center">
  <img src="desktop/resources/hide-icon-1024.png" width="132" alt="hide owl app icon">
</p>

<h1 align="center">hide</h1>

<p align="center"><strong>Many agents. One clear view.</strong></p>

<p align="center">
  A calm IDE for coding with multiple agents, with each task in context and attention on what matters.
</p>

<p align="center">
  <img alt="macOS 14+" src="https://img.shields.io/badge/macOS-14%2B-black?logo=apple">
  <img alt="Apple Silicon" src="https://img.shields.io/badge/Apple%20Silicon-arm64-111111">
  <img alt="Electron" src="https://img.shields.io/badge/Electron-desktop-9FEAF9?logo=electron&logoColor=black">
  <img alt="Rust 2024" src="https://img.shields.io/badge/Rust-2024-000000?logo=rust&logoColor=white">
  <img alt="Bundled Herdr runtime" src="https://img.shields.io/badge/Herdr-bundled-B9FF66">
  <a href="LICENSE"><img alt="MIT License" src="https://img.shields.io/badge/license-MIT-blue"></a>
</p>

hide is a multi-agent IDE built on [Herdr](https://github.com/herdrdev/herdr), with an Electron desktop host and a Rust daemon.
It brings local and remote workspaces, checkouts, terminal panes, files, and coding agents into one keyboard-first window.
It surfaces the questions, approvals, and results that need a person's attention while leaving session state and credentials with the tools that own them.

## Why hide

- **One map of active work.** Navigate Herdr workspaces, checkouts, tabs, panes, and agent activity from a single sidebar.
- **Terminals stay where the work is.** Attached Herdr panes render in place with their real layout, focus, zoom, scrollback, and terminal state.
- **Agents start in the right checkout.** Launch Claude Code or Codex from the selected workspace without rebuilding its context by hand.
- **Local and remote stay distinct.** Work on this Mac or an SSH-connected Mac while keeping remote file writes inside the attached terminal.
- **Files remain understandable.** Browse and edit existing local files in the Explorer, with remote trees exposed as read-only context.
- **A browser shows beside the work.** `hide browser open <url>` opens a browser as a View area display in the desktop app.
- **Attention is visible.** Questions, approvals, completed turns, and failures make the next action easy to spot.

## How it works

```text
Herdr server and panes
        │
        │ versioned snapshots + terminal chunks
        ▼
herdr-core (Rust)
        │
        │ typed events in, snapshot deltas out
        ▼
hided (daemon: loopback HTTP + a token WebSocket)
        │
        │ serves the built web shell
        ▼
web shell (React)
        │
        ├─ workspace and checkout navigator
        ├─ live pane layout and terminal surfaces
        ├─ local Explorer and remote read-only tree
        └─ agent launcher and settings
```

shown by the Electron desktop app, or by a browser pointed at the daemon.

The Rust core owns the authoritative runtime state behind one snapshot boundary.
The web shell renders that state and sends typed user events back, so the UI does not maintain a competing model of the session.

## Install

The packaging targets are Apple Silicon Macs running macOS 14 or later, Windows x64 and Linux x64 ([Windows and Linux](docs/INSTALL.md#windows-and-linux)).
Published builds are listed on [GitHub Releases](https://github.com/modakbul-gongbang/hide/releases); a draft or a successful package build is not a public download.
To build locally, install a Rust toolchain with Rust 2024 support, Node.js 22.12.0 or later and pnpm 10, then:

```sh
git clone https://github.com/modakbul-gongbang/hide.git
cd hide
pnpm install --frozen-lockfile
HIDE_VERSION=0.0.0-local pnpm --dir desktop package
sudo /usr/bin/ditto --rsrc --extattr --qtn desktop/out/hide-darwin-arm64/hide.app /Applications/hide.app
open /Applications/hide.app
```

On macOS the source build creates an ad-hoc signed `hide.app`, a versioned zip archive, and a SHA-256 sidecar under `desktop/out/`.
`0.0.0-local` identifies a local build; set the intended version explicitly when packaging another build.
The app bundles the pinned upstream Herdr runtime, so a separate Herdr install is not required for a first launch.

See [Install hide](docs/INSTALL.md) for prerequisites, release checksum verification, Gatekeeper steps, first-launch behavior, updates, and troubleshooting.

See [Browser displays](docs/BROWSER_DISPLAYS.md) for `hide browser open`, the pages the desktop app draws in View areas, and the `file:` address boundary.

## Runtime boundaries

- The app ships the pinned Herdr binary and names it to the daemon as `HERDR_BIN_PATH`.
  When no Herdr server answers on the socket the app uses, as after a reboot, the app starts its bundled Herdr there; a server that answers is used as it is and never stopped or replaced, and the server keeps running after the app quits.
- Provider sign-in and SSH authentication use the operator's installed CLIs and SSH configuration.
  For Codex usage status, hide reads the CLI's existing `auth.json` access token and account ID and sends them to the provider's usage endpoint.
- Claude Code and Codex remain separate tools and must already be installed and signed in if you want to launch them from hide.
- Remote Explorer trees are read-only.
  Remote edits stay in the terminal attached to that remote Herdr session.

<!-- herdr-provenance:start -->
hide distributes the [upstream Herdr release v0.9.1](https://github.com/herdrdev/herdr/releases/tag/v0.9.1).
The bundled binary is not modified by hide.
The weekly `herdr-update.yml` workflow proposes upstream stable releases with `--repo herdrdev/herdr`; updates must pass contract and runtime checks.
<!-- herdr-provenance:end -->

## Development

Contributions go through pull requests gated by the `verify` workflow; `CONTRIBUTING.md` lists the gates and how to run them locally, and `SECURITY.md` says how to report a vulnerability privately.

Prerequisites are a Rust toolchain with Rust 2024 edition support, Node.js 22.12.0 or later, and pnpm 10.

```sh
bash scripts/verify-cargo.sh test
bash scripts/verify-web.sh
pnpm --dir desktop dev
HIDE_VERSION=0.0.0-local pnpm --dir desktop package
```

`pnpm --dir desktop dev` runs the desktop app unpackaged against this worktree's own `hide` build.
`HIDE_VERSION=0.0.0-local pnpm --dir desktop package` produces a local-version bundle and archive under `desktop/out/`.

Repository map:

- `desktop/` - the Electron desktop host.
- `web/` - the React web shell.
- `hided/` - the daemon and `hide` CLI.
- `herdr-core/` - the platform-neutral Rust runtime.
- `hide-agent-hooks/`, `hide-ai/`, `hide-herdr-client/`, `hide-host/`, `hide-memory/`, `hide-project/`, `hide-session/` - supporting Rust crates.
- `plugins/` - Herdr plugins shipped from this repository.
- `contracts/` - the pinned Herdr protocol contract.
- `assets/pet-theme/` - artwork kept for a future Electron feature ([issue #184](https://github.com/modakbul-gongbang/hide/issues/184)); the current app does not use it.
- `docs/` - [current documentation map](docs/README.md), with runtime guides separated from historical and visual references.

Before visual verification, read [Which app is actually running](docs/dev-runtime.md) and [Verification](docs/VERIFICATION.md).
Identify the candidate by its executable, build, PID and window; an isolated candidate can run beside the operator's app.

## Design and license

The interface follows the behavior contract in [docs/UI_BEHAVIOR.md](docs/UI_BEHAVIOR.md), styled from the Pen design-system library and `design/tokens.json`, and uses original React and TypeScript code.
hide is available under the [MIT License](LICENSE).
