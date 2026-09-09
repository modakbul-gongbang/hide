import AppKit
import ImageIO
import SwiftUI
import Testing
import UniformTypeIdentifiers
@testable import HerdrMacOS

@Suite("Local image attachment resources", .serialized)
@MainActor
struct ImageAttachmentTests {
    @Test func dropPreparationAndRemovalRoundTripThroughTheCoreSnapshot() async throws {
        let root = FileManager.default.temporaryDirectory.resolvingSymlinksInPath().appendingPathComponent("image-bridge-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let bitmap = try #require(NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 16, pixelsHigh: 12, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0))
        let source = root.appendingPathComponent("input.png")
        try #require(bitmap.representation(using: .png, properties: [:])).write(to: source)
        let bridge = CoreBridge(arguments: ["HerdrMacOS", "--verification-ui-fixture", "--verification-no-remote",
            "--workspace-root", root.path, "--state-path", root.appendingPathComponent("state.json").path])
        try await eventually { bridge.snapshot?.focusedPaneID != nil }
        // Status-only fixture agents need not own a layout leaf. Drop on the actual focused terminal.
        let pane = try #require(bridge.snapshot?.focusedPaneID)
        #expect(bridge.snapshot?.navigator.agents.contains { $0.paneID == pane && ["claude", "codex"].contains($0.agentKind) } == true)
        bridge.stageImages([source], paneID: pane)
        try await eventually { bridge.snapshot?.terminal.attachments?.first { $0.paneID == pane }?.items.first?.state == "awaiting_prompt" }
        let item = try #require(bridge.snapshot?.terminal.attachments?.first { $0.paneID == pane }?.items.first)
        let path = try #require(item.path)
        #expect(path != source.path)
        #expect(try Data(contentsOf: URL(fileURLWithPath: path)) == Data(contentsOf: source))
        bridge.attachmentAction("remove", paneID: pane, id: item.id)
        try await eventually { bridge.snapshot?.terminal.attachments?.first { $0.paneID == pane }?.items.isEmpty == true }
        try await eventually { !FileManager.default.fileExists(atPath: path) }
        #expect(FileManager.default.fileExists(atPath: source.path))
    }

    @Test func clipboardClassifiesTextMixedInvalidAndOrderedImagesWithoutTouchingTheGeneralBoard() throws {
        let board = NSPasteboard.withUniqueName()
        defer { board.releaseGlobally() }
        board.setString("Keep 한글 prompt", forType: .string)
        #expect(ImageAttachmentClipboard.capture(board) == nil)
        board.clearContents()
        let document = NSPasteboardItem()
        document.setString("file:///tmp/notes.txt", forType: .fileURL)
        board.writeObjects([document])
        #expect(ImageAttachmentClipboard.capture(board) == nil)
        board.clearContents()
        let first = NSPasteboardItem(), last = NSPasteboardItem()
        first.setString("file:///tmp/first.png", forType: .fileURL)
        last.setString("file:///tmp/last.jpg", forType: .fileURL)
        board.writeObjects([first, last])
        let images = try #require(ImageAttachmentClipboard.capture(board))
        #expect(images.map(\.name) == ["first.png", "last.jpg"])
        board.clearContents()
        let mixedImage = NSPasteboardItem(), text = NSPasteboardItem()
        mixedImage.setString("file:///tmp/first.png", forType: .fileURL)
        text.setString("Must not lose this text", forType: .string)
        board.writeObjects([mixedImage, text])
        let mixed = try #require(ImageAttachmentClipboard.capture(board))
        #expect(mixed.count == 1)
        guard case .failure(let reason) = mixed[0] else { Issue.record("Mixed paste must report an explicit failure"); return }
        #expect(reason.contains("nothing was sent"))
        board.clearContents()
        board.setData(Data("broken bitmap".utf8), forType: .tiff)
        guard case .bitmap(let data) = try #require(ImageAttachmentClipboard.capture(board)?.first) else { Issue.record("An advertised bitmap must not leak into text paste"); return }
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("invalid-clipboard-\(UUID())")
        defer { try? FileManager.default.removeItem(at: root) }
        #expect(throws: (any Error).self) { try LocalAttachmentImage.prepareBitmap(data, in: root) }
    }

    @Test func screenshotAndFilePasteShareLifetimeAndRepeatedIngressCannotResurrectRemovedImages() async throws {
        let root = FileManager.default.temporaryDirectory.resolvingSymlinksInPath().appendingPathComponent("paste-bridge-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let bitmap = try #require(NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 16, pixelsHigh: 12, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0))
        let source = root.appendingPathComponent("first.png")
        let original = try #require(bitmap.representation(using: .png, properties: [:]))
        try original.write(to: source)
        let screenshot = try #require(bitmap.tiffRepresentation)
        let board = NSPasteboard.withUniqueName()
        defer { board.releaseGlobally() }
        let fileItem = NSPasteboardItem(), bitmapItem = NSPasteboardItem()
        fileItem.setString(source.absoluteString, forType: .fileURL)
        bitmapItem.setData(screenshot, forType: .tiff)
        board.writeObjects([fileItem, bitmapItem])
        let bridge = CoreBridge(arguments: ["HerdrMacOS", "--verification-ui-fixture", "--verification-no-remote",
            "--workspace-root", root.path, "--state-path", root.appendingPathComponent("state.json").path])
        try await eventually { bridge.snapshot?.focusedPaneID != nil }
        let pane = try #require(bridge.snapshot?.focusedPaneID)
        let files = ImageAttachmentFiles()
        let request = files.request(try #require(ImageAttachmentClipboard.capture(board)))
        files.accept(request, paneID: pane, bridge: bridge)
        files.accept(request, paneID: pane, bridge: bridge)
        try await eventually {
            files.reconcile(bridge.snapshot?.terminal.attachments ?? [], bridge: bridge)
            return bridge.snapshot?.terminal.attachments?.first?.items.allSatisfy { $0.state == "awaiting_prompt" } == true
                && bridge.snapshot?.terminal.attachments?.first?.items.count == 2
        }
        let images = try #require(bridge.snapshot?.terminal.attachments?.first?.items)
        #expect(images.map(\.name) == ["first.png", "Clipboard image"])
        #expect(Set(images.map(\.id)).count == 2)
        let paths = try images.map { try #require($0.path) }
        #expect(try Data(contentsOf: URL(fileURLWithPath: paths[0])) == original)
        let normalized = try #require(NSBitmapImageRep(data: Data(contentsOf: URL(fileURLWithPath: paths[1]))))
        #expect(normalized.pixelsWide == 16 && normalized.pixelsHigh == 12)
        #expect(paths[1].hasSuffix(".png"))
        for image in images { bridge.attachmentAction("remove", paneID: pane, id: image.id) }
        try await eventually {
            files.reconcile(bridge.snapshot?.terminal.attachments ?? [], bridge: bridge)
            return bridge.snapshot?.terminal.attachments?.first?.items.isEmpty == true && paths.allSatisfy { !FileManager.default.fileExists(atPath: $0) }
        }
        files.accept(request, paneID: pane, bridge: bridge)
        try await Task.sleep(for: .milliseconds(30))
        #expect(bridge.snapshot?.terminal.attachments?.first?.items.isEmpty == true)
        #expect(try Data(contentsOf: source) == original)
    }

    @Test func squareGridAccumulatesRowsAtNarrowWidths() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("grid-fixture-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let model = ShellModel(core: CoreBridge(arguments: ["HerdrMacOS", "--verification-ui-fixture", "--verification-no-remote",
            "--workspace-root", root.path, "--state-path", root.appendingPathComponent("state.json").path]))
        let items = (1...4).map { CoreImageAttachment(id: "image-\($0)", name: "한글 image \($0).png", path: nil,
            state: "handoff_unconfirmed", message: "Check the native composer", provider: "codex") }
        let tooltips = HideTooltipController()
        let wide = NSHostingView(rootView: ImageAttachmentGrid(items: items, remove: { _ in }).environmentObject(model).environmentObject(tooltips).frame(width: 400))
        let narrow = NSHostingView(rootView: ImageAttachmentGrid(items: items, remove: { _ in }).environmentObject(model).environmentObject(tooltips).frame(width: 170))
        wide.frame = NSRect(x: 0, y: 0, width: 400, height: 300)
        narrow.frame = NSRect(x: 0, y: 0, width: 170, height: 300)
        let window = NSWindow(contentRect: narrow.frame, styleMask: [.titled], backing: .buffered, defer: false)
        window.contentView = narrow
        defer { window.contentView = nil; tooltips.stop() }
        wide.layoutSubtreeIfNeeded(); narrow.layoutSubtreeIfNeeded()
        try await Task.sleep(for: .milliseconds(30))
        #expect(wide.fittingSize.height == HideTheme.Attachment.thumbnailSize)
        #expect(narrow.fittingSize.height == HideTheme.Attachment.thumbnailSize * 2 + HideTheme.spacingSM)
        // AppKit does not expose the offscreen SwiftUI accessibility tree here.
        // Button presence/activation is a human candidate check, not a mocked tree.

    }

    private func eventually(_ predicate: () -> Bool) async throws {
        let deadline = ContinuousClock.now.advanced(by: .seconds(5))
        while !predicate(), ContinuousClock.now < deadline { try await Task.sleep(for: .milliseconds(10)) }
        #expect(predicate())
    }

    @Test func preparationCopiesOutsideCheckoutImageWithoutChangingOriginal() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("image-test-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: root) }
        let bitmap = try #require(NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 16, pixelsHigh: 12, bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0))
        for kind in [NSBitmapImageRep.FileType.png, .jpeg] {
            let bytes = try #require(bitmap.representation(using: kind, properties: [:]))
            let source = root.appendingPathComponent("한글 image.\(kind == .png ? "png" : "jpg")")
            try bytes.write(to: source)
            let destination = root.appendingPathComponent(UUID().uuidString)
            let prepared = try LocalAttachmentImage.prepare(source, in: destination)
            #expect(prepared != source)
            #expect(try Data(contentsOf: source) == bytes)
            #expect(try Data(contentsOf: prepared) == bytes)
            #expect(NSImage(contentsOf: destination.appendingPathComponent("preview.png")) != nil)
        }
        let invalid = root.appendingPathComponent("invalid.png")
        try Data("not an image".utf8).write(to: invalid)
        #expect(throws: (any Error).self) { try LocalAttachmentImage.prepare(invalid, in: root.appendingPathComponent("invalid-copy")) }
        let oversized = root.appendingPathComponent("oversized.png")
        FileManager.default.createFile(atPath: oversized.path, contents: nil)
        let handle = try FileHandle(forWritingTo: oversized)
        try handle.truncate(atOffset: UInt64(LocalAttachmentImage.maximumBytes + 1))
        try handle.close()
        #expect(throws: (any Error).self) { try LocalAttachmentImage.prepare(oversized, in: root.appendingPathComponent("large-copy")) }
        #expect(throws: (any Error).self) { try LocalAttachmentImage.prepare(root, in: root.appendingPathComponent("folder-copy")) }
    }
}
