import AppKit
import CoreText
import Foundation
import SwiftUI

enum HideTheme {
    static let modifierSymbols: [PaneShortcut.Modifier: String] = [.control: "⌃", .option: "⌥", .shift: "⇧", .command: "⌘"]
    @MainActor private static let inter: CGFont = {
        guard let url = Bundle.module.url(forResource: "InterVariable", withExtension: "ttf"),
              let provider = CGDataProvider(url: url as CFURL), let font = CGFont(provider) else {
            preconditionFailure("The bundled Inter font is missing or unreadable")
        }
        return font
    }()

    @MainActor static func font(size: CGFloat, weight: SwiftUI.Font.Weight, design: SwiftUI.Font.Design) -> SwiftUI.Font {
        if design == .monospaced { return .system(size: size, weight: weight, design: .monospaced) }
        return SwiftUI.Font(nativeFont(size: size, weight: weight))
    }

    @MainActor static func nativeFont(size: CGFloat, weight: SwiftUI.Font.Weight = .regular) -> NSFont {
        let numericWeight: Double = switch weight {
        case .ultraLight: 100
        case .thin: 200
        case .light: 300
        case .medium: 500
        case .semibold: 600
        case .bold: 700
        case .heavy: 800
        case .black: 900
        default: 400
        }
        let descriptor = CTFontDescriptorCreateWithAttributes([
            kCTFontFeatureSettingsAttribute: [[kCTFontFeatureTypeIdentifierKey: kStylisticAlternativesType,
                kCTFontFeatureSelectorIdentifierKey: kStylisticAltThreeOnSelector]],
            kCTFontVariationAttribute: [NSNumber(value: 0x77676874): numericWeight],
        ] as CFDictionary)
        return CTFontCreateWithGraphicsFont(inter, size, nil, descriptor) as NSFont
    }
    /// The excluded pet dashboard retains its existing appearance (N2).
    /// These are active surface tokens, not a second copy of its components.
    enum PetDashboard {
        static let panel = Color(red: 0.070, green: 0.080, blue: 0.098)
        static let elevated = Color(red: 0.105, green: 0.118, blue: 0.142)
        static let divider = Color.white.opacity(0.09)
        static let primary = Color.white.opacity(0.92)
        static let secondary = Color.white.opacity(0.52)
        static let muted = Color.white.opacity(0.32)
        static let accent = Color(red: 0.725, green: 1.0, blue: 0.40)
        static let countLabelSize: CGFloat = 8
        static let countValueSize: CGFloat = 18
        static let contentInset: CGFloat = 18
        static let countBottomInset: CGFloat = 14
        static let itemInset: CGFloat = 10
        static let countGap: CGFloat = 3
        static let rowGap: CGFloat = 5
        static let cardRadius: CGFloat = 7
        static let cardOpacity: Double = 0.75
        static let badgeOpacity: Double = 0.13
    }
    enum AgentMark {
        static let cornerRatio: CGFloat = 0.26
        static let insetRatio: CGFloat = 0.16
        static let fontRatio: CGFloat = 0.53
    }

    enum Opacity {
        static let subtleFill: Double = 0.08
        static let selectedFill: Double = 0.12
        static let emphasisFill: Double = 0.16
        static let disabled: Double = 0.45
        static let dimmed: Double = 0.5
        static let secondary: Double = 0.72
    }

