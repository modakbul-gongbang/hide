// Packages dist/ as an unsigned local hide.app for this Mac (desktop PRD
// D-05): no signing, notarization or installer. The app finds the `hide`
// CLI through HIDE_CLI_PATH or the login shell's PATH, never inside itself.

import path from "node:path";
import { fileURLToPath } from "node:url";
import { packager } from "@electron/packager";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const [appPath] = await packager({
  dir: root,
  out: path.join(root, "out"),
  name: "hide",
  appBundleId: "me.grab.hide.desktop",
  platform: "darwin",
  arch: process.arch,
  overwrite: true,
  asar: true,
  prune: true,
  // Only the bundles and the status page ship; sources, tests and tooling do not.
  ignore: (file) => file !== "" && file !== "/package.json" && !file.startsWith("/dist"),
});
console.log(appPath);
