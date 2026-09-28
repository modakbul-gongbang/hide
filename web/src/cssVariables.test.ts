import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";

// A class such as `size-(--size-icon-xs)` naming a variable nothing defines is
// invalid at computed time, so the icon falls back to lucide's 24px and nothing
// fails. Every variable the shell's source reads has to be declared in its CSS
// or set inline by the source itself; Radix sets its own at runtime.
function sourceFiles(dir: string): string[] {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return sourceFiles(full);
    return /\.(tsx?|css)$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name) ? [full] : [];
  });
}

describe("CSS variables", () => {
  it("reads only variables the stylesheets declare or the source sets inline", () => {
    const files = sourceFiles(__dirname).map((file) => ({ file, text: fs.readFileSync(file, "utf8") }));
    const declared = new Set<string>();
    for (const { file, text } of files) {
      const pattern = file.endsWith(".css") ? /(--[a-z0-9-]+)\s*:/g : /["'](--[a-z0-9-]+)["']\s*:/g;
      for (const [, name] of text.matchAll(pattern)) if (name) declared.add(name);
    }
    const unknown = new Set<string>();
    for (const { file, text } of files) {
      if (file.endsWith(".css")) continue;
      for (const [, name] of text.matchAll(/(?:\(|var\()(--[a-z0-9-]+)\)/g)) {
        if (name && !name.startsWith("--radix-") && !declared.has(name)) unknown.add(`${path.relative(__dirname, file)}: ${name}`);
      }
    }
    expect([...unknown].sort()).toEqual([]);
  });
});
