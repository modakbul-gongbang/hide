#!/usr/bin/env python3
"""Check tracked bytes without printing matched values.

Default scope is tracked checkout files, including unstaged edits, not the staged
blobs. Use --scope index to inspect exactly the staged blobs, including new files.
Neither scope inspects untracked files, history, releases, image metadata or
pixels. Generic home/workstation shapes cannot prove arbitrary names, account
aliases, email addresses or credentials absent. Keep those separate audit tasks.
"""

import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys


MAX_BYTES = 16 * 1024 * 1024
MAX_DIAGNOSTICS = 200
# Audited current fixtures: generic example; alice/al test component prefixes;
# me, remote and u stand for local/remote test users. Exact occurrences only.
NEUTRAL_ACCOUNTS = frozenset({"example", "alice", "al", "me", "remote", "u"})
ACCOUNT = r"[^/\\\s\x00\"'`<>{}\[\];,:()]+"
BOUNDARY = r"(?:(?<![\w./\\])|(?<=file://))"
HOME_PATTERNS = (
    ("macos_home", re.compile(BOUNDARY + r"/Users/(?P<account>" + ACCOUNT + r")")),
    ("linux_home", re.compile(BOUNDARY + r"/home/(?P<account>" + ACCOUNT + r")")),
    ("windows_home", re.compile(
        BOUNDARY + r"[A-Za-z]:[\\/]+Users[\\/]+(?P<account>" + ACCOUNT + r")", re.I)),
)
WORKSTATION = re.compile(
    r"(?<![\w.-])(?P<account>[\w.]+(?:-[\w.]+)*)-"
    r"(?:MacBook(?:-Pro|-Air)?|Mac-mini|Mac-Studio|Mac-Pro|iMac|mbp)"
    r"(?:\.local)?(?![\w-])", re.I)
DYNAMIC_ACCOUNT = re.compile(
    r"\.?(?:\$[A-Za-z_][A-Za-z_0-9]*|\$\{[A-Za-z_][A-Za-z_0-9]*\})"
    r"(?=[/\\\s\x00\"'`<>{}\[\];,:()]|$)")
EVIDENCE = re.compile(
    r"^(?:docs/(?:verification|screenshots)(?:/|$)|spikes/[^/]+/evidence(?:/|$))")
PROFILE = re.compile(
    r"(?:^|/)(?:Cookies|History|Login Data|Web Data)(?:-journal)?$"
    r"|(?:^|/)(?:Local State|Preferences|Secure Preferences)$"
    r"|\.pma$|(?:^|/)(?:Session_|Tabs_)[0-9]+$"
    r"|(?:^|/)(?:browser-profile|chromium-profile|chrome-profile)(?:/|$)", re.I)


class ScanError(Exception):
    def __init__(self, classification, path="."):
        self.classification = classification
        self.path = path


def safe_path(path):
    for _, pattern in HOME_PATTERNS:
        path = pattern.sub(lambda m: m.group(0)[:m.start("account") - m.start()]
                           + "[redacted]", path)
    return WORKSTATION.sub("[redacted-workstation]", path)


def diagnostic(path, line, classification, scope):
    print(json.dumps({"path": safe_path(path), "line": line,
                      "class": classification, "scope": scope}), file=sys.stderr)


def git(root, *args):
    result = subprocess.run(["git", "-C", str(root), *args], capture_output=True)
    if result.returncode:
        raise ScanError("git_read_failed")
    return result.stdout


def tracked_entries(root):
    entries = []
    for row in git(root, "ls-files", "--stage", "-z").split(b"\0"):
        if not row:
            continue
        header, raw_path = row.split(b"\t", 1)
        mode, oid, stage = header.decode("ascii").split()
        path = os.fsdecode(raw_path)
        if stage != "0":
            raise ScanError("unmerged_index", path)
        if mode not in {"100644", "100755", "120000"}:
            raise ScanError("unsupported_index_entry", path)
        entries.append((path, oid))
    if not entries:
        raise ScanError("no_tracked_files")
    return entries


