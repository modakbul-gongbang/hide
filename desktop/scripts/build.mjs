// Bundles the main process and the preload into dist/, and copies the status
// page with the generated design tokens it is styled from. The tokens file is
// written for Tailwind: its `@theme` blocks, which a plain page drops, become
// `:root` blocks here, so the status page reads the same names the web shell
// does. The window's background is the dark theme's `--background` (the web
// shell's default theme), read here so no color is written into the host.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "esbuild";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const dist = path.join(root, "dist");
const tokens = path.resolve(root, "..", "web", "src", "tokens.css");

const tokenCss = fs.readFileSync(tokens, "utf8");
const background = /\.dark\s*\{[^}]*?--background:\s*(#[0-9a-fA-F]{3,8})\s*;/.exec(tokenCss)?.[1];
if (!background) throw new Error(`${tokens} has no dark --background`);
const themeBlocks = /@theme(?:\s+(?:static|inline))?\s*\{/g;
if (!themeBlocks.test(tokenCss)) throw new Error(`${tokens} has no @theme block; the status page expects Tailwind v4 tokens`);

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
fs.writeFileSync(path.join(dist, "static", "tokens.css"), tokenCss.replace(themeBlocks, ":root {"));
