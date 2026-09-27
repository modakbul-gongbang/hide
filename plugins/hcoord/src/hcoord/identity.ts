import fs from "node:fs";
import { spawnSync } from "node:child_process";
import { HcoordError } from "./model";

export interface MachineIdEnvironment {
  platform?: NodeJS.Platform;
  readFile?: (file: string) => string;
  run?: (file: string, args: string[]) => { status: number | null; stdout: string; stderr: string };
}

const normalize = (value: string): string => value.trim().toLowerCase();

/** The stable host identity hcoord writes into cross-machine lineage tokens. */
export function machineId(environment: MachineIdEnvironment = {}): string {
  const platform = environment.platform ?? process.platform;
  const readFile = environment.readFile ?? ((file: string) => fs.readFileSync(file, "utf8"));
  const run = environment.run ?? ((file: string, args: string[]) => {
    const result = spawnSync(file, args, { encoding: "utf8", shell: false, timeout: 5000, killSignal: "SIGKILL" });
    return { status: result.error ? null : result.status, stdout: result.stdout ?? "", stderr: result.stderr ?? String(result.error ?? "") };
  });
  if (platform === "darwin") {
    const result = run("/usr/sbin/ioreg", ["-rd1", "-c", "IOPlatformExpertDevice"]);
    const value = /"IOPlatformUUID"\s*=\s*"([^"]+)"/.exec(result.stdout)?.[1];
    if (result.status !== 0 || value === undefined || normalize(value) === "") {
      throw new HcoordError("machine_id_unavailable", `macOS did not report IOPlatformUUID: ${(result.stderr || result.stdout).trim().slice(0, 200) || "no diagnostic"}`);
    }
    return normalize(value);
  }
  if (platform === "linux") {
    let value: string;
    try { value = normalize(readFile("/etc/machine-id")); }
    catch (error) { throw new HcoordError("machine_id_unavailable", `Linux machine id could not be read: ${error instanceof Error ? error.message : "unknown error"}`); }
    if (!/^[0-9a-f]{32}$/.test(value)) throw new HcoordError("machine_id_unavailable", "/etc/machine-id is not a 32-character hexadecimal machine id");
    return value;
  }
  throw new HcoordError("unsupported_platform", `machine identity is not implemented for ${platform}; supported hosts are macOS and Linux`);
}
