import Foundation
import Testing
@testable import HerdrMacOS

@Suite("Worktree panel contract")
struct GitWorktreesPresentationTests {
    @Test func gitSelectionAndLineageCollapseDecodeFromPersistentState() throws {
        let state = try JSONDecoder().decode(CoreUIStateSnapshot.self, from: Data(#"{"right_panel_section":"git","expanded_paths":[],"collapsed_agent_pane_ids":["parent"],"collapsed_checkout_ids":["main"],"project_base_branches":{"/repo":"release"}}"#.utf8))
        #expect(state.rightPanelSection == .git)
        #expect(state.collapsedAgentPaneIDs == ["parent"])
        #expect(state.collapsedCheckoutIDs == ["main"])
        #expect(state.projectBaseBranches["/repo"] == "release")
    }

    @Test func lineageArrivesWithBothCanonicalDepthAndFlatRaisedHint() throws {
        let row = try JSONDecoder().decode(SidebarAgent.self, from: Data(#"{"id":"child","pane_id":"p2","workspace_label":"Repo","agent_kind":"terminal","demand":"none","activity":"working","unread":false,"blocked":false,"group":"working","symbol":"~","emphasized":false,"status_label":"Working","requires_close_confirmation":true,"summary":"task","elapsed":"1m","last_activity":"now","lineage_depth":3,"lineage_child_pane_ids":["p3"],"lineage_root_checkout_id":"main","lineage_worktree_badge":"linked","lineage_orphan":false,"raised_hint":"↳ from parent","lineage_collapsed":true}"#.utf8))
        #expect(row.lineageDepth == 3)
        #expect(row.lineageRootCheckoutID == "main")
        #expect(row.lineageChildPaneIDs == ["p3"])
        #expect(row.lineageCollapsed)
        #expect(row.lineageWorktreeBadge == "linked")
        #expect(row.raisedHint == "↳ from parent")
        #expect(row.group == "working")
        #expect(!row.unread)
    }
    @Test func visibleTreeAndNumberingPreserveCrossCheckoutChildrenAndCollapse() {
        func row(_ id: String, depth: Int, children: [String] = []) -> SidebarAgent {
            var result = SidebarAgent(id: id, paneID: id, workspaceLabel: "Repo", agentKind: "terminal",
                symbol: "~", summary: "task", elapsed: "", lastActivity: "", ambient: nil)
            result.lineageDepth = depth
            result.lineageRootCheckoutID = "main"
            result.lineageChildPaneIDs = children
            return result
        }
        var parent = row("parent", depth: 0, children: ["child"])
        var child = row("child", depth: 1, children: ["grandchild"])
        child.lineageWorktreeBadge = "linked"
        let grandchild = row("grandchild", depth: 2)
        let agents = [grandchild, parent, child]
        #expect(SidebarGrouping.tree(agents, checkoutID: "main", excluding: []).map(\.id) == ["parent", "child", "grandchild"])
        #expect(AgentShortcutNumbering.candidates(for: .projects, agents: agents, visibleCheckoutIDs: ["main"]).map(\.id) == ["parent", "child", "grandchild"])
        parent.lineageCollapsed = true
        #expect(SidebarGrouping.tree([parent, child, grandchild], checkoutID: "main", excluding: []).map(\.id) == ["parent"])
        #expect(!child.unread)
    }

    @Test func raisedFamilyMembersKeepTheirTreeAndShareOneShortcut() {
        var parent = SidebarAgent(id: "parent", paneID: "parent", workspaceLabel: "Repo", agentKind: "terminal",
            demand: "question", activity: "stopped", unread: true, group: "needs_you",
            symbol: "?", summary: "Review needed", elapsed: "", lastActivity: "", ambient: nil)
        parent.lineageRootCheckoutID = "main"
        parent.lineageChildPaneIDs = ["child"]
        var child = SidebarAgent(id: "child", paneID: "child", workspaceLabel: "Repo", agentKind: "terminal",
            demand: "none", activity: "stopped", unread: true, group: "done",
            symbol: "✓", summary: "Completed task", elapsed: "", lastActivity: "", ambient: nil)
        child.lineageDepth = 1
        child.lineageRootCheckoutID = "main"
        let agents = [child, parent]
        let raised = Set(SidebarGrouping.raised(agents).flatMap(\.agents).map(\.id))
        #expect(SidebarGrouping.tree(agents, checkoutID: "main", excluding: raised).map(\.id) == ["parent", "child"])
        #expect(AgentShortcutNumbering.candidates(for: .projects, agents: agents, visibleCheckoutIDs: ["main"]).map(\.id) == ["parent", "child"])
        #expect(AgentShortcutNumbering.candidates(for: .projects, agents: agents, visibleCheckoutIDs: ["main"], collapsedCheckoutIDs: ["main"]).map(\.id) == ["parent", "child"])
        parent.lineageCollapsed = true
        #expect(SidebarGrouping.tree([child, parent], checkoutID: "main", excluding: raised).map(\.id) == ["parent"])
    }

}
