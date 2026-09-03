import SwiftUI

/// The one place an agent's mark color is decided.
///
/// It reads the axes the core publishes, never a state name, so the pet
/// dashboard, the sidebar rows, and anything added later cannot drift apart.
/// A row the operator has already read keeps its hue and loses its urgency.
enum AgentStatusStyle {
    static func color(
        demand: String,
        activity: String,
        emphasized: Bool,
        accent: Color
    ) -> Color {
        let base: Color = switch demand {
        case "error": HideTheme.danger
        case "question", "approval": HideTheme.warning
        default:
            switch activity {
            case "working": accent
            case "stopped": emphasized ? HideTheme.success : HideTheme.secondary
            default: HideTheme.secondary
            }
        }
        guard !emphasized, demand != "none" else { return base }
        return base.opacity(HideTheme.readStatusOpacity)
    }
}

/// How much room a row has. It decides sizes only; what the row says is the
/// presentation's business, so a row cannot mean one thing in the sidebar and
/// another in the dashboard.
enum AgentRowDensity {
    /// A top-level entry: the two sidebar views' sections and the pet
    /// dashboard.
    case prominent
    /// Nested under a checkout in the project tree, where the project name is
    /// already the heading above it.
    case compact

    var badgeSize: CGFloat { self == .prominent ? 19 : 16 }
    var titleSize: CGFloat { self == .prominent ? 11 : 11 }
    var titleWeight: Font.Weight { self == .prominent ? .semibold : .regular }
    var titleColor: Color { self == .prominent ? HideTheme.primary : HideTheme.secondary }
    /// The mark column sits at the indent, so the agent badge lands where it
    /// did before the mark existed: level with the section label at the top
    /// level, and one step in from the checkout label when nested.
    var leadingPadding: CGFloat { self == .prominent ? 14 : 23 }
    var trailingPadding: CGFloat { self == .prominent ? 14 : 9 }
    var verticalPadding: CGFloat { self == .prominent ? 7 : 5 }
}

/// Everything an agent row draws.
///
/// The mark, the status word, and the color are read out of the core's derived
/// values here and nowhere else, so Needs You, Done, the Agents view, the
/// project tree, and the pet dashboard cannot show one agent three ways (R5).
struct AgentRowPresentation: Equatable {
    let paneID: String
    let agentKind: String
    /// The core's mark: `?` `!` `×` `●` `○` `~`.
    let symbol: String
    /// The core's short human word. No view builds one out of an axis value.
    let statusLabel: String
    let statusColor: Color
    let title: String
    /// The second line, when the row has room for one.
    let detail: String?
    /// A small note beside the status word: where the agent lives, or which
    /// pane it is.
    let qualifier: String?
    let elapsed: String
    let ambient: CoreAmbientSignal?
}

extension AgentRowPresentation {
    /// A sidebar row. At `prominent` the project name is the title and the
    /// summary sits beneath it; nested under a checkout the summary is the
    /// title on its own, because the project name is already the heading.
    init(agent: SidebarAgent, density: AgentRowDensity, accent: Color) {
        paneID = agent.paneID
        agentKind = agent.agentKind
        symbol = agent.symbol
        statusLabel = agent.statusLabel
        statusColor = AgentStatusStyle.color(
            demand: agent.demand,
            activity: agent.activity,
            emphasized: agent.emphasized,
            accent: accent
        )
        title = density == .prominent ? agent.workspaceLabel : agent.summary
        detail = density == .prominent ? agent.summary : nil
        qualifier = density == .prominent ? agent.checkoutQualifier : nil
        elapsed = agent.elapsed
        ambient = agent.ambient
    }

    /// A pet dashboard row. The dashboard groups by project, so the project
    /// name is the heading and the summary is the title. A server that stopped
    /// answering is a state of the row, not of the agent, so it takes the
    /// warning hue and says so in its own word.
    init(row: PetDashboardRow, accent: Color) {
        paneID = row.paneID
        agentKind = row.agentKind
        symbol = row.symbol
        statusLabel = row.statusLabel
        statusColor = row.connection == "connected"
            ? AgentStatusStyle.color(
                demand: row.demand,
                activity: row.activity,
                emphasized: row.emphasized,
                accent: accent
            )
            : HideTheme.warning
        title = row.summary
        detail = nil
        qualifier = row.paneID
        elapsed = row.elapsed
        ambient = row.ambient
    }
}

