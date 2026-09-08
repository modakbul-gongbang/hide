import SwiftUI

/// The one place an agent's mark color is decided.
///
/// It reads the axes the core publishes, never a state name, so the pet
/// dashboard, the sidebar rows, and anything added later cannot drift apart.
/// A row the operator has already read keeps its hue and loses its urgency.
struct AgentStatusPresentation: Equatable {
    let symbol: String
    let label: String
    let color: Color

    init(demand: String, activity: String, emphasized: Bool, symbol: String, label: String, connected: Bool) {
        guard connected else {
            self.symbol = "⊘"
            self.label = "Disconnected"
            self.color = HideTheme.secondary
            return
        }
        self.symbol = symbol
        self.label = label
        let base: Color = switch demand {
        case "error": HideTheme.danger
        case "question", "approval": HideTheme.warning
        default:
            switch activity {
            case "working": HideTheme.agentWorking
            case "stopped": emphasized ? HideTheme.success : HideTheme.secondary
            default: HideTheme.secondary
            }
        }
        color = !emphasized && demand != "none" ? base.opacity(HideTheme.readStatusOpacity) : base
    }

    init(agent: SidebarAgent, connected: Bool) {
        self.init(demand: agent.demand, activity: agent.activity, emphasized: agent.emphasized,
                  symbol: agent.symbol, label: agent.statusLabel, connected: connected)
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
    var titleWeight: Font.Weight { self == .prominent ? .semibold : .regular }
    var titleColor: Color { self == .prominent ? HideTheme.primary : HideTheme.secondary }
    /// Compact marks align beneath the Workspace branch icon.
    var leadingPadding: CGFloat { self == .prominent ? 14 : HideTheme.compactAgentLeadingInset }
    var iconSpacing: CGFloat { self == .compact ? HideTheme.spacingXS : HideTheme.spacingSM }
    var trailingPadding: CGFloat { self == .prominent ? 14 : 9 }
    var verticalPadding: CGFloat { self == .prominent ? 7 : 5 }
}

/// A row's resolved visual policy. Surface wrappers choose it once so the
/// shared row only renders values and never needs to know which surface owns
/// it.
struct AgentRowStyle {
    let titleColor: Color
    let secondary: Color
    let muted: Color
    let contentSpacing: CGFloat

    static func shell(density: AgentRowDensity) -> AgentRowStyle {
        AgentRowStyle(
            titleColor: density.titleColor,
            secondary: HideTheme.secondary,
            muted: HideTheme.muted,
            contentSpacing: HideTheme.spacingXS
        )
    }
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
    init(agent: SidebarAgent, density: AgentRowDensity, connected: Bool) {
        paneID = agent.paneID
        agentKind = agent.agentKind
        let status = AgentStatusPresentation(agent: agent, connected: connected)
        symbol = status.symbol
        statusLabel = status.label
        statusColor = status.color
        title = density == .prominent ? agent.workspaceLabel : agent.summary
        detail = density == .prominent ? agent.summary : nil
        qualifier = density == .prominent ? agent.checkoutQualifier : nil
        elapsed = agent.elapsed
        ambient = agent.ambient
    }

    /// A Scratch row. The chat's own title is the line that identifies it -
    /// there is no project name to stand in for one - and the agent's summary
    /// sits beneath it.
    init(agent: SidebarAgent, title: String, connected: Bool) {
        paneID = agent.paneID
        agentKind = agent.agentKind
        let status = AgentStatusPresentation(agent: agent, connected: connected)
        symbol = status.symbol
        statusLabel = status.label
        statusColor = status.color
        self.title = title
        detail = agent.summary
        qualifier = nil
        elapsed = agent.elapsed
        ambient = agent.ambient
    }

    /// A pet dashboard row. The dashboard groups by project, so the project
    /// name is the heading and the summary is the title. A server that stopped
    /// answering is a state of the row, not of the agent, so it takes the
    /// disconnected mark and says so in its own word.
    init(row: PetDashboardRow) {
        paneID = row.paneID
        agentKind = row.agentKind
        let status = AgentStatusPresentation(
            demand: row.demand, activity: row.activity, emphasized: row.emphasized,
            symbol: row.symbol, label: row.statusLabel, connected: row.connection == "connected"
        )
        symbol = status.symbol
        statusLabel = status.label
        statusColor = status.color
        title = row.summary
        detail = nil
        qualifier = row.paneID
        elapsed = row.elapsed
        ambient = row.ambient
    }
}

/// The core's mark for one agent, in its own column so the marks line up down
/// the list whichever symbol each row carries.
struct AgentStatusMark: View {
    let symbol: String
    let color: Color

