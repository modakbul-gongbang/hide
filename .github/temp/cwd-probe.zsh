#!/bin/zsh
# Probe: what cwd does Herdr 0.9.1 report for a pane right after `tab create`?
# Isolated server only: env -i keeps this shell's Herdr pane identity out.
set -u
bin=$1
count=${2:-40}
out=$3
root=$(mktemp -d /tmp/hde-cwdprobe-XXXXXX)
root=${root:A}
mkdir -p $root/{home,xdg-config,xdg-state,fixture,bin}
print -r -- "PS1='fixture %# '" > $root/home/.zshrc
print -r -- $'[update]\nversion_check = false\nmanifest_check = false\n[terminal]\ndefault_shell = "/bin/zsh"\n' > $root/herdr-config.toml
sock=/tmp/hde-cwdprobe-$RANDOM$RANDOM.sock
E=(env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin HOME=$root/home SHELL=/bin/zsh TERM=xterm-256color
  HERDR_SESSION=hde-cwdprobe-${root:t} HERDR_SOCKET_PATH=$sock HERDR_CONFIG_PATH=$root/herdr-config.toml
  XDG_CONFIG_HOME=$root/xdg-config XDG_STATE_HOME=$root/xdg-state HERDR_DISABLE_SOUND=1 skip_global_compinit=1)
(cd ${SERVER_CWD:-$PWD} && exec $E $bin server > $root/server.log 2>&1) &
server=$!
for i in {1..100}; do [[ -S $sock ]] && break; sleep 0.05; done
$E $bin workspace create --cwd $root/fixture --label fixture > $root/ws.json
ws=$(/usr/bin/python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["result"]["workspace"]["workspace_id"])' $root/ws.json)
: > $out
for i in $(seq 1 $count); do
  created=$($E $bin tab create --workspace $ws --cwd $root/fixture --no-focus)
  pane=$(print -r -- $created | /usr/bin/python3 -c 'import json,sys; print(json.load(sys.stdin)["result"]["root_pane"]["pane_id"])')
  made_cwd=$(print -r -- $created | /usr/bin/python3 -c 'import json,sys; print(json.load(sys.stdin)["result"]["root_pane"].get("cwd"))')
  read_now=$($E $bin pane get $pane | /usr/bin/python3 -c 'import json,sys; print(json.load(sys.stdin)["result"]["pane"].get("cwd"))')
  print -r -- "$pane created=$made_cwd read=$read_now" >> $out
done
sleep 1
while read -r pane rest; do
  later=$($E $bin pane get $pane | /usr/bin/python3 -c 'import json,sys; print(json.load(sys.stdin)["result"]["pane"].get("cwd"))')
  print -r -- "$pane $rest later=$later"
done < $out > $out.final
$E $bin server stop > /dev/null 2>&1
wait $server 2>/dev/null
print -r -- "root=$root"
sed "s|$root|ROOT|g" $out.final | sort | uniq -c -f1 | head -5
print -r -- "--- distinct cwd triples:"
sed "s|$root|ROOT|g" $out.final | awk '{print $2, $3, $4}' | sort | uniq -c
rm -rf $root