def checkout_bytes(root, path):
    target = root / path
    try:
        if not target.parent.resolve().is_relative_to(root):
            raise ScanError("checkout_path_outside_repository", path)
        mode = target.lstat().st_mode
        if stat.S_ISLNK(mode):
            return os.fsencode(os.readlink(target))
        if not stat.S_ISREG(mode):
            raise ScanError("unsupported_checkout_entry", path)
        if target.stat().st_size > MAX_BYTES:
            raise ScanError("tracked_blob_too_large", path)
        with target.open("rb") as source:
            data = source.read(MAX_BYTES + 1)
        if len(data) > MAX_BYTES:
            raise ScanError("tracked_blob_too_large", path)
        return data
    except OSError:
        raise ScanError("checkout_read_failed", path) from None


def content_findings(data):
    text = data.decode("utf-8", errors="replace")
    for classification, pattern in HOME_PATTERNS:
        for match in pattern.finditer(text):
            account = match.group("account")
            if account in NEUTRAL_ACCOUNTS:
                continue
            if DYNAMIC_ACCOUNT.match(text, match.start("account")):
                continue
            yield text.count("\n", 0, match.start()) + 1, classification
    for match in WORKSTATION.finditer(text):
        if match.group("account") not in NEUTRAL_ACCOUNTS:
            yield text.count("\n", 0, match.start()) + 1, "named_workstation"


def index_bytes(batch, path, oid):
    batch.stdin.write(oid.encode("ascii") + b"\n")
    batch.stdin.flush()
    header = batch.stdout.readline().split()
    if len(header) != 3 or header[0] != oid.encode("ascii") or header[1] != b"blob":
        raise ScanError("index_blob_read_failed", path)
    try:
        size = int(header[2])
    except ValueError:
        raise ScanError("index_blob_read_failed", path) from None
    if size < 0 or size > MAX_BYTES:
        raise ScanError("tracked_blob_too_large", path)
    data = batch.stdout.read(size)
    if len(data) != size or batch.stdout.read(1) != b"\n":
        raise ScanError("index_blob_read_failed", path)
    return data


@contextmanager
def owned_git_batch(root, scope):
    """One local helper; stdin EOF ends it even if the scanner is killed."""
    batch = None
    try:
        if scope == "index":
            batch = subprocess.Popen(["git", "-C", str(root), "cat-file", "--batch"],
                                     stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     stderr=subprocess.DEVNULL)
        yield batch
    finally:
        if batch:
            batch.stdin.close()
            batch.stdout.close()
            batch.wait()


def scan(root, scope):
    entries = tracked_entries(root)
    failures = 0
    with owned_git_batch(root, scope) as batch:
        for path, oid in entries:
            classification = "tracked_run_artifact" if EVIDENCE.search(path) else (
                "browser_profile_artifact" if PROFILE.search(path) else None)
            if classification:
                diagnostic(path, 0, classification, scope)
                failures += 1
            else:
                data = index_bytes(batch, path, oid) if batch else checkout_bytes(root, path)
                for line, classification in content_findings(data):
                    diagnostic(path, line, classification, scope)
                    failures += 1
                    if failures >= MAX_DIAGNOSTICS:
                        raise ScanError("diagnostic_limit_exceeded")
            if failures >= MAX_DIAGNOSTICS:
                raise ScanError("diagnostic_limit_exceeded")
    print(json.dumps({"scope": scope, "tracked_files": len(entries), "findings": failures,
                      "status": "fail" if failures else "pass"}))
    return int(bool(failures))


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--scope", choices=("checkout", "index"), default="checkout",
                        help="tracked checkout bytes (default) or exact staged blobs")
    args = parser.parse_args()
    try:
        root = Path(os.fsdecode(git(Path.cwd(), "rev-parse", "--show-toplevel")).strip()).resolve()
        return scan(root, args.scope)
    except ScanError as error:
        diagnostic(error.path, 0, error.classification, args.scope)
    except OSError:
        diagnostic(".", 0, "scan_io_failed", args.scope)
    return 1


if __name__ == "__main__":
    sys.exit(main())
