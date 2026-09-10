import AppKit

/// File drop is text ingress, not a second provider composer or attachment store.
enum TerminalFileDrop {
    static func accepts(_ board: NSPasteboard) -> Bool {
        board.canReadObject(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true])
    }

    static func input(from board: NSPasteboard, bracketedPaste: Bool) throws -> [UInt8] {
        guard let urls = board.readObjects(forClasses: [NSURL.self],
            options: [.urlReadingFileURLsOnly: true]) as? [URL], !urls.isEmpty else {
            throw failure("Drop local files into the terminal.")
        }
        let paths = try urls.map { url -> String in
            guard url.isFileURL, !url.path.isEmpty,
                  !url.path.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) }) else {
                throw failure("A file path contains unsupported control characters. Rename the file before dropping it.")
            }
            // Double quoting preserves spaces, Korean and apostrophes. Escape
            // expansion characters so a later shell Enter cannot execute a
            // command embedded in a filename.
            let escaped = url.path.reduce(into: "") { result, character in
                if "\\\"$`".contains(character) { result.append("\\") }
                result.append(character)
            }
            return "\"" + escaped + "\""
        }
        // TUIs recognize one image path per paste. Keep those frames separate,
        // but enqueue the complete drop once so keyboard input cannot interleave.
        let text = bracketedPaste
            ? paths.map { "\u{1b}[200~" + $0 + "\u{1b}[201~" }.joined(separator: " ")
            : paths.joined(separator: " ") + " "
        return Array(text.utf8)
    }

    private static func failure(_ message: String) -> NSError {
        NSError(domain: "HideTerminalFileDrop", code: 1,
            userInfo: [NSLocalizedDescriptionKey: message])
    }
}
