import AppKit
import ImageIO
import UniformTypeIdentifiers

/// Owns temporary OS resources, not attachment/turn state. Core decides all lifecycle transitions.
@MainActor
final class ImageAttachmentFiles {
    private struct Pending {
        var source: ImageAttachmentSource?
        let paneID: String
        var started = false
    }
    struct Request {
        fileprivate let sequence: UInt64
        fileprivate let sources: [ImageAttachmentSource]
    }
    private var issued: UInt64 = 0
    private var accepted: UInt64 = 0
    private let identity = UUID().uuidString
    private var pending: [String: Pending] = [:]
    private let decoder = AttachmentImageDecoder()
    private let directory = FileManager.default.temporaryDirectory.appendingPathComponent("hide-attachments-\(UUID())", isDirectory: true)

    deinit { LocalAttachmentImage.removeResources(directory) }

    // Ingress is serialized on the main actor. A repeated/older request is a no-op,
    // including after cancellation. Two deliberate pastes create two new requests.
    // One sequence watermark bounds deduplication without retaining clipboard data.
    func request(_ sources: [ImageAttachmentSource]) -> Request {
        issued += 1
        return Request(sequence: issued, sources: sources)
    }

    func accept(_ request: Request, paneID: String, bridge: CoreBridge) {
        guard request.sequence > accepted else { return }
        accepted = request.sequence
        // A fifth intent reports the authoritative four-image pane limit.
        for (index, source) in request.sources.prefix(5).enumerated() {
            let id = "\(identity)-\(request.sequence)-\(index)"
            let canPrepare = pending.count < 20
            if canPrepare { pending[id] = Pending(source: source, paneID: paneID) }
            bridge.attachmentAction("stage", paneID: paneID, id: id, name: source.name)
            if !canPrepare {
                bridge.attachmentAction("prepared", paneID: paneID, id: id, error: "Image preparation capacity is full. Remove an attachment and try again.")
            }
        }
    }

    func stage(_ urls: [URL], paneID: String, bridge: CoreBridge) {
        accept(request(urls.map(ImageAttachmentSource.file)), paneID: paneID, bridge: bridge)
    }

    func paste(_ board: NSPasteboard, paneID: String, bridge: CoreBridge) -> Bool {
        guard let sources = ImageAttachmentClipboard.capture(board) else { return false }
        accept(request(sources), paneID: paneID, bridge: bridge)
        return true
    }

    func reconcile(_ shelves: [CoreAttachmentShelf], bridge: CoreBridge) {
        let live = Set(shelves.flatMap(\.items).map(\.id))
        let livePanes = Set(bridge.snapshot?.paneLayouts.flatMap { $0.root.paneIDs } ?? [])
        for (id, item) in pending where (item.started && !live.contains(id)) || (!livePanes.contains(item.paneID)) {
            pending.removeValue(forKey: id)
            let directory = directory
            Task.detached { LocalAttachmentImage.removeResources(directory.appendingPathComponent(id)) }
        }
        for shelf in shelves {
            for item in shelf.items where item.state == "loading" {
                guard var entry = pending[item.id], !entry.started, let source = entry.source else { continue }
                entry.started = true
                entry.source = nil
                pending[item.id] = entry
                let destination = directory.appendingPathComponent(item.id, isDirectory: true)
                let decoder = decoder
                Task { [weak self, weak bridge] in
                    let result = await decoder.prepare(source, in: destination)
                    guard let self, let bridge, self.pending[item.id] != nil else {
                        LocalAttachmentImage.removeResources(destination)
                        return
                    }
                    switch result {
                    case .success(let path): bridge.attachmentAction("prepared", paneID: shelf.paneID, id: item.id, path: path.path)
                    case .failure(let error): bridge.attachmentAction("prepared", paneID: shelf.paneID, id: item.id, error: error.localizedDescription)
                    }
                }
            }
            // Rejected stages have no image bytes to retain.
            for (id, entry) in pending where entry.paneID == shelf.paneID && !entry.started {
                if shelf.notice != nil || shelf.items.contains(where: { $0.id == id && $0.state == "failed" }) {
                    pending.removeValue(forKey: id)
                }
            }
        }
    }
}

private actor AttachmentImageDecoder {
    func prepare(_ source: ImageAttachmentSource, in destination: URL) -> Result<URL, Error> {
        Result {
            switch source {
            case .file(let url): try LocalAttachmentImage.prepare(url, in: destination)
            case .bitmap(let data): try LocalAttachmentImage.prepareBitmap(data, in: destination)
            case .failure(let reason): throw LocalAttachmentImage.Failure(errorDescription: reason)
            }
        }
    }
}

enum LocalAttachmentImage {
    static let maximumBytes = 20 * 1024 * 1024
    static let maximumPixels = 40_000_000

