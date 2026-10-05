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
  return path.join(outputDir, `${name}-${hash}${windows ? ".exe" : ""}`);
}

/** Compiles every program whose current source has no finished build, and removes older builds of it. */
export function buildFixtureShims(): void {
  fs.mkdirSync(outputDir, { recursive: true });
  const compiler = windows ? "clang.exe" : "cc";
  for (const name of BUILT) {
    const program = programOf(name);
    if (fs.existsSync(program)) continue;
    const partial = `${program}.${process.pid}.partial`;
    try {
      execFileSync(compiler, ["-O1", "-o", partial, sourceOf(name)], { encoding: "utf8", stdio: "pipe" });
    } catch (error) {
      const failed = error as { stdout?: string; stderr?: string };
      fs.rmSync(partial, { force: true });
      throw new Error(`fixture C compiler ${compiler} failed on ${sourceOf(name)}:\n${failed.stdout ?? ""}${failed.stderr ?? ""}`, { cause: error });
    }
    for (const old of fs.readdirSync(outputDir)) {
      if (old.startsWith(`${name}-`) && path.join(outputDir, old) !== partial) fs.rmSync(path.join(outputDir, old), { force: true });
    }
    fs.renameSync(partial, program);
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
