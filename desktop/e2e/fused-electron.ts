// A copy of the Electron the suite would run, with the fuses the shipped app is
// built with (`scripts/fuses.mjs`), so an end-to-end test proves what ships: a
// stock Electron grants `file:` pages privileges the app turns off. Made once,
// before any test, inside this worktree's ignored `dist/`, and made again when
// Electron's version or the fuse list changes.
import fs from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { applyFuses, assertFuses, SHIPPED_FUSES } from "../scripts/fuses.mjs";
import { FUSED_ELECTRON_ENV } from "./fused-env";

const requireFrom = createRequire(__filename);
const DESKTOP_DIR = path.resolve(__dirname, "..");
const FUSED = path.join(DESKTOP_DIR, "dist", "electron-fused");
const STAMP = path.join(FUSED, ".fused");

/** The stock Electron's executable and the folder it ships in. */
const stock = () => {
  const executable = requireFrom("electron") as string;
  const folder = path.join(path.dirname(requireFrom.resolve("electron/package.json")), "dist");
  return { executable, folder, version: (requireFrom("electron/package.json") as { version: string }).version };
};

/** The executable of the fused Electron copy; it exists once `makeFusedElectron` has run. */
export function fusedElectron(): string {
  const { executable, folder } = stock();
  return path.join(FUSED, path.relative(folder, executable));
}

/** Builds the copy unless one of this Electron version and fuse list is already there, and names it for the workers. */
export async function makeFusedElectron(): Promise<void> {
  process.env[FUSED_ELECTRON_ENV] = fusedElectron();
  const { folder, version } = stock();
  const stamp = JSON.stringify({ version, fuses: SHIPPED_FUSES });
  if (fs.existsSync(STAMP) && fs.readFileSync(STAMP, "utf8") === stamp && fs.existsSync(fusedElectron())) return;
  fs.rmSync(FUSED, { recursive: true, force: true });
  fs.mkdirSync(path.dirname(FUSED), { recursive: true });
  fs.cpSync(folder, FUSED, { recursive: true, verbatimSymlinks: true });
  const target = process.platform === "darwin" ? path.join(FUSED, "Electron.app") : fusedElectron();
  await applyFuses(target, { adHocSign: process.platform === "darwin" });
  await assertFuses(target);
  fs.writeFileSync(STAMP, stamp);
}
