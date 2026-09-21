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
    /// The rolling task title or workspace fallback (PRD D-01).
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
    /// lives, or which pane it is. In the tree it is the worktree a child
    /// runs in when that differs from its parent's (PRD B11).
    let qualifier: String?
    /// The glyph before the qualifier, when the qualifier names a worktree.
    var qualifierSystemImage: String? = nil
    let elapsed: String
    /// Whether this row is somebody else's work. A delegated row is subdued
    /// so that scanning the sidebar for bright rows finds the operator's own
    /// (PRD B11, D-36).
    var delegated: Bool = false
    /// What the row's live descendants are doing, drawn as a badge before
    /// the elapsed time while they are folded away. A raised row never
    /// unfolds, so it always wears the badge (PRD B3, B12, D-04).
    var descendantBadge: CoreDescendantCounts? = nil
    /// What the row's leading role column draws (PRD B9, B10, B12).
    var role: AgentRowRole = .blank
    /// Why Hide cannot say what this pane's session has spawned, as the
    /// tooltip sentence and the mark's accessible name (PRD B21, D-60).
    var uninstrumentedReason: String? = nil
    var uninstrumentedLabel: String? = nil
}

extension AgentRowPresentation {
    /// A sidebar row. The canonical identity is the title at both densities;
    /// a `prominent` row sits outside the project tree, so it names its home
    /// in the qualifier, where a nested row already has the heading above it.
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
        if density == .prominent {
            qualifier = agent.contextLabel
        } else if agent.lineageDepth > 0, let worktree = agent.lineageWorktreeBadge {
            // Under its parent, a child running elsewhere says where; under
            // its own worktree the heading already does (PRD B11).
            qualifier = worktree
            qualifierSystemImage = HideTheme.GitIcon.worktree
        } else {
            qualifier = nil
        }
        elapsed = agent.elapsed
        delegated = agent.delegated
        role = AgentRowRole(agent: agent, density: density)
        // Opened descendants carry their own marks, so the badge leaves with
        // the fold; a raised row has no fold to open (PRD D-04).
        let folded = density == .prominent || agent.lineageCollapsed
        descendantBadge = folded && !agent.descendantCounts.isEmpty ? agent.descendantCounts : nil
        if let children, !children.instrumented {
            uninstrumentedReason = children.uninstrumentedReason
            uninstrumentedLabel = children.uninstrumentedLabel
        }
    }

    /// A pet dashboard row. The dashboard groups by project, so the project
    /// name is the heading and the agent's canonical title is the row title. A server that
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

/// What the leading role column of an agent row says: that the row has
/// descendants to open, that it is somebody else's (with the way back to
/// them), or nothing.
///
/// One glyph, never a sentence (design rule 7). In the tree the disclosure
/// is the toggle; on a raised row it is an indicator, because a raised row
/// never unfolds (PRD B9, B10, B12).
enum AgentRowRole: Equatable {
    case blank
    case disclosure(collapsed: Bool, interactive: Bool)
    case returnToParent(paneID: String)

    init(agent: SidebarAgent, density: AgentRowDensity) {
        // A child drawn away from its parent - raised, listed flat, or under
        // its own worktree where the tree rebased it to depth zero - says
        // whose it is. Under its parent the tree line already says so.
        if let parent = agent.lineageParentPaneID, density == .prominent || agent.lineageDepth == 0 {
            self = .returnToParent(paneID: parent)
        } else if !agent.lineageChildPaneIDs.isEmpty {
            self = .disclosure(collapsed: agent.lineageCollapsed, interactive: density == .compact)
        } else {
            self = .blank
        }
    }
}

/// One state the descendant badge draws: the core's mark, its color, and
/// how many descendants are in it. Order is the badge's own, worst first.
struct DescendantBadgeCell: Equatable, Identifiable {
    let id: String
    let symbol: String
    let label: String
    let color: Color
    let count: Int

    /// The non-zero cells in the order the badge draws them (PRD D-08).
    static func cells(_ counts: CoreDescendantCounts) -> [DescendantBadgeCell] {
        [
            DescendantBadgeCell(id: "error", symbol: "\u{d7}", label: "error", color: HideTheme.danger, count: counts.error),
            DescendantBadgeCell(id: "approval", symbol: "!", label: "approval", color: HideTheme.warning, count: counts.approval),
            DescendantBadgeCell(id: "question", symbol: "?", label: "question", color: HideTheme.warning, count: counts.question),
            DescendantBadgeCell(id: "working", symbol: "\u{25cf}", label: "working", color: HideTheme.agentWorking, count: counts.working),
            DescendantBadgeCell(id: "done", symbol: "✓", label: "done", color: HideTheme.success, count: counts.done),
        ]
        .filter { $0.count > 0 }
    }

    /// The badge read aloud: "Descendants: 1 question, 2 working".
    static func accessibilityLabel(_ counts: CoreDescendantCounts) -> String {
        "Descendants: " + cells(counts).map { "\($0.count) \($0.label)" }.joined(separator: ", ")
    }
}

/// The pill a row wears while its descendants are folded away: one mark and
/// count per state, reusing the row marks so the badge and the rows it
/// stands for cannot disagree (PRD B3, D-02, D-08).
struct AgentDescendantBadge: View {
    let counts: CoreDescendantCounts

    var body: some View {
        HStack(spacing: HideTheme.spacingXS) {
            ForEach(DescendantBadgeCell.cells(counts)) { cell in
                HStack(spacing: HideTheme.spacingXXS) {
                    Text(cell.symbol)
                        .hideFont(size: HideTheme.Typography.caption, weight: .bold, design: .monospaced)
                        .foregroundStyle(cell.color)
                    Text("\(cell.count)")
                        .hideFont(size: HideTheme.Typography.micro, design: .monospaced)
                        .foregroundStyle(HideTheme.secondary)
                }
            }
        }
        .padding(.horizontal, HideTheme.spacingXS)
        .frame(height: HideTheme.badgeHeight)
        .background(HideTheme.elevated, in: Capsule())
        .fixedSize()
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(DescendantBadgeCell.accessibilityLabel(counts))
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
                        // else's (PRD B11, D-36).
                        .foregroundStyle(presentation.delegated ? style.muted : style.titleColor)
                        .lineLimit(1)
                    Spacer(minLength: 0)
                    if let shortcutNumber {
                        HideKeycap(command: .agent(shortcutNumber))
                            .opacity(shortcutVisible ? 1 : 0)
                    }
                    if let counts = presentation.descendantBadge {
                        AgentDescendantBadge(counts: counts)
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
                    HStack(spacing: HideTheme.spacingXXS) {
                        if let image = presentation.qualifierSystemImage {
                            Image(systemName: image)
                                .hideFont(size: HideTheme.Typography.micro, weight: .semibold)
                        }
                        Text(qualifier)
                            .hideFont(size: HideTheme.Typography.micro)
                            .lineLimit(1)
                    }
                    .foregroundStyle(style.muted)
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
        if let counts = presentation.descendantBadge {
            values.append(DescendantBadgeCell.accessibilityLabel(counts))
        }
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
