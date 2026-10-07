#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#ifdef _WIN32
#include <windows.h>
#include <io.h>
#include <sys/stat.h>
#define read _read
#define write _write
#define open _open
typedef int fixture_count_t;
static void raw_terminal(void) {
  HANDLE input = GetStdHandle(STD_INPUT_HANDLE);
  DWORD mode;
  if (!GetConsoleMode(input, &mode)) { fputs("fixture console input unavailable\n", stderr); exit(1); }
  mode &= ~(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT);
  mode |= ENABLE_VIRTUAL_TERMINAL_INPUT;
  if (!SetConsoleMode(input, mode) || !SetConsoleCP(CP_UTF8) || !SetConsoleOutputCP(CP_UTF8)) exit(1);
}
static fixture_count_t terminal_read(char *bytes, size_t size) {
  static WCHAR pending = 0;
  WCHAR input[4096];
  for (;;) {
    DWORD prefix = pending ? 1 : 0, count;
    input[0] = pending;
    if (!ReadConsoleW(GetStdHandle(STD_INPUT_HANDLE), input + prefix, 4095 - prefix, &count, NULL)) return -1;
    if (!count) return pending ? -1 : 0;
    count += prefix;
    pending = input[count - 1] >= 0xd800 && input[count - 1] <= 0xdbff ? input[--count] : 0;
    if (count) {
      int converted = WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, input, (int)count, bytes, (int)size, NULL, NULL);
      return converted ? converted : -1;
    }
  }
}
static fixture_count_t terminal_write(const char *bytes, size_t size) {
  DWORD count;
  return WriteFile(GetStdHandle(STD_OUTPUT_HANDLE), bytes, (DWORD)size, &count, NULL) ? (fixture_count_t)count : -1;
}
#else
#include <termios.h>
#include <time.h>
#include <unistd.h>
typedef ssize_t fixture_count_t;
#define terminal_read(bytes, size) read(0, bytes, size)
#define terminal_write(bytes, size) write(1, bytes, size)
static void raw_terminal(void) {
  struct termios tio;
  if (tcgetattr(0, &tio) == 0) {
    tio.c_lflag &= ~(ICANON | ECHO | IEXTEN);
    tio.c_cc[VMIN] = 1;
    tio.c_cc[VTIME] = 0;
    tcsetattr(0, TCSANOW, &tio);
  }
}
#endif
static char prompt[1 << 20];
static int provider(int argc, char **argv) {
  if (argc > 2 && strcmp(argv[1], "auth") == 0 && strcmp(argv[2], "status") == 0) {
    puts("{\"loggedIn\":true}");
    return 0;
  }
  size_t len = 0; fixture_count_t n;
  while (len < sizeof prompt - 1 && (n = read(0, prompt + len, sizeof prompt - 1 - len)) > 0) len += (size_t)n;
  prompt[len] = 0;
  char *label = NULL;
  for (char *at = prompt; (at = strstr(at, "HIDE_E2E_LABEL ")) != NULL; at++) label = at;
  if (!label) {
    /* A judgment's input is JSON, where a label cannot stand unescaped: a spec
       that drives one names a file whose one-line answer every such call gets. */
    const char *answer = getenv("HIDE_E2E_PROVIDER_ANSWER");
    FILE *file = answer ? fopen(answer, "rb") : NULL;
    if (file) {
      static char fixed[1 << 16];
      size_t got = fread(fixed, 1, sizeof fixed - 1, file);
      fclose(file);
      fixed[got] = 0;
      char *line = strchr(fixed, '\n');
      if (line) *line = 0;
      if (got > 0) {
        printf("{\"type\":\"result\",\"is_error\":false,\"structured_output\":%s}\n", fixed);
        return 0;
      }
    }
    puts("{\"type\":\"result\",\"is_error\":true,\"subtype\":\"error_during_execution\"}");
    return 1;
  }
  char *delay = NULL;
  for (char *at = prompt; (at = strstr(at, "HIDE_E2E_DELAY_MS ")) != NULL && at < label; at++) delay = at;
  if (delay) {
    long ms = atol(delay + strlen("HIDE_E2E_DELAY_MS "));
#ifdef _WIN32
    Sleep((DWORD)ms);
#else
    struct timespec wait = { ms / 1000, (ms % 1000) * 1000000L };
    nanosleep(&wait, NULL);
#endif
  }
  label += strlen("HIDE_E2E_LABEL ");
  char *end = strchr(label, '\n');
  if (end) *end = 0;
  const char *calls = getenv("HIDE_E2E_PROVIDER_LOG");
  FILE *log = calls ? fopen(calls, "a") : NULL;
  if (log) { fprintf(log, "%s\n", label); fclose(log); }
  printf("{\"type\":\"result\",\"is_error\":false,\"structured_output\":%s}\n", label);
  return 0;
}
// The model list Hide AI reads: one `initialize` control request on stdin, answered with
// the models and no turn (the same shape `hide-ai/tests/fixtures/fake-claude.py` answers).
static int models(void) {
  char sink[4096];
  while (read(0, sink, sizeof sink) > 0) {}
  puts("{\"type\":\"control_response\",\"response\":{\"subtype\":\"success\",\"request_id\":\"hide-models\","
       "\"response\":{\"models\":[{\"value\":\"default\",\"resolvedModel\":\"claude-opus-5-5\"},"
       "{\"value\":\"opus\",\"resolvedModel\":\"claude-opus-5-5\"},"
       "{\"value\":\"sonnet\",\"resolvedModel\":\"claude-sonnet-5-5\"}]}}}");
  return 0;
}
// Specs copy this shim as `codex`. Herdr 0.9.2 reads no idle Codex screen as
// idle, and `agent start --kind codex` succeeds only once the screen shows
// Codex's startup composer, so the copy draws that line.
static int run_as_codex(const char *argv0) {
  const char *base = argv0;
  for (const char *at = argv0; *at; at++) {
    if (*at == '/' || *at == '\\') base = at + 1;
  }
  return strncmp(base, "codex", 5) == 0;
}

