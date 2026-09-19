import AppKit
import PDFKit
import SwiftUI

/// Reads a PDF the core classified (`document_kind == pdf`) and says why when
/// it cannot be shown. The core carries no bytes for a PDF, so this is the one
/// place the file is read; PDFKit decodes it and the reason for a refusal is
/// what the empty state shows rather than a blank page (D-03).
enum PDFDocumentPresentation {
    enum Outcome {
        case document(PDFDocument)
        case unavailable(String)
    }

    static func load(_ url: URL) -> Outcome {
        switch read(url) {
        case .failure(let error): return .unavailable(error.message)
        case .success(let data): return decode(data)
        }
    }

    struct ReadFailure: Error, Sendable {
        let message: String
    }

    /// The disk read, separable from decoding because `Data` can cross to a
    /// background task while a `PDFDocument` cannot.
    static func read(_ url: URL) -> Result<Data, ReadFailure> {
        do {
            return .success(try Data(contentsOf: url))
        } catch {
            return .failure(ReadFailure(message: "The file could not be read: \(error.localizedDescription)"))
        }
    }

    static func decode(_ data: Data) -> Outcome {
        guard let document = PDFDocument(data: data) else {
            return .unavailable("PDFKit could not decode this file.")
        }
        // A locked document opens, but every page is empty until a password is
        // given, and this viewer takes none; saying so beats a blank scroll.
        if document.isLocked {
            return .unavailable("The PDF is password-protected and cannot be shown.")
        }
        return .document(document)
    }
}

/// The PDF adapter of the file viewer: one read per file identity, then
/// PDFKit's own view or the stated reason it could not be used.
struct PDFDocumentSurface: View {
    let url: URL
    @State private var loaded: (url: URL, outcome: PDFDocumentPresentation.Outcome)?

    var body: some View {
        Group {
            switch loaded {
            case .some(let loaded) where loaded.url == url:
                switch loaded.outcome {
                case .document(let document):
                    PDFDocumentView(document: document)
                        .accessibilityIdentifier("pdf-document")
                case .unavailable(let reason):
                    HideEmptyState(
                        "PDF unavailable",
                        systemImage: "doc.text.magnifyingglass",
                        description: Text(reason)
                    )
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .padding(ShellMetrics.panelPadding)
                    .accessibilityIdentifier("pdf-unavailable")
                }
            default:
                ProgressView()
                    .controlSize(.small)
                    .tint(HideTheme.secondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .accessibilityIdentifier("pdf-loading")
            }
        }
        .task(id: url) {
            let target = url
            // The read is the whole file; a large PDF must not hold the main
            // thread while the toolbar and tab strip wait to draw. PDFKit's
            // document is not Sendable, so decoding stays here.
            let read = await Task.detached(priority: .userInitiated) {
                PDFDocumentPresentation.read(target)
            }.value
            guard !Task.isCancelled else { return }
            switch read {
            case .failure(let error): loaded = (target, .unavailable(error.message))
            case .success(let data): loaded = (target, PDFDocumentPresentation.decode(data))
            }
        }
    }
}

/// PDFKit's view configured the one way the file viewer uses it: continuous
/// vertical pages fitted to the width, text selectable, nothing editable.
struct PDFDocumentView: NSViewRepresentable {
    let document: PDFDocument

    func makeNSView(context: Context) -> PDFView {
        let view = PDFView()
        view.autoScales = true
        view.displayMode = .singlePageContinuous
        view.displayDirection = .vertical
        view.displaysPageBreaks = true
        view.backgroundColor = NSColor(HideTheme.background)
        view.document = document
        return view
    }

    func updateNSView(_ view: PDFView, context: Context) {
        // The surface hands over a new document only for a new file identity;
        // replacing the same one would reset the scroll position on every
        // snapshot the overlay redraws for.
        if view.document !== document {
            view.document = document
        }
    }
}
