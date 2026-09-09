import AppKit
import UniformTypeIdentifiers

/// Drag and clipboard adapters feed the same validation/lifetime service.
/// A paste is an image intent only when the OS advertises image content.
/// Plain text/non-image paste is left entirely to the existing terminal path.
enum ImageAttachmentSource {
    case file(URL)
    case bitmap(Data)
    case failure(String)

    var name: String {
        switch self {
        case .file(let url): url.lastPathComponent
        case .bitmap: "Clipboard image"
        case .failure: "Clipboard attachment"
        }
    }
}

enum ImageAttachmentClipboard {
    static func capture(_ board: NSPasteboard) -> [ImageAttachmentSource]? {
        let items = board.pasteboardItems ?? []
        var sources: [ImageAttachmentSource] = []
        var hasOtherItem = false
        for item in items.prefix(5) {
            if let value = item.string(forType: .fileURL), let url = URL(string: value), url.isFileURL {
                guard UTType(filenameExtension: url.pathExtension)?.conforms(to: .image) == true else {
                    hasOtherItem = true
                    continue
                }
                sources.append(.file(url))
            } else if let type = item.types.first(where: { UTType($0.rawValue)?.conforms(to: .image) == true }) {
                // Prefer PNG if the OS offers alternative representations of one image.
                let selected: NSPasteboard.PasteboardType = item.types.contains(.png) ? .png : type
                if let data = item.data(forType: selected), !data.isEmpty, data.count <= LocalAttachmentImage.maximumBytes {
                    sources.append(.bitmap(data))
                } else {
                    sources.append(.failure("Clipboard image is empty, unreadable or larger than 20 MiB."))
                }
            } else {
                hasOtherItem = true
            }
        }
        guard !sources.isEmpty else { return nil }
        // Different clipboard items must not silently lose their text/file content.
        // Alternate text representations on an image item describe that same item.
        if hasOtherItem {
            return [.failure("Mixed image and non-image clipboard items cannot be pasted together. Copy images or text separately; nothing was sent to the terminal.")]
        }
        return sources
    }
}
