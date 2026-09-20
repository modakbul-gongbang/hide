import AppKit
import Darwin
import Testing
import SwiftTerm
@testable import HerdrMacOS

@Suite("Terminal attachment ingress", .serialized)
@MainActor
struct TerminalFileDropTests {
    @Test func dropCapturesOriginalFilesWithoutPrematureTerminalInput() throws {
        let board = NSPasteboard(name: .init("file-drop-\(UUID())"))
        defer { board.releaseGlobally() }
        let paths = ["/tmp/first image.png", "/tmp/한글's.png"]
        #expect(board.writeObjects(paths.map { NSURL(fileURLWithPath: $0) }))
        #expect(TerminalFileDrop.accepts(board))
        let view = ImeTerminalView(frame: NSRect(x: 0, y: 0, width: 600, height: 300))
        let window = NSWindow(contentRect: view.frame, styleMask: [.titled], backing: .buffered, defer: false)
        window.contentView = view
        defer { window.contentView = nil }
        let output = Output()
        view.terminalDelegate = output
        var focused = false
        var captured: [String] = []
        var bracketed = false
        view.onPointerFocus = { focused = true }
        view.onAttachment = { input, mode in
            if case .files(let values) = input { captured = values }
            bracketed = mode
        }
        view.feed(text: "\u{1b}[?2004h")
        view.send(txt: "KEEP ")
        #expect(view.pasteDroppedFiles(board))
        #expect(focused && window.firstResponder === view)
        #expect(captured == paths && bracketed)
        #expect(String(decoding: output.bytes, as: UTF8.self) == "KEEP ")
        view.insertText(" AFTER", replacementRange: NSRange(location: 0, length: 0))
        view.doCommand(by: #selector(NSResponder.insertNewline(_:)))
        #expect(String(decoding: output.bytes, as: UTF8.self) == "KEEP  AFTER\r")
        view.isHidden = true
        #expect(!view.pasteDroppedFiles(board))
        view.isHidden = false
        view.onPointerFocus = nil
        #expect(!view.pasteDroppedFiles(board))
    }

    @Test func controlVPastesExplicitImageButPreservesOrdinaryControlVAndComposition() throws {
        let board = NSPasteboard(name: .init("image-paste-\(UUID())"))
        defer { board.releaseGlobally() }
        let view = ImeTerminalView(frame: NSRect(x: 0, y: 0, width: 600, height: 300))
        let output = Output()
        view.terminalDelegate = output
        view.attachmentPasteboard = board
        var images = 0
        view.onAttachment = { input, _ in if case .image = input { images += 1 } }
        let event = try #require(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: .control,
            timestamp: 0, windowNumber: 0, context: nil, characters: "\u{16}",
            charactersIgnoringModifiers: "v", isARepeat: false, keyCode: 9))
        board.setString("ordinary clipboard text", forType: .string)
        view.keyDown(with: event)
        #expect(output.bytes == [0x16])
        board.clearContents()
        #expect(board.setData(try tinyPNG(), forType: .png))
        view.keyDown(with: event)
        #expect(images == 1 && output.bytes == [0x16])
        view.paste(nil)
        #expect(images == 2 && output.bytes == [0x16])
        view.setMarkedText("조합", selectedRange: NSRange(location: 2, length: 0), replacementRange: NSRange(location: NSNotFound, length: 0))
        #expect(!view.pasteClipboardImage(board), "Image ingress must not consume active IME composition")
        view.paste(nil)
        #expect(view.hasMarkedText(), "The actual image-only paste action must preserve Korean composition")
        view.unmarkText()
        view.allowsPaneInput = false
        #expect(!view.pasteClipboardImage(board))
    }

    @Test func imageShortcutConsumesMatchingKittyReleaseAndRepeatsOnly() throws {
        let board = NSPasteboard(name: .init("kitty-image-\(UUID())"))
        defer { board.releaseGlobally() }
        let view = ImeTerminalView(frame: NSRect(x: 0, y: 0, width: 600, height: 300))
        let output = Output()
        view.terminalDelegate = output
        view.attachmentPasteboard = board
        var images = 0
        view.onAttachment = { input, _ in if case .image = input { images += 1 } }
        view.feed(text: "\u{1b}[>11u") // disambiguation, event types and all keys
        board.setData(try tinyPNG(), forType: .png)
        func event(_ type: NSEvent.EventType, modifiers: NSEvent.ModifierFlags, repeated: Bool = false) throws -> NSEvent {
            try #require(NSEvent.keyEvent(with: type, location: .zero, modifierFlags: modifiers,
                timestamp: 0, windowNumber: 0, context: nil, characters: "v",
                charactersIgnoringModifiers: "v", isARepeat: repeated, keyCode: 9))
        }
        view.keyDown(with: try event(.keyDown, modifiers: .control))
        view.keyDown(with: try event(.keyDown, modifiers: .control, repeated: true))
        view.keyUp(with: try event(.keyUp, modifiers: [])) // Control may be released first.
        #expect(images == 1 && output.bytes.isEmpty)
        board.clearContents()
        board.setString("ordinary text", forType: .string)
        view.keyDown(with: try event(.keyDown, modifiers: .control))
        view.keyUp(with: try event(.keyUp, modifiers: .control))
        let encoded = String(decoding: output.bytes, as: UTF8.self)
        #expect(encoded.contains("118") && encoded.contains(":3"), "Ordinary Control-V retains kitty press and release events")
    }

    @Test func invalidDropsAndOversizedClipboardHaveActionableFailures() {
        let board = NSPasteboard(name: .init("invalid-paste-\(UUID())"))
        defer { board.releaseGlobally() }
        board.writeObjects([NSURL(fileURLWithPath: "/tmp/image\ncommand.png")])
        if case .failure(let reason) = TerminalFileDrop.files(from: board) { #expect(reason.contains("control characters")) }
        else { Issue.record("Control-character path must be refused") }
        board.clearContents()
        board.setData(Data(count: TerminalFileDrop.maximumFileBytes + 1), forType: .png)
        if case .failure(let reason) = TerminalFileDrop.image(from: board) { #expect(reason.contains("20 MiB")) }
        else { Issue.record("Oversized image must be refused before decoding") }
    }

    @Test func clipboardNormalizationCreatesPrivatePNGAndExpiresOnlyGeneratedOldFiles() async throws {
        let root = temporaryRoot()
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let staging = root.appendingPathComponent("TerminalClipboard")
        let data = try tinyPNG()
        let first = try await Task.detached { try TerminalFileDrop.stageImage(data, root: staging, requestID: UUID().uuidString) }.value
        var metadata = stat()
        #expect(lstat(first.path, &metadata) == 0 && metadata.st_mode & 0o777 == 0o600)
        #expect(lstat(staging.path, &metadata) == 0 && metadata.st_mode & 0o777 == 0o700)
        #expect(NSImage(contentsOf: first)?.size == NSSize(width: 2, height: 2))
        try FileManager.default.setAttributes([.modificationDate: Date(timeIntervalSinceNow: -TerminalFileDrop.stagingLifetime - 1)], ofItemAtPath: first.path)
        let second = try await Task.detached { try TerminalFileDrop.stageImage(data, root: staging, requestID: UUID().uuidString) }.value
        #expect(!FileManager.default.fileExists(atPath: first.path))
        #expect(FileManager.default.fileExists(atPath: second.path))
        let link = root.appendingPathComponent("symlink")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: staging)
        do {
            _ = try await Task.detached { try TerminalFileDrop.stageImage(data, root: link, requestID: UUID().uuidString) }.value
            Issue.record("Symlink staging root must be refused")
        } catch { #expect(error.localizedDescription.contains("private directory")) }
        #expect(FileManager.default.fileExists(atPath: second.path))
    }

    @Test func coreHoldsInputOnFileFailureAndRetriesInOriginalOrder() async throws {
        let root = temporaryRoot()
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let bridge = CoreBridge(arguments: ["HerdrMacOS", "--verification-ui-fixture",
            "--workspace-root", root.path, "--state-path", root.appendingPathComponent("state.json").path])
        try await eventually { bridge.snapshot?.terminal.panes.isEmpty == false }
        let pane = try #require(bridge.snapshot?.terminal.panes.first?.paneID)
        var received: [UInt8] = []
        let registration = bridge.registerTerminal(paneID: pane, receive: { received.append(contentsOf: $0.bytes) }, focus: {})
        defer { bridge.unregisterTerminal(paneID: pane, registrationID: registration) }
        let file = root.appendingPathComponent("image $name.png")
        bridge.pasteTerminalAttachment(.files([file.path]), paneID: pane, bracketed: true)
        try await eventually { bridge.snapshot?.status.asyncOperations.first { $0.kind == "terminal.attachment" }?.phase == "failed" }
        let operation = try #require(bridge.snapshot?.status.asyncOperations.first { $0.kind == "terminal.attachment" })
        #expect(operation.message?.contains("unavailable") == true)
        received.removeAll()
        bridge.sendTerminalInput(Array(" AFTER\r".utf8), paneID: pane)
        try await Task.sleep(for: .milliseconds(60))
        #expect(received.isEmpty, "A failed attachment must not forward Enter")
        try Data("file bytes".utf8).write(to: file)
        bridge.terminalAttachmentAction(requestID: operation.id, paneID: pane, action: "retry")
        try await eventually { bridge.snapshot?.status.asyncOperations.contains { $0.id == operation.id } == false }
        let quoted = file.path.replacingOccurrences(of: "$", with: "\\$")
        #expect(String(decoding: received, as: UTF8.self) == "\u{1b}[200~\"\(quoted)\"\u{1b}[201~ AFTER\r")
    }

    @Test func competingPasteDismissalDoesNotCancelImageAndExplicitCancelCleansPreparation() async throws {
        let root = temporaryRoot()
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let bridge = CoreBridge(arguments: ["HerdrMacOS", "--verification-ui-fixture",
            "--workspace-root", root.path, "--state-path", root.appendingPathComponent("state.json").path])
        try await eventually { bridge.snapshot?.terminal.panes.isEmpty == false }
        let pane = try #require(bridge.snapshot?.terminal.panes.first?.paneID)
        var received: [UInt8] = []
        let registration = bridge.registerTerminal(paneID: pane, receive: { received.append(contentsOf: $0.bytes) }, focus: {})
        defer { bridge.unregisterTerminal(paneID: pane, registrationID: registration) }
        received.removeAll()
        bridge.pasteTerminalAttachment(.image(try tinyPNG()), paneID: pane, bracketed: true)
        let original = try #require(bridge.snapshot?.status.asyncOperations.first { $0.kind == "terminal.attachment" })
        bridge.pasteTerminalAttachment(.files(["/unavailable/file"]), paneID: pane, bracketed: true)
        let refusal = try #require(bridge.snapshot?.status.asyncOperations.first { $0.kind == "terminal.attachment" && $0.phase == "refused" })
        bridge.terminalAttachmentAction(requestID: refusal.id, paneID: pane, action: "cancel")
        try await eventually { bridge.snapshot?.status.asyncOperations.contains { $0.id == original.id } == false }
        #expect(String(decoding: received, as: UTF8.self).contains("TerminalClipboard/hide-\(original.id).png"))
        let accepted = root.appendingPathComponent("TerminalClipboard/hide-\(original.id).png")
        #expect(NSImage(contentsOf: accepted) != nil)
        received.removeAll()
        bridge.pasteTerminalAttachment(.image(try tinyPNG()), paneID: pane, bracketed: true)
        let cancelled = try #require(bridge.snapshot?.status.asyncOperations.first { $0.kind == "terminal.attachment" })
        bridge.terminalAttachmentAction(requestID: cancelled.id, paneID: pane, action: "cancel")
        try await eventually { bridge.snapshot?.status.asyncOperations.contains { $0.id == cancelled.id } == false }
        #expect(received.isEmpty)
        #expect(!FileManager.default.fileExists(atPath: root.appendingPathComponent("TerminalClipboard/hide-\(cancelled.id).png").path))
        #expect(FileManager.default.fileExists(atPath: accepted.path), "Cancellation cannot delete a previous accepted attachment")
    }

    private func temporaryRoot() -> URL {
        FileManager.default.temporaryDirectory.resolvingSymlinksInPath().appendingPathComponent("hide-attachment-test-\(UUID())")
    }

    private func tinyPNG() throws -> Data {
        let bitmap = try #require(NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 2, pixelsHigh: 2,
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
            colorSpaceName: .deviceRGB, bytesPerRow: 8, bitsPerPixel: 32))
        bitmap.setColor(.red, atX: 0, y: 0)
        return try #require(bitmap.representation(using: .png, properties: [:]))
    }

    private func eventually(_ condition: () -> Bool) async throws {
        for _ in 0..<200 {
            if condition() { return }
            try await Task.sleep(for: .milliseconds(10))
        }
        #expect(condition(), "Attachment state did not settle")
    }

    @MainActor private final class Output: NSObject, @preconcurrency TerminalViewDelegate {
        var bytes: [UInt8] = []
        func send(source: TerminalView, data: ArraySlice<UInt8>) { MainActor.assumeIsolated { bytes.append(contentsOf: data) } }
        func sizeChanged(source: TerminalView, newCols: Int, newRows: Int) {}
        func setTerminalTitle(source: TerminalView, title: String) {}
        func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}
        func scrolled(source: TerminalView, position: Double) {}
        func rangeChanged(source: TerminalView, startY: Int, endY: Int) {}
        func requestOpenLink(source: TerminalView, link: String, params: [String: String]) {}
    }
}
