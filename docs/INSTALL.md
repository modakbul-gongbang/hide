# Install hide

hide supports Apple Silicon Macs running macOS 14 or later.
You can install a published release when one is available or build the same app bundle from source today.

## Requirements

- An Apple Silicon Mac with macOS 14 or later.
- No Xcode is required to install or run hide.
- For source builds only: a current stable Rust toolchain with Rust 2024 edition support, Node.js 22, and pnpm 10.
- For source builds only: network access for the pinned Rust crates, the npm packages, the Electron runtime download, and the pinned Herdr asset.
- An agent CLI (Claude Code or Codex), installed and signed in, only if you want hide to launch that agent.

Check the build tools before a source install:

```sh
rustc --version
cargo --version
node --version
pnpm --version
```

## Install a release

Open the [hide Releases page](https://github.com/modakbul-gongbang/hide/releases).
If it says there are no releases, continue with [Build and install from source](#build-and-install-from-source).

Download both files for the same version:

- `hide-v<version>-macos-arm64.zip`
- `hide-v<version>-macos-arm64.zip.sha256`

Verify the archive from the directory that contains both downloads:

```sh
shasum -a 256 -c hide-v<version>-macos-arm64.zip.sha256
```

Continue only when the command prints `OK`.
Expand the archive, move `hide.app` to `/Applications`, then launch it:

```sh
open /Applications/hide.app
```

The release is ad-hoc signed and is not notarized.
On the first launch, Control-click `hide.app`, choose **Open**, and confirm the Gatekeeper dialog.

If macOS still keeps the verified app in quarantine, remove the quarantine attribute only after confirming the downloaded checksum:

```sh
xattr -dr com.apple.quarantine /Applications/hide.app
open /Applications/hide.app
```

## Build and install from source

Clone the repository and build the release bundle:

```sh
git clone https://github.com/modakbul-gongbang/hide.git
cd hide
pnpm install --frozen-lockfile
HIDE_VERSION=<version> pnpm --dir desktop package
```

`HIDE_VERSION` sets the version the packaged app reports.
Omit it to build from a checkout that a Git tag matching `v[0-9]*` already describes; the packaging script fails rather than ship a version nothing was released under.

Packaging builds the web shell and hcoord, builds the release `hided`, `hide`, `hide-agent-hooks`, `hide-host-helper` and `hide-agent-context-labels` binaries, fetches and digest-verifies the pinned Herdr binary, and stops with a named error and no app if any of those, or the labels plugin's packaged manifest, is missing or not executable.
It then packages everything into `desktop/out/hide-darwin-arm64/hide.app`, ad-hoc signs it, verifies the signature, and writes `desktop/out/hide-v<version>-macos-arm64.zip` with a `.sha256` sidecar.

Install the app into the system Applications directory and launch the installed bundle:

```sh
sudo /usr/bin/ditto --rsrc --extattr --qtn desktop/out/hide-darwin-arm64/hide.app /Applications/hide.app
open /Applications/hide.app
```

## Verify the installed app

Verify the bundle signature:

```sh
/usr/bin/codesign --verify --deep --strict --verbose=2 /Applications/hide.app
```

After opening hide, confirm the installed executable is the one running:

```sh
pgrep -fl '/Applications/hide.app/Contents/MacOS/hide'
```

Exactly one matching process should be active before checking the UI; a second match, at another path, is a dev or worktree build still running.
The app's bundle id is `me.grab.hide.desktop`.

## First launch

The app runs its own bundled `hide` CLI, which starts `hided` beside it.
`hided` serves the web shell inside the app's window.
The app passes its bundled Herdr binary to that CLI as `HERDR_BIN_PATH` unless the environment names one as an explicit override, one that arrives without `HERDR_PANE_ID`; the value a Herdr pane exports is replaced, so opening the app from a pane behaves like opening it from the Dock.
The CLI search order, and `HIDE_CLI_PATH`, are documented in [ARCHITECTURE.md](ARCHITECTURE.md), "The desktop host".

A Herdr server must be running for panes to attach.
The binary the app bundles is at `hide.app/Contents/Resources/herdr`; running it once, with no arguments, starts the default server on `~/.config/herdr/herdr.sock`.
Set `HERDR_SOCKET_PATH` to an absolute path before launching to use another socket.
A server already running on the socket hided uses is used as it is; hide never stops or replaces it.

Authentication is not bundled: SSH, Herdr, Claude Code, and Codex continue to own their own sign-in state and credentials.
If you want to start agents from hide, install and sign in to the relevant CLI before launching the app.

### What the first launch installs

Every launch of the installed app installs Hide's kit on this Mac without asking, and puts back only what a newer app needs:

- `~/.local/bin/hide`, a link to the app's `hide` command, unless a `hide` that is not Hide's is already there;
- Hide's entries in `~/.claude/settings.json` and `~/.codex/hooks.json`, for each of Claude Code and Codex that is set up on this Mac, next to whatever other tools put there;
- the agent labels Herdr plugin, `hide.agent-context-labels`, copied to `~/.hide/kit/plugins/` and linked in the Herdr server hide uses, replacing a copy of the same plugin installed from GitHub or linked from a checkout;
- hcoord, copied to `~/.hide/kit/hcoord/`, with its command at `~/.hcoord/bin/hcoord` and its daemon.

Settings > Devices shows each of these on This Mac's row, with where it is or why it is not.
A part you remove by hand stays removed; Reinstall on that row puts it back.
A `hided` run outside the installed app, such as a development build, installs nothing on this Mac and says so there.
The standalone `herdr plugin install <owner>/<repo>/plugins/agent-context-labels` still works for a machine without hide.

## Connect another Mac over SSH

hide can show and drive a Herdr server on another machine.
Open Settings, choose Devices, and add the machine with a label and the alias `~/.ssh/config` already knows it by; that alias is the only thing hide stores about it.
Authentication stays with SSH: the alias's `IdentityFile`, or the running SSH agent, is what hide signs in with, and hide never asks for or keeps a password.

The remote machine needs Herdr installed where a non-login shell finds it (`~/.local/bin`, Homebrew, or the system paths) and a running `herdr server`, and Node 22.12 or later for hcoord.
hide asks that machine `herdr status server --json` to learn where the server socket is, so nothing about the remote user or home directory is configured on this side.
Adding the machine installs the same kit a first launch installs here, under that account's home, with the form listing each part and where it goes: Hide's helper and every part's files in `~/.local/share/hide/host-helper`, the `hide` link in `~/.local/bin`, the Claude Code and Codex hook entries, the labels plugin in that machine's Herdr, and hcoord.
Every connection brings the kit up to this Hide's version, and a part you removed there stays removed until Reinstall on that machine's row.
hcoord keeps its daemon running only on macOS, so on another system its row says so and nothing is installed for it.
A pane on that machine can then run `hide file open`, `hide diff open` or `hide browser open http://localhost:3000`, and the result opens in this Hide, with `localhost` meaning that machine; that shell's `PATH` has to include `~/.local/bin` for a bare `hide` to be found.
Its agent panes show labels, subagent counts and Workspace guidance as panes on this Mac do; Project Memory stays on this Mac and is not given to a device's sessions.
A machine allowed by an earlier version of Hide gets the whole kit on its next connection without asking again; a machine added without the helper installs nothing until you press Allow and install on its row.
Only machines on the platform this build carries get the kit; another platform shows why on its row and is only viewed and driven.
Removing a machine while it is connected takes Hide's hook entries, the labels plugin link, the `hide` link and the helper folder off it, and leaves hcoord, which other tools there may use; removing it while it is not connected leaves them there, where they do no harm, and adding it again replaces them.
Each device row in Settings shows whether the remote session is connected and, when it is not, the reason in the words the connection failed with; `Test` runs the SSH, authentication, Herdr, protocol, PTY, SFTP, and Git stages one after another and lists the first one that needs attention on that host.

## Update

Quit hide, replace `/Applications/hide.app` with the new release or rebuild, and reopen it.
State under `~/.local/state/hide` and the desktop profile at `~/Library/Application Support/hide-desktop` both persist across an update.
Files the previous Swift app left under `~/Library/Application Support/hide/` are not read by the current app, apart from `state.json`'s shortcut bindings, which are imported once, and can be deleted by hand.
Each launch replaces an outdated part of the kit on this Mac, and each device connection does the same there, hook entries included; a part you removed stays removed.

## Uninstall

Quit hide and move `/Applications/hide.app` to the Trash.
This removes the application but keeps `~/.local/state/hide` and `~/Library/Application Support/hide-desktop`.
Delete both directories only when you deliberately want to reset hide's saved state.
Hide's hook entries stay in `~/.claude/settings.json` and `~/.codex/hooks.json` and do nothing once the app is gone; delete the entries whose command carries `hide-subagents@` to take them out.
The labels plugin link (`herdr plugin unlink hide.agent-context-labels`), `~/.local/bin/hide`, `~/.hide/kit` and hcoord (`~/.hcoord`) stay as well until you remove them.

## Troubleshooting

### The window shows a connection failure instead of the app

The desktop host shows a status page while it is connecting, and a failure page with a `Retry` button when it cannot reach the daemon: `cli_missing` when no `hide` executable was found, `start_failed` when the CLI could not start it, and `no_response` when it started but never answered.
Read the host log at `<profile>/logs/desktop.log` (the profile is `~/Library/Application Support/hide-desktop`, or `HIDE_DESKTOP_USER_DATA_DIR`) for the detail behind whichever reason the page shows.

### No Herdr session

If the app connects but no workspace, terminal, or agent can start, no `herdr server` is running on the socket hided uses.
The daemon logs `herdr_bin.missing` when it cannot find a herdr binary at all: check `HERDR_BIN_PATH`, then PATH, then run the bundled `hide.app/Contents/Resources/herdr` once to start the default server.
When `HERDR_BIN_PATH` names a file that no longer exists, `hide connect` and the daemon refuse to start and print that path: the Herdr server was started from an app bundle that has since been replaced, and every pane it opens still carries the old path.
The packaged app replaces that pane value with its own bundled `herdr`, but the `hide` CLI run directly in such a pane, and an unpackaged development host, still inherit it; unset `HERDR_BIN_PATH` there, or hand the server off to the new bundle's `herdr`.

<!-- herdr-provenance:start -->
hide distributes the [upstream Herdr release v0.9.1](https://github.com/herdrdev/herdr/releases/tag/v0.9.1).
The bundled binary is not modified by hide.
The weekly `herdr-update.yml` workflow proposes upstream stable releases with `--repo herdrdev/herdr`; updates must pass contract and runtime checks.
<!-- herdr-provenance:end -->

### An agent cannot start

Run `command -v claude` or `command -v codex` in your login shell and complete that CLI's own sign-in flow.
hide never substitutes the other agent when the selected executable is missing.

### Remote workspaces do not connect

Confirm the SSH host works outside hide first, with the same alias the device was added with:

```sh
ssh <host-alias> true
ssh <host-alias> 'PATH="$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH" herdr status server --json'
```

The second command is what hide runs to find the remote socket.
`command not found` means Herdr is not installed there or not on a non-login shell's `PATH`; `"running":false` means the server is stopped, and running `herdr` once on that machine starts it.
The device row in Settings carries the same answer, and `Test` names the stage that failed.
hide delegates authentication to the existing SSH configuration and agent.
It does not display, copy, or store a password.
