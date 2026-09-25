// Bundles the main process and the preload into dist/, and copies the status
// page with the generated design tokens it is styled from. The window's
// background is the tokens' `--color-background`, read here so no color is
// written into the host's source.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const dist = path.join(root, "dist");
const tokens = path.resolve(root, "..", "web", "src", "tokens.css");

const background = /--color-background:\s*(#[0-9a-fA-F]{3,8})\s*;/.exec(fs.readFileSync(tokens, "utf8"))?.[1];
if (!background) throw new Error(`${tokens} has no --color-background`);

fs.rmSync(dist, { recursive: true, force: true });
const common = {
  bundle: true,
  platform: "node",
  format: "cjs",
  target: "node22",
  external: ["electron"],
  sourcemap: "linked",
  logLevel: "warning",
};
await build({ ...common, entryPoints: [path.join(root, "src/main/index.ts")], outfile: path.join(dist, "main.js"), define: { __HIDE_BACKGROUND__: JSON.stringify(background) } });
await build({ ...common, entryPoints: [path.join(root, "src/preload/index.ts")], outfile: path.join(dist, "preload.js") });

fs.cpSync(path.join(root, "static"), path.join(dist, "static"), { recursive: true });
fs.copyFileSync(tokens, path.join(dist, "static", "tokens.css"));
