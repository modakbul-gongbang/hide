import SwiftUI

/// The product mark for an agent kind, bundled rather than fetched so the
/// sidebar draws the same offline and never waits on a network round trip.
@MainActor
enum AgentMark {
    private static let cache = NSCache<NSString, NSImage>()

    static func image(for agentKind: String) -> NSImage? {
        let name: String
        switch agentKind {
        case "claude": name = "agent-claude"
        case "codex": name = "agent-codex"
        default: return nil
        }
        if let cached = cache.object(forKey: name as NSString) {
            return cached
        }
        guard let url = PackagedResourceBundle.app?.url(forResource: name, withExtension: "png"),
              let image = NSImage(contentsOf: url)
        else {
            return nil
        }
        cache.setObject(image, forKey: name as NSString)
        return image
    }

    /// The mark at a point size.
    ///
    /// A SwiftUI `Menu` label is hosted by AppKit, which draws an
    /// `Image(nsImage:)` at the NSImage's own size and ignores
    /// `.resizable().frame(...)`: the composer's agent chip drew the 128pt mark
    /// beside an 11pt title. Sizing the image itself is the instruction that
    /// surface honors. The copy is a second handle over the same bitmap, so the
    /// cache still holds one decode.
    static func image(for agentKind: String, side: CGFloat) -> NSImage? {
        guard let mark = image(for: agentKind), let sized = mark.copy() as? NSImage else {
            return nil
        }
        sized.size = NSSize(width: side, height: side)
        return sized
    }
}

/// An agent's mark, tinted by its state.
///
/// The mark says which tool is running. What the agent needs is the row's
/// status mark, drawn once in its own column beside this, so the badge carries
/// only the hue and never a second indicator of the same thing (design rule
/// 7). An agent kind with no bundled mark falls back to its initial instead of
/// drawing an empty square.
struct AgentBadge: View {
    let agentKind: String
    let stateColor: Color
    var size: CGFloat = 19

    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: size * 0.26)
                .fill(stateColor.opacity(0.13))
            if let mark = AgentMark.image(for: agentKind) {
                Image(nsImage: mark)
                    .resizable()
                    .interpolation(.high)
                    .aspectRatio(contentMode: .fit)
                    .padding(size * 0.16)
            } else {
                Text(agentKind.prefix(1).uppercased())
                    .hideFont(size: size * 0.53, weight: .bold, design: .rounded)
                    .foregroundStyle(stateColor)
            }
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }
}
