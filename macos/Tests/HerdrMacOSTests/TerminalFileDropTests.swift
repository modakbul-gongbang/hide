import AppKit
import Testing
import SwiftTerm
@testable import HerdrMacOS

@Suite("Terminal file drop", .serialized)
@MainActor
struct TerminalFileDropTests {
    @Test func dropInsertsOrderedPathsWithoutSubmittingOrOwningAttachments() throws {
        let board = NSPasteboard(name: .init("file-drop-\(UUID())"))
        defer { board.releaseGlobally() }
        let urls = [URL(fileURLWithPath: "/tmp/first image.png"), URL(fileURLWithPath: "/tmp/한글's.png")]
        #expect(board.writeObjects(urls as [NSURL]))
        #expect(TerminalFileDrop.accepts(board))
        let view = ImeTerminalView(frame: NSRect(x: 0, y: 0, width: 600, height: 300))
        let window = NSWindow(contentRect: view.frame, styleMask: [.titled], backing: .buffered, defer: false)
        window.contentView = view
        defer { window.contentView = nil }
        let output = Output()
        view.terminalDelegate = output
        view.onPointerFocus = {}
        view.feed(text: "\u{1b}[?2004h")
        view.send(txt: "KEEP ")
        #expect(view.pasteDroppedFiles(board))
        #expect(window.firstResponder === view)
        let expected = "KEEP \u{1b}[200~\"/tmp/first image.png\"\u{1b}[201~ \u{1b}[200~\"/tmp/한글's.png\"\u{1b}[201~"
        #expect(String(decoding: output.bytes, as: UTF8.self) == expected)
        #expect(!output.bytes.contains(13) && !output.bytes.contains(10))
        view.insertText(" AFTER", replacementRange: NSRange(location: 0, length: 0))
        view.doCommand(by: #selector(NSResponder.insertNewline(_:)))
        #expect(String(decoding: output.bytes, as: UTF8.self) == expected + " AFTER\r")

        let before = output.bytes
        view.isHidden = true
        #expect(!view.pasteDroppedFiles(board))
        view.isHidden = false
        view.onPointerFocus = nil
        #expect(!view.pasteDroppedFiles(board))
        #expect(output.bytes == before, "Hidden or detached panes cannot consume a drop")
    }

    @Test func plainTerminalPathsAreQuotedAndInvalidDropsProduceNoPayload() throws {
        let board = NSPasteboard(name: .init("file-drop-\(UUID())"))
        defer { board.releaseGlobally() }
        board.writeObjects([URL(fileURLWithPath: "/tmp/space $HOME `echo` \"quote\" \\.png") as NSURL])
        let bytes = try TerminalFileDrop.input(from: board, bracketedPaste: false)
        #expect(String(decoding: bytes, as: UTF8.self) == "\"/tmp/space \\$HOME \\`echo\\` \\\"quote\\\" \\\\.png\" ")
        #expect(!bytes.contains(13) && !bytes.contains(10))
        board.clearContents()
        board.writeObjects([URL(fileURLWithPath: "/tmp/image\ncommand.png") as NSURL])
        #expect(throws: NSError.self) { try TerminalFileDrop.input(from: board, bracketedPaste: true) }
        board.clearContents()
        board.setString("ordinary text", forType: .string)
        #expect(!TerminalFileDrop.accepts(board))
        #expect(throws: NSError.self) { try TerminalFileDrop.input(from: board, bracketedPaste: false) }
    }

    /// Records the actual outgoing terminal byte boundary, independent of any provider UI.
    @MainActor private final class Output: NSObject, @preconcurrency TerminalViewDelegate {
        var bytes: [UInt8] = []
        func send(source: TerminalView, data: ArraySlice<UInt8>) {
            MainActor.assumeIsolated { bytes.append(contentsOf: data) }
        }
        func sizeChanged(source: TerminalView, newCols: Int, newRows: Int) {}
        func setTerminalTitle(source: TerminalView, title: String) {}
        func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}
        func scrolled(source: TerminalView, position: Double) {}
        func rangeChanged(source: TerminalView, startY: Int, endY: Int) {}
        func requestOpenLink(source: TerminalView, link: String, params: [String: String]) {}
    }
}
