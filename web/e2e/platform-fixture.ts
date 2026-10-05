// Native spellings for the isolated e2e homes and tools. The home/state
// contract is hide-platform::host; Herdr accepts a filesystem socket path
// on every OS and maps that path to a named pipe on Windows.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { execFileSync } from "node:child_process";

export const fixtureExecutable = (name: string): string => `${name}${process.platform === "win32" ? ".exe" : ""}`;

const compiledByWorker = new Map<string, string>();
let compileDir: string | undefined;

/**
 * Writes the executable `source` compiles to at `executable`. A worker
 * compiles each distinct source once, into a directory it removes at exit,
 * and copies the result: every test starts its own Herdr, and compiling the
 * same shim for each one is what exceeded the compiler's process bound on a
 * loaded Windows runner. The compiler is an owned, bounded child, separate
 * from test deadlines.
 */
export function compileFixtureC(source: string, executable: string): void {
  let built = compiledByWorker.get(source);
  if (built === undefined) {
    if (compileDir === undefined) {
      const dir = fs.mkdtempSync(path.join(os.tmpdir(), "hide-e2e-cc-"));
      process.once("exit", () => fs.rmSync(dir, { recursive: true, force: true }));
      compileDir = dir;
    }
    const base = path.join(compileDir, String(compiledByWorker.size));
    const compiler = process.platform === "win32" ? "clang.exe" : "cc";
    fs.writeFileSync(`${base}.c`, source);
    built = `${base}${fixtureExecutable("")}`;
    try {
      execFileSync(compiler, ["-O1", "-o", built, `${base}.c`], { timeout: 20_000 });
    } catch (error) {
      throw new Error(`fixture C compiler ${compiler} failed (20 second process bound); the runner needs its native C toolchain: ${String(error)}`, { cause: error });
    }
    compiledByWorker.set(source, built);
  }
  fs.copyFileSync(built, executable);
}

/** Ends the worker's compiled helpers now; a unit test that replaces the temp folder calls it between tests. */
export function forgetCompiledFixtures(): void {
  if (compileDir !== undefined) fs.rmSync(compileDir, { recursive: true, force: true });
  compileDir = undefined;
  compiledByWorker.clear();
}

/**
 * Where a client reaches the local stream Herdr names by `socket`: the path
 * itself on Unix, the pipe `\\.\pipe\<path>` on Windows (hide-platform's
 * `ipc` makes the same mapping, so a fixture standing in front of Herdr
 * listens and connects there).
 */
export const localEndpoint = (socket: string): string => (process.platform === "win32" ? `\\\\.\\pipe\\${socket}` : socket);

/** What the system says a running process has used: cumulative CPU seconds and resident memory. */
export function processUsage(pid: number): { cpuSeconds: number; rssKiB: number } {
  if (process.platform === "win32") {
    const [cpu, bytes] = powershell(`$p = Get-Process -Id ${pid}; "$($p.TotalProcessorTime.TotalSeconds) $($p.WorkingSet64)"`).trim().split(" ");
    return { cpuSeconds: Number(cpu), rssKiB: Number(bytes) / 1024 };
  }
  const [time, rss] = execFileSync("/bin/ps", ["-p", String(pid), "-o", "time=,rss="], { encoding: "utf8" }).trim().split(/\s+/);
  return { cpuSeconds: time!.split(":").map(Number).reduce((sum, part) => sum * 60 + part, 0), rssKiB: Number(rss) };
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
    compileFixtureC("int main(void) { return 0; }\n", command);
  }
  if (!fs.existsSync(command)) throw new Error(`Windows fixture opener is missing: ${command}`);
  return command;
}

/** A Windows process, named by its id and creation time so a reused id is never taken for it. */
export type WindowsProcess = { id: number; created: string };

const powershell = (script: string, env: NodeJS.ProcessEnv = {}): string =>
  execFileSync("powershell.exe", ["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", `$ErrorActionPreference = 'Stop'\n${script}`], {
    env: { ...process.env, ...env },
    encoding: "utf8",
    timeout: 30_000,
    maxBuffer: 1024 * 1024,
    windowsHide: true,
  });

/**
 * `pid` and its live descendants. Windows keeps a running executable and a
 * process's working folder locked, and a pane's processes can outlive the
 * Herdr server that started them, so a fixture lists its tree before it
 * stops the server and ends what is left with `endWindowsProcesses`.
 */
export function windowsProcessTree(pid: number): WindowsProcess[] {
  const listed = powershell(`
    $all = @(Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId,CreationDate)
    $owned = @($all | Where-Object { $_.ProcessId -eq ${pid} })
    $frontier = $owned
    while ($frontier.Count -gt 0) {
      if ($owned.Count -gt 256) { throw 'fixture process tree exceeded 256 processes' }
      $ids = @($owned | ForEach-Object { $_.ProcessId })
      $frontier = @($all | Where-Object { $child = $_; $ids -notcontains $child.ProcessId -and @($frontier | Where-Object { $_.ProcessId -eq $child.ParentProcessId -and $child.CreationDate -ge $_.CreationDate }).Count -gt 0 })
      $owned += $frontier
    }
    ConvertTo-Json -Compress -InputObject @($owned | ForEach-Object { @{ id = $_.ProcessId; created = $_.CreationDate.ToFileTimeUtc().ToString() } })
  `);
  return JSON.parse(listed) as WindowsProcess[];
}

/**
 * Ends `owned` and every process started from an executable under `root`
 * (the fixture's shims, which hided also runs), and returns once none is
 * left, so the caller can delete `root`. Throws naming the survivors after
 * five seconds.
 */
export function endWindowsProcesses(owned: WindowsProcess[], root: string): void {
  powershell(`
    $owned = @($env:FIXTURE_OWNED | ConvertFrom-Json | ForEach-Object { $_ })
    $prefix = $env:FIXTURE_ROOT.TrimEnd('\\') + '\\'
    $live = {
      $all = @(Get-CimInstance Win32_Process -Property ProcessId,CreationDate,ExecutablePath)
      @($all | Where-Object {
        $process = $_
        ($process.ExecutablePath -and $process.ExecutablePath.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) -or
          @($owned | Where-Object { $_.id -eq $process.ProcessId -and $_.created -eq $process.CreationDate.ToFileTimeUtc().ToString() }).Count -gt 0
      })
    }
    foreach ($process in & $live) { Stop-Process -Id $process.ProcessId -Force -ErrorAction SilentlyContinue }
    $until = [DateTime]::UtcNow.AddSeconds(5)
    do {
      $remaining = @(& $live)
      if ($remaining.Count -eq 0) { exit 0 }
      Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $until)
    throw "fixture processes still running after 5 s: $(($remaining | ForEach-Object { "$($_.ProcessId) $($_.ExecutablePath)" }) -join ', ')"
  `, { FIXTURE_OWNED: JSON.stringify(owned), FIXTURE_ROOT: root });
}