    struct Failure: LocalizedError { let errorDescription: String? }

    static func removeResources(_ directory: URL) {
        do { try FileManager.default.removeItem(at: directory) }
        catch let error as NSError where error.domain == NSCocoaErrorDomain && error.code == NSFileNoSuchFileError { }
        catch {
            let record: [String: String] = ["component": "image_attachment", "kind": "attachment.cleanup_failed",
                "attachment_id": directory.lastPathComponent, "message": "Private attachment resources could not be removed"]
            if let data = try? JSONSerialization.data(withJSONObject: record), let line = String(data: data, encoding: .utf8) {
                FileHandle.standardError.write(Data((line + "\n").utf8))
            }
        }
    }

    static func prepareBitmap(_ data: Data, in destination: URL) throws -> URL {
        guard !data.isEmpty, data.count <= maximumBytes,
              let source = CGImageSourceCreateWithData(data as CFData, nil),
              let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
              let width = properties[kCGImagePropertyPixelWidth] as? Int,
              let height = properties[kCGImagePropertyPixelHeight] as? Int,
              width > 0, height > 0, width <= maximumPixels / height,
              let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
            throw Failure(errorDescription: "Clipboard image must be decodable, at most 20 MiB and 40 million pixels.")
        }
        // AppKit screenshots commonly offer TIFF. Normalize only clipboard bytes;
        // dropped/file-URL originals continue through the byte-preserving path.
        let png = NSMutableData()
        guard let output = CGImageDestinationCreateWithData(png, UTType.png.identifier as CFString, 1, nil) else {
            throw Failure(errorDescription: "Clipboard image could not be converted to PNG.")
        }
        CGImageDestinationAddImage(output, image, nil)
        guard CGImageDestinationFinalize(output), png.length <= maximumBytes else {
            throw Failure(errorDescription: "Clipboard PNG could not be encoded within the 20 MiB limit.")
        }
        return try prepareData(png as Data, in: destination)
    }

    static func prepare(_ source: URL, in destination: URL) throws -> URL {
        guard source.isFileURL else { throw Failure(errorDescription: "Only local PNG and JPEG files can be attached.") }
        let access = source.startAccessingSecurityScopedResource()
        defer { if access { source.stopAccessingSecurityScopedResource() } }
        let values = try source.resourceValues(forKeys: [.isRegularFileKey, .fileSizeKey])
        guard values.isRegularFile == true else { throw Failure(errorDescription: "The dropped item is not a regular image file.") }
        guard let size = values.fileSize, size > 0, size <= maximumBytes else {
            throw Failure(errorDescription: "Images must be nonempty and no larger than 20 MiB.")
        }
        let handle = try FileHandle(forReadingFrom: source)
        defer { try? handle.close() }
        let data = try handle.read(upToCount: maximumBytes + 1) ?? Data()
        return try prepareData(data, in: destination)
    }

    private static func prepareData(_ data: Data, in destination: URL) throws -> URL {
        guard data.count <= maximumBytes,
              let imageSource = CGImageSourceCreateWithData(data as CFData, nil),
              let type = CGImageSourceGetType(imageSource) as String?,
              [UTType.png.identifier, UTType.jpeg.identifier].contains(type),
              let properties = CGImageSourceCopyPropertiesAtIndex(imageSource, 0, nil) as? [CFString: Any],
              let width = properties[kCGImagePropertyPixelWidth] as? Int,
              let height = properties[kCGImagePropertyPixelHeight] as? Int,
              width > 0, height > 0, width <= maximumPixels / height else {
            throw Failure(errorDescription: "Use a decodable PNG or JPEG with no more than 40 million pixels.")
        }
        guard let thumbnail = CGImageSourceCreateThumbnailAtIndex(imageSource, 0, [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceThumbnailMaxPixelSize: 192,
            kCGImageSourceCreateThumbnailWithTransform: true,
        ] as CFDictionary) else { throw Failure(errorDescription: "The image could not be decoded.") }
        try FileManager.default.createDirectory(at: destination, withIntermediateDirectories: true, attributes: [.posixPermissions: 0o700])
        let copy = destination.appendingPathComponent(type == UTType.png.identifier ? "image.png" : "image.jpg")
        try data.write(to: copy, options: .atomic)
        let preview = destination.appendingPathComponent("preview.png")
        guard let output = CGImageDestinationCreateWithURL(preview as CFURL, UTType.png.identifier as CFString, 1, nil) else {
            throw Failure(errorDescription: "The image thumbnail could not be created.")
        }
        CGImageDestinationAddImage(output, thumbnail, nil)
        guard CGImageDestinationFinalize(output) else { throw Failure(errorDescription: "The image thumbnail could not be saved.") }
        return copy
    }
}
