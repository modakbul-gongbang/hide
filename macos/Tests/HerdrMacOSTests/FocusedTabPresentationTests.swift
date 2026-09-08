import Foundation
import Testing
@testable import HerdrMacOS

@Test func tabTitleFollowsItsFocusedPaneAndPreservesStripIdentity() throws {
    let tab = try JSONDecoder().decode(CoreTabSnapshot.self, from: Data(#"{"id":"w1:t1","workspace_id":"w1","checkout_id":"main","label":"Review","empty":false,"panes":[{"id":"w1:p1","herdr_label":"Build output","cwd":"/tmp/repo","status_label":"Idle","requires_close_confirmation":false},{"id":"w1:p2","terminal_title":"Terminal title","cwd":"/tmp/repo","status_label":"Working","requires_close_confirmation":false}]}"#.utf8))
    let agent = SidebarAgent(id: "a2", paneID: "w1:p2", workspaceLabel: "Repo", agentKind: "terminal",
        symbol: "~", summary: "Review sidebar changes", elapsed: "", lastActivity: "", ambient: nil)
    func item(_ paneID: String?) -> ShellTabItem? {
        ShellTabStrip.items(
            strip: [CoreStripTabSnapshot(id: "herdr:w1:t1", kind: .herdr, sourceID: "w1:t1", label: "Review")],
            herdrTabs: [tab], editorTabs: [], activeHerdrTabID: "w1:t1", activeFileTabID: nil,
            focusedPaneIDsByTab: paneID.map { ["w1:t1": $0] } ?? [:], agents: [agent]
        ).first
    }
    #expect(item("w1:p1")?.label == "Build output")
    #expect(item("w1:p1")?.focusedAgent == nil)
    #expect(item("w1:p2")?.label == "Review sidebar changes")
    #expect(item("w1:p2")?.focusedAgent?.paneID == "w1:p2")
    #expect(item("w1:p2")?.id == "herdr:w1:t1")
    #expect(item("w1:p2")?.active == true)
    #expect(item("missing")?.label == "Review")
    #expect(item(nil)?.label == "Review")
}
