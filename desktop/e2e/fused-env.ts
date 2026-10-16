// The variable that carries the fused Electron's executable from the Playwright
// global setup (`fused-electron.ts`) to every worker, so a launch reads it and
// loads nothing of the fuse tooling.
export const FUSED_ELECTRON_ENV = "HIDE_E2E_FUSED_ELECTRON";
