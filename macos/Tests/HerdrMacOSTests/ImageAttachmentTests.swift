import AppKit
import ImageIO
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
        try await eventually { bridge.snapshot?.terminal.attachments?.first { $0.paneID == pane }?.items.first?.state == "ready" }
        let item = try #require(bridge.snapshot?.terminal.attachments?.first { $0.paneID == pane }?.items.first)
        let path = try #require(item.path)
        #expect(path != source.path)
        #expect(try Data(contentsOf: URL(fileURLWithPath: path)) == Data(contentsOf: source))
        bridge.attachmentAction("remove", paneID: pane, id: item.id)
        try await eventually { bridge.snapshot?.terminal.attachments?.first { $0.paneID == pane }?.items.isEmpty == true }
        try await eventually { !FileManager.default.fileExists(atPath: path) }
        #expect(FileManager.default.fileExists(atPath: source.path))
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
