import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { afterAll, describe, expect, it } from "vitest";
import { fromPage, spelling, toPage, WirePathError, type WireRefusal } from "./wirePath";

// The expected spellings are `hide_platform::path`'s (its unit tests and
// `hide-platform/tests/it/path.rs`); the desktop host has to answer the page
// exactly as hided does. The Windows rules are text, so every system checks
// them, as the Rust crate does.

const windows = spelling("win32");
const unix = spelling("darwin");

function refusal(convert: () => string): WireRefusal | string {
  try {
    return convert();
  } catch (error) {
    if (error instanceof WirePathError) return error.reason;
    throw error;
  }
}

describe("the Windows spelling", () => {
  it("writes a path with slashes, its drive upper case and no long-path prefix where the short one names the same file", () => {
    expect(windows.toWire("C:\\repo\\src\\a.rs")).toBe("C:/repo/src/a.rs");
    expect(windows.toWire("c:\\repo")).toBe("C:/repo");
    expect(windows.toWire("C:\\")).toBe("C:/");
    expect(windows.toWire("\\\\?\\C:\\repo\\a")).toBe("C:/repo/a");
    expect(windows.toWire("\\\\server\\share\\repo")).toBe("//server/share/repo");
    expect(windows.toWire("\\\\?\\UNC\\server\\share\\repo")).toBe("//server/share/repo");
    // A device name is matched with ASCII case folding and Unicode white space trimmed, so these are names, not devices.
    expect(windows.toWire("\\\\?\\C:\\a\\con\u0131n$")).toBe("C:/a/con\u0131n$");
    expect(windows.toWire("\\\\?\\C:\\a\\lpt1\uFEFF")).toBe("C:/a/lpt1\uFEFF");
    expect(windows.toWire(`\\\\?\\C:\\${"a".repeat(300)}`)).toBe(`C:/${"a".repeat(300)}`);
  });

  it("refuses a path with no short spelling, and one that is not absolute", () => {
    for (const native of ["\\\\?\\C:\\repo\\name.", "\\\\?\\C:\\repo\\NUL", "\\\\?\\C:\\repo\\nul\u0085", "\\\\?\\Volume{1234}\\a", "\\\\.\\pipe\\x"]) {
      expect(refusal(() => windows.toWire(native)), native).toBe("unrepresentable");
    }
    for (const native of ["repo\\a", "C:repo", "\\repo", "/repo"]) {
      expect(refusal(() => windows.toWire(native)), native).toBe("not_absolute");
    }
    expect(refusal(() => windows.toWire("C:\\a\uD800"))).toBe("not_utf8");
  });

  it("reads a wire spelling back as the native path, and nothing else", () => {
    expect(windows.fromWire("C:/repo/src/a.rs")).toBe("C:\\repo\\src\\a.rs");
    expect(windows.fromWire("//server/share/a")).toBe("\\\\server\\share\\a");
    for (const wire of ["/repo", "repo/a", "C:repo", "C:\\repo", "\\repo", "//?/C:/repo", "//server"]) {
      expect(refusal(() => windows.fromWire(wire)), wire).toBe("not_absolute");
    }
    expect(refusal(() => windows.fromWire("C:/a\0b"))).toBe("unrepresentable");
  });

  it("counts a path as absolute only with both a drive or share and a root", () => {
    for (const native of ["C:\\repo", "C:/repo", "\\\\server\\share\\a", "\\\\?\\C:\\repo"]) expect(windows.isAbsolute(native), native).toBe(true);
    for (const native of ["\\repo", "/repo", "C:repo", "repo"]) expect(windows.isAbsolute(native), native).toBe(false);
  });
});

describe("the macOS and Linux spelling", () => {
  it("is the native text both ways, for an absolute path only", () => {
    expect(unix.toWire("/repo/src/a.rs")).toBe("/repo/src/a.rs");
    expect(unix.fromWire("/repo/src/a.rs")).toBe("/repo/src/a.rs");
    expect(refusal(() => unix.toWire("repo/a"))).toBe("not_absolute");
    expect(refusal(() => unix.fromWire("repo/a"))).toBe("not_absolute");
    expect(refusal(() => unix.fromWire("/a\0b"))).toBe("unrepresentable");
    expect(unix.isAbsolute("\\repo")).toBe(false);
  });
});

describe("paths between this computer and the page", () => {
  const root = fs.realpathSync.native(fs.mkdtempSync(path.join(os.tmpdir(), "hide-wire-path-")));
  afterAll(() => fs.rmSync(root, { recursive: true, force: true }));
  /** The page's spelling of a native path on this system: `/` between names. */
  const page = (native: string): string => native.split(path.sep).join("/");

  it("answers a real folder in the spelling the page joins with `/`, and reads it back", () => {
    const folder = path.join(root, "Checkout", "src");
    fs.mkdirSync(folder, { recursive: true });
    const wire = toPage(folder);
    expect(wire).toBe(page(folder));
    expect(fromPage(wire)).toBe(folder);
    expect(fromPage(`${page(root)}/Checkout/../Checkout/src`)).toBe(folder);
  });

  it("takes only an absolute wire spelling from the page", () => {
    for (const value of ["projects/hide", "", 42, { path: "/tmp" }, `${page(root)}/a\0b`, `${page(root)}/${"a".repeat(5000)}`, "\\tmp", "~/a"]) {
      expect(fromPage(value), JSON.stringify(value)).toBeNull();
    }
    // The native spelling is not the wire one on Windows.
    if (process.platform === "win32") expect(fromPage(root)).toBeNull();
  });
});
