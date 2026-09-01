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
}

/// An agent's mark with its state on it.
///
/// The mark alone says which tool is running but nothing about whether it
/// needs the user, so the state stays encoded as a dot rather than as a word
/// (design rule 7). An agent kind with no bundled mark falls back to its
/// initial instead of drawing an empty square.
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
        .overlay(alignment: .bottomTrailing) {
            Circle()
                .fill(stateColor)
                .frame(width: size * 0.36, height: size * 0.36)
                .overlay(
                    Circle().stroke(HideTheme.sidebar, lineWidth: size * 0.08)
                )
                .offset(x: size * 0.11, y: size * 0.11)
        }
        .accessibilityHidden(true)
    }
}
