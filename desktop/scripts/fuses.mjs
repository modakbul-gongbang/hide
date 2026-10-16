// The Electron fuses the shipped app is built with, one list for the packager
// and for the end-to-end suite, which runs a copy of Electron flipped the same
// way so that what it proves is what ships (docs/BROWSER_DISPLAYS.md, The file
// boundary).
//
// GrantFileProtocolExtraPrivileges is off: with it on, Electron lets every
// `file:` page read other files and treats all of them as one origin, which a
// browser does not. Off, a local HTML page is an opaque origin that reads only
// what a browser lets a local page read. Hide itself needs none of it: the
// shell is served by hided over loopback, the preload and the main process load
// from the asar, and the status page loads its own script and style, which
// loading allows. Every other fuse stays at Electron's default; each is its own
// decision.

import { FuseV1Options, FuseVersion, flipFuses, getCurrentFuseWire } from "@electron/fuses";

export const SHIPPED_FUSES = {
  version: FuseVersion.V1,
  [FuseV1Options.GrantFileProtocolExtraPrivileges]: false,
};

/**
 * Flips the shipped fuses in the Electron at `electron`: the executable, or on
 * macOS the `.app`. A flipped macOS binary has lost its signature, so an
 * ad-hoc one is put back; a Developer ID build is signed after this, by the
 * packager.
 */
export async function applyFuses(electron, { adHocSign = false } = {}) {
  await flipFuses(electron, { ...SHIPPED_FUSES, resetAdHocDarwinSignature: adHocSign });
}

/** Throws unless the Electron at `electron` has the shipped fuses, read back from the file itself. */
export async function assertFuses(electron) {
  const wire = await getCurrentFuseWire(electron);
  // 48 is a disabled fuse's byte ('0'), 49 an enabled one's.
  const state = wire[FuseV1Options.GrantFileProtocolExtraPrivileges];
  if (state !== 48) throw new Error(`${electron} still grants file: pages extra privileges (fuse state ${state})`);
}
