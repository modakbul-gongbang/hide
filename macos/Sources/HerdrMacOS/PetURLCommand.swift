import Foundation

/// The `herdr-ide://` commands that drive the pet's shared visibility state.
///
/// The retired pet app's own `herdr-pet://` scheme is deliberately not
/// registered, so a call to it must never resolve here (D-10).
enum PetURLCommand: String, CaseIterable, Equatable {
    case show
    case hide
    case toggle

    static let scheme = "herdr-ide"

    /// Parses tolerantly, because the same command is typed by hand, pasted
    /// from notes, and sent by other tools: `herdr-ide://toggle`,
    /// `herdr-ide://toggle/`, `herdr-ide:toggle`, and any capitalisation all
    /// mean the same thing. Anything else is rejected rather than guessed at.
    static func parse(_ url: URL) -> PetURLCommand? {
        guard url.scheme?.lowercased() == scheme else { return nil }
        let raw = [url.host, url.path.isEmpty ? nil : url.path, url.absoluteString]
            .lazy
            .compactMap { $0 }
            .map { candidate -> String in
                candidate
                    .replacingOccurrences(of: "\(scheme):", with: "", options: .caseInsensitive)
                    .trimmingCharacters(in: CharacterSet(charactersIn: "/ "))
                    .lowercased()
            }
            .first(where: { !$0.isEmpty })
        guard let raw else { return nil }
        return PetURLCommand(rawValue: raw)
    }
}
