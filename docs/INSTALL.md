# Install hide

hide supports Apple Silicon Macs running macOS 14 or later.
You can install a published release when one is available or build the same app bundle from source today.
Each release also carries unsigned packages for Windows x64 and Linux x64, built from the same source; [Windows and Linux](#windows-and-linux) says how to install one and what it does not do yet.

## Requirements

- An Apple Silicon Mac with macOS 14 or later, or a Windows x64 or Linux x64 machine for the unsigned packages.
- No Xcode is required to install or run hide.
- For source builds only: a current stable Rust toolchain with Rust 2024 edition support, Node.js 22, and pnpm 10.
- For source builds on Windows only: the Rust MSVC toolchain with the Visual Studio C++ build tools, Git for Windows, and PowerShell 7 (`pwsh` fetches the pinned Herdr); run the build from Git Bash, so `bash` is Git's and not the WSL launcher in `System32`.
- For source builds on Linux only: `zsh`, `jq`, and `curl`, `shasum` and `awk` under `/usr/bin`, which the Herdr fetch uses.
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

## Windows and Linux

The Windows and Linux packages are not signed.
Each is a folder in an archive: `hide-win32-x64` in `hide-v<version>-windows-x64.zip`, and `hide-linux-x64` in `hide-v<version>-linux-x64.tar.gz`, each with a `.sha256` file beside it on the Releases page.
The folder holds the Electron app at its top and, in its `resources` folder, the same `hided`, `hide`, `hide-agent-hooks`, device helper, pinned Herdr the macOS app carries; on Windows those are `.exe` files, and Herdr's ConPTY runtime is the `conpty` folder beside `herdr.exe`.

### Install on Windows

Download both files for the same version, then check the archive in PowerShell from the folder that holds them:

```powershell
(Get-FileHash -Algorithm SHA256 .\hide-v<version>-windows-x64.zip).Hash.ToLower()
Get-Content .\hide-v<version>-windows-x64.zip.sha256
```

Continue only when the first line printed is the hash at the start of the second.
Extract the zip (Extract All in File Explorer), move the `hide-win32-x64` folder where you keep programs, and open `hide.exe` inside it.

Windows SmartScreen warns about an unsigned app it has not seen before, and the first launch stops at "Windows protected your PC".
Choose **More info**, check that the dialog names the app `hide.exe` with an unknown publisher, then choose **Run anyway**.

### Install on Linux

Download both files for the same version, then check and unpack the archive from the folder that holds them:

```sh
sha256sum -c hide-v<version>-linux-x64.tar.gz.sha256
tar -xzf hide-v<version>-linux-x64.tar.gz
./hide-linux-x64/hide
```

Continue only when `sha256sum` prints `OK`.
The tar keeps the files executable, so there is nothing to `chmod`; nothing is installed system-wide, and no menu entry is created.
The app needs the libraries every Electron app does (GTK 3, NSS, ALSA), which a desktop distribution already has.
Electron needs a sandbox on Linux: where the system lets an unprivileged process make user namespaces it needs nothing more, and where it does not (Ubuntu 24.04 and later restrict them through AppArmor), the app stops with a message about `chrome-sandbox`.
Then make the bundled sandbox helper setuid root once, from the folder you unpacked:

```sh
sudo chown root:root hide-linux-x64/chrome-sandbox
sudo chmod 4755 hide-linux-x64/chrome-sandbox
```

### What differs from the macOS app

- Closing the last window quits the app on Windows and Linux, as other apps there do; on macOS the app keeps running with no window until you quit it.
  Either way `hided` keeps running after the app is gone, as [First launch](#first-launch) describes.
- The app's profile is `%APPDATA%\hide-desktop` on Windows and `~/.config/hide-desktop` on Linux, instead of `~/Library/Application Support/hide-desktop`.
- First launch installs the `hide` command and the Claude Code and Codex hooks for runtimes configured on this machine.
  Settings > Devices shows their installation results under This machine.
- Linux installs `~/.local/bin/hide` as a link to the package's `resources/hide`.
  Windows installs `%USERPROFILE%\.local\bin\hide.cmd`, which runs `resources\hide.exe` through a `.hide-kit` directory junction beside the command; neither Administrator access nor Developer Mode is needed.
  Keep the unpacked package folder in place while using it.
- Opening a newer package replaces a running daemon of another build and refreshes the command and hook paths, as on macOS.
  A development or standalone `hide` still refuses to replace a packaged daemon.
- A package carries the device helper for its own system only, as the macOS app does: a Linux package installs it on a Linux x64 device, while no device runs the Windows one yet (Windows devices are not supported), so from Windows a device's files and Git stay unavailable.
- Nothing updates itself, and uninstalling is deleting the folder; `~/.hide` (`%USERPROFILE%\.hide` on Windows) and the profile stay until you delete them.

### What each system supports

Supported means the package is built from the release, the feature is written for that system, and CI exercises it there.
Unverified means the feature is written to work there, but no check on that system has proved it.

| | macOS (Apple Silicon, 14 or later) | Windows x64 | Linux x64 |
| --- | --- | --- | --- |
| Package | signed app bundle | unsigned folder | unsigned folder |
| Daemon, Herdr, install kit, agent hooks | supported | supported (CI runs them against the pinned Herdr) | supported (the web shell's whole e2e suite runs against the pinned Herdr) |
| Desktop app window and menu | supported (desktop e2e) | unverified (no desktop e2e runs on Windows) | unverified (no desktop e2e runs on Linux) |
| Keyboard shortcuts | supported | supported by registry tests; the table is in [UI_BEHAVIOR.md](UI_BEHAVIOR.md#keyboard-shortcuts-per-system) | same as Windows |
| Device helper (a remote machine's files and Git) | supported | not supported | supported |

Unverified on Windows and Linux (the macOS assumptions below are left as they are, and no check on those systems has proved them):

- Paths outside the `$HOME` boundary, and drive letters and UNC paths on Windows.
- Opening a path in the system's file manager (Reveal), and opening a file or link in the default app.
- Move to Trash through the system's trash.
- Terminal programs that expect macOS text-editing keys; Windows and Linux send `Ctrl+U`, `Home` and `End` as typed.
- Whether Alt+Shift also switches the input language on a Windows or Linux layout; hide's chord still runs.

### Make the command available in a new shell

Hide never changes PATH or the Windows registry.
On Linux, if your shell does not already include `~/.local/bin`, add `export PATH="$HOME/.local/bin:$PATH"` to your shell's startup file and open a new shell.
On Windows, add `%USERPROFILE%\.local\bin` to your user Path using **Edit environment variables for your account**, then open a new terminal.
Run `hide status --json` to check that the installed command resolves.
You can also call the command by its full path without changing PATH.

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

Packaging builds the web shell, builds the release `hided`, `hide`, `hide-agent-hooks` and `hide-host-helper` binaries, fetches and digest-verifies the pinned Herdr binary, and stops with a named error and no app if any of those is missing or not executable.
It then packages everything into `desktop/out/hide-darwin-arm64/hide.app`, ad-hoc signs it, verifies the signature, and writes `desktop/out/hide-v<version>-macos-arm64.zip` with a `.sha256` sidecar.
The same command on Windows x64 writes `desktop/out/hide-win32-x64/` and `hide-v<version>-windows-x64.zip`, and on Linux x64 `desktop/out/hide-linux-x64/` and `hide-v<version>-linux-x64.tar.gz`, each with its `.sha256` and unsigned; each system builds only its own package.

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
It keeps running after the window closes for as long as the Herdr server it follows answers, so agent labels stay current with no window open; it exits after ten minutes with no client connected and no answering Herdr server.
The app passes its bundled Herdr binary to that CLI as `HERDR_BIN_PATH` unless the environment names one as an explicit override, one that arrives without `HERDR_PANE_ID`; the value a Herdr pane exports is replaced, so opening the app from a pane behaves like opening it from the Dock.
The CLI search order, and `HIDE_CLI_PATH`, are documented in [ARCHITECTURE.md](ARCHITECTURE.md), "The desktop host".

Panes attach to a Herdr server.
When none answers on the socket hide uses, the app starts the Herdr it bundles (`hide.app/Contents/Resources/herdr`) there before it connects, and that server restores the saved workspaces and resumes the agents Herdr can resume, as a server `herdr` starts from a terminal does.
The default socket is the one Herdr itself uses: `$XDG_CONFIG_HOME/herdr/herdr.sock` when that is set, otherwise `~/.config/herdr/herdr.sock`; set `HERDR_SOCKET_PATH` to an absolute path before launching to use another socket.
A server already running on that socket is used as it is; hide never stops or replaces it, and a server the app started keeps running after the app quits.

Authentication is not bundled: SSH, Herdr, Claude Code, and Codex continue to own their own sign-in state and credentials.
If you want to start agents from hide, install and sign in to the relevant CLI before launching the app.

### What the first launch installs

Every launch of the installed app installs Hide's kit on this Mac without asking, and puts back only what a newer app needs.
The one exception is a Mac the kit has never run on: it holds Claude Code's and Codex's hook entries and skills, and Codex's per-pane daemon setting, until the operator answers the first-run agent choice (UI_BEHAVIOR.md, Settings > Agents); both agents are recorded off in the hold, installed or not, so an agent installed later also starts off, and the rest below is installed as usual:

- `~/.local/bin/hide`, a link to the app's `hide` command, unless a `hide` that is not Hide's is already there;
- Hide's entries in `~/.claude/settings.json` and `~/.codex/hooks.json`, for each of Claude Code and Codex that is set up on this Mac, next to whatever other tools put there;
- A one-release retirement stage removes the former coordination installation after its read-only preflight succeeds; see Coordination retirement below;
- `Codex를 pane마다 실행`: when this Mac's Codex has the shared app-server daemon turned on, `codex features disable daemon_auto_start`, so each Codex runs in its own pane and Hide can read it. A daemon already running keeps running, and the setting reaches each Codex started after it. Settings > Devices turns it back on (`codex features enable daemon_auto_start`), and Hide then leaves it on.

### Coordination retirement

Merging this change and installing its build performs the transition.
The operator chooses a time with no active sasu runs, open requests or active watches.
The kit retains this stage for one release on each machine, with its result shown in Settings > Devices.

Before changing anything, the stage checks the legacy ledgers, Hide's delivery ledger and the sasu run registry and registered checkout run state.
Both the legacy v1 and current Hide-only v2 registries are inspected read-only; a selected revision that disappears causes a refusal and a retry instruction, never an empty-registry assumption.
An open item, an unreadable or unknown record, or a capacity limit stops the pass, names the reason and recovery action, and leaves the machine unchanged.
An acknowledged letter with an explicit unconfirmed hook receipt is still open; a legacy acknowledgement with no receipt and no answer wait remains closed for compatibility and provides no confirmation evidence, while malformed or contradictory receipts refuse the pass.
Every local directory below the selected HOME or registered checkout must be a real directory owned by the account and unwritable by other accounts; an alias at or above that selected root remains valid.
An outside-HOME `HIDE_STATE_DIR` or `XDG_STATE_HOME` remains supported only after its entire namespace is inspected, including before a missing ledger is treated as empty; protected system directories and authenticated system aliases are allowed, while account-controlled links and directories another account can modify are refused without changing them.
A relocated legacy home must stay below HOME and cannot overlap the active Hide state or kit; an indexed run outside HOME needs its owning checkout registered before inspection.
On a device, the existing SSH/SFTP connection performs this read-only check against that device's registered checkouts before uploading a helper, creating its folders or changing its current link.
It shares the local ledger and run predicates, needs no remote interpreter, and stops at the existing 15-second SSH operation deadline.
The kit never stops an old test server by name without an ownership receipt.
Before the transition, the operator checks earlier test servers and refuses automatic cleanup of any whose ownership cannot be confirmed.
After that check succeeds, the ordered steps stop the old daemon, unload its LaunchAgent and delete the plist, remove only Hide-owned command and plugin links, remove the kit copy, and rename the old `~/.hide/hcoord` and `~/.hcoord` folders to siblings ending in `.retired-YYYY-MM-DD`.
When Herdr's server is unavailable, the link step strictly inspects its owned `plugins.json` registry at the platform layer's pinned Herdr config location: `$XDG_CONFIG_HOME/herdr`, otherwise `~/.config/herdr` on Unix or `%APPDATA%\herdr` on Windows.
A missing registry or absent coordination entry needs no server or Herdr command; a registered entry is removed through Herdr's offline `plugin uninstall`, then its absence is checked again.
Foreign plugin entries and locally linked source remain intact; an unreadable, untrusted or oversized registry, a missing command for a registered entry, or unconfirmed removal keeps the link step failed for retry.
A socket confirmed absent, or an owned socket that refuses a connection, is already stopped; a missing socket does not need its name validated by the local transport.
A linked, foreign or unreadable endpoint is refused, and other connection failures remain visible at the stop step.
The old ledger is never imported into Hide or deleted.
There is no compatibility command and no rollback.

The private `~/.hide/kit/coordination-retirement.json` receipt records completion or the failed step.
An interrupted pass retries idempotently at the next kit pass or Reinstall, with completed removals staying removed.
A failed retirement step stays visible while the ordinary kit parts continue their pass; a preflight refusal stops the whole pass before it changes anything.
A completed stage does no more work, and the operator decides when a later release removes this stage after every device has passed it.
Retirement is tested only with private homes, fixture ledgers and injected service control.

### Where Hide keeps its files

Everything Hide owns on a machine is under `~/.hide`:

| Folder | What it holds |
| --- | --- |
| `~/.hide/state` | The daemon's state: registered projects, screen layout, labels, phone pairing, the session search index (`session-search.sqlite3`) and the link record (`links.sqlite3`), logs (`HIDE_STATE_DIR` or a set `XDG_STATE_HOME` choose another folder) |
| `~/.hide/kit` | The kit record and one-release retirement receipt |
| `~/.hide/agent-hooks` | The hook helper's per-pane counters and last report |
| `~/.hide/host-helper` | On a device: Hide's helper builds |

Outside it stay only what another program reads at a place it chose: Hide's entries in `~/.claude/settings.json` and `~/.codex/hooks.json`, and the `hide` link in `~/.local/bin`.
macOS's own places (`~/Library/Application Support/hide-desktop`, `~/Library/Application Support/hide` with the AI settings and Project Memory), the Home folder `~/hide`, and the label generator lock beside the Herdr socket are not moved.

A build from before this layout kept the same files in `~/.local/state/hide`, `~/.local/share/hide`; the retired coordination folders are handled by the stage above.
The first launch of a newer app moves them once:

- the state folder is renamed to `~/.hide/state` as a whole after the daemon running from it is stopped; a `~/.hide/state` that already exists is used, the old folder is left untouched, and the daemon's log records `state.legacy_left` with both paths;
- `~/.local/state/hide-plugin-upgrade`, `~/.local/share/hide/agent-context-labels`, and then `~/.local/share/hide` if it is empty, are removed; `~/.local/state` and `~/.local/share` stay.


Settings > Devices shows each of these on This Mac's row, with where it is or why it is not.
A part you remove by hand stays removed; Reinstall on that row puts it back.
Each agent Hide knows also has a switch on that row, and Claude Code and Codex are on until you turn them off.
An agent that is on gets a `hide-browser` skill stub in the folder it reads skills from, and, where its documentation supports one, a `SessionStart` hook entry; every other agent is off until you switch it on, and an agent that is not set up on the machine has no switch there.
Switching an agent off takes out only the entries and stubs Hide wrote, and [agent-hooks.md](agent-hooks.md#other-agents-skill-and-guidance-hook) lists every agent, its skill folder, and why a hook is or is not written for it.
A `hided` run outside the installed app, such as a development build, installs nothing on this Mac and says so there.
Agent labels are made by hide's own daemon, so there is no labels part to install.
A machine that still has the old agent labels Herdr plugin, `hide.agent-context-labels`, loses it on the first kit pass after an update: the plugin is unlinked from Herdr (a copy installed from GitHub through `herdr plugin uninstall`), any watcher process holding its lock is stopped, and its copy under `~/.hide/kit/plugins/` and its state folder `~/.local/state/hide.agent-context-labels` are removed.
On this Mac the first launch reads that state folder once, before it is removed, so a pane whose session the plugin had already labeled keeps that label.
Herdr itself, your agent sessions, Herdr's own plugin configuration folder and your `config.toml` are not touched.
If a part cannot be taken out, for example because Herdr is not running, it stays, the diagnostic log records `plugin.retire_incomplete`, and the next kit pass tries again.

## Connect another Mac over SSH

hide can show and drive a Herdr server on another machine.
Open Settings, choose Devices, and add the machine with a label and the alias `~/.ssh/config` already knows it by; that alias is the only thing hide stores about it.
Authentication stays with SSH: the alias's `IdentityFile`, or the running SSH agent, is what hide signs in with, and hide never asks for or keeps a password.

The remote machine needs Herdr installed where a non-login shell finds it (`~/.local/bin`, Homebrew, or the system paths) and a running `herdr server`,.
hide asks that machine `herdr status server --json` to learn where the server socket is, so nothing about the remote user or home directory is configured on this side.
Adding the machine installs the same kit a first launch installs here, under that account's home, with the form listing each part and where it goes: Hide's helper and every part's files in `~/.hide/host-helper`, the `hide` link in `~/.local/bin`, the Claude Code and Codex hook entries.
A machine allowed for the older `~/.local/share/hide/host-helper` is not asked again: its next connection installs in `~/.hide/host-helper`, re-points the `hide` link and the hook entries there, and then removes the old helper folder, the old `~/.local/state/hide/workspace-bridges`, and `~/.local/share/hide` and `~/.local/state/hide` when they are left empty.
Once the helper runs from `~/.hide/host-helper`, a later step that fails (a hook entry or a link) shows on that machine's row while the old helper folder is kept as long as anything still names it, and the next connection finishes it; removing the old folders is retried on each connection and logged, not shown.
Every connection brings the kit up to this Hide's version, and a part you removed there stays removed until Reinstall on that machine's row.
A pane on that machine can then run `hide file open`, `hide diff open` or `hide browser open http://localhost:3000`, and the result opens in this Hide, with `localhost` meaning that machine; that shell's `PATH` has to include `~/.local/bin` for a bare `hide` to be found.
Its agent panes show labels, subagent counts and Workspace guidance as panes on this Mac do, with the labels made on this Mac from the device's conversations, which the helper reads and sends in memory only; Project Memory stays on this Mac and is not given to a device's sessions.
A machine allowed by an earlier version of Hide gets the whole kit on its next connection without asking again; a machine added without the helper installs nothing until you press Allow and install on its row.
Only machines on the platform this build carries get the kit; another platform shows why on its row and is only viewed and driven.
Removing a machine while it is connected takes Hide's hook entries, the `hide` link and the helper folder `~/.hide/host-helper` off it, and leaves `~/.hide/kit` and `~/.hide/agent-hooks`; removing it while it is not connected leaves them there, where they do no harm, and adding it again replaces them.
Each device row in Settings shows whether the remote session is connected and, when it is not, the reason in the words the connection failed with; `Test` runs the SSH, authentication, Herdr, protocol, PTY, SFTP, and Git stages one after another and lists the first one that needs attention on that host.

## Update

Quit hide, replace `/Applications/hide.app` on macOS or unpack the new Windows/Linux package into its permanent folder, and reopen it.
Opening the updated app replaces a `hided` still running from the previous build with one of its own build, and leaves the Herdr server, its panes and the agents running; a `hided` that will not stop makes the app show its start failure instead of attaching to the old one.
State under `~/.hide/state` and the desktop profile at `~/Library/Application Support/hide-desktop` both persist across an update.
Files the previous Swift app left under `~/Library/Application Support/hide/` are not read by the current app, apart from `state.json`'s shortcut bindings, which are imported once, and can be deleted by hand.
Each packaged launch replaces an outdated part of the kit on this machine, and each device connection does the same there, hook entries included; a part you removed stays removed.

## Uninstall

Quit hide and move `/Applications/hide.app` to the Trash.
This removes the application but keeps `~/.hide` and `~/Library/Application Support/hide-desktop`.
Delete both directories only when you deliberately want to reset hide's saved state.
Hide's hook entries stay in `~/.claude/settings.json` and `~/.codex/hooks.json` and do nothing once the app is gone; delete the entries whose command carries `hide-subagents@` to take them out.
`~/.local/bin/hide` stays as well until you remove it.
On Windows or Linux, delete the unpacked package folder after quitting and running its `resources/hide[.exe] stop`.
The hook entries and home state likewise remain; remove `~/.local/bin/hide` on Linux, or `%USERPROFILE%\.local\bin\hide.cmd` and the `.hide-kit` junction beside it on Windows.
Remove the junction itself, not the directory it leads to.

### Earlier builds and the transition

An older build reads only the old places, so opened after the move it starts empty.
The coordination transition has no rollback; the preserved old folders remain available for the operator to inspect or delete.

## Troubleshooting

### The window shows a connection failure instead of the app

The desktop host shows a status page while it is connecting, and a failure page with a `Retry` button when it cannot reach the daemon: `cli_missing` when no `hide` executable was found, `start_failed` when the CLI could not start it, and `no_response` when it started but never answered.
Read the host log at `<profile>/logs/desktop.log` (the profile is `~/Library/Application Support/hide-desktop`, or `HIDE_DESKTOP_USER_DATA_DIR`) for the detail behind whichever reason the page shows.

### No Herdr session

If the app connects but no workspace, terminal, or agent can start, no `herdr server` is running on the socket hided uses.
The packaged app starts one on launch, Retry, a second launch or a Dock click; the host log records `herdr.server_started`, or `herdr.status_failed` or `herdr.server_start_failed` with the reason it could not.
A development host, or a launch with `HERDR_BIN_PATH` set as an explicit override, starts none; run that Herdr once to start its server.
The daemon logs `herdr_bin.missing` when it cannot find a herdr binary at all: check `HERDR_BIN_PATH`, then PATH.
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
