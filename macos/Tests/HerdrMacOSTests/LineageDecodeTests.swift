import Foundation
import Testing

@testable import HerdrMacOS

/// Pins the Swift side of the lineage and agent-hook wire: the keys the core
/// emits for a pane's children, a breadcrumb, an agent row's ownership, the
/// Settings diagnosis, and an Overview worktree row's agent line.

@Test func paneChildrenAndBreadcrumbDecodeFromTheCoresKeys() throws {
    let payload = """
    {
        "id": "w1:p1",
        "cwd": "/fixture",
        "status_label": "Working",
        "requires_close_confirmation": false,
        "summary": "building",
        "activity_at_unix_ms": 1,
        "children": {
            "instrumented": true,
            "uninstrumented_reason": null,
            "uninstrumented_label": null,
            "uninstrumented_code": null,
            "chips": [
                {
                    "pane_id": "w1:p2", "label": "Implementor", "detail": "running tests",
                    "agent_kind": "claude", "demand": "none", "activity": "working",
                    "emphasized": false, "symbol": "\\u25cf", "status_label": "Working",
                    "delegated": true
                }
            ],
            "representative": {
                "pane_id": "w1:p2", "label": "Implementor", "detail": "running tests",
                "agent_kind": "claude", "demand": "none", "activity": "working",
                "emphasized": false, "symbol": "\\u25cf", "status_label": "Working",
                "delegated": true
            },
            "subagents": {"working": 2, "done": 4, "blocked": null}
        },
        "lineage_path": [
            {"pane_id": "w1:p0", "label": "Observer", "siblings": []}
        ]
    }
    """
    let pane = try JSONDecoder().decode(CorePaneSnapshot.self, from: Data(payload.utf8))
    let children = try #require(pane.children)

    #expect(children.instrumented)
    #expect(children.chips.map(\.label) == ["Implementor"])
    #expect(children.representative?.paneID == "w1:p2")
    #expect(children.chips.allSatisfy { $0.delegated })
    #expect(children.subagents.working == 2)
    #expect(children.subagents.done == 4)
    // A count no adapter reports stays unknown; it is never drawn as zero.
    #expect(children.subagents.blocked == nil)
    #expect(!children.subagents.isSilent)
    #expect(pane.lineagePath.map(\.label) == ["Observer"])
    // A root has no siblings, which is why its step draws no chevron.
    #expect(pane.lineagePath.first?.siblings.isEmpty == true)
}

@Test func aPaneWithNoAgentCarriesNoChildrenAtAll() throws {
    let payload = """
    {
        "id": "w1:p9",
        "cwd": "/fixture",
        "status_label": "Idle",
        "requires_close_confirmation": false,
        "summary": null,
        "activity_at_unix_ms": null
    }
    """
    let pane = try JSONDecoder().decode(CorePaneSnapshot.self, from: Data(payload.utf8))
    #expect(pane.children == nil, "no session in the pane is why nothing is drawn")
    #expect(pane.lineagePath.isEmpty)
}

@Test func anUninstrumentedPaneCarriesItsReasonItsMarkNameAndItsCode() throws {
    let payload = """
    {
        "instrumented": false,
        "uninstrumented_reason": "This session started before the Hide hook was installed. Restart the agent to instrument it.",
        "uninstrumented_label": "Children unknown: session started before the hook was installed",
        "uninstrumented_code": "session_predates_install",
        "chips": [],
        "representative": null,
        "subagents": {"working": null, "done": null, "blocked": null}
    }
    """
    let children = try JSONDecoder().decode(CorePaneChildren.self, from: Data(payload.utf8))
    #expect(!children.instrumented)
    #expect(children.uninstrumentedCode == "session_predates_install")
    #expect(children.uninstrumentedReason?.contains("Restart the agent") == true)
    #expect(children.uninstrumentedLabel?.isEmpty == false)
    #expect(children.subagents.isSilent, "unknown is not zero")
}

