import AppKit
import Darwin
import ImageIO
import UniformTypeIdentifiers

enum TerminalAttachmentInput: Sendable {
    case files([String])
    case image(Data)
    case failure(String)
}

/// macOS clipboard ingress only. The core owns transfer, ordering and outcomes.
enum TerminalFileDrop {
    static let maximumFileBytes = 20 * 1024 * 1024
    static let maximumPixels = 16 * 1024 * 1024
    static let maximumStagedFiles = 128
    static let maximumStagedBytes = 256 * 1024 * 1024
    static let stagingLifetime: TimeInterval = 24 * 60 * 60
    static func accepts(_ board: NSPasteboard) -> Bool {
        board.canReadObject(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true])
    }

    static func files(from board: NSPasteboard) -> TerminalAttachmentInput {
        guard let urls = board.readObjects(forClasses: [NSURL.self],
            options: [.urlReadingFileURLsOnly: true]) as? [URL], !urls.isEmpty, urls.count <= 8 else {
            return .failure("Choose between 1 and 8 regular files.")
        }
        guard urls.allSatisfy({ $0.isFileURL && !$0.path.isEmpty && !$0.path.unicodeScalars.contains(where: CharacterSet.controlCharacters.contains) }) else {
            return .failure("A file path contains unsupported control characters. Rename the file before pasting it.")
        }
        return .files(urls.map(\.path))
    }

    static func image(from board: NSPasteboard) -> TerminalAttachmentInput? {
        guard let type = board.availableType(from: [.png, .tiff]), let data = board.data(forType: type) else { return nil }
        guard data.count <= maximumFileBytes else { return .failure("The clipboard image exceeds the 20 MiB attachment limit.") }
        return .image(data)
    }

    /// Runs on the single preparation task, never the AppKit input thread.
    static func stageImage(_ data: Data, root: URL, requestID: String) throws -> URL {
        try Task.checkCancellation()
        guard UUID(uuidString: requestID) != nil else { throw failure("Invalid clipboard attachment identity.") }
        guard data.count <= maximumFileBytes,
              let source = CGImageSourceCreateWithData(data as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
              let width = properties[kCGImagePropertyPixelWidth] as? Int,
              let height = properties[kCGImagePropertyPixelHeight] as? Int,
              width > 0, height > 0, width <= maximumPixels, height <= maximumPixels / width else {
            throw failure("Clipboard image is unreadable or exceeds the 16-megapixel limit.")
        }
        guard let image = CGImageSourceCreateImageAtIndex(source, 0, [kCGImageSourceShouldCacheImmediately: true] as CFDictionary) else {
            throw failure("Could not decode the clipboard image.")
        }
        let png = NSMutableData()
        guard let destination = CGImageDestinationCreateWithData(png, UTType.png.identifier as CFString, 1, nil) else {
            throw failure("Could not prepare a PNG attachment.")
        }
        CGImageDestinationAddImage(destination, image, nil)
        guard CGImageDestinationFinalize(destination), png.length <= maximumFileBytes else {
            throw failure("The prepared PNG exceeds the 20 MiB attachment limit.")
        }
        try Task.checkCancellation()
        try preparePrivateRoot(root, incomingBytes: png.length)
        let url = root.appendingPathComponent("hide-\(requestID).png")
        let descriptor = open(url.path, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, S_IRUSR | S_IWUSR)
        guard descriptor >= 0 else { throw failure("Could not create a private clipboard attachment.") }
        let file = FileHandle(fileDescriptor: descriptor, closeOnDealloc: true)
        do {
            try file.write(contentsOf: png as Data)
            try file.close()
            try Task.checkCancellation()
            return url
        } catch {
            try? file.close()
            try? FileManager.default.removeItem(at: url)
            throw error
        }
    }

    private static func preparePrivateRoot(_ root: URL, incomingBytes: Int) throws {
        let manager = FileManager.default
        if mkdir(root.path, S_IRWXU) != 0 && errno != EEXIST {
            throw failure("Could not create the private clipboard directory.")
        }
        var status = stat()
        guard lstat(root.path, &status) == 0, status.st_mode & S_IFMT == S_IFDIR,
              status.st_uid == geteuid(), status.st_mode & 0o777 == 0o700 else {
            throw failure("Clipboard directory must be an owned private directory (0700), not a symbolic link.")
        }
        var enumerationFailed = false
        guard let enumerator = manager.enumerator(at: root, includingPropertiesForKeys: nil, options: [.skipsSubdirectoryDescendants], errorHandler: { _, _ in
            enumerationFailed = true
            return false
        }) else {
            throw failure("Could not inspect the clipboard directory.")
        }
        var visited = 0
        var retained = 0
        var bytes = incomingBytes
        for case let url as URL in enumerator {
            visited += 1
            guard visited <= maximumStagedFiles else { throw failure("Clipboard storage exceeds its 128-file limit. Remove old TerminalClipboard files and retry.") }
            let name = url.lastPathComponent
            guard name.hasPrefix("hide-"), name.hasSuffix(".png"), UUID(uuidString: String(name.dropFirst(5).dropLast(4))) != nil,
                  lstat(url.path, &status) == 0, status.st_mode & S_IFMT == S_IFREG,
                  status.st_uid == geteuid(), status.st_mode & 0o777 == 0o600 else {
                throw failure("Clipboard directory contains an unrecognized or unsafe entry. Inspect TerminalClipboard before retrying.")
            }
            if Date().timeIntervalSince1970 - TimeInterval(status.st_mtimespec.tv_sec) >= stagingLifetime {
                try manager.removeItem(at: url)
            } else {
                retained += 1
                guard status.st_size >= 0, status.st_size <= maximumStagedBytes else { throw failure("Clipboard storage exceeds 256 MiB.") }
                bytes += Int(status.st_size)
            }
        }
        guard !enumerationFailed else { throw failure("Could not completely inspect the clipboard directory. Check permissions and retry.") }
        guard retained < maximumStagedFiles, bytes <= maximumStagedBytes else {
            throw failure("Clipboard storage is full (128 files or 256 MiB). Remove old TerminalClipboard files and retry; files expire after 24 hours.")
        }
    }

    private static func failure(_ message: String) -> NSError {
        NSError(domain: "HideTerminalAttachment", code: 1,
            userInfo: [NSLocalizedDescriptionKey: message])
    }
}
