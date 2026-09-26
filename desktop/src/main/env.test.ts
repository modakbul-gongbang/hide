import fs from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { ENV_REGISTRY, loadEnv } from "./env";

const SRC = path.resolve(__dirname, "..");

function sources(dir: string): string[] {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return sources(full);
    return entry.name.endsWith(".ts") && !entry.name.endsWith(".test.ts") ? [full] : [];
  });
}

describe("the environment registry", () => {
  it("is the only reader of process.env, and every key it reads is registered", () => {
    const readers = sources(SRC).filter((file) => fs.readFileSync(file, "utf8").includes("process.env"));
    expect(readers.map((file) => path.relative(SRC, file))).toEqual(["main/index.ts"]);
    const envSource = fs.readFileSync(path.join(SRC, "main/env.ts"), "utf8");
    const read = [...envSource.matchAll(/(?:absolute\(|source\.)"?([A-Z_]+)"?/g)].map((match) => match[1]);
    const registered = ENV_REGISTRY.map((entry) => entry.key);
    for (const key of read) expect(registered, key).toContain(key);
  });

  it("names every bad key at once and never echoes a value", () => {
    expect(() => loadEnv({ HIDE_CLI_PATH: "relative/hide", HIDE_DESKTOP_USER_DATA_DIR: "also-relative" })).toThrow(
      "desktop environment is not usable: HIDE_CLI_PATH: not an absolute path; HIDE_DESKTOP_USER_DATA_DIR: not an absolute path",
    );
    expect(loadEnv({ PATH: "/bin", HOME: "/h" })).toEqual({ cliPath: null, userDataDir: null, shell: "/bin/zsh", home: "/h", path: "/bin" });
  });
});
