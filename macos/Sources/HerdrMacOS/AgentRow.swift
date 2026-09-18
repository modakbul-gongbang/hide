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

    var badgeSize: CGFloat { self == .prominent ? 19 : HideTheme.compactAgentBadgeSize }
    var titleWeight: Font.Weight { self == .prominent ? .semibold : .regular }
    var titleColor: Color { self == .prominent ? HideTheme.primary : HideTheme.secondary }
    /// Compact marks align beneath the Workspace branch icon.
    var leadingPadding: CGFloat { self == .prominent ? 14 : HideTheme.compactAgentLeadingInset }
    var iconSpacing: CGFloat { self == .compact ? HideTheme.spacingXS : HideTheme.spacingSM }
    var trailingPadding: CGFloat { self == .prominent ? 14 : 9 }
    var verticalPadding: CGFloat { self == .prominent ? 7 : HideTheme.compactAgentRowVerticalPadding }
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
    /// The stable session name (PRD D-01).
    let title: String
    /// The sentence the core chose for this row's state: what the operator is
    /// asked for, or what the agent is doing. `nil` draws none (PRD D-06).
    let detail: String?
    /// Whether the status word stands in for a sentence the row does not
    /// have. Beside a sentence the mark already says it (PRD D-07).
    let statusWordVisible: Bool
    /// Whether the sentence is drawn bright. A row that still concerns the
    /// operator is; a working row's progress is subdued (PRD D-07).
    let emphasized: Bool
    /// A small note on its own line under the sentence: where the agent
    /// lives, or which pane it is.
    let qualifier: String?
    let elapsed: String
    /// Whether this row is somebody else's work. A delegated row is subdued
    /// so that scanning the sidebar for bright rows finds the operator's own
    /// (PRD B11, D-36).
    var delegated: Bool = false
    /// A descendant of this row has been waiting too long. The core writes
    /// the sentence; no view builds one out of a level.
    var stallNotice: String? = nil
    /// Why Hide cannot say what this pane's session has spawned, as the
    /// tooltip sentence and the mark's accessible name (PRD B21, D-60).
    var uninstrumentedReason: String? = nil
    var uninstrumentedLabel: String? = nil
}

extension AgentRowPresentation {
    /// A sidebar row. The session name is the title at both densities; a
    /// `prominent` row sits outside the project tree, so it names its home in
    /// the qualifier, where a nested row already has the heading above it.
    init(
        agent: SidebarAgent,
        density: AgentRowDensity,
        connected: Bool,
        children: CorePaneChildren? = nil
    ) {
        paneID = agent.paneID
        agentKind = agent.agentKind
        let status = AgentStatusPresentation(agent: agent, connected: connected)
        symbol = status.symbol
        statusLabel = status.label
        statusColor = status.color
        title = agent.identityLabel
        detail = agent.detail
        statusWordVisible = agent.statusWordVisible
        emphasized = agent.emphasized
        qualifier = density == .prominent ? agent.contextLabel : nil
        elapsed = agent.elapsed
        delegated = agent.delegated
        stallNotice = agent.stallNotice
        if let children, !children.instrumented {
            uninstrumentedReason = children.uninstrumentedReason
            uninstrumentedLabel = children.uninstrumentedLabel
        }
    }

    /// A pet dashboard row. The dashboard groups by project, so the project
    /// name is the heading and the agent's name is the title. A server that
    /// stopped answering is a state of the row, not of the agent, so it takes
    /// the disconnected mark and says so in its own word.
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
        title = row.identityLabel
        detail = row.detail
        statusWordVisible = row.statusWordVisible
        emphasized = row.emphasized
        qualifier = row.paneID
        elapsed = row.elapsed
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
    /// The agent tree draws its own toggle in the column this inset would
    /// otherwise fill, so it starts the row flush against it. Every other
    /// compact caller keeps the inset that lines the mark up with the
    /// checkout row above.
    var leadingInset: CGFloat?
    let action: () -> Void
    @Environment(\.hidePetAppearance) private var petAppearance

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
                        // Delegation is drawn as emphasis, not as a new color:
                        // bright is the operator's, subdued is somebody
                        // else's, and a stall lifts it back by clearing the
                        // flag in the core (PRD B11, D-36).
                        .foregroundStyle(presentation.delegated ? style.muted : style.titleColor)
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
                // The second line. The core chose the word or the sentence
                // from the row's state (PRD D-06); this only draws them. The
                // word is caption medium in the status colour, the sentence
                // caption regular in the row's emphasis, one line, cut at the
                // tail, with the whole of it on the tooltip (PRD D-07).
                HStack(alignment: .firstTextBaseline, spacing: style.contentSpacing) {
                    if presentation.statusWordVisible {
                        Text(presentation.statusLabel)
                            .hideFont(size: HideTheme.Typography.caption, weight: .medium)
                            .foregroundStyle(presentation.statusColor)
                    }
                    if let detail = presentation.detail {
                        Text(detail)
                            .hideFont(size: HideTheme.Typography.caption)
                            .foregroundStyle(
                                presentation.delegated
                                    ? style.muted
                                    : presentation.emphasized ? style.titleColor : style.secondary
                            )
                            .lineLimit(1)
                            .truncationMode(.tail)
                            .hideTooltip(detail)
                    }
                    if let reason = presentation.uninstrumentedReason {
                        // The second of the mark's three positions. A symbol
                        // and a name, never a color alone (PRD B21, B37).
                        Image(systemName: "questionmark.circle")
                            .hideFont(size: HideTheme.Typography.micro, weight: .semibold)
                            .foregroundStyle(style.muted)
                            .hideTooltip(reason)
                            .accessibilityLabel(presentation.uninstrumentedLabel ?? reason)
                    }
                }
                // The qualifier takes a line of its own: beside the sentence
                // it took the width the sentence needed, and a prominent row
                // is the one place the project context has nowhere else to sit.
                if let qualifier = presentation.qualifier {
                    Text(qualifier)
                        .hideFont(size: HideTheme.Typography.micro)
                        .foregroundStyle(style.muted)
                        .lineLimit(1)
                }
                if let notice = presentation.stallNotice {
                    // The safety net saying so in words, beside the mark that
                    // carries it, so the meaning is never the color's alone
                    // (PRD B17, B18, B37).
                    HStack(spacing: HideTheme.spacingXXS) {
                        Image(systemName: "clock.badge.exclamationmark")
                            .hideFont(size: HideTheme.Typography.micro, weight: .semibold)
                        Text(notice)
                            .hideFont(size: HideTheme.Typography.micro, weight: .medium)
                            .lineLimit(2)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    .foregroundStyle(HideTheme.warning)
                    .accessibilityLabel(notice)
                }
            }
        }
        .padding(.leading, leadingInset ?? density.leadingPadding)
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
        if let qualifier = presentation.qualifier { values.append(qualifier) }
        if !presentation.elapsed.isEmpty { values.append(presentation.elapsed) }
        if presentation.delegated { values.append("Delegated") }
        if let label = presentation.uninstrumentedLabel { values.append(label) }
        if let notice = presentation.stallNotice { values.append(notice) }
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
        // The status word is read whether or not it is drawn, so a row that
        // dropped it on screen still says its state (PRD D-10).
        .accessibilityLabel(
            [presentation.title, presentation.agentKind, presentation.statusLabel, presentation.detail]
                .compactMap { $0 }
                .joined(separator: ", ")
        )
        .accessibilityValue(rowAccessibilityValue)
    }
}