/// The core's mark for one agent, in its own column so the marks line up down
/// the list whichever symbol each row carries.
private struct AgentStatusMark: View {
    let symbol: String
    let color: Color

    var body: some View {
        Text(symbol)
            .hideFont(size: 11, weight: .bold, design: .monospaced)
            .foregroundStyle(color)
            .frame(width: HideTheme.agentMarkWidth, alignment: .center)
            .accessibilityHidden(true)
    }
}

/// The one agent row. Needs You, Done, the Agents view, the checkout group in
/// the project tree, and the pet dashboard all draw this.
struct AgentRow<Accessory: View>: View {
    let presentation: AgentRowPresentation
    var density: AgentRowDensity = .prominent
    var isFocused: Bool = false
    var shortcutNumber: Int?
    let action: () -> Void
    @ViewBuilder var accessory: () -> Accessory

    private var ambientLabel: String? {
        guard let ambient = presentation.ambient else { return nil }
        var parts: [String] = []
        if ambient.subagentsActive > 0 { parts.append("\(ambient.subagentsActive) sub") }
        if ambient.backgroundRunning > 0 { parts.append("\(ambient.backgroundRunning) bg") }
        if ambient.backgroundFailed > 0 { parts.append("\(ambient.backgroundFailed) failed") }
        return parts.isEmpty ? nil : parts.joined(separator: "  ")
    }

    var body: some View {
        Button(action: action) {
            HStack(alignment: .top, spacing: HideTheme.spacingSM) {
                AgentStatusMark(symbol: presentation.symbol, color: presentation.statusColor)
                AgentBadge(
                    agentKind: presentation.agentKind,
                    stateColor: presentation.statusColor,
                    size: density.badgeSize
                )
                VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                    HStack(spacing: 5) {
                        Text(presentation.title)
                            .hideFont(size: density.titleSize, weight: density.titleWeight)
                            .foregroundStyle(density.titleColor)
                            .lineLimit(1)
                        Spacer(minLength: 0)
                        if let shortcutNumber {
                            SidebarBadge(label: "⌃\(shortcutNumber)", color: HideTheme.secondary)
                                .transition(.opacity)
                        }
                        Text(presentation.elapsed)
                            .hideFont(size: 9, design: .monospaced)
                            .foregroundStyle(HideTheme.muted)
                    }
                    if let detail = presentation.detail {
                        Text(detail)
                            .hideFont(size: 10)
                            .foregroundStyle(HideTheme.secondary)
                            .lineLimit(2)
                    }
                    HStack(spacing: 5) {
                        Text(presentation.statusLabel)
                            .hideFont(size: 9, weight: .medium)
                            .foregroundStyle(presentation.statusColor)
                        if let qualifier = presentation.qualifier {
                            Text(qualifier)
                                .hideFont(size: 9)
                                .foregroundStyle(HideTheme.muted)
                                .lineLimit(1)
                        }
                        if let ambientLabel {
                            Text(ambientLabel)
                                .hideFont(size: 9)
                                .foregroundStyle(HideTheme.muted)
                                .lineLimit(1)
                        }
                    }
                }
                accessory()
            }
            .padding(.leading, density.leadingPadding)
            .padding(.trailing, density.trailingPadding)
            .padding(.vertical, density.verticalPadding)
            .background(
                isFocused ? HideTheme.panel : .clear,
                in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(
            "\(presentation.title), \(presentation.agentKind), \(presentation.statusLabel)"
        )
        .accessibilityValue(isFocused ? "Selected" : "Not selected")
    }
}

extension AgentRow where Accessory == EmptyView {
    init(
        presentation: AgentRowPresentation,
        density: AgentRowDensity = .prominent,
        isFocused: Bool = false,
        shortcutNumber: Int? = nil,
        action: @escaping () -> Void
    ) {
        self.init(
            presentation: presentation,
            density: density,
            isFocused: isFocused,
            shortcutNumber: shortcutNumber,
            action: action,
            accessory: { EmptyView() }
        )
    }
}
