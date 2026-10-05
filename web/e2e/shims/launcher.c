#include <windows.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(void) {
  char script[MAX_PATH];
  DWORD length = GetModuleFileNameA(NULL, script, sizeof script);
  if (length == 0 || length >= sizeof script) return 70;
  char *dot = strrchr(script, '.');
  if (!dot) return 70;
  strcpy(dot, ".js");
  FILE *file = fopen(script, "rb");
  if (!file) { fprintf(stderr, "launcher: no script %s\n", script); return 70; }
  char line[MAX_PATH + 3];
  char *got = fgets(line, sizeof line, file);
  fclose(file);
  if (!got || strncmp(line, "#!", 2) != 0) { fprintf(stderr, "launcher: %s has no interpreter line\n", script); return 70; }
  char *interpreter = line + 2;
  interpreter[strcspn(interpreter, "\r\n")] = 0;
  const char *arguments = GetCommandLineA();
  if (*arguments == '"') {
    arguments++;
    while (*arguments && *arguments != '"') arguments++;
    if (*arguments) arguments++;
  } else {
    while (*arguments && *arguments != ' ' && *arguments != '\t') arguments++;
  }
  char *command = malloc(strlen(interpreter) + strlen(script) + strlen(arguments) + 8);
  if (!command) return 70;
  sprintf(command, "\"%s\" \"%s\"%s", interpreter, script, arguments);
  STARTUPINFOA startup = { sizeof startup };
  PROCESS_INFORMATION process;
  if (!CreateProcessA(NULL, command, NULL, NULL, TRUE, 0, NULL, NULL, &startup, &process)) {
    fprintf(stderr, "launcher: cannot start %s (error %lu)\n", interpreter, GetLastError());
    return 71;
  }
  WaitForSingleObject(process.hProcess, INFINITE);
  DWORD status = 1;
  GetExitCodeProcess(process.hProcess, &status);
  return (int)status;
}
