#!/usr/bin/env python3
"""Keep CoreBridge's Swift responsibility boundaries mechanically enforced.

The split is intentionally a source-level contract. SwiftPM will compile every
file in the target, so a later edit can move a DTO back into CoreBridge.swift
without changing the build or any behavior test. This check keeps the module
inventory and top-level access levels explicit, and fails when that inventory
drifts.
"""

from __future__ import annotations

import argparse
import re
import sys
from collections import Counter
from pathlib import Path


Declaration = tuple[str, str, str]


# The first component is the top-level access level. Swift's default is
# internal, which is recorded explicitly so an accidental private/public move
# is visible to this gate as well.
EXPECTED_DECLARATIONS: dict[str, tuple[Declaration, ...]] = {
    "CoreBridge.swift": (
        ("struct", "TerminalDelivery", "internal"),
        ("struct", "RuntimeStartupPreparation", "private"),
        ("class", "CoreBridge", "internal"),
    ),
    "CoreBridgeAgentHookSnapshot.swift": (
        ("struct", "CoreAgentHooks", "internal"),
        ("struct", "CoreAgentHookRuntime", "internal"),
        ("struct", "CoreAgentHookPane", "internal"),
    ),
    "CoreBridgeChangesSnapshot.swift": (
        ("struct", "CoreChangesSnapshot", "internal"),
        ("struct", "CoreChangedFile", "internal"),
        ("enum", "CoreChangedFileStatus", "internal"),
        ("struct", "CoreChangedFileDiff", "internal"),
    ),
    "CoreBridgeDispatchPolicy.swift": (
        ("struct", "HerdrProtocolMismatchDetails", "internal"),
        ("enum", "LocalHerdrMutationReadiness", "internal"),
        ("enum", "LocalHerdrMutationPolicy", "internal"),
        ("struct", "CoreDispatchRoutingPolicy", "internal"),
        ("struct", "LocalHerdrMutationDispatchPolicy", "internal"),
        ("enum", "CoreDispatchOutcome", "internal"),
    ),
    "CoreBridgeEditorSnapshot.swift": (
        ("struct", "CoreEditorSnapshot", "internal"),
        ("enum", "CoreEditorTabKind", "internal"),
        ("struct", "CoreEditorTabSnapshot", "internal"),
        ("struct", "CoreEditorDocumentSnapshot", "internal"),
        ("struct", "CoreEditorConflict", "internal"),
    ),
    "CoreBridgeRemoteSnapshot.swift": (
        ("struct", "CoreRemoteStatus", "internal"),
        ("struct", "CoreRemoteFileList", "internal"),
        ("struct", "CoreRemoteFileEntry", "internal"),
        ("struct", "CoreRemoteSessionSnapshot", "internal"),
    ),
    "CoreBridgeSnapshot.swift": (
        ("struct", "CoreSnapshot", "internal"),
        ("struct", "CoreSnapshotDelta", "internal"),
        ("extension", "CoreSnapshot", "internal"),
        ("struct", "CoreRestSnapshot", "internal"),
        ("struct", "CoreRecentClosedSnapshot", "internal"),
        ("struct", "CoreRecentClosedNotice", "internal"),
        ("struct", "CoreRecentClosedPending", "internal"),
        ("struct", "CorePaneFindSnapshot", "internal"),
        ("struct", "CorePetSnapshot", "internal"),
        ("struct", "CorePetBadges", "internal"),
        ("struct", "CorePetOrigin", "internal"),
        ("struct", "CoreUIStateSnapshot", "internal"),
        ("struct", "CoreWorkspaceRegistration", "internal"),
        ("struct", "CoreDeviceRegistration", "internal"),
        ("enum", "RightPanelSection", "internal"),
    ),
    "CoreBridgeStatusSnapshot.swift": (
        ("struct", "CoreStatusSnapshot", "internal"),
        ("struct", "CoreAsyncOperation", "internal"),
        ("struct", "CorePaneFocusRequest", "internal"),
        ("struct", "CoreBackgroundAI", "internal"),
        ("struct", "CoreBackgroundAIProvider", "internal"),
        ("struct", "CoreHerdrStatus", "internal"),
        ("struct", "CoreChromuxStatus", "internal"),
        ("struct", "CoreEnvironmentStatus", "internal"),
        ("struct", "CoreDiagnostic", "internal"),
        ("struct", "CoreLastError", "internal"),
    ),
    "CoreBridgeTerminalSnapshot.swift": (
        ("struct", "CoreTerminalSnapshot", "internal"),
        ("struct", "CoreTerminalChunk", "internal"),
        ("struct", "CoreTerminalFrame", "internal"),
        ("struct", "CoreTerminalInputSent", "internal"),
        ("struct", "CoreTerminalPaneSnapshot", "internal"),
    ),
    "CoreBridgeWorkspaceSnapshot.swift": (
        ("struct", "CorePaneLayoutSnapshot", "internal"),
        ("enum", "CorePaneLayoutNode", "internal"),
        ("struct", "CoreNavigatorSnapshot", "internal"),
        ("struct", "CoreInactiveProjectGroupSnapshot", "internal"),
        ("struct", "CoreProviderUsageSnapshot", "internal"),
        ("struct", "CoreProviderUsageBucketSnapshot", "internal"),
        ("struct", "CoreDeviceSnapshot", "internal"),
        ("struct", "CoreWorkspaceSnapshot", "internal"),
        ("struct", "CoreInactiveCheckoutGroupSnapshot", "internal"),
        ("struct", "CoreCheckoutAgentSummary", "internal"),
        ("struct", "CoreCheckoutSnapshot", "internal"),
        ("struct", "CoreStripTabSnapshot", "internal"),
        ("struct", "CoreUnpushed", "internal"),
        ("enum", "CorePullRequestBadge", "internal"),
        ("enum", "CoreReviewDecision", "internal"),
        ("enum", "CorePullRequestChecks", "internal"),
        ("struct", "CorePullRequest", "internal"),
        ("struct", "CoreGithubStatus", "internal"),
        ("struct", "CoreDiskUsage", "internal"),
        ("struct", "CoreCheckoutCard", "internal"),
        ("struct", "CoreCheckoutPaneContext", "internal"),
        ("struct", "CoreTabSnapshot", "internal"),
        ("struct", "CorePaneSnapshot", "internal"),
        ("struct", "CoreAgentChip", "internal"),
        ("struct", "CorePaneChildren", "internal"),
        ("struct", "CoreSubagentCounts", "internal"),
        ("struct", "CoreLineageStep", "internal"),
        ("struct", "CorePaneFork", "internal"),
        ("struct", "SidebarAgent", "internal"),
        ("struct", "CoreAmbientSignal", "internal"),
    ),
}