    enum Typography {
        static let micro: CGFloat = 9
        static let caption: CGFloat = 10
        static let body: CGFloat = 11
        static let subhead: CGFloat = 12
        static let title: CGFloat = 13
        static let headline: CGFloat = 17
        static let display: CGFloat = 30
    }
    enum Editor {
        static let contentInset = spacingMD
        static let lineNumberColumnWidth: CGFloat = 44
        static let diffLineNumberColumnWidth: CGFloat = 40
        static let minimumContentWidth: CGFloat = 720
        static let documentWidth: CGFloat = 720
        static let documentFontSize: CGFloat = 15
        static let documentLineSpacing: CGFloat = 5
    }
    enum Hint {
        static let delay: TimeInterval = 0.150
        static let tooltipDelay: TimeInterval = 0.400
        static let fadeDuration: TimeInterval = 0.120
        static let keycapHeight: CGFloat = 18
        static let gap: CGFloat = 4
        static let windowInset: CGFloat = 8
        static let horizontalPadding = spacingXS
        static let tooltipMaxWidth: CGFloat = 360
    }
    static let balloon = color(for: "#34373B")
    static let background = color(for: "#101112")
    static let sidebar = color(for: "#171819")
    static let panel = color(for: "#1D1F21")
    static let elevated = color(for: "#27292C")
    static let divider = color(for: "#34363A")
    static let primary = color(for: "#F4F4F6")
    static let secondary = color(for: "#A4A5A8")
    static let muted = color(for: "#92959A")
    /// File-row icons are category illustration, so their neutrals sit on this
    /// system's own neutral ladder rather than on Seti's. Seti's own neutral,
    /// `#6D8086`, is a dark slate that reads as a speck against the panel at
    /// 12px - which is what made `.gitignore` and `Cargo.toml` look unrendered.
    /// These two are DESIGN.md's `mute` and `charcoal` steps.
    static let fileIconNeutralHex = "#9C9C9D"
    static let fileIconDocumentHex = "#D3D3D4"
    /// Monospaced content sizes at a pane's default scale. The per-pane zoom
    /// chords multiply these; they are tokens rather than call-site literals so
    /// the two content surfaces cannot drift apart.
    static let terminalBaseFontSize: CGFloat = 14
    static let editorBaseFontSize: CGFloat = 12
    static let accent = Color(red: 211.0 / 255, green: 211.0 / 255, blue: 212.0 / 255)
    static let danger = Color(red: 1.0, green: 0.35, blue: 0.36)
    static let warning = Color(red: 1.0, green: 0.72, blue: 0.28)
    static let agentWorking = Color(red: 0.38, green: 0.65, blue: 1.0)
    static let success = Color(red: 0.37, green: 0.90, blue: 0.62)
    /// How far a status mark is dimmed once the operator has read it. The mark
    /// keeps its shape and its hue so the row still says what it is; only its
    /// urgency drops (DESIGN.md, R5).
    static let readStatusOpacity: Double = 0.55
    /// The column an agent's status mark sits in. Fixed, so `?` `!` `×` and
    /// `~` line up down a list instead of shifting each row's text.
    static let agentMarkWidth: CGFloat = 12
    /// Diff line tints, named here so the changes view and any later diff
    /// surface cannot drift apart. They lean on the semantic pair above
    /// rather than introducing hues of their own.
    static let diffAdded = success
    static let diffRemoved = danger
    static let diffAddedBackground = success.opacity(0.10)
    static let diffRemovedBackground = danger.opacity(0.10)
    /// The fill behind a search match that is not the current one. Content
    /// emphasis rather than chrome, so it is allowed a saturated tint.
    static let searchMatchHighlight = accent.opacity(0.24)

    /// The same tokens as `NSColor`, for the AppKit views the shell hosts.
    /// They are converted here rather than at each call site so a view and its
    /// SwiftUI neighbours cannot end up on different values.
    enum Native {
        static let background = NSColor(HideTheme.background)
        static let panel = NSColor(HideTheme.panel)
        static let elevated = NSColor(HideTheme.elevated)
        static let divider = NSColor(HideTheme.divider)
        static let primary = NSColor(HideTheme.primary)
        static let secondary = NSColor(HideTheme.secondary)
        static let muted = NSColor(HideTheme.muted)
        static let danger = NSColor(HideTheme.danger)
        static let searchMatchHighlight = NSColor(HideTheme.searchMatchHighlight)
    }

    static let spacingNone: CGFloat = 0
    static let spacingXXS: CGFloat = 2
    static let spacingXS: CGFloat = 4
    static let spacingSM: CGFloat = 8
    static let spacingMD: CGFloat = 12
    static let spacingLG: CGFloat = 16
    static let spacingXL: CGFloat = 24
    static let spacingXXL: CGFloat = 32
    static let spacingXXXL: CGFloat = 40

    static let radiusExtraSmall: CGFloat = 4
    static let radiusSmall: CGFloat = 6
    static let radiusMedium: CGFloat = 8
    static let radiusLarge: CGFloat = 10
    static let radiusExtraLarge: CGFloat = 16

