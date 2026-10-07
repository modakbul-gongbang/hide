/* Opt-in behavior of the existing Claude/Codex fixture, never a real model. */
static void live_screen(const char *text) {
  const char clear[] = "\033[2J\033[H";
  terminal_write(clear, sizeof clear - 1);
  terminal_write(text, strlen(text));
}

static int live_check(int argc, char **argv) {
  for (int i = 1; i < argc; i++) {
    if (strcmp(argv[i], "--version") == 0) {
      puts("live-check fixture 1");
      return 0;
    }
  }
  raw_terminal();
  const char *scene = getenv("HIDE_LIVE_CHECK_SCENE");
  if (!scene) return 2;
#ifndef _WIN32
  /* Only this opt-in lane gets a private HOME. Exercise the controller's
     real byte recovery, and let e2e identify the exact child/socket to end. */
  const char *home = getenv("HOME");
  if (!home) return 2;
  char location[4096];
  if (snprintf(location, sizeof location, "%s/.claude.json", home) >= (int)sizeof location) return 2;
  FILE *config = fopen(location, "wb");
  if (!config) return 2;
  fputs("{\"fixture\":\"temporarily-mutated\"}\n", config);
  fclose(config);
  if (snprintf(location, sizeof location, "%s/.live-check-child.json", home) >= (int)sizeof location) return 2;
  FILE *identity = fopen(location, "wb");
  if (!identity) return 2;
  const char *socket = getenv("HERDR_SOCKET_PATH");
  fprintf(identity, "{\"pid\":%ld,\"socket\":\"%s\"}\n", (long)getpid(), socket ? socket : "");
  fclose(identity);
  const char *scenario = getenv("LIVE_CHECK_FIXTURE_CASE");
  if (scenario && strcmp(scenario, "hold") == 0) {
    live_screen("LIVE_FIXTURE_HOLD\r\n");
    char ignored;
    while (terminal_read(&ignored, 1) == 1) {}
    return 0;
  }
#endif
  const char *composer = run_as_codex(argv[0])
    ? "\xe2\x80\xba Ask Codex to do anything\r\n" : "\xe2\x9d\xaf \r\n";
  live_screen(strcmp(scene, "startup") == 0 ? "Do you trust this folder?\r\n1. Trust\r\n2. Cancel\r\n" : composer);
  char line[8192];
  size_t used = 0;
  int reached = strcmp(scene, "startup") == 0;
  char byte;
  while (terminal_read(&byte, 1) == 1) {
    if (byte != '\r' && byte != '\n') {
      if (used + 1 >= sizeof line) return 2;
      line[used++] = byte;
      continue;
    }
    line[used] = 0;
    used = 0;
    if (!reached) {
      reached = 1;
      if (strcmp(scene, "working") == 0) live_screen("Working... Esc to interrupt\r\n");
      else if (strcmp(scene, "shell_approval") == 0) live_screen("Do you want to run this command?\r\n  touch probe-1.txt\r\n1. Yes\r\n2. No\r\n");
      else if (strcmp(scene, "file_approval") == 0) live_screen("Apply these changes?\r\n1. Yes\r\n2. No\r\n");
      else if (strcmp(scene, "question") == 0) live_screen("1. One\r\n2. Two\r\nEnter to select - Esc to cancel\r\n");
      else if (strcmp(scene, "plan_approval") == 0) live_screen("Implement this plan?\r\n  1. Yes, implement this plan\r\n  2. No, stay in Plan mode\r\n");
      else if (strcmp(scene, "model_picker") == 0) live_screen("Select model\r\n1. fixture-small\r\n2. fixture-large\r\n");
      else if (strcmp(scene, "resume_picker") == 0) live_screen("Resume session\r\n1. fixture-previous\r\n");
      else if (strcmp(scene, "mcp_approval") == 0) live_screen("Allow MCP live_probe?\r\n1. Yes\r\n2. No\r\n");
      else live_screen(composer);
      continue;
    }
    if (strcmp(scene, "resume_picker") == 0) live_screen("Resumed session: fixture-previous\r\n");
    else if (strcmp(scene, "model_picker") == 0) live_screen("Selected model: fixture-small\r\n");
    else if (strcmp(scene, "startup") == 0) live_screen("Trust saved: fixture-only\r\n");
    else if (strcmp(scene, "plan_approval") == 0) {
      int file = open("probe-3.txt", O_WRONLY | O_CREAT, 0600);
      if (file >= 0) {
#ifdef _WIN32
        _close(file);
#else
        close(file);
#endif
      }
      live_screen("Approval accepted: fixture plan\r\n");
    } else if (strcmp(scene, "rest") == 0) {
      live_screen(composer);
    } else live_screen("Approval accepted: fixture-only\r\n");
  }
  return 0;
}
