#include <windows.h>
#include <tlhelp32.h>
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
  if (argc != 2) return 64;
  DWORD parent = (DWORD)strtoul(argv[1], NULL, 10);
  HANDLE snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
  if (snapshot == INVALID_HANDLE_VALUE) return 1;
  PROCESSENTRY32 entry;
  entry.dwSize = sizeof entry;
  int running = 0;
  for (BOOL more = Process32First(snapshot, &entry); more; more = Process32Next(snapshot, &entry)) {
    if (entry.th32ProcessID == parent) running = 1;
    if (entry.th32ParentProcessID == parent) printf("%lu %s\n", (unsigned long)entry.th32ProcessID, entry.szExeFile);
  }
  CloseHandle(snapshot);
  return running ? 0 : 2;
}
