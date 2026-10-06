// Lists the processes that name a pid as their parent, one per line:
// `<pid> <started> <older|newer|unknown> <name>`. `started` is the process's
// start in UTC (`unknown` when Windows does not say), and `older` marks one
// that started before the parent itself: Windows gave the parent a dead
// parent's pid, so that process is not its child.
#include <windows.h>
#include <tlhelp32.h>
#include <stdio.h>
#include <stdlib.h>

// When `pid` started, in FILETIME units; 0 when Windows does not say.
static ULONGLONG started(DWORD pid) {
  HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
  if (!process) return 0;
  FILETIME created, exited, kernel, user;
  ULONGLONG at = 0;
  if (GetProcessTimes(process, &created, &exited, &kernel, &user)) at = ((ULONGLONG)created.dwHighDateTime << 32) | created.dwLowDateTime;
  CloseHandle(process);
  return at;
}

static void print_started(ULONGLONG at) {
  FILETIME time;
  SYSTEMTIME utc;
  time.dwLowDateTime = (DWORD)at;
  time.dwHighDateTime = (DWORD)(at >> 32);
  if (at == 0 || !FileTimeToSystemTime(&time, &utc)) {
    printf("unknown");
    return;
  }
  printf("%04u-%02u-%02uT%02u:%02u:%02u.%03uZ", utc.wYear, utc.wMonth, utc.wDay, utc.wHour, utc.wMinute, utc.wSecond, utc.wMilliseconds);
}

int main(int argc, char **argv) {
  if (argc != 2) return 64;
  DWORD parent = (DWORD)strtoul(argv[1], NULL, 10);
  ULONGLONG parent_started = started(parent);
  HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
  if (snapshot == INVALID_HANDLE_VALUE) return 1;
  PROCESSENTRY32 entry;
  entry.dwSize = sizeof entry;
  int running = 0;
  for (BOOL more = Process32First(snapshot, &entry); more; more = Process32Next(snapshot, &entry)) {
    if (entry.th32ProcessID == parent) running = 1;
    if (entry.th32ParentProcessID != parent) continue;
    ULONGLONG at = started(entry.th32ProcessID);
    printf("%lu ", (unsigned long)entry.th32ProcessID);
    print_started(at);
    printf(" %s %s\n", at == 0 || parent_started == 0 ? "unknown" : at < parent_started ? "older" : "newer", entry.szExeFile);
  }
  CloseHandle(snapshot);
  return running ? 0 : 2;
}
