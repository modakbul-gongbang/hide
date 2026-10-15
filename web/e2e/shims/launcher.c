#include <windows.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>

int main(void) {
  wchar_t script[MAX_PATH];
  DWORD length = GetModuleFileNameW(NULL, script, MAX_PATH);
  if (length == 0 || length >= MAX_PATH) return 70;
  wchar_t *dot = wcsrchr(script, L'.');
  if (!dot) return 70;
  wcscpy(dot, L".js");
  FILE *file = _wfopen(script, L"rb");
  if (!file) { fprintf(stderr, "launcher: cannot open script\n"); return 70; }
  // fixtureProgram writes its interpreter line as UTF-8 on every system.
  char line[4 * MAX_PATH + 3];
  char *got = fgets(line, sizeof line, file);
  fclose(file);
  if (!got || strncmp(line, "#!", 2) != 0) { fprintf(stderr, "launcher: script has no interpreter line\n"); return 70; }
  line[strcspn(line, "\r\n")] = 0;
  wchar_t interpreter[MAX_PATH];
  if (!MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, line + 2, -1, interpreter, MAX_PATH)) {
    fprintf(stderr, "launcher: invalid or oversized UTF-8 interpreter path\n");
    return 70;
  }
  // The ANSI command line loses Korean and other non-code-page characters.
  // Preserve the caller's Unicode arguments, including its original quoting.
  const wchar_t *arguments = GetCommandLineW();
  if (*arguments == L'"') {
    arguments++;
    while (*arguments && *arguments != L'"') arguments++;
    if (*arguments) arguments++;
  } else {
    while (*arguments && *arguments != L' ' && *arguments != L'\t') arguments++;
  }
  size_t capacity = wcslen(interpreter) + wcslen(script) + wcslen(arguments) + 8;
  wchar_t *command = malloc(capacity * sizeof *command);
  if (!command) return 70;
  if (swprintf(command, capacity, L"\"%ls\" \"%ls\"%ls", interpreter, script, arguments) < 0) {
    free(command);
    return 70;
  }
  STARTUPINFOW startup = { sizeof startup };
  PROCESS_INFORMATION process;
  if (!CreateProcessW(NULL, command, NULL, NULL, TRUE, 0, NULL, NULL, &startup, &process)) {
    DWORD error = GetLastError();
    free(command);
    fprintf(stderr, "launcher: cannot start interpreter (error %lu)\n", error);
    return 71;
  }
  free(command);
  DWORD status;
  if (WaitForSingleObject(process.hProcess, INFINITE) != WAIT_OBJECT_0 || !GetExitCodeProcess(process.hProcess, &status)) {
    fprintf(stderr, "launcher: cannot read interpreter exit (error %lu)\n", GetLastError());
    status = 72;
  }
  CloseHandle(process.hThread);
  CloseHandle(process.hProcess);
  return (int)status;
}