    var body: some View {
        Text(symbol)
            .hideFont(size: HideTheme.Typography.body, weight: .bold, design: .monospaced)
            .foregroundStyle(color)
            .frame(width: HideTheme.agentMarkWidth, alignment: .center)
            .accessibilityHidden(true)
    }
}

/// The one agent row. Needs You, Done, the Agents view, the checkout group in
/// the project tree, and the pet dashboard all draw this.
struct AgentRow: View {
    let presentation: AgentRowPresentation
    let style: AgentRowStyle
    var density: AgentRowDensity = .prominent
    var isFocused: Bool = false
    var shortcutNumber: Int?
    var shortcutVisible = true
    let action: () -> Void
    @Environment(\.hidePetAppearance) private var petAppearance

    /// Work the agent started that is still running. It is background noise
    /// next to the agent's own state, so it stays muted.
    private var ambientLabel: String? {
        guard let ambient = presentation.ambient else { return nil }
        var parts: [String] = []
        if ambient.subagentsActive > 0 { parts.append("\(ambient.subagentsActive) sub") }
        if ambient.backgroundRunning > 0 { parts.append("\(ambient.backgroundRunning) bg") }
        return parts.isEmpty ? nil : parts.joined(separator: "  ")
    }

    /// Work that failed, which is the one ambient signal worth a color. Every
    /// surface says it once, in the same place and the same hue; the pet
    /// dashboard used to repeat it in a trailing label of its own.
    private var failedLabel: String? {
        guard let failed = presentation.ambient?.backgroundFailed, failed > 0 else { return nil }
        return "\(failed) failed"
    }

    private var rowContent: some View {
        HStack(alignment: .top, spacing: density.iconSpacing) {
            AgentStatusMark(symbol: presentation.symbol, color: presentation.statusColor)
            AgentBadge(
                agentKind: presentation.agentKind,
                stateColor: presentation.statusColor,
                size: density.badgeSize
            )
            VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                HStack(spacing: style.contentSpacing) {
                    Text(presentation.title)
                        .hideFont(size: HideTheme.Typography.body, weight: density.titleWeight)
                        .foregroundStyle(style.titleColor)
                        .lineLimit(1)
                    Spacer(minLength: 0)
                    if let shortcutNumber {
                        HideKeycap(command: .agent(shortcutNumber))
                            .opacity(shortcutVisible ? 1 : 0)
                    }
                    Text(presentation.elapsed)
                        .hideFont(size: HideTheme.Typography.micro, design: .monospaced)
                        .foregroundStyle(style.muted)
                }
                if let detail = presentation.detail {
                    Text(detail)
                        .hideFont(size: HideTheme.Typography.caption)
                        .foregroundStyle(style.secondary)
                        .lineLimit(2)
                }
                HStack(spacing: style.contentSpacing) {
                    Text(presentation.statusLabel)
                        .hideFont(size: HideTheme.Typography.micro, weight: .medium)
                        .foregroundStyle(presentation.statusColor)
                    if let qualifier = presentation.qualifier {
                        Text(qualifier)
                            .hideFont(size: HideTheme.Typography.micro)
                            .foregroundStyle(style.muted)
                            .lineLimit(1)
                    }
                    if let ambientLabel {
                        Text(ambientLabel)
                            .hideFont(size: HideTheme.Typography.micro)
                            .foregroundStyle(style.muted)
                            .lineLimit(1)
                    }
                    if let failedLabel {
                        Text(failedLabel)
                            .hideFont(size: HideTheme.Typography.micro, weight: .medium)
                            .foregroundStyle(HideTheme.danger)
                            .lineLimit(1)
                    }
                }
            }
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

    private var rowAccessibilityValue: String {
        var values = [isFocused ? "Selected" : "Not selected"]
        if let detail = presentation.detail { values.append(detail) }
        if let qualifier = presentation.qualifier { values.append(qualifier) }
        if !presentation.elapsed.isEmpty { values.append(presentation.elapsed) }
        if let ambientLabel { values.append(ambientLabel) }
        if let failedLabel { values.append(failedLabel) }
        return values.joined(separator: ", ")
    }

    @ViewBuilder
    private var rowButton: some View {
        if petAppearance {
            Button(action: action) {
                rowContent
            }
            .buttonStyle(.plain)
        } else {
            Button(action: action) {
                rowContent
            }
            .buttonStyle(HideInteractiveButtonStyle())
        }
    }

    var body: some View {
        rowButton
        .accessibilityLabel(
            "\(presentation.title), \(presentation.agentKind), \(presentation.statusLabel)"
        )
        .accessibilityValue(rowAccessibilityValue)
    }
}
