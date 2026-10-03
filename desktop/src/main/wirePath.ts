// How a path is spelled between this computer and the page, the desktop
// host's side of `hide_platform::path` (docs/ARCHITECTURE.md, The platform
// layer). The page joins and compares absolute paths with `/`, as hided and
// the core send them, so every path the host answers leaves in that wire
// spelling and every path the page names is turned back into this system's
// own before anything is asked of the filesystem; this file is the one place
// either direction happens.
//
// The wire spelling of an absolute path keeps its root as the owning system
// writes it and changes only its separators: `/repo/src` on macOS and Linux,
// exactly the native text, and `C:/repo/src` or `//server/share/src` on
// Windows, with the `\\?\` prefix gone where the short spelling names the
// same file and the drive letter upper case, so one folder has one spelling.
// The rules below are the Rust ones letter for letter; a difference between
// the two is a bug in one of them.

import path from "node:path";

/** Why a path has no spelling in the asked direction; the names follow Rust's `PathError`. */
export type WireRefusal = "not_absolute" | "not_utf8" | "unrepresentable";

export class WirePathError extends Error {
  constructor(readonly reason: WireRefusal) {
    super(`the path has no wire spelling: ${reason}`);
  }
}

/** One system's spelling rules: what is absolute there, and the two conversions. */
export type Spelling = {
  /** Whether a native path names one file wherever the process stands: a rooted path without a drive (`\foo`) or a drive without a root (`C:foo`) does not on Windows. */
  isAbsolute(native: string): boolean;
  /** The wire spelling of an absolute native path; throws `WirePathError`. */
  toWire(native: string): string;
  /** The native path an absolute wire spelling names; throws `WirePathError`. */
  fromWire(wire: string): string;
};

/** A lone UTF-16 surrogate, which a Windows name may hold and UTF-8 cannot carry. */
const LONE_SURROGATE = /\p{Cs}/u;

const posix: Spelling = {
  isAbsolute: (native) => native.startsWith("/"),
  toWire(native) {
    if (!native.startsWith("/")) throw new WirePathError("not_absolute");
    if (LONE_SURROGATE.test(native)) throw new WirePathError("not_utf8");
    return native;
  },
  fromWire(wire) {
    if (wire.includes("\0")) throw new WirePathError("unrepresentable");
    if (!wire.startsWith("/")) throw new WirePathError("not_absolute");
    return wire;
  },
};

/** What follows `X:` in `X:/...`, or null when the spelling is not a drive path. */
function driveRest(wire: string): string | null {
  return /^[A-Za-z]:\//u.test(wire) ? wire.slice(2) : null;
}

/** What follows `//server/share` in `//server/share/...`, or null when the spelling is not a share path. */
function shareRest(wire: string): string | null {
  if (!wire.startsWith("//")) return null;
  const [server, share, ...rest] = wire.slice(2).split("/");
  if (!server || share === undefined || share === "" || server === "?" || server === ".") return null;
  return rest.join("/");
}

/** Whether Win32 reads the name as a device (`CON`, `NUL`, `COM1`, ...), with or without an extension. */
function reservedDevice(name: string): boolean {
  const stem = (name.split(".")[0] ?? "").trimEnd().toUpperCase();
  if (["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].includes(stem)) return true;
  return /^(?:COM|LPT)[1-9¹²³]$/u.test(stem);
}

/** Whether Win32 would read a name as something other than itself outside a `\\?\` path. */
function win32Rewrites(name: string): boolean {
  return name.endsWith(" ") || name.endsWith(".") || name.includes("/") || reservedDevice(name);
}

/** The short spelling of a `\\?\` path when it names the same file, or null. */
function shortVerbatim(native: string): string | null {
  let stripped: string;
  let body: string;
  let unc = false;
  if (native.startsWith("\\\\?\\UNC\\")) {
    stripped = native.slice(8);
    body = stripped;
    unc = true;
  } else {
    if (!native.startsWith("\\\\?\\")) return null;
    stripped = native.slice(4);
    if (!/^[A-Za-z]:\\/u.test(stripped)) return null;
    body = stripped.slice(3);
  }
  if (body !== "" && body.split("\\").some((name) => name === "" || name === "." || name === ".." || win32Rewrites(name))) return null;
  return unc ? `\\\\${stripped}` : stripped;
}

const win32: Spelling = {
  isAbsolute: (native) => /^[A-Za-z]:[\\/]/u.test(native) || /^[\\/]{2}[^\\/]+[\\/]+[^\\/]/u.test(native),
  toWire(native) {
    if (!win32.isAbsolute(native)) throw new WirePathError("not_absolute");
    if (LONE_SURROGATE.test(native)) throw new WirePathError("not_utf8");
    let short = native;
    if (native.startsWith("\\\\?\\")) {
      const unprefixed = shortVerbatim(native);
      if (unprefixed === null) throw new WirePathError("unrepresentable");
      short = unprefixed;
    }
    if (short.startsWith("\\\\.\\") || short.startsWith("\\??\\")) throw new WirePathError("unrepresentable");
    const wire = short.replaceAll("\\", "/");
    const rest = driveRest(wire);
    if (rest !== null) return `${wire[0]!.toUpperCase()}:${rest}`;
    if (shareRest(wire) !== null) return wire;
    throw new WirePathError("not_absolute");
  },
  fromWire(wire) {
    if (wire.includes("\0")) throw new WirePathError("unrepresentable");
    if (driveRest(wire) === null && shareRest(wire) === null) throw new WirePathError("not_absolute");
    return wire.replaceAll("/", "\\");
  },
};

/** The spelling rules of `system` as Node names it; any system's rules can be read on any system. */
export function spelling(system: NodeJS.Platform): Spelling {
  return system === "win32" ? win32 : posix;
}

const local = spelling(process.platform);

/** Whether a native path on this computer is absolute in the sense above. */
export const isAbsolute = (native: string): boolean => local.isAbsolute(native);

/** Longer than any macOS or Linux path (`PATH_MAX` is 1024 and 4096) and any Windows path without the long-path opt-in; a longer value is not a path the page sent. */
export const MAX_PATH_LENGTH = 4096;

/**
 * The native path an absolute path the page names is on this computer, with
 * `.` and `..` resolved, or null when the value is not an absolute wire
 * spelling: a relative one would resolve against the host's own working
 * directory, which the page knows nothing of, and on Windows a native
 * `C:\x`, a rooted `\x` or a drive-relative `C:x` is not the wire spelling.
 */
export function fromPage(value: unknown): string | null {
  if (typeof value !== "string" || value.length === 0 || value.length > MAX_PATH_LENGTH) return null;
  try {
    return path.normalize(local.fromWire(value));
  } catch (error) {
    if (error instanceof WirePathError) return null;
    throw error;
  }
}

/** The wire spelling of an absolute path on this computer, for an answer to the page; throws `WirePathError`. */
export function toPage(native: string): string {
  return local.toWire(native);
}
