#!/usr/bin/env bash
# The capabilities that shell out - the changes view's Git reader, and the
# port reader alongside it - run on the session-sync coordinator thread and
# never under the runtime mutex or on a per-event path.
#
# Both properties are structural, so they are asserted structurally: the
# runtime module holds the mutex, so it must fork nothing; and the readers'
# entry points must be reachable only from the coordinator.
set -euo pipefail

cd "$(dirname "$0")/.."

readers=(changes ports)

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

# 4. The coordinator reads the request under the lock and releases it before
#    the reader runs.
if ! grep -q 'let Some(request) = read_changes_request' herdr-core/src/session_sync.rs; then
    printf 'the coordinator no longer reads the changes request before running git\n' >&2
    exit 1
fi

printf 'capability readers run off the runtime mutex and off every per-event path\n'
