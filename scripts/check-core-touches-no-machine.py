#!/usr/bin/env python3
"""The core decides and keeps state; it never touches a machine (PRD
core-host-node D-21). Files, processes, the Herdr socket and session files
are a node's, reached through `NodeLink`; this check fails when production
code under `herdr-core/src` reaches one directly.

Test code is exempt: test-only files, a module file its parent declares
behind `#[cfg(test)]`, and every item behind `#[cfg(test)]`. The core's own
stores are exempt by name below, each with the reason it is the core's and
not a machine fact, and so is what a later layer of the PRD moves out, named
with that layer; nothing else may join either list without a reason.
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
CORE = ROOT / "herdr-core" / "src"

# What reaching a machine looks like, with what to do instead.
BANNED = [
    (r"\bstd::process\b(?!::id\(\))|\bCommand::new\b|\bprocess::Command\b",
     "starts a process; ask a node (`Call::*`) instead"),
    (r"\bstd::fs\b|(?<![\w:])fs::|\bFile::(open|create)\b|\bOpenOptions\b",
     "touches a file; ask a node, or keep it in one of the core's stores"),
    (r"\.(exists|is_file|is_dir|canonicalize|metadata|symlink_metadata|read_dir|read_link)\(\)",
     "reads the file system; ask a node"),
    (r"\bcap_std\b", "opens a directory capability; ask a node"),
    (r"\bhide_host::", "calls node code in process; go through `NodeLink`"),
    (r"\bhide_platform::(process|watch|ipc|fs)\b",
     "uses the platform layer's machine access; that is a node's"),
    (r"\bhide_project::(resolve|facts|git)\b",
     "reads a folder; ask a node for `Call::Project`"),
    (r"\bhide_session::(SessionCatalog|SessionCursor|read_bounded|search_read)\b"
     r"|\bhide_session::(links::(candidates|read)|label_transcript::read|session_activity::read)\b",
     "reads session files; ask the node that holds them"),
    (r"\blibc::", "calls the operating system; that is a node's"),
    (r"\bUnixStream\b|\bTcpStream\b|\bLocalSocketConnector\b",
     "opens a socket; a node holds the Herdr connection"),
]

# The core's own stores and their plumbing: state the core owns and moves
# with it (PRD core-host-node D-10), never a fact of the machine it runs on.
STORES = {
    "herdr-core/src/persistence.rs": "the core's UI state file",
    "herdr-core/src/workspace_views.rs": "the core's saved View layouts",
    "herdr-core/src/delivery/ledger.rs": "the core's delivery ledger",
    "herdr-core/src/labels/store.rs": "the core's agent label store",
    "herdr-core/src/labels/import.rs": "reads the core's own earlier label store once",
    "herdr-core/src/links/store.rs": "the core's link graph store",
    "herdr-core/src/local_issues.rs": "the core's local issue store",
    "herdr-core/src/github_store.rs": "the core's saved GitHub answers",
    "herdr-core/src/diagnostics.rs": "the core's own log file",
    "herdr-core/src/node_migration.rs": (
        "converts the core's own stores to node ids once, where they were "
        "written, before any link exists (PRD core-host-node D-10)"
    ),
}

# What a later layer of the PRD moves to a node, with that layer. Each entry
# goes in the change that moves it; the check fails once a listed file no
# longer reaches a machine, so a stale entry cannot linger.
LATER_LAYERS = {
    "herdr-core/src/remote.rs": "2b: the SSH transport moves into hide-node",
    "herdr-core/src/remote/host.rs": "2b: the SSH transport moves into hide-node",
    "herdr-core/src/remote/retirement.rs": "2b: the SSH transport moves into hide-node",
    "herdr-core/src/remote/attachments.rs": "2b: the SSH transport moves into hide-node",
    "herdr-core/src/ssh_hosts.rs": "2b: reading ~/.ssh/config is the node's with the transport",
    "herdr-core/src/live.rs": "3: the terminal attach child moves to the node's terminal path",
    "herdr-core/src/terminal_attachments.rs": "3: pasted attachments move with the terminal path",
    "herdr-core/src/labels/generator.rs": (
        "4: the one-generator lock is keyed by the Herdr server it labels, "
        "which a node-role hided answers for its own Herdr"
    ),
}

# Fixtures that are built for tests and never shipped.
FIXTURES = {
    "herdr-core/src/bin/herdr-ide-fixture.rs": "the e2e fixture binary; the package ships none of herdr-core's binaries",
}


def test_only(path: pathlib.Path) -> bool:
    relative = path.relative_to(CORE).as_posix()
    name = path.name
    return (
        name == "tests.rs"
        or name.endswith("_tests.rs")
        or "/tests/" in f"/{relative}"
    )


def blank(match: re.Match) -> str:
    return re.sub(r"[^\n]", " ", match.group(0))


def strip_comments_and_literals(text: str) -> str:
    """Blanks comments, string literals and character literals, keeping
    line numbers, so a brace in a string cannot end an item early and a path
    in a message is not code."""
    pattern = re.compile(
        r"/\*.*?\*/"
        r"|//[^\n]*"
        r"|b?'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F]{1,6}\}|.)|[^'\\\n])'"
        r"|(?<![A-Za-z0-9_])b?r(#*)\".*?\"\1"
        r"|b?\"(?:\\.|[^\"\\])*\"",
        re.S,
    )
    return pattern.sub(blank, text)


def test_modules() -> set:
    """Module files a parent declares behind `#[cfg(test)]` (or a cfg that
    requires test), which only a test build compiles."""
    found = set()
    declaration = re.compile(
        r"#\[cfg\((?:all\()?test\b[^\]]*\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;"
    )
    for parent in CORE.rglob("*.rs"):
        text = strip_comments_and_literals(parent.read_text())
        directory = parent.parent if parent.name in ("lib.rs", "mod.rs", "main.rs") else parent.with_suffix("")
        for match in declaration.finditer(text):
            name = match.group(1)
            for candidate in (directory / f"{name}.rs", directory / name / "mod.rs"):
                if candidate.exists():
                    found.add(candidate.relative_to(ROOT).as_posix())
    return found


def strip_test_items(text: str) -> str:
    """Blanks every item behind `#[cfg(test)]`, keeping line numbers."""
    out = list(text)
    for match in re.finditer(r"#\[cfg\((all\()?test\b[^\]]*\]", text):
        start = match.start()
        position = match.end()
        depth = 0
        end = None
        while position < len(text):
            char = text[position]
            if char == "{":
                depth += 1
            elif char == "}":
                depth -= 1
                if depth == 0:
                    end = position + 1
                    break
            elif char == ";" and depth == 0:
                end = position + 1
                break
            position += 1
        if end is None:
            continue
        for index in range(start, end):
            if out[index] != "\n":
                out[index] = " "
    return "".join(out)


def findings(path: pathlib.Path) -> list:
    relative = path.relative_to(ROOT).as_posix()
    text = strip_test_items(strip_comments_and_literals(path.read_text()))
    found = []
    for number, line in enumerate(text.splitlines(), 1):
        for pattern, advice in BANNED:
            if re.search(pattern, line):
                found.append(f"{relative}:{number}: {line.strip()}\n    {advice}")
    return found


def main() -> int:
    failures = []
    excused = {**STORES, **LATER_LAYERS, **FIXTURES}
    for listed in sorted(excused):
        if not (ROOT / listed).exists():
            failures.append(f"{listed}: listed as excused but no longer exists; remove the entry")
    tests = test_modules()
    for path in sorted(CORE.rglob("*.rs")):
        relative = path.relative_to(ROOT).as_posix()
        if test_only(path) or relative in tests:
            continue
        found = findings(path)
        if relative in excused:
            if not found:
                failures.append(
                    f"{relative}: listed as excused ({excused[relative]}) but reaches no machine; remove the entry"
                )
            continue
        failures.extend(found)
    if failures:
        print("herdr-core reaches the machine directly (PRD core-host-node D-21):", file=sys.stderr)
        print("\n".join(failures), file=sys.stderr)
        print(f"{len(failures)} finding(s)", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
