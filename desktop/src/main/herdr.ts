// Which Herdr binary a daemon started from this host attaches pane terminals
// with, decided once from the host's environment.
//
// A packaged app ships its pinned Herdr in `Contents/Resources/herdr` and
// hands it to every `hide` child as HERDR_BIN_PATH. An inherited
// HERDR_BIN_PATH is honored only as an explicit override: Herdr exports its
// own server's path into every pane together with HERDR_PANE_ID, and that
// path dies when the bundle that started the server is replaced, so a
// packaged app opened from a pane would hand `hide connect` a value it
// refuses on every attempt (issue 214). A value that arrives with
// HERDR_PANE_ID is therefore Herdr's, not the operator's, and the bundled
// binary replaces it. An unpackaged host has no bundled binary and adds
// nothing.

import path from "node:path";

export type HerdrChoice = {
  /** The HERDR_BIN_PATH every `hide` child gets, or null to leave the inherited environment as it is. */
  path: string | null;
  source: "bundled" | "inherited";
  /** The pane-exported value the bundled binary replaced, kept for the log. */
  replacedPaneValue: string | null;
};

export function chooseHerdr(input: { bundledDir: string | null; herdrBinPath: string | null; herdrPaneId: string | null }): HerdrChoice {
  const { bundledDir, herdrBinPath, herdrPaneId } = input;
  if (bundledDir === null) return { path: null, source: "inherited", replacedPaneValue: null };
  const bundled = path.join(bundledDir, "herdr");
  if (herdrBinPath === null) return { path: bundled, source: "bundled", replacedPaneValue: null };
  if (herdrPaneId !== null) return { path: bundled, source: "bundled", replacedPaneValue: herdrBinPath };
  return { path: null, source: "inherited", replacedPaneValue: null };
}
