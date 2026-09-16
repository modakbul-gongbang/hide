import AppKit
import Testing
@testable import HerdrMacOS

@Suite("Conversation focus safety")
@MainActor
struct ConversationFocusTests {
    @Test func conversationModeRoutesFocusToLedgerAndRefusesTerminalInput() {
        #expect(ConversationInputPolicy.terminalInputAllowed(isConversation: true) == false)
        #expect(ConversationInputPolicy.terminalInputAllowed(isConversation: false) == true)

        let terminal = ImeTerminalView(
            frame: .zero,
            font: NSFont.monospacedSystemFont(ofSize: 13, weight: .regular)
        )
        terminal.allowsPaneInput = false
        let enter: [UInt8] = [0x0d]

        #expect(terminal.shouldDeliverToPane(enter[...]) == false)

        terminal.allowsPaneInput = true
        #expect(terminal.shouldDeliverToPane(enter[...]) == true)
    }
}