    /// Git section and lineage geometry, shared by the sidebar's agent tree and
    /// the worktree list so the two indent the same way.
    /// Semantic PR colors follow GitHub's dark status palette, independent of chrome.
    enum PullRequest {
        static let open = color(for: "#3FB950")
        static let merged = color(for: "#A371F7")
        static let closed = color(for: "#F85149")
        static let draft = color(for: "#9198A1")
        static let iconSize: CGFloat = checkoutIconWidth
    }
    enum GitIcon {
        static let refresh = "arrow.clockwise"
        static let merged = "checkmark.circle"
        static let unmerged = "circle"
        static let dirty = "circle.fill"
        static let clean = "checkmark"
        static let unavailable = "exclamationmark.circle"
        static let noPullRequest = "minus.circle"
    }
    static let gitSectionIcon = "externaldrive.badge.checkmark"
    static let gitPullRequestIcon = "arrow.triangle.pull"
    /// One step down the agent tree.
    ///
    /// It is not a spacing value chosen by eye: it is exactly the distance
    /// from a row's status mark to its agent badge, so a child's mark lands
    /// centered under its parent's badge and every level reads as one column.
    /// Compact is the density the tree uses; the flat views do not indent.
    static let lineageIndent: CGFloat =
        agentMarkWidth + spacingXS + (compactAgentBadgeSize - agentMarkWidth) / 2
    /// The compact row's agent badge, repeated here because the indent is
    /// derived from it and `AgentRowDensity` reads it back.
    static let compactAgentBadgeSize: CGFloat = 16
    static let lineageChevronWidth: CGFloat = 16
    static let worktreeDialogWidth: CGFloat = 440
    static let formControlHeight: CGFloat = 36
    static let settingsFieldHeight: CGFloat = 24
    /// The right-hand control column in a settings row. Wide enough for a
    /// full model name and narrow enough that the row still reads as a table.
    static let settingsControlWidth: CGFloat = 200
    static let checkoutRowHeight: CGFloat = 36
    static let checkoutIconWidth: CGFloat = 14
    /// Align the root agent mark center with the checkout branch center.
    static let compactAgentLeadingInset = spacingSM + agentMarkWidth + spacingSM
        + checkoutIconWidth / 2 - lineageChevronWidth - agentMarkWidth / 2
    static let tabTitleMaxWidth: CGFloat = 200
    /// A descendant's inset in the agent tree: one column per level.
    ///
    /// The step used to shrink after two levels to keep a deep lineage on
    /// screen, which broke the column the connector and the marks share -
    /// only the first two levels lined up with anything. Depth is what the
    /// guide draws, so it stays uniform and the guide stays true.
    static func lineageInset(depth: Int) -> CGFloat {
        CGFloat(max(0, depth)) * lineageIndent
    }

    /// How far below a row's top its status mark is centered.
    ///
    /// A fixed offset, not a fraction of the row: a row grows downward when
    /// it carries a stall notice or a second summary line, and an elbow tied
    /// to the height would slide off the mark exactly when it did.
    static let lineageElbowY: CGFloat = compactAgentRowVerticalPadding + compactAgentBadgeSize / 2
    /// The compact row's vertical padding, shared with `AgentRowDensity` so
    /// the guide and the row cannot disagree about where the mark sits.
    static let compactAgentRowVerticalPadding: CGFloat = 5

    /// Where the trunk descending from a row at `depth` is drawn, measured
    /// from the leading edge of the tree's rows.
    ///
    /// It is that row's collapse toggle: the line leaves the control that
    /// opens it, so a branch and the thing that shows or hides it are the
    /// same column rather than two.
    static func lineageTrunkX(depth: Int) -> CGFloat {
        lineageInset(depth: depth) + compactAgentLeadingInset + lineageChevronWidth / 2
    }

    /// Where a row's guide stops: at its toggle when it has one, and at its
    /// status mark when it does not, so the line arrives at something the
    /// operator can see rather than crossing an empty column.
    static func lineageElbowEndX(depth: Int, hasToggle: Bool) -> CGFloat {
        let leading = lineageInset(depth: depth) + compactAgentLeadingInset
        return hasToggle ? leading : leading + lineageChevronWidth
    }

