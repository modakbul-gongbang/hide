import AppKit
import Foundation
import PDFKit
import SwiftUI
import Testing
@testable import HerdrMacOS

@Suite("PDF document view", .serialized)
@MainActor
struct PDFDocumentViewTests {
    private func fixtureDirectory() throws -> URL {
        let root = FileManager.default.temporaryDirectory
            .resolvingSymlinksInPath()
            .appendingPathComponent("pdf-view-\(UUID())")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        return root
    }

    /// A one-page PDF PDFKit itself wrote, so the fixture is a real document
    /// rather than bytes that happen to start with the signature.
    private func writeSinglePagePDF(to url: URL) throws {
        let image = NSImage(size: NSSize(width: 200, height: 120), flipped: false) { rect in
            NSColor.white.setFill()
            rect.fill()
            NSColor.black.setFill()
            NSRect(x: 20, y: 20, width: 60, height: 40).fill()
            return true
        }
        let page = try #require(PDFPage(image: image))
        let document = PDFDocument()
        document.insert(page, at: 0)
        #expect(document.write(to: url))
    }

    private func pdfView(in view: NSView) -> PDFView? {
        (view as? PDFView) ?? view.subviews.lazy.compactMap { pdfView(in: $0) }.first
    }

    /// B1, D-03: the surface hands PDFKit the document and lays it out at the
    /// size it was given, continuous and fitted, so the first frame is not a
    /// zero-sized blank the risk table warned about.
    @Test func aRealPDFIsDrawnByPDFKitAtTheHostedSize() async throws {
        let root = try fixtureDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("report.pdf")
        try writeSinglePagePDF(to: file)

        let host = NSHostingView(rootView: PDFDocumentSurface(url: file))
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 640, height: 480), styleMask: [.titled], backing: .buffered, defer: false)
        window.contentView = host
        defer { window.contentView = nil }
        host.layoutSubtreeIfNeeded()

        var view: PDFView?
        for _ in 0..<40 where view == nil {
            try await Task.sleep(for: .milliseconds(50))
            host.layoutSubtreeIfNeeded()
            view = pdfView(in: host)
        }
        let pdf = try #require(view)
        #expect(pdf.document?.pageCount == 1)
        #expect(pdf.autoScales)
        #expect(pdf.displayMode == .singlePageContinuous)
        #expect(pdf.displayDirection == .vertical)
        #expect(pdf.frame.width == 640)
        #expect(pdf.frame.height == 480)
    }

    /// B3: a file that is not a PDF PDFKit can read produces the stated
    /// reason, and a locked document is refused rather than drawn empty.
    @Test func aBrokenOrLockedFileStatesWhyItCannotBeShown() throws {
        let root = try fixtureDirectory()
        defer { try? FileManager.default.removeItem(at: root) }
        let broken = root.appendingPathComponent("broken.pdf")
        try Data("%PDF-1.7 and then nothing a parser can use".utf8).write(to: broken)
        guard case .unavailable(let reason) = PDFDocumentPresentation.load(broken) else {
            Issue.record("a truncated file decoded as a document")
            return
        }
        #expect(reason == "PDFKit could not decode this file.")

        let missing = root.appendingPathComponent("missing.pdf")
        guard case .unavailable(let missingReason) = PDFDocumentPresentation.load(missing) else {
            Issue.record("a missing file decoded as a document")
            return
        }
        #expect(missingReason.hasPrefix("The file could not be read: "))

        let locked = root.appendingPathComponent("locked.pdf")
        try writeSinglePagePDF(to: locked)
        let source = try #require(PDFDocument(url: locked))
        #expect(source.write(to: locked, withOptions: [.ownerPasswordOption: "owner", .userPasswordOption: "user"]))
        guard case .unavailable(let lockedReason) = PDFDocumentPresentation.load(locked) else {
            Issue.record("a password-protected file was accepted")
            return
        }
        #expect(lockedReason == "The PDF is password-protected and cannot be shown.")
    }
}
