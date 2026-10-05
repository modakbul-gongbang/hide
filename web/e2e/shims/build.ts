// The C programs the e2e fixtures run in place of a real provider, opener and
// process lister. They are compiled once, by the e2e entry point's
// `globalSetup`, into `web/.e2e-shims` (ignored by git), each named with a
// hash of its source: a test only copies a finished program, so no test
// holds a compiler and a stale program cannot be mistaken for the current one.
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const windows = process.platform === "win32";
const ext = windows ? ".exe" : "";

/** A program the fixtures copy, by the name of its source in `web/e2e/shims`. */
export type FixtureShim = "claude-shim" | "spawn-provider" | "noop" | "launcher" | "hide-children";

/** The programs this system builds: the launcher and process lister only exist for Windows. */
const BUILT: FixtureShim[] = windows ? ["claude-shim", "spawn-provider", "noop", "launcher", "hide-children"] : ["claude-shim", "spawn-provider", "noop"];

// Both e2e packages run with their own folder as the working directory, and
// `web` and `desktop` are siblings.
const sourceDir = path.resolve("..", "web", "e2e", "shims");
const outputDir = path.resolve("..", "web", ".e2e-shims");

const sourceOf = (name: FixtureShim): string => path.join(sourceDir, `${name}.c`);

function programOf(name: FixtureShim): string {
  const hash = createHash("sha256").update(fs.readFileSync(sourceOf(name))).digest("hex").slice(0, 12);
  return path.join(outputDir, `${name}-${hash}${ext}`);
}

/** Compiles every program whose current source has no finished build, and removes older builds of it. */
export function buildFixtureShims(): void {
  fs.mkdirSync(outputDir, { recursive: true });
  const compiler = windows ? "clang.exe" : "cc";
  for (const name of BUILT) {
    const program = programOf(name);
    if (fs.existsSync(program)) continue;
    const partial = `${program.slice(0, program.length - ext.length)}.${process.pid}.partial${ext}`;
    try {
      execFileSync(compiler, ["-O1", "-o", partial, sourceOf(name)], { encoding: "utf8", stdio: "pipe" });
    } catch (error) {
      const failed = error as { stdout?: string; stderr?: string };
      fs.rmSync(partial, { force: true });
      // A compiler that cannot start has no output; its error is the reason.
      const output = `${failed.stdout ?? ""}${failed.stderr ?? ""}` || String(error);
      throw new Error(`fixture C compiler ${compiler} failed on ${sourceOf(name)}:\n${output}`, { cause: error });
    }
    // Another build of the same source may have finished first; keep its program.
    if (fs.existsSync(program)) {
      fs.rmSync(partial, { force: true });
      continue;
    }
    fs.renameSync(partial, program);
    // Older finished builds of this program; another process's partial file is not one.
    const finished = new RegExp(`^${name}-[0-9a-f]{12}${windows ? "\\.exe" : ""}$`);
    for (const old of fs.readdirSync(outputDir)) {
      if (finished.test(old) && path.join(outputDir, old) !== program) fs.rmSync(path.join(outputDir, old), { force: true });
    }
  }
}

/** Copies the finished `name` to `executable`; a program that was not built is a failure that says how to build it. */
export function copyFixtureShim(name: FixtureShim, executable: string): void {
  const program = programOf(name);
  if (!fs.existsSync(program)) {
    throw new Error(`fixture program ${name} is not built (${program}); Playwright's globalSetup builds it, so run the e2e through \`playwright test\` (scripts/verify-web.sh web e2e)`);
  }
  fs.copyFileSync(program, executable);
}
