# Desktop TypeScript working guide

Read the repository `AGENTS.md` and [documentation map](../docs/README.md) first.
The desktop host contract lives in [ARCHITECTURE.md](../docs/ARCHITECTURE.md#the-desktop-host); this file only points to its code conventions.

- Keep the host limited to the window, daemon discovery through the `hide` CLI, and native integration. The web shell and core own product behavior and state.
- Keep Electron OS work in `src/main/`. Expose only bounded operations through `src/preload/` and shared channel names in `src/channel.ts`; preserve the sandbox, context isolation, and daemon-origin checks.
- Route child processes through `src/main/spawn.ts` and environment keys through `src/main/env.ts`. Extend the existing host state machine in `src/main/host.ts` instead of creating a second discovery or lifecycle path.
- Put focused unit tests beside their owning module. Use `desktop/e2e/fixture.ts` for app flows so each test has private runtime state; follow [VERIFICATION.md](../docs/VERIFICATION.md) before native UI claims or tests that need focus.
- Run `bash scripts/verify-web.sh` from the repository root for web and desktop TypeScript checks. Read [CONTRIBUTING.md](../CONTRIBUTING.md) before delivery for the full gates.

`CLAUDE.md` beside this file is a symlink to this file so both supported runtimes load the same instructions.
