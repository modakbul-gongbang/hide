# Web TypeScript working guide

Read the repository `AGENTS.md` and [documentation map](../docs/README.md) first.
The architecture, UI behavior, design workflow, and verification guides own their respective contracts; this file only points to web code conventions.

- Keep product state in the core and Herdr as described in [ARCHITECTURE.md](../docs/ARCHITECTURE.md). Use `src/ui.ts` for page-local navigation and transient presentation state; derive the rest from the snapshot in `src/store.ts` and `src/snapshot.ts`.
- Send one typed event for one user intent through `src/actions.ts`. Do not make a component's optimistic view a second authority for a core-owned decision.
- Keep wire shapes aligned with `contracts/hided-ws.schema.json`. Generate declared frame types with `pnpm --dir web gen:types`; extend the existing `src/snapshot.ts` boundary for schema gaps rather than defining competing shapes in components.
- Put reusable interaction rules in a focused `.ts` module beside the screen that uses them. Keep components responsible for rendering and local interaction; reuse `src/components/ui/` and the generated tokens for visual work.
- Add focused `.test.ts` or `.test.tsx` coverage for behavior with a meaningful observable result. Use `web/e2e/` and its private fixtures for browser flows; follow [VERIFICATION.md](../docs/VERIFICATION.md) for the applicable lane.
- Run `bash scripts/verify-web.sh` from the repository root for web and desktop TypeScript checks. Read [CONTRIBUTING.md](../CONTRIBUTING.md) before delivery for the full gates.

`CLAUDE.md` beside this file is a symlink to this file so both supported runtimes load the same instructions.
