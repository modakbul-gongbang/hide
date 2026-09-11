#!/usr/bin/env bash
# The capabilities that shell out - the changes view's Git reader, the port
# reader, the project panel's worktree, GitHub, and disk readers, and the
# Background AI provider probe - run on the session-sync coordinator thread and
# never under the runtime mutex or on a per-event path.
#
# Both properties are structural, so they are asserted structurally: the
# runtime module holds the mutex, so it must fork nothing; and the readers'
# entry points must be reachable only from the coordinator.
set -euo pipefail

cd "$(dirname "$0")/.."

readers=(changes ports worktrees github disk ai)

# These readers reach the network, walk a whole tree, or start a provider CLI,
# so they must also move the blocking part off the coordinator thread itself.
# `gh pr list` takes seconds, `du` over a build tree takes longer, and the AI
# probe starts a `codex app-server` child and runs `claude auth status`; run
# inline, any of them would be that much added latency on every Herdr pane
# event.
worker_readers=(worktrees github disk ai)

# 1. The module that holds the mutex, and the file module it calls
#    synchronously while holding it, execute no subprocess at all. The test
#    module at the end of each file is exempt: its fixtures build git
#    repositories in a temporary directory with no runtime and no mutex.
for module in runtime files; do
    source="herdr-core/src/${module}.rs"
    boundary="$(grep -n '^#\[cfg(test)\]' "$source" | head -1 | cut -d: -f1)"
    forks="$(awk -v boundary="${boundary:-0}" \
        'boundary > 0 && NR >= boundary { exit } /Command::new/ { print FILENAME ":" NR ": " $0 }' \
        "$source")"
    if [[ -n "$forks" ]]; then
        printf '%s runs a subprocess while the runtime mutex is held:\n%s\n' "$source" "$forks" >&2
        exit 1
    fi
done

for reader in "${readers[@]}"; do
    module="herdr-core/src/${reader}.rs"
    if [[ ! -f "$module" ]]; then
        printf '%s is missing; the reader it asserts no longer exists\n' "$module" >&2
        exit 1
    fi

    # 2. Only the coordinator drives the reader. A call from anywhere else
    #    would put a `git` fork back on an event or a snapshot pull.
    callers="$(grep -rln "${reader}::.*Reader\|${reader}_reader" herdr-core/src \
        --include='*.rs' | grep -v "^${module}$" | sort)"
    if [[ "$callers" != "herdr-core/src/session_sync.rs" ]]; then
        printf 'the %s reader is driven from outside the session-sync coordinator:\n%s\n' \
            "$reader" "$callers" >&2
        exit 1
    fi

    # 3. The reader answers a request rather than reading on every wake, so it
    #    cannot become a per-tick fork.
    if ! grep -q 'fn read_if_due' "$module"; then
        printf '%s no longer gates its reads behind a refresh window\n' "$module" >&2
        exit 1
    fi
done

for reader in "${worker_readers[@]}"; do
    module="herdr-core/src/${reader}.rs"

    # 4. The slow readers hand their subprocess to a worker thread rather than
    #    running it on the thread that applies every Herdr event.
    if ! grep -q 'BackgroundRead' "$module"; then
        printf '%s no longer runs its subprocess on a worker thread\n' "$module" >&2
        exit 1
    fi
done

# 5. The coordinator reads each request under the lock and releases it before
#    the reader runs.
for request in read_changes_request read_worktrees_request read_github_request read_disk_request read_ai_request; do
    # The binding may destructure - one lock acquisition can answer for more
    # than the request - so what is asserted is that the request is read out of
    # a `let Some(...)` before the reader runs, not the exact binding shape.
    if ! grep -qE "let Some\([^=]*request[^=]*\) = ${request}\(" herdr-core/src/session_sync.rs; then
        printf 'the coordinator no longer reads %s before running its subprocess\n' "$request" >&2
        exit 1
    fi
done

printf 'capability readers run off the runtime mutex, off every per-event path, and off the coordinator thread\n'
