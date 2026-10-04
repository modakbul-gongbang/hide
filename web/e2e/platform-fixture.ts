// Native spellings for the isolated e2e homes and tools. The home/state
// contract is hide-platform::host; Herdr accepts a filesystem socket path
// on every OS and maps that path to a named pipe on Windows.
import fs from "node:fs";
import path from "node:path";
import { execFileSync, type ChildProcess } from "node:child_process";

export const fixtureExecutable = (name: string): string => `${name}${process.platform === "win32" ? ".exe" : ""}`;

/** Kill and confirm only the owned tree before deleting its executable/home.
 * Unix fixtures start a private process group. Windows captures descendants
 * and creation times before shutdown, because stopping the parent can orphan
 * a locked provider executable. The two-second cleanup bound is unchanged. */
export function stopFixtureProcess(child: ChildProcess, graceful?: () => void): void {
  if (!child.pid) {
    if (child.exitCode === null && child.signalCode === null) throw new Error("fixture child has no PID or confirmed exit");
    return;
  }
  const pid = child.pid;
  if (process.platform === "win32") {
    // One bounded observer owns process enumeration, tree termination and
    // identity-safe exit confirmation, even during synchronous worker exit.
    execFileSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", `
      $ErrorActionPreference = 'Stop'
      $all = @(Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId,CreationDate)
      $ids = @(${pid})
      $owned = @()
      for ($depth=0; $depth -lt 64; $depth++) {
        $next = @($all | Where-Object { $ids -contains $_.ProcessId -and $owned.ProcessId -notcontains $_.ProcessId })
        if ($next.Count -eq 0) { break }
        $owned += $next
        if ($owned.Count -gt 256) { throw 'fixture tree exceeded 256 processes; preserve home' }
        $ids = @($all | Where-Object { $next.ProcessId -contains $_.ParentProcessId } | ForEach-Object { $_.ProcessId })
      }
      foreach ($record in $owned) {
        $live = Get-CimInstance Win32_Process -Filter "ProcessId = $($record.ProcessId)" -Property ProcessId,CreationDate
        if ($live -and $live.CreationDate -eq $record.CreationDate) { Stop-Process -Id $record.ProcessId -Force -ErrorAction SilentlyContinue }
      }
      $until = [DateTime]::UtcNow.AddSeconds(2)
      do {
        $remaining = @($owned | Where-Object {
          $live = Get-CimInstance Win32_Process -Filter "ProcessId = $($_.ProcessId)" -Property ProcessId,CreationDate
          $live -and $live.CreationDate -eq $_.CreationDate
        })
        if ($remaining.Count -eq 0) { exit 0 }
        Start-Sleep -Milliseconds 50
      } while ([DateTime]::UtcNow -lt $until)
      throw "fixture exit unconfirmed for owned PIDs $($remaining.ProcessId -join ','); preserve home"
    `], { timeout: 10_000, maxBuffer: 64 * 1024, windowsHide: true });
    // The endpoint stop may still be needed after its process has exited.
    graceful?.();
    return;
  }
  graceful?.();
  const liveMembers = () => {
    const rows = execFileSync("ps", ["-axo", "pid=,pgid=,stat="], { encoding: "utf8", timeout: 1000, maxBuffer: 1024 * 1024 });
    return rows.split("\n").some((row) => { const [, group, state] = row.trim().split(/\s+/); return Number(group) === pid && !state?.startsWith("Z"); });
  };
  if (!liveMembers()) return;
  const killGroup = (signal: NodeJS.Signals) => {
    try { process.kill(-pid, signal); } catch (error) { if (liveMembers()) throw error; }
  };
  killGroup("SIGTERM");
  const until = Date.now() + 2000;
  let forced = false;
  while (Date.now() < until) {
    if (!liveMembers()) return;
    if (!forced && Date.now() >= until - 1000) { killGroup("SIGKILL"); forced = true; }
    Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 50);
  }
  throw new Error(`fixture exit unconfirmed for owned process group ${pid}; preserve home`);
}

/** A fixture compiler is an owned, bounded child, separate from test deadlines. */
export function compileFixtureC(source: string, executable: string): void {
  const compiler = process.platform === "win32" ? "clang.exe" : "cc";
  try {
    execFileSync(compiler, ["-O1", "-o", executable, source], { timeout: 20_000 });
  } catch (error) {
    throw new Error(`fixture C compiler ${compiler} failed (20 second process bound); the runner needs its native C toolchain: ${String(error)}`, { cause: error });
  }
}

/** Windows environment keys are case-insensitive, including Path/PATH. */
export function inheritedFixtureEnv(): Record<string, string> {
  return Object.fromEntries(Object.entries(process.env).flatMap(([key, value]) => {
    const canonical = process.platform === "win32" ? key.toUpperCase() : key;
    if (value === undefined || ["HERDR_", "HIDE_", "ELECTRON_", "HCOORD_", "SASU_"].some((prefix) => key.toUpperCase().startsWith(prefix))) return [];
    return [[canonical, value]];
  }));
}

/** Move native home lookups and CLI credential discovery together. */
export function fixtureHomeEnv(home: string): Record<string, string> {
  const windows: Record<string, string> = process.platform === "win32" ? {
    USERPROFILE: home,
    APPDATA: path.join(home, "AppData", "Roaming"),
    LOCALAPPDATA: path.join(home, "AppData", "Local"),
  } : {};
  for (const dir of Object.values(windows)) fs.mkdirSync(dir, { recursive: true });
  return {
    HOME: home,
    ...windows,
    HCOORD_HOME: path.join(home, ".hcoord"),
    XDG_CONFIG_HOME: path.join(home, ".config"),
    XDG_STATE_HOME: path.join(home, ".local", "state"),
    CLAUDE_CONFIG_DIR: path.join(home, ".claude"),
    CODEX_HOME: path.join(home, ".codex"),
  };
}

/** System tools only: do not append the account's agent-provider PATH. */
export function fixtureToolPath(bin: string): string {
  if (process.platform !== "win32") return [bin, "/usr/bin", "/bin", "/usr/sbin", "/sbin"].join(path.delimiter);
  const env = inheritedFixtureEnv();
  const system = env.SYSTEMROOT;
  const programs = env.PROGRAMFILES;
  if (!system || !path.isAbsolute(system) || !programs || !path.isAbsolute(programs)) throw new Error("Windows fixture needs absolute SystemRoot and ProgramFiles for its controlled system-tool PATH");
  return [bin, path.join(system, "System32"), system, path.join(system, "System32", "WindowsPowerShell", "v1.0"), path.join(programs, "Git", "cmd"), path.join(programs, "Git", "usr", "bin")].join(path.delimiter);
}

/** The Unix no-op has no native Windows counterpart; use the compiled shim. */
export function fixtureOpenCommand(root?: string, privateRoot?: string): string {
  if (process.platform !== "win32") return "/usr/bin/true";
  if (!root && !privateRoot) throw new Error("Windows fixture opener needs a private root");
  const command = path.join((root ?? privateRoot)!, "bin", fixtureExecutable("hide-open"));
  // Unit/private-home fixtures need no Herdr server. Give those homes the
  // same native no-op without falling back to an account's GUI opener.
  if (!root) {
    fs.mkdirSync(path.dirname(command), { recursive: true });
    const source = path.join(privateRoot!, "hide-open.c");
    fs.writeFileSync(source, "int main(void) { return 0; }\n");
    compileFixtureC(source, command);
  }
  if (!fs.existsSync(command)) throw new Error(`Windows fixture opener is missing: ${command}`);
  return command;
}
