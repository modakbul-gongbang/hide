// How the macOS package is signed, chosen by HIDE_MACOS_SIGNING.
//
// Unset, it means `adhoc`: the bundle carries an ad-hoc signature, as every
// local and pull-request package does, and a downloaded copy opens only after
// the Gatekeeper step docs/INSTALL.md gives. `developer-id` is what
// release.yml asks for: every executable in the bundle (the Electron helpers,
// the bundled CLIs and Herdr) is signed with the Developer ID certificate
// under the hardened runtime, Apple notarizes the app and its ticket is
// stapled, so a downloaded copy opens like any other app. Herdr ships with
// only the linker's ad-hoc signature and notarization refuses an executable
// without a Developer ID one, so hide's signature replaces it; Herdr's code is
// not changed.
//
// The keys below are the whole contract, read here and nowhere else. Every
// missing or malformed one is reported at once, before the build starts.

import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

/** The keys `developer-id` needs, each with the test a usable value passes. */
const DEVELOPER_ID_KEYS = {
  // The certificate as `security find-identity -v -p codesigning` lists it:
  // its full name or its SHA-1. Its keychain must be on the search list.
  HIDE_SIGN_IDENTITY: (value) => /^Developer ID Application: .+ \([A-Z0-9]{10}\)$/.test(value) || /^[0-9A-F]{40}$/.test(value),
  // The App Store Connect API key notarytool submits with: the .p8 file,
  // its key ID and its issuer ID.
  HIDE_NOTARY_KEY_PATH: (value) => path.isAbsolute(value) && fs.statSync(value, { throwIfNoEntry: false })?.isFile() === true,
  HIDE_NOTARY_KEY_ID: (value) => /^[A-Z0-9]{10}$/.test(value),
  HIDE_NOTARY_ISSUER: (value) => /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(value),
};

/**
 * Reads the signing choice from `env`: `{ mode: "adhoc" }`, or the Developer
 * ID identity and notary credentials once the identity is found in a keychain
 * codesign searches and notarytool is installed. Anything else throws.
 */
export function macosSigning(env) {
  const mode = env.HIDE_MACOS_SIGNING ?? "adhoc";
  if (mode === "adhoc") return { mode };
  if (mode !== "developer-id") throw new Error(`HIDE_MACOS_SIGNING must be adhoc or developer-id (got '${mode}')`);
  const unusable = Object.entries(DEVELOPER_ID_KEYS)
    .filter(([key, usable]) => !env[key] || !usable(env[key]))
    .map(([key]) => key);
  if (unusable.length > 0) throw new Error(`HIDE_MACOS_SIGNING=developer-id needs these set and well formed: ${unusable.join(", ")}`);
  const identities = execFileSync("/usr/bin/security", ["find-identity", "-v", "-p", "codesigning"], { encoding: "utf8" });
  if (!identities.includes(env.HIDE_SIGN_IDENTITY)) throw new Error(`no keychain on the search list holds the signing identity '${env.HIDE_SIGN_IDENTITY}'`);
  execFileSync("/usr/bin/xcrun", ["--find", "notarytool"], { stdio: "ignore" });
  return {
    mode,
    identity: env.HIDE_SIGN_IDENTITY,
    notary: { appleApiKey: env.HIDE_NOTARY_KEY_PATH, appleApiKeyId: env.HIDE_NOTARY_KEY_ID, appleApiIssuer: env.HIDE_NOTARY_ISSUER },
  };
}

/**
 * The packager's `osxSign` and `osxNotarize` options for a Developer ID
 * package. `executables` names the programs shipped flat in
 * Contents/Resources, and `entitlements` is the folder holding the two
 * entitlement files.
 */
export function developerIdOptions(signing, executables, entitlements) {
  return {
    osxSign: {
      identity: signing.identity,
      // The packager only warns about a failed signature unless told
      // otherwise, and an unsigned app must not reach an archive.
      continueOnError: false,
      optionsForFile: (file) => {
        if (path.basename(path.dirname(file)) === "Resources" && executables.includes(path.basename(file))) {
          return { entitlements: path.join(entitlements, "executable.plist") };
        }
        // Electron's helpers keep the Chromium entitlements @electron/osx-sign gives each.
        if (file.includes(".app/Contents/Frameworks/")) return null;
        return { entitlements: path.join(entitlements, "app.plist") };
      },
    },
    // Submits the signed app with notarytool, waits for Apple's answer and
    // staples the ticket; a rejection throws with Apple's log.
    osxNotarize: signing.notary,
  };
}

/**
 * Throws unless Gatekeeper accepts the app inside `archive` as notarized
 * Developer ID software, the way it judges a downloaded copy. Expands the
 * archive into a new folder under `scratch` and removes it on every path.
 */
export function assertNotarized(archive, scratch) {
  const folder = fs.mkdtempSync(path.join(scratch, "notarized-"));
  try {
    execFileSync("/usr/bin/ditto", ["-x", "-k", archive, folder]);
    const app = path.join(folder, "hide.app");
    execFileSync("/usr/bin/xcrun", ["stapler", "validate", app], { stdio: "inherit" });
    // spctl answers on stderr: "accepted" or "rejected", and the source it trusted.
    const assessment = spawnSync("/usr/sbin/spctl", ["--assess", "--type", "execute", "--verbose=4", app], { encoding: "utf8" });
    const answer = `${assessment.stdout}${assessment.stderr}`.trim();
    if (assessment.status !== 0 || !answer.includes("source=Notarized Developer ID")) {
      throw new Error(`Gatekeeper does not accept the app in ${archive} as notarized: ${answer || `spctl exited ${assessment.status}`}`);
    }
  } finally {
    fs.rmSync(folder, { recursive: true, force: true });
  }
}
