#!/usr/bin/env bash
set -euo pipefail

macos_root="$(cd -- "$(dirname -- "$0")/.." && pwd)"
worktree_root="$(cd -- "$macos_root/.." && pwd)"
source_path="${1:-$worktree_root/docs/assets/hide-icon-candidates/hide-icon-02.png}"
prepared_path="${2:-$macos_root/Resources/hide-icon-1024.png}"
output_path="${3:-$macos_root/Resources/hide.icns}"

if [[ ! -f "$source_path" ]]; then
    printf 'icon source not found: %s\n' "$source_path" >&2
    exit 1
fi

mkdir -p "$(dirname -- "$prepared_path")" "$(dirname -- "$output_path")"
temporary_root="$(mktemp -d "${TMPDIR:-/tmp}/hide-icon.XXXXXX")"
iconset_path="$temporary_root/hide.iconset"
mkdir -p "$iconset_path"
trap 'rm -rf -- "$temporary_root"' EXIT

/usr/bin/sips -s format png -z 1024 1024 "$source_path" --out "$prepared_path" >/dev/null

icon_variants=(
    "16:icon_16x16.png"
    "32:icon_16x16@2x.png"
    "32:icon_32x32.png"
    "64:icon_32x32@2x.png"
    "128:icon_128x128.png"
    "256:icon_128x128@2x.png"
    "256:icon_256x256.png"
    "512:icon_256x256@2x.png"
    "512:icon_512x512.png"
    "1024:icon_512x512@2x.png"
)

for variant in "${icon_variants[@]}"; do
    IFS=: read -r size filename <<< "$variant"
    /usr/bin/sips -z "$size" "$size" "$prepared_path" --out "$iconset_path/$filename" >/dev/null
done

/usr/bin/iconutil --convert icns --output "$output_path" "$iconset_path"

printf 'source=%s\n' "$source_path"
printf 'prepared=%s\n' "$prepared_path"
printf 'prepared_dimensions=1024x1024\n'
printf 'icns=%s\n' "$output_path"
/usr/bin/file "$prepared_path" "$output_path"