#include "live-check.h"

int main(int argc, char **argv) {
#ifdef _WIN32
  _setmode(0, _O_BINARY);
  _setmode(1, _O_BINARY);
#endif
  if (getenv("HIDE_E2E_LIVE_CHECK")) return live_check(argc, argv);
  for (int i = 1; i < argc; i++) {
    if (strcmp(argv[i], "--json-schema") == 0 || (i == 1 && strcmp(argv[i], "auth") == 0)) return provider(argc, argv);
    if (strcmp(argv[i], "--input-format") == 0) return models();
  }
  const char *log_path = getenv("HIDE_E2E_INPUT_LOG");
  int flags = O_WRONLY | O_CREAT | O_APPEND;
#ifdef _WIN32
  flags |= _O_BINARY;
#endif
  int log = log_path ? open(log_path, flags, 0644) : -1;
  raw_terminal();
  if (run_as_codex(argv[0])) {
    static const char composer[] = "\xe2\x80\xba Ask Codex to do anything\r\n";
    if (terminal_write(composer, sizeof composer - 1) != (fixture_count_t)(sizeof composer - 1)) return 1;
  }
#ifdef _WIN32
  // Herdr's encoded PowerShell launch has no visible agent name.
  // Announce from the initialized interactive process before reading input.
  static const char ready[] = "claude fixture ready\r\n";
  if (terminal_write(ready, sizeof ready - 1) != (fixture_count_t)(sizeof ready - 1)) return 1;
  char b[16384];
#else
  char b[4096];
#endif
  fixture_count_t n;
  while ((n = terminal_read(b, sizeof b)) > 0) {
    if (log >= 0 && write(log, b, (size_t)n) < 0) return 1;
#ifdef _WIN32
    // A Unix tty turns Enter's CR into LF on input and LF into CRLF on
    // output, so the line ends. A console delivers CR alone: echo it as the
    // line end too, or the next line is drawn over this one.
    static char echo[sizeof b * 2];
    size_t out = 0;
    for (fixture_count_t i = 0; i < n; i++) {
      echo[out++] = b[i];
      if (b[i] == '\r' && (i + 1 == n || b[i + 1] != '\n')) echo[out++] = '\n';
    }
    if (terminal_write(echo, out) < 0) return 1;
#else
    if (terminal_write(b, (size_t)n) < 0) return 1;
#endif
  }
#ifdef _WIN32
  if (n < 0) return 1;
#endif
  return 0;
}
