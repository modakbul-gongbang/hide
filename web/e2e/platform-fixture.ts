// Native spellings for the isolated e2e homes and tools. The home/state
// contract is hide-platform::host; Herdr accepts a filesystem socket path
// on every OS and maps that path to a named pipe on Windows.
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";
import { copyFixtureShim } from "./shims/build";

export const fixtureExecutable = (name: string): string => `${name}${process.platform === "win32" ? ".exe" : ""}`;

/**
 * Where a client reaches the local stream Herdr names by `socket`: the path
 * itself on Unix, the pipe `\\.\pipe\<path>` on Windows (hide-platform's
 * `ipc` makes the same mapping, so a fixture standing in front of Herdr
 * listens and connects there).
 */
export const localEndpoint = (socket: string): string => (process.platform === "win32" ? `\\\\.\\pipe\\${socket}` : socket);

// `launcher.c` (built once, e2e/shims/build.ts) runs the Node script next to it
// that shares its name: `name.exe` runs `name.js`, interpreted by the program
// its first line (`#!<path>`) names, with the arguments it was given, and
// exits with that program's status.

/**
 * An executable `name` in `dir` that runs the Node `script`, the same way on
 * every system: Unix runs the file through its interpreter line, and Windows,
 * which cannot run a script by name, gets `name.exe`, one compiled launcher
 * copied beside `name.js`. Returns the executable's path. The script reads
 * its arguments from `process.argv.slice(2)`.
 */
export function fixtureProgram(dir: string, name: string, script: string): string {
  fs.mkdirSync(dir, { recursive: true });
  const body = `#!${process.execPath}\n${script}`;
  if (process.platform !== "win32") {
    const executable = path.join(dir, name);
    fs.writeFileSync(executable, body, { mode: 0o755 });
    return executable;
  }
  fs.writeFileSync(path.join(dir, `${name}.js`), body);
  const executable = path.join(dir, `${name}.exe`);
  copyFixtureShim("launcher", executable);
  return executable;
}

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
    copyFixtureShim("noop", command);
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
 * Ends every `vctip.exe` and returns their pids once each has exited. MSVC's
 * compiler and linker start that telemetry helper and leave it running after
 * a build, still naming as its parent a linker that has exited; Windows
 * reuses that pid, and a pane shell that gets it has the helper for a child,
 * so Herdr 0.9.1, which counts a shell's children by parent pid alone, refuses
 * `agent start` there for as long as the helper runs. Throws naming the
 * survivors after five seconds.
 */
export function endVctip(): number[] {
  const ended = powershell(`
    $found = @(Get-Process -Name vctip -ErrorAction SilentlyContinue)
    foreach ($process in $found) { Stop-Process -InputObject $process -Force -ErrorAction SilentlyContinue }
    if ($found.Count -gt 0) { Wait-Process -InputObject $found -Timeout 5 }
    ConvertTo-Json -Compress -InputObject @($found | ForEach-Object { $_.Id })
  `);
  return JSON.parse(ended) as number[];
}

/**
 * Ends `owned`, every process they started, and, given `root`, every process
 * started from an executable under it (the fixture's shims, which hided also
 * runs), and returns once none is left, so the caller can delete `root`.
 * What an owned process started is found from its identity, so a child it
 * started after `owned` was listed is ended even once its parent is gone: an
 * owner killed while it starts a child leaves that child behind, and on
 * Windows a child started suspended and not yet in its job object (hided's
 * `OwnedChild::spawn`) stays suspended for good and keeps its executable
 * locked, where the path check did not find it. Throws naming the survivors
 * after five seconds.
 */
export function endWindowsProcesses(owned: WindowsProcess[], root?: string): void {
  powershell(`
    $owned = @($env:FIXTURE_OWNED | ConvertFrom-Json | ForEach-Object { $_ } | ForEach-Object { [pscustomobject]@{ id = [uint32]$_.id; created = [int64]$_.created } })
    $prefix = if ($env:FIXTURE_ROOT) { $env:FIXTURE_ROOT.TrimEnd('\\') + '\\' } else { $null }
    $live = {
      $rows = @(Get-CimInstance Win32_Process -Property ProcessId,ParentProcessId,CreationDate,ExecutablePath | ForEach-Object {
        [pscustomobject]@{ id = [uint32]$_.ProcessId; parent = [uint32]$_.ParentProcessId; created = $_.CreationDate.ToFileTimeUtc(); path = $_.ExecutablePath }
      })
      $holders = @{}
      $children = @{}
      foreach ($row in $rows) {
        $holders[$row.id] = $row
        if (-not $children.ContainsKey($row.parent)) { $children[$row.parent] = @() }
        $children[$row.parent] += $row
      }
      # The owned processes and what they started, alive or not: a child names
      # its parent's pid and started after it, and a live process holding that
      # pid that started before the child is its parent instead.
      $tree = @{}
      foreach ($process in $owned) { $tree["$($process.id)/$($process.created)"] = $true }
      $frontier = $owned
      while ($frontier.Count -gt 0) {
        if ($tree.Count -gt 512) { throw 'fixture process tree exceeded 512 processes' }
        $next = @()
        foreach ($parent in $frontier) {
          $holder = $holders[$parent.id]
          foreach ($row in @($children[$parent.id])) {
            if (-not $row -or $row.created -lt $parent.created -or $tree.ContainsKey("$($row.id)/$($row.created)")) { continue }
            if ($holder -and $holder.created -ne $parent.created -and $holder.created -le $row.created) { continue }
            $tree["$($row.id)/$($row.created)"] = $true
            $next += $row
          }
        }
        $frontier = $next
      }
      @($rows | Where-Object {
        $tree.ContainsKey("$($_.id)/$($_.created)") -or
          ($prefix -and $_.path -and $_.path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase))
      })
    }
    $until = [DateTime]::UtcNow.AddSeconds(5)
    do {
      $remaining = @(& $live)
      if ($remaining.Count -eq 0) { exit 0 }
      # A process an owned one started while it was ending is found on a later pass.
      foreach ($process in $remaining) { Stop-Process -Id $process.id -Force -ErrorAction SilentlyContinue }
      Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $until)
    throw "fixture processes still running after 5 s: $(($remaining | ForEach-Object { "$($_.id) $($_.path)" }) -join ', ')"
  `, { FIXTURE_OWNED: JSON.stringify(owned), FIXTURE_ROOT: root ?? "" });
}
