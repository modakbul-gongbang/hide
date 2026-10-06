// The fixture's view of Windows processes, read from the system and handles
// directly instead of through PowerShell and WMI, whose cold start and query
// time on a loaded runner once outlasted a 30 s limit (`spawnSync powershell.exe
// ETIMEDOUT`, #560).
//
//   hide-processes tree <pid>
//       prints `<id> <created>` for the process and every live descendant,
//       one per line; `created` is its start as a FILETIME in decimal.
//   hide-processes end <root|-> [<id>:<created> ...]
//       ends those processes, everything they started (also what a process
//       started after it was listed), and, given a root folder, every process
//       running an executable under it; returns once none is left. Each end is
//       waited for on the process's own handle. Exits 1, naming the survivors
//       on stderr, when some remain after five seconds.
//
// A process is its id and its start together, so a reused id is never taken
// for it. A process is a child of a parent when it names the parent's id and
// started no earlier than the parent; if a live process with that id started
// before the child and is not the parent, it is the child's real parent
// instead (Windows reuses the id of an exited parent).
#include <windows.h>
#include <tlhelp32.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define MAX_ROWS 8192
#define LIMIT 512
#define END_MS 5000

typedef struct {
  DWORD id;
  DWORD parent;
  ULONGLONG created;
  char path[MAX_PATH];
  int marked;
} Row;

static Row rows[MAX_ROWS];
static int row_count;

// When `process` started, in FILETIME units; 0 when Windows does not say.
static ULONGLONG started(HANDLE process) {
  FILETIME created, exited, kernel, user;
  if (!GetProcessTimes(process, &created, &exited, &kernel, &user)) return 0;
  return ((ULONGLONG)created.dwHighDateTime << 32) | created.dwLowDateTime;
}

// Reads the live processes into `rows`.
static int snapshot(void) {
  HANDLE all = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
  if (all == INVALID_HANDLE_VALUE) return 0;
  PROCESSENTRY32 entry;
  entry.dwSize = sizeof entry;
  row_count = 0;
  for (BOOL more = Process32First(all, &entry); more && row_count < MAX_ROWS; more = Process32Next(all, &entry)) {
    Row *row = &rows[row_count];
    row->id = entry.th32ProcessID;
    row->parent = entry.th32ParentProcessID;
    row->created = 0;
    row->path[0] = 0;
    row->marked = 0;
    HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, row->id);
    if (process) {
      row->created = started(process);
      DWORD size = sizeof row->path;
      if (!QueryFullProcessImageNameA(process, 0, row->path, &size)) row->path[0] = 0;
      CloseHandle(process);
    }
    row_count += 1;
  }
  CloseHandle(all);
  return 1;
}

// Owned identities, the dead ones the caller named included: a child starts
// after its parent and names its id, so a child of a dead owner is found.
typedef struct {
  DWORD id;
  ULONGLONG created;
} Identity;

static Identity owned[LIMIT];
static int owned_count;

static int is_owned(DWORD id, ULONGLONG created) {
  for (int i = 0; i < owned_count; i += 1) {
    if (owned[i].id == id && owned[i].created == created) return 1;
  }
  return 0;
}

// Grows `owned` with every live process that is a child of one of them.
static int grow(void) {
  int from = 0;
  while (from < owned_count) {
    int until = owned_count;
    for (; from < until; from += 1) {
      Identity parent = owned[from];
      for (int i = 0; i < row_count; i += 1) {
        Row *row = &rows[i];
        if (row->parent != parent.id || row->created == 0 || row->created < parent.created) continue;
        if (is_owned(row->id, row->created)) continue;
        // A live process holding the parent's id that is not the parent and
        // started no later than this one is its real parent.
        int taken = 0;
        for (int j = 0; j < row_count; j += 1) {
          Row *holder = &rows[j];
          if (holder->id == parent.id && holder->created != parent.created && holder->created != 0 && holder->created <= row->created) taken = 1;
        }
        if (taken) continue;
        if (owned_count >= LIMIT) {
          fprintf(stderr, "fixture process tree exceeded %d processes\n", LIMIT);
          return 0;
        }
        owned[owned_count].id = row->id;
        owned[owned_count].created = row->created;
        owned_count += 1;
      }
    }
  }
  return 1;
}