@Test func anAgentRowCarriesItsOwnershipAndAnyStallNotice() throws {
    let payload = """
    {
        "id": "Observer", "pane_id": "w1:p1", "workspace_label": "hide",
        "agent_kind": "claude", "demand": "none", "activity": "working",
        "unread": false, "blocked": false, "group": "needs_you",
        "symbol": "\\u25cf", "emphasized": true, "status_label": "Working",
        "requires_close_confirmation": true, "summary": "waiting",
        "elapsed": "16m", "last_activity": "1",
        "delegated": false,
        "stall_level": "hard",
        "stall_notice": "Implementor has been waiting 16 minutes on an approval"
    }
    """
    let agent = try JSONDecoder().decode(SidebarAgent.self, from: Data(payload.utf8))
    #expect(!agent.delegated)
    #expect(agent.stallLevel == "hard")
    #expect(agent.stallNotice?.contains("16 minutes") == true)

    // An older core, or a row with nothing to say, reads as the quiet answer.
    let quiet = """
    {
        "id": "Child", "pane_id": "w1:p2", "workspace_label": "hide",
        "agent_kind": "claude", "demand": "question", "activity": "stopped",
        "unread": true, "blocked": false, "group": "seen",
        "symbol": "?", "emphasized": false, "status_label": "Asked",
        "requires_close_confirmation": false, "summary": "asking",
        "elapsed": "1m", "last_activity": "2",
        "delegated": true
    }
    """
    let child = try JSONDecoder().decode(SidebarAgent.self, from: Data(quiet.utf8))
    #expect(child.delegated)
    #expect(child.stallLevel.isEmpty)
    #expect(child.stallNotice == nil)
    #expect(child.group == "seen", "a delegated question is not the operator's Needs You")
}

@Test func aTabHoldingOnlyDelegatedChildrenSaysSo() throws {
    let payload = """
    {"id": "t2", "workspace_id": "w1", "label": "Implementor", "empty": false,
     "panes": [], "delegated": true}
    """
    let tab = try JSONDecoder().decode(CoreTabSnapshot.self, from: Data(payload.utf8))
    #expect(tab.delegated)
}

@Test func theSettingsDiagnosisDecodesItsRuntimesAndRestartablePanes() throws {
    let payload = """
    {
        "runtimes": [
            {"id": "claude-code", "label": "Claude Code",
             "path": "/fixture/.claude/settings.json", "headline": "Installed (v1)",
             "installed": true, "offers_install": false},
            {"id": "codex", "label": "Codex",
             "path": "/fixture/.codex/hooks.json", "headline": "Not installed",
             "installed": false, "offers_install": true}
        ],
        "sessions_predating_install": [
            {"pane_id": "w1:p2", "label": "Older",
             "message": "This session started before the Hide hook was installed. Restart the agent to instrument it."}
        ]
    }
    """
    let hooks = try JSONDecoder().decode(CoreAgentHooks.self, from: Data(payload.utf8))
    #expect(hooks.runtimes.map(\.id) == ["claude-code", "codex"])
    #expect(hooks.runtimes[0].installed)
    #expect(!hooks.runtimes[0].offersInstall, "a healthy hook is not offered a reinstall")
    #expect(hooks.runtimes[1].offersInstall)
    #expect(hooks.sessionsPredatingInstall.map(\.label) == ["Older"])
}

@Test func anOverviewAgentLineSeparatesNobodyHereFromCannotSee() throws {
    let quiet = try JSONDecoder().decode(
        CoreWorktreeAgentLine.self,
        from: Data("""
        {"agents": [], "uninstrumented_reason": null, "uninstrumented_label": null,
         "uninstrumented_code": null}
        """.utf8)
    )
    #expect(quiet.agents.isEmpty)
    #expect(quiet.uninstrumentedCode == nil, "nobody working here is an answer")

    let unseen = try JSONDecoder().decode(
        CoreWorktreeAgentLine.self,
        from: Data("""
        {
            "agents": [
                {"pane_id": "w1:p1", "label": "Observer", "detail": "building",
                 "agent_kind": "claude", "demand": "none", "activity": "working",
                 "emphasized": false, "symbol": "\\u25cf", "status_label": "Working",
                 "delegated": false}
            ],
            "uninstrumented_reason": "This runtime's Hide hook is not installed.",
            "uninstrumented_label": "Children unknown: the hook is not installed",
            "uninstrumented_code": "hooks_not_installed"
        }
        """.utf8)
    )
    #expect(unseen.agents.map(\.label) == ["Observer"])
    #expect(unseen.uninstrumentedCode == "hooks_not_installed")
    #expect(unseen.uninstrumentedLabel?.isEmpty == false)
}
