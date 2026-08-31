import Foundation

enum TerminalLinkReference: Equatable {
    case external(URL)
    case file(path: String, line: Int?, column: Int?)
    case invalid(String)
}

enum TerminalFileResolution: Equatable {
    case file(URL)
    case failure(String)
}

/// Centralizes terminal link parsing and local Workbench path resolution.
///
/// SwiftTerm deliberately recognizes a broad, Ghostty-derived class of URL
/// and path shapes. Hide keeps the product policy here instead of growing a
/// second collection of per-extension or per-command regular expressions.
enum TerminalLinkResolver {
    static func parse(_ rawValue: String) -> TerminalLinkReference {
        let value = stripBalancedBoundaryQuotes(
            rawValue.trimmingCharacters(in: .whitespacesAndNewlines)
        )
        guard !value.isEmpty else {
            return .invalid("The terminal link is empty.")
        }

        if let url = URL(string: value),
           url.scheme?.lowercased() == "file" {
            guard url.isFileURL, !url.path.isEmpty else {
                return .invalid("The terminal file URL has no usable path.")
            }
            let location = splitSourceLocation(url.path)
            return .file(path: location.path, line: location.line, column: location.column)
        }

        if let url = explicitExternalURL(in: value) {
            if let scheme = url.scheme?.lowercased(),
               ["http", "https"].contains(scheme),
               url.host == nil {
                return .invalid("The terminal URL has no host and cannot be opened.")
            }
            return .external(url)
        }

        let location = splitSourceLocation(value)
        guard !location.path.isEmpty else {
            return .invalid("The terminal file path is empty.")
        }
        return .file(path: location.path, line: location.line, column: location.column)
    }

    private static func explicitExternalURL(in value: String) -> URL? {
        guard let url = URL(string: value),
              let scheme = url.scheme?.lowercased()
        else { return nil }
        let schemesWithoutSlashes: Set<String> = [
            "ftp", "gemini", "git", "gopher", "ipfs", "ipns", "mailto",
            "magnet", "news", "ssh", "tel",
        ]
        if value.contains("://") || schemesWithoutSlashes.contains(scheme) {
            return url
        }
        return nil
    }

    static func resolveLocalFile(
        path: String,
        paneCWD: String,
        checkoutRoot: URL?
    ) -> TerminalFileResolution {
        guard let checkoutRoot else {
            return .failure("Hide cannot open this path because the selected checkout has no local root.")
        }

        let root = checkoutRoot
            .standardizedFileURL
            .resolvingSymlinksInPath()
        let expandedPath = NSString(string: path).expandingTildeInPath
        let candidates: [URL]
        if expandedPath.hasPrefix("/") {
            candidates = [URL(fileURLWithPath: expandedPath)]
        } else {
            var bases: [URL] = []
            if !paneCWD.isEmpty {
                let cwd = URL(fileURLWithPath: paneCWD, isDirectory: true)
                    .standardizedFileURL
                    .resolvingSymlinksInPath()
                if contains(cwd, within: root) {
                    bases.append(cwd)
                }
            }
            if !bases.contains(root) {
                bases.append(root)
            }
            candidates = bases.map { $0.appendingPathComponent(expandedPath) }
        }

        var firstDirectory: URL?
        for candidate in candidates {
            let resolved = candidate
                .standardizedFileURL
                .resolvingSymlinksInPath()
            guard contains(resolved, within: root) else { continue }

            var isDirectory: ObjCBool = false
            guard FileManager.default.fileExists(
                atPath: resolved.path,
                isDirectory: &isDirectory
            ) else { continue }
            if isDirectory.boolValue {
                firstDirectory = firstDirectory ?? resolved
                continue
            }
            guard FileManager.default.isReadableFile(atPath: resolved.path) else {
                return .failure("Hide found \(resolved.lastPathComponent), but it is not readable.")
            }
            guard (try? resolved.resourceValues(forKeys: [.isRegularFileKey]).isRegularFile) == true else {
                return .failure("Hide found \(resolved.lastPathComponent), but it is not a regular file.")
            }
            return .file(resolved)
        }

        if let firstDirectory {
            return .failure("\(firstDirectory.lastPathComponent) is a folder. Terminal links currently open files in Workbench.")
        }

        let displayPath = path.count > 120 ? String(path.prefix(117)) + "..." : path
        return .failure("Hide could not find \(displayPath) inside the selected checkout.")
    }

    private static func stripBalancedBoundaryQuotes(_ value: String) -> String {
        guard value.count >= 2,
              let first = value.first,
              let last = value.last,
              (first == last),
              first == "\"" || first == "'" || first == "`"
        else { return value }
        return String(value.dropFirst().dropLast())
    }

    private static func splitSourceLocation(
        _ value: String
    ) -> (path: String, line: Int?, column: Int?) {
        var pieces = value.split(separator: ":", omittingEmptySubsequences: false)
        guard pieces.count > 1,
              let trailingNumber = positiveInteger(pieces.last)
        else { return (value, nil, nil) }
        pieces.removeLast()

        if pieces.count > 1,
           let line = positiveInteger(pieces.last) {
            pieces.removeLast()
            return (pieces.joined(separator: ":"), line, trailingNumber)
        }
        return (pieces.joined(separator: ":"), trailingNumber, nil)
    }

    private static func positiveInteger(_ value: Substring?) -> Int? {
        guard let value,
              !value.isEmpty,
              value.allSatisfy(\.isNumber),
              let number = Int(value),
              number > 0
        else { return nil }
        return number
    }

    private static func contains(_ candidate: URL, within root: URL) -> Bool {
        let rootComponents = root.pathComponents
        let candidateComponents = candidate.pathComponents
        guard candidateComponents.count >= rootComponents.count else { return false }
        return Array(candidateComponents.prefix(rootComponents.count)) == rootComponents
    }
}