    static let compactControlSize: CGFloat = 36
    static let searchSheetSize = CGSize(width: 570, height: 430)
    static let settingsSheetSize = CGSize(width: 720, height: 560)
    static let addDeviceSheetSize = CGSize(width: 470, height: 300)

    static let badgeHeight: CGFloat = 16

    enum Control {
        static let compactHeight: CGFloat = 24
        static let regularHeight = formControlHeight
        static let checkboxSize: CGFloat = 16
        static let tabIndicatorHeight: CGFloat = 2
    }

    enum IconButton {
        static let standardSize = CGSize(width: 32, height: 32)
        static let toolbarSize = CGSize(width: 24, height: 24)
    }

    /// Sizes that describe the window's three-column frame rather than the
    /// spacing and radius scale above, which any view may reach for.
    enum Overview {
        static let cleanupHeight: CGFloat = 560
        static let railWidth: CGFloat = 64
        static let laneInset: CGFloat = 12
        static let laneSpacing: CGFloat = 14
        static let nodeOffset: CGFloat = 28
        static let nodeSize: CGFloat = 7
        static let commitHeight: CGFloat = 28
        static let workspaceHeight: CGFloat = 96
        static let rowWidth: CGFloat = 236
        static let selectedNodeSize: CGFloat = 16
        static let graphLineWidth: CGFloat = 1.5
        // Git lane categories, independent of agent lifecycle colors.
        static let lanes = [agentWorking, color(for: "#B69AFF"), success, warning]
        static func laneColor(_ lane: Int) -> Color { lanes[lane % lanes.count] }
    }

    enum Layout {
        static let pullRequestPopoverWidth: CGFloat = 360
        static let hairlineWidth: CGFloat = 1
        static let resizeHandleThickness: CGFloat = 2
        /// The strip that answers the pointer. Wider than the 2pt marker it
        /// draws, because a divider has to be easy to grab, not easy to see.
        static let resizeHandleGrabWidth: CGFloat = 20
        static let panelCollapseControlSize: CGFloat = 18
        /// How far a press has to travel on a tab before it is a reorder
        /// rather than a click. Below this a tremor while selecting a tab
        /// would carry it out of its slot.
        static let tabDragActivationDistance: CGFloat = 6
        /// The window's first row. A tab, the new-tab control, and the strip
        /// itself are all this tall, so the row cannot grow taller than the
        /// thing inside it.
        static let tabStripHeight: CGFloat = 32
        /// How much of the window's first row the traffic lights own. They end
        /// 61pt from the left edge, the zoom button spanning 47 to 61, so
        /// whichever surface reaches that corner keeps this much clear: the
        /// measurement plus one spacing step.
        static let trafficLightInset: CGFloat = 69
        static let paneHeaderHeight: CGFloat = 28
        /// The pane header's second row, which exists only when the pane has
        /// children. The breadcrumb keeps the 28pt row above it; a pane with
        /// no children is 28pt and nothing else (DESIGN.md, user decision).
        static let paneChildRowHeight: CGFloat = 24
        /// How wide one child chip is allowed to get before its name is
        /// truncated. Long identifiers and Korean names both have to fit
        /// several chips on one row rather than one chip pushing the rest off.
        static let paneChildChipMaxWidth: CGFloat = 132
        static let sidebarMinWidth: CGFloat = 220
        static let sidebarIdealWidth: CGFloat = 292
        static let sidebarMaxWidth: CGFloat = 440
        static let terminalMinWidth: CGFloat = 540
        static let terminalIdealWidth: CGFloat = 760
        static let rightPanelMinWidth: CGFloat = 260
        static let rightPanelIdealWidth: CGFloat = 355
        static let rightPanelMaxWidth: CGFloat = 560
    }

    static func color(for hex: String) -> Color {
        let value = hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
        guard value.count == 6, let number = UInt64(value, radix: 16) else { return accent }
        return Color(
            red: Double((number >> 16) & 0xff) / 255,
            green: Double((number >> 8) & 0xff) / 255,
            blue: Double(number & 0xff) / 255
        )
    }

    static let gitRowFontSize: CGFloat = 11
    static let gitDetailFontSize: CGFloat = 10
    /// The composer is a message box, not a form: wide enough for a sentence
    /// to breathe and short enough to read as a prompt rather than a page.
    static let composerSheetSize = CGSize(width: 560, height: 250)
}
