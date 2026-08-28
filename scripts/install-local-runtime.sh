#!/bin/zsh
set -euo pipefail

export LC_ALL=en_US.UTF-8
export LC_CTYPE=en_US.UTF-8
export LANG=en_US.UTF-8

usage() {
  print -u2 -- "usage: $0 --herdr-root PATH [--install-path PATH]"
}

herdr_root=""
install_path=""
while (( $# > 0 )); do
  case "$1" in
    --herdr-root)
      (( $# >= 2 )) || { usage; exit 2; }
      herdr_root=$2
      shift 2
      ;;
    --install-path)
      (( $# >= 2 )) || { usage; exit 2; }
      install_path=$2
      shift 2
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

[[ -n "$herdr_root" ]] || { usage; exit 2; }
herdr_root=${herdr_root:A}
[[ -f "$herdr_root/Cargo.toml" ]] || {
  print -u2 -- "error: Herdr source root is invalid: $herdr_root"
  exit 1
}

if [[ -z "$install_path" ]]; then
  install_path=$(command -v herdr || true)
fi
[[ -n "$install_path" ]] || {
  print -u2 -- "error: --install-path is required when Herdr is not installed"
  exit 1
}
install_path=${install_path:A}

script_dir=${0:A:h}
project_root=${script_dir:h}
contract_schema=$project_root/contracts/herdr-api.schema.json

"$script_dir/sync-herdr-contract.sh" --herdr-root "$herdr_root" >/dev/null
contract_protocol=$(jq -er '.protocol' "$contract_schema")

before_snapshot=""
server_running=false
if [[ -x "$install_path" ]] && "$install_path" status server >/dev/null 2>&1; then
  before_snapshot=$("$install_path" api snapshot)
  server_running=true
fi

print -r -- "building Herdr from $herdr_root"
zig_bin=/opt/homebrew/opt/zig@0.15/bin/zig
if [[ ! -x "$zig_bin" ]]; then
  zig_bin=$(command -v zig || true)
fi
[[ -n "$zig_bin" && -x "$zig_bin" ]] || {
  print -u2 -- "error: Herdr requires Zig 0.15.2, but no Zig executable was found"
  exit 1
}
zig_version=$("$zig_bin" version)
[[ "$zig_version" == "0.15.2" ]] || {
  print -u2 -- "error: Herdr requires Zig 0.15.2, found $zig_version at $zig_bin"
  exit 1
}
(
  cd "$herdr_root"
  if command -v just >/dev/null 2>&1; then
    ZIG="$zig_bin" just build
  else
    ZIG="$zig_bin" cargo build --release --locked
  fi
)

candidate=$herdr_root/target/release/herdr
[[ -x "$candidate" ]] || {
  print -u2 -- "error: Herdr release build is missing: $candidate"
  exit 1
}

temporary_root=$(mktemp -d "${TMPDIR:-/tmp}/herdr-local-install.XXXXXX")
temporary_install=""
cleanup() {
  rm -rf -- "$temporary_root"
  [[ -z "$temporary_install" ]] || rm -f -- "$temporary_install"
}
trap cleanup EXIT

candidate_schema=$temporary_root/candidate-schema.json
contract_normalized=$temporary_root/contract-schema.json
"$candidate" api schema --json | jq -S . > "$candidate_schema"
jq -S . "$contract_schema" > "$contract_normalized"
if ! cmp -s "$candidate_schema" "$contract_normalized"; then
  print -u2 -- "error: built Herdr schema does not match the IDE contract"
  exit 1
fi

candidate_version=$("$candidate" --version | awk '{print $2}')
candidate_hash=$(/usr/bin/shasum -a 256 "$candidate" | awk '{print $1}')
installed_hash=""
installed_changed=false
if [[ -f "$install_path" ]]; then
  installed_hash=$(/usr/bin/shasum -a 256 "$install_path" | awk '{print $1}')
fi

if [[ "$candidate_hash" != "$installed_hash" ]]; then
  install_dir=${install_path:h}
  mkdir -p "$install_dir"
  temporary_install=$install_dir/.herdr.installing.$$
  cp "$candidate" "$temporary_install"
  chmod 755 "$temporary_install"
  [[ $(/usr/bin/shasum -a 256 "$temporary_install" | awk '{print $1}') == "$candidate_hash" ]] || {
    print -u2 -- "error: staged Herdr binary hash mismatch"
    exit 1
  }
  if [[ -f "$install_path" ]]; then
    cp "$install_path" "$install_path.previous"
  fi
  mv "$temporary_install" "$install_path"
  temporary_install=""
  installed_changed=true
fi

if [[ "$server_running" == true ]]; then
  print -r -- "handing the live Herdr session to protocol $contract_protocol"
  if ! "$install_path" server live-handoff \
      --import-exe "$install_path" \
      --expected-protocol "$contract_protocol" \
      --expected-version "$candidate_version"; then
    if [[ "$installed_changed" == true && -f "$install_path.previous" ]]; then
      rollback_install=${install_path:h}/.herdr.rollback.$$
      cp "$install_path.previous" "$rollback_install"
      chmod 755 "$rollback_install"
      mv "$rollback_install" "$install_path"
    fi
    print -u2 -- "error: live handoff failed; the old server was retained and the installed binary was restored"
    exit 1
  fi

  for _ in {1..50}; do
    if "$install_path" status server >/dev/null 2>&1; then
      break
    fi
    sleep 0.1
  done

  after_snapshot=$("$install_path" api snapshot)
  before_workspace_ids=$(print -r -- "$before_snapshot" | jq -cS '[.result.snapshot.workspaces[].workspace_id] | sort')
  after_workspace_ids=$(print -r -- "$after_snapshot" | jq -cS '[.result.snapshot.workspaces[].workspace_id] | sort')
  before_pane_ids=$(print -r -- "$before_snapshot" | jq -cS '[.result.snapshot.panes[].pane_id] | sort')
  after_pane_ids=$(print -r -- "$after_snapshot" | jq -cS '[.result.snapshot.panes[].pane_id] | sort')
  [[ "$before_workspace_ids" == "$after_workspace_ids" ]] || {
    print -u2 -- "error: live handoff changed the workspace identity set"
    exit 1
  }
  [[ "$before_pane_ids" == "$after_pane_ids" ]] || {
    print -u2 -- "error: live handoff changed the pane identity set"
    exit 1
  }
else
  print -r -- "no running Herdr server was found; installed the binary without starting a session"
fi

if [[ "$server_running" == true ]]; then
  "$script_dir/check-herdr-contract.sh" --herdr-bin "$install_path"
else
  jq -n \
    --arg herdr_bin "$install_path" \
    --arg version "$candidate_version" \
    --arg sha256 "$candidate_hash" \
    --argjson protocol "$contract_protocol" \
    '{status: "installed", server: "not-running", herdr_bin: $herdr_bin, version: $version, protocol: $protocol, sha256: $sha256}'
fi
