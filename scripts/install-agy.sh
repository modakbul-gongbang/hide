#!/usr/bin/env bash
# Installs the vendor's current agy (the Antigravity CLI) into a folder of the
# CI runner and prints the program's path. CI only: on a workstation agy is
# the operator's, installed and updated by agy itself, and this script would
# hand a test a second copy.
set -euo pipefail
if [ "${CI:-}" != "true" ]; then
  echo "install-agy.sh installs agy on a CI runner only" >&2
  exit 2
fi
dir="${RUNNER_TEMP:?RUNNER_TEMP names the runner's scratch folder}/agy-bin"
mkdir -p "$dir"
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*)
    windows_dir=$(cygpath -w "$dir")
    pwsh -NoProfile -Command "& ([scriptblock]::Create((Invoke-RestMethod https://antigravity.google/cli/install.ps1))) --dir '$windows_dir'" >&2
    program="$dir/agy.exe"
    ;;
  *)
    curl -fsSL https://antigravity.google/cli/install.sh | bash -s -- --dir "$dir" >&2
    program="$dir/agy"
    ;;
esac
if [ ! -f "$program" ]; then
  echo "the installer left no agy at $program" >&2
  ls -la "$dir" >&2
  exit 1
fi
case "$(uname -s)" in
  MINGW* | MSYS* | CYGWIN*) cygpath -w "$program" ;;
  *) echo "$program" ;;
esac