_DECLARATION = re.compile(
    r"^(?P<modifiers>(?:(?:public|internal|private|fileprivate|open|final|indirect|nonisolated)\s+)*)"
    r"(?P<kind>struct|enum|class|actor|protocol|extension)\s+"
    r"(?P<name>[A-Za-z_][A-Za-z0-9_]*)\b"
)
_VISIBILITY = {"public", "internal", "private", "fileprivate", "open"}


def _declarations(source: str) -> list[tuple[str, str, str, int]]:
    declarations: list[tuple[str, str, str, int]] = []
    for line_number, line in enumerate(source.splitlines(), start=1):
        # All declarations in this inventory are top-level. Indentation keeps
        # nested CodingKeys and helper types out of the module ownership check.
        if line[:1].isspace():
            continue
        match = _DECLARATION.match(line)
        if match is None:
            continue
        modifiers = match.group("modifiers").split()
        visibility = next(
            (modifier for modifier in modifiers if modifier in _VISIBILITY),
            "internal",
        )
        declarations.append(
            (match.group("kind"), match.group("name"), visibility, line_number)
        )
    return declarations


def _label(declaration: Declaration) -> str:
    kind, name, visibility = declaration
    return f"{visibility} {kind} {name}"


def check(root: Path) -> list[str]:
    """Return source-boundary violations for a repository root."""
    root = Path(root)
    source_dir = root / "macos" / "Sources" / "HerdrMacOS"
    expected_files = set(EXPECTED_DECLARATIONS)
    actual_files = {path.name for path in source_dir.glob("CoreBridge*.swift")}
    issues: list[str] = []

    for missing in sorted(expected_files - actual_files):
        issues.append(f"missing CoreBridge module: macos/Sources/HerdrMacOS/{missing}")
    for unexpected in sorted(actual_files - expected_files):
        issues.append(
            "unowned CoreBridge module; add it to the responsibility inventory: "
            f"macos/Sources/HerdrMacOS/{unexpected}"
        )

    for filename, expected in EXPECTED_DECLARATIONS.items():
        path = source_dir / filename
        if not path.is_file():
            continue
        expected_counter = Counter(expected)
        actual_records = _declarations(path.read_text())
        actual_counter = Counter(
            (kind, name, visibility)
            for kind, name, visibility, _ in actual_records
        )
        missing = expected_counter - actual_counter
        extra = actual_counter - expected_counter
        if not missing and not extra:
            continue

        detail: list[str] = []
        if missing:
            detail.append(
                "missing "
                + ", ".join(_label(declaration) for declaration in sorted(missing.elements()))
            )
        if extra:
            detail.append(
                "unexpected "
                + ", ".join(_label(declaration) for declaration in sorted(extra.elements()))
            )
        issues.append(f"{path}: " + "; ".join(detail))

    return issues


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "root",
        nargs="?",
        type=Path,
        default=Path(__file__).resolve().parents[1],
        help="repository root (defaults to this script's repository)",
    )
    args = parser.parse_args(argv)
    issues = check(args.root)
    if issues:
        print("CoreBridge module structure failed:", file=sys.stderr)
        for issue in issues:
            print(f"- {issue}", file=sys.stderr)
        return 1
    print("CoreBridge module structure: responsibility inventory and access levels PASS")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
