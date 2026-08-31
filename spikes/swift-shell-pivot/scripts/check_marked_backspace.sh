#!/usr/bin/env bash
set -euo pipefail

spike_root="$(cd "$(dirname "$0")/.." && pwd)"
evidence_argument="${1:?usage: check_marked_backspace.sh <new-evidence.json>}"
evidence_directory="$(cd "$(dirname "$evidence_argument")" && pwd)"
evidence_path="$evidence_directory/$(basename "$evidence_argument")"
boundary_build="$spike_root/.build-boundary"
boundary_app="$spike_root/build/SwiftShellPivotBoundaryCheck.app"
boundary_exec="$boundary_app/Contents/MacOS/SwiftShellSpike"

if [ -e "$evidence_path" ]; then
    printf 'evidence path already exists: %s\n' "$evidence_path" >&2
    exit 2
fi

if pgrep -f "^${boundary_exec}( |$)" >/dev/null; then
    printf 'a spike app instance is already running\n' >&2
    exit 3
fi

cd "$spike_root/rust-core"
cargo build --release

cd "$spike_root"
rust_archive="$spike_root/rust-core/target/release/libherdr_core_spike.a"
rust_archive_hash="$(LC_ALL=C LANG=C /usr/bin/shasum -a 256 "$rust_archive" | /usr/bin/awk '{print $1}')"
swift build -c release --arch arm64 \
    --scratch-path "$boundary_build" \
    -Xswiftc -D \
    -Xswiftc SPIKE_BOUNDARY_PROBE \
    -Xswiftc -D \
    -Xswiftc "HERDR_CORE_${rust_archive_hash}"

mkdir -p "$boundary_app/Contents/MacOS"
install -m 755 "$boundary_build/arm64-apple-macosx/release/SwiftShellSpike" "$boundary_exec"
install -m 644 "$spike_root/resources/Info.plist" "$boundary_app/Contents/Info.plist"
codesign --force --deep --sign - "$boundary_app"
codesign --verify --deep --strict --verbose=2 "$boundary_app"

/usr/bin/open -n "$boundary_app" --args --marked-text-probe "$evidence_path"

for _ in $(seq 1 100); do
    if [ -s "$evidence_path" ]; then
        break
    fi
    sleep 0.1
done

if [ ! -s "$evidence_path" ]; then
    printf 'marked-text probe did not write evidence\n' >&2
    exit 4
fi

jq -e '
  .passed == true and
  .acceptance_scope == "document-coordinate and ordinary-backspace AppKit boundary only; not a real IME verdict" and
  .window_is_key == true and
  .first_responder_is_terminal == true and
  .implicit_marked_state.has_text == true and
  .explicit_marked_state.has_text == true and
  .partial_substring == "나" and
  .ordinary_delegate_event_delta == 1 and
  .ordinary_rust_byte_delta == 1 and
  .ordinary_delegate_bytes == [127]
' "$evidence_path" >/dev/null

for _ in $(seq 1 30); do
    if ! pgrep -f "^${boundary_exec}( |$)" >/dev/null; then
        exit 0
    fi
    sleep 0.1
done

printf 'boundary probe app did not exit\n' >&2
exit 5