static int starts_with(const char *text, const char *prefix) {
  size_t length = strlen(prefix);
  return length > 0 && _strnicmp(text, prefix, length) == 0;
}

static int tree(DWORD pid) {
  if (!snapshot()) return 1;
  int found = 0;
  for (int i = 0; i < row_count; i += 1) {
    if (rows[i].id == pid && rows[i].created != 0) {
      owned[0].id = pid;
      owned[0].created = rows[i].created;
      owned_count = 1;
      found = 1;
    }
  }
  // A process that is not there has no tree to list.
  if (!found) return 0;
  if (!grow()) return 1;
  for (int i = 0; i < owned_count; i += 1) printf("%lu %llu\n", (unsigned long)owned[i].id, owned[i].created);
  return 0;
}

// Ends every process in `rows` marked for it, and waits for each to be gone on
// its own handle. Returns how many were marked.
static int end_marked(DWORD deadline) {
  int marked = 0;
  for (int i = 0; i < row_count; i += 1) {
    Row *row = &rows[i];
    if (!row->marked || row->id == GetCurrentProcessId()) continue;
    HANDLE process = OpenProcess(PROCESS_TERMINATE | SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, FALSE, row->id);
    if (!process) continue;
    // The id may have been reused since the snapshot.
    if (started(process) == row->created) {
      marked += 1;
      TerminateProcess(process, 1);
      DWORD now = GetTickCount();
      DWORD left = deadline > now ? deadline - now : 0;
      WaitForSingleObject(process, left);
    }
    CloseHandle(process);
  }
  return marked;
}

static int end(const char *root, int argc, char **argv) {
  owned_count = 0;
  for (int i = 0; i < argc; i += 1) {
    unsigned long id = 0;
    unsigned long long created = 0;
    if (sscanf(argv[i], "%lu:%llu", &id, &created) != 2 || owned_count >= LIMIT) {
      fprintf(stderr, "bad process identity: %s\n", argv[i]);
      return 64;
    }
    owned[owned_count].id = (DWORD)id;
    owned[owned_count].created = created;
    owned_count += 1;
  }
  DWORD deadline = GetTickCount() + END_MS;
  for (;;) {
    if (!snapshot()) return 1;
    if (!grow()) return 1;
    int targets = 0;
    for (int i = 0; i < row_count; i += 1) {
      Row *row = &rows[i];
      row->marked = (row->created != 0 && is_owned(row->id, row->created)) || (root[0] && row->path[0] && starts_with(row->path, root));
      if (row->marked && row->id != GetCurrentProcessId()) targets += 1;
    }
    if (targets == 0) return 0;
    end_marked(deadline);
    if ((LONG)(GetTickCount() - deadline) >= 0) break;
  }
  // What is still there is named; a process that ended meanwhile is not.
  if (!snapshot()) return 1;
  int survivors = 0;
  for (int i = 0; i < row_count; i += 1) {
    Row *row = &rows[i];
    if (row->id == GetCurrentProcessId()) continue;
    if ((row->created != 0 && is_owned(row->id, row->created)) || (root[0] && row->path[0] && starts_with(row->path, root))) {
      if (survivors == 0) fprintf(stderr, "fixture processes still running after %d s:", END_MS / 1000);
      fprintf(stderr, " %lu %s", (unsigned long)row->id, row->path);
      survivors += 1;
    }
  }
  if (survivors == 0) return 0;
  fprintf(stderr, "\n");
  return 1;
}

int main(int argc, char **argv) {
  if (argc >= 3 && strcmp(argv[1], "tree") == 0) return tree((DWORD)strtoul(argv[2], NULL, 10));
  if (argc >= 3 && strcmp(argv[1], "end") == 0) {
    // `-` names no root.
    const char *root = strcmp(argv[2], "-") == 0 ? "" : argv[2];
    return end(root, argc - 3, argv + 3);
  }
  return 64;
}
