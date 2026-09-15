#!/bin/bash
# PreToolUse hook for Claude Code and Codex: the root worktree of this
# repository stays on main. A branch is worked on in its own worktree under
# ../<repo>.worktrees/, so a `git checkout`/`git switch` that would move the
# root checkout off main is refused before it runs, with the reason returned
# to the agent. Linked worktrees, file restores (`checkout -- <path>`) and
# `checkout main` pass through.
#
# Input: the hook JSON on stdin. Exit 2 with the reason on stderr refuses the
# call; any other exit lets it run. The guard applies only to the repository
# this script is checked into, so a global Codex registration is still scoped.
set -u

# The hook payload travels through the environment: python's stdin carries
# the script itself.
input=$(cat)
script_dir=$(cd "$(dirname "$0")" && pwd -P)
verdict=$(HOOK_INPUT="$input" GUARDED_ROOT="$(dirname "$(dirname "$script_dir")")" python3 - <<'PY'
import json, os, re, shlex, subprocess, sys

try:
    p = json.loads(os.environ.get("HOOK_INPUT", ""))
except Exception:
    sys.exit(0)
if p.get("hook_event_name") not in (None, "PreToolUse"):
    sys.exit(0)
ti = p.get("tool_input") or {}
cmd = ti.get("command", ti.get("cmd", ""))
if isinstance(cmd, list):
    cmd = " ".join(shlex.quote(c) for c in cmd)
if not isinstance(cmd, str) or "git" not in cmd:
    sys.exit(0)

cwd = p.get("cwd") or os.getcwd()

def git(*args, cwd=None):
    try:
        return subprocess.run(["git", *args], cwd=cwd, capture_output=True,
                              text=True, timeout=5).stdout.strip()
    except Exception:
        return ""

def is_guarded_root(cwd):
    """The root worktree of the repository this script is checked into:
    there `--git-dir` and `--git-common-dir` name the same directory."""
    git_dir = git("rev-parse", "--absolute-git-dir", cwd=cwd)
    common = git("rev-parse", "--git-common-dir", cwd=cwd)
    if not git_dir or not common:
        return None
    if not os.path.isabs(common):
        common = os.path.join(cwd, common)
    if os.path.realpath(git_dir) != os.path.realpath(common):
        return None
    root = git("rev-parse", "--show-toplevel", cwd=cwd)
    # Codex registers hooks globally, so the script scopes itself to the
    # repository it is checked into.
    if os.path.realpath(root) != os.path.realpath(os.environ.get("GUARDED_ROOT", "")):
        return None
    return root

# Split a shell line into simple commands and look at each `git checkout` /
# `git switch`, following a `cd` in an earlier segment. A tokenizer is enough
# here: the goal is to refuse the plain forms an agent types, not to parse
# every shell construct.
for segment in re.split(r"&&|\|\||;|\||\n", cmd):
    try:
        toks = shlex.split(segment)
    except ValueError:
        continue
    while toks and ("=" in toks[0] or toks[0] in ("env", "command")):
        toks.pop(0)
    if not toks:
        continue
    if toks[0] == "cd":
        dest = os.path.expanduser(toks[1]) if len(toks) > 1 else os.path.expanduser("~")
        cwd = os.path.normpath(os.path.join(cwd, dest))
        continue
    if len(toks) < 2 or os.path.basename(toks[0]) != "git":
        continue
    i = 1
    git_cwd = cwd
    while i < len(toks) and toks[i].startswith("-"):
        if toks[i] == "-C" and i + 1 < len(toks):
            git_cwd = os.path.normpath(os.path.join(cwd, os.path.expanduser(toks[i + 1])))
        i += 2 if toks[i] in ("-C", "-c") else 1
    if i >= len(toks) or toks[i] not in ("checkout", "switch"):
        continue
    sub, args = toks[i], toks[i + 1:]
    if sub == "checkout" and ("--" in args or any(a in ("--ours", "--theirs", "--merge", "-m", "-p", "--patch") for a in args)):
        continue  # file restore, not a branch move
    target = None
    j = 0
    while j < len(args):
        a = args[j]
        if a in ("-b", "-B", "-c", "-C", "--orphan"):
            target = args[j + 1] if j + 1 < len(args) else "(new branch)"
            break
        if a == "--detach":
            target = "(detached HEAD)"
            break
        if a.startswith("-"):
            j += 1
            continue
        target = a
        break
    if target is None or target == "main":
        continue
    root = is_guarded_root(git_cwd)
    if root is None:
        continue
    # `git checkout <path>` restores a file; only a name git resolves as a
    # ref, or one that names nothing at all (a remote branch DWIM), moves HEAD.
    if (sub == "checkout" and not target.startswith("(")
            and not git("rev-parse", "--verify", "-q", target, cwd=git_cwd)
            and os.path.exists(os.path.join(git_cwd, target))):
        continue
    print(f"root worktree {root} stays on main; refused `{segment.strip()}` (target: {target}).\n"
          f"Work on a branch in its own worktree instead:\n"
          f"  git worktree add ../{os.path.basename(root)}.worktrees/<name> -b {target}\n"
          f"Only `git checkout main` is allowed here.")
    sys.exit(2)
PY
)
rc=$?
if [ "$rc" -eq 2 ]; then
  printf '%s\n' "$verdict" >&2
  exit 2
fi
exit 0
