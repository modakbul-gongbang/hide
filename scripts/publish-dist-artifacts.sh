#!/bin/zsh
set -euo pipefail

if [[ $# -ne 4 ]]; then
  print -u2 "usage: publish-dist-artifacts.sh <bundle> <archive> <checksum> <dist-root>"
  exit 2
fi

temporary_bundle=$1
temporary_archive=$2
temporary_checksum=$3
dist_root=$4
archive_name=${temporary_archive:t}

[[ -d "$temporary_bundle" ]] || { print -u2 "bundle is missing: $temporary_bundle"; exit 1; }
[[ -f "$temporary_archive" ]] || { print -u2 "archive is missing: $temporary_archive"; exit 1; }
[[ -f "$temporary_checksum" ]] || { print -u2 "checksum is missing: $temporary_checksum"; exit 1; }
[[ "$archive_name" == hide-v*-macos-arm64.zip ]] || {
  print -u2 "archive name does not match Hide's versioned artifact contract: $archive_name"
  exit 1
}

expected_digest=$(/usr/bin/awk 'NR == 1 { print $1 }' "$temporary_checksum")
expected_name=$(/usr/bin/awk 'NR == 1 { print $2 }' "$temporary_checksum")
actual_digest=$(/usr/bin/shasum -a 256 "$temporary_archive" | /usr/bin/awk '{print $1}')
[[ "$expected_digest" == "$actual_digest" && "$expected_name" == "$archive_name" ]] || {
  print -u2 "checksum does not describe the ready archive: $temporary_checksum"
  exit 1
}

mkdir -p "$dist_root"
bundle_path="$dist_root/hide.app"
archive_path="$dist_root/$archive_name"
checksum_path="$archive_path.sha256"
staged_bundle="$dist_root/.hide.app.next.$$"
staged_archive="$dist_root/.$archive_name.next.$$"
staged_checksum="$dist_root/.$archive_name.sha256.next.$$"
backup_bundle="$dist_root/.hide.app.previous.$$"
backup_archive="$dist_root/.$archive_name.previous.$$"
backup_checksum="$dist_root/.$archive_name.sha256.previous.$$"
transaction_started=0
committed=0

cleanup() {
  exit_status=$?
  trap - EXIT HUP INT TERM
  set +e
  if (( transaction_started && ! committed )); then
    if [[ -e "$backup_checksum" ]]; then
      rm -f -- "$checksum_path"
      mv -- "$backup_checksum" "$checksum_path"
    elif [[ ! -e "$staged_checksum" ]]; then
      rm -f -- "$checksum_path"
    fi
    if [[ -e "$backup_archive" ]]; then
      rm -f -- "$archive_path"
      mv -- "$backup_archive" "$archive_path"
    elif [[ ! -e "$staged_archive" ]]; then
      rm -f -- "$archive_path"
    fi
    if [[ -e "$backup_bundle" ]]; then
      rm -rf -- "$bundle_path"
      mv -- "$backup_bundle" "$bundle_path"
    elif [[ ! -e "$staged_bundle" ]]; then
      rm -rf -- "$bundle_path"
    fi
  fi
  rm -rf -- "$staged_bundle"
  rm -f -- "$staged_archive" "$staged_checksum"
  (( committed )) && rm -rf -- "$backup_bundle"
  (( committed )) && rm -f -- "$backup_archive" "$backup_checksum"
  exit "$exit_status"
}
trap cleanup EXIT
trap 'exit 130' HUP INT TERM

# Everything above this line is validation only. A failed build or incomplete
# archive therefore leaves the last successful local distribution untouched.
/usr/bin/ditto "$temporary_bundle" "$staged_bundle"
/bin/cp "$temporary_archive" "$staged_archive"
/bin/cp "$temporary_checksum" "$staged_checksum"
transaction_started=1

# Rename the old target set aside before installing any new final path. All
# renames now stay on dist's filesystem, and the EXIT trap restores this set
# after a command failure or handled termination signal.
if [[ -e "$bundle_path" ]]; then
  mv -- "$bundle_path" "$backup_bundle"
fi
if [[ -e "$archive_path" ]]; then
  mv -- "$archive_path" "$backup_archive"
fi
if [[ -e "$checksum_path" ]]; then
  mv -- "$checksum_path" "$backup_checksum"
fi
mv -- "$staged_bundle" "$bundle_path"
mv -- "$staged_archive" "$archive_path"
mv -- "$staged_checksum" "$checksum_path"
committed=1

# Historical release artifacts live on GitHub Releases. Locally retain the
# one successful archive pair this checkout just produced and preserve every
# unrelated file in dist.
setopt local_options null_glob
for candidate in "$dist_root"/hide-v*-macos-arm64.zip "$dist_root"/hide-v*-macos-arm64.zip.sha256; do
  if [[ "$candidate" != "$archive_path" && "$candidate" != "$checksum_path" ]]; then
    rm -f -- "$candidate"
  fi
done
