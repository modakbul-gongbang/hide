import Foundation
import Testing
@testable import HerdrMacOS

@Suite("Pane content")
struct PaneContentTests {
    @Test func browserDecodesTheExactProfileAndTarget() throws {
        let data = Data(#"{"kind":"browser","binding_id":"login-qa","profile":"work.qa","target_id":"ABC123","session":"hide-login-qa","cdp_port":9300,"owns_target":false}"#.utf8)
        let content = try JSONDecoder().decode(CorePaneContent.self, from: data)
        guard case .browser(let binding) = content else {
            Issue.record("Expected a browser, not a terminal")
            return
        }
        #expect(binding.profile == "work.qa")
        #expect(binding.targetID == "ABC123")
        #expect(binding.bindingID == "login-qa")
        #expect(!binding.ownsTarget)
    }

    @Test func missingTargetAndUnknownKindNeverChooseAnotherPage() {
        for json in [
            #"{"kind":"browser","binding_id":"qa","profile":"work","session":"qa"}"#,
            #"{"kind":"future-content"}"#,
        ] {
            #expect(throws: (any Error).self) {
                try JSONDecoder().decode(CorePaneContent.self, from: Data(json.utf8))
            }
        }
    }

    @Test func unavailableReasonIsPreservedForTheOperator() throws {
        let data = Data(#"{"kind":"unavailable","reason":"Remote browser cannot attach locally"}"#.utf8)
        #expect(
            try JSONDecoder().decode(CorePaneContent.self, from: data) ==
                .unavailable("Remote browser cannot attach locally")
        )
    }

    @Test func closingBrowserContentStatesItsOwnershipConsequenceForPanesAndTabs() {
        for ownsTarget in [false, true] {
            let content = CorePaneContent.browser(BrowserPaneBinding(
                bindingID: "qa", profile: "work", targetID: "ABC", session: "qa",
                cdpPort: 9300, ownsTarget: ownsTarget
            ))
            let consequence = content.closeConsequence!
            #expect(consequence.contains(ownsTarget ? "Unsaved page input may be lost" : "existing browser tab and session stay open"))
            let target = DestructiveTarget(
                id: "w1:p1", label: "Browser", statusLabel: "Idle",
                requiresCloseConfirmation: true, summary: consequence, contentConsequence: consequence
            )
            for kind in [DestructiveTargetKind.pane, .tab] {
                let notice = ConsequencePolicy.notice(kind: kind, targets: [target])
                #expect(notice.requiresConfirmation)
                #expect(notice.consequence.contains(consequence))
            }
        }
        #expect(CorePaneContent.terminal.closeConsequence == nil)
        #expect(CorePaneContent.unavailable("Host lost").closeConsequence != nil)
    }
}
