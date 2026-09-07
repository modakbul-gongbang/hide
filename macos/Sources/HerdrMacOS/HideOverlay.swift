import SwiftUI

private struct HideAnchor {
    let bounds: Anchor<CGRect>
    let target: HideHintTarget
    let label: String
    let inline: Bool
    let allowsHint: Bool
}
private struct HideAnchorPreference: PreferenceKey {
    static var defaultValue: [String: HideAnchor] { [:] }
    static func reduce(value: inout [String: HideAnchor], nextValue: () -> [String: HideAnchor]) {
        value.merge(nextValue(), uniquingKeysWith: { _, next in next })
    }
}
private struct HideTooltipModifier: ViewModifier {
    let label: String
    let command: HideCommand
    let paneID: String?
    let tabID: String?
    let inline: Bool
    let allowsHint: Bool
    @State private var id = UUID().uuidString
    @EnvironmentObject private var model: ShellModel
    @EnvironmentObject private var tooltips: HideTooltipController

    func body(content: Content) -> some View {
        content
            .anchorPreference(key: HideAnchorPreference.self, value: .bounds) { bounds in
                [id: HideAnchor(bounds: bounds, target: HideHintTarget(id: id, command: command, paneID: paneID, tabID: tabID), label: label, inline: inline, allowsHint: allowsHint)]
            }
            .onHover { tooltips.hover(id, inside: $0) }
            .onDisappear { tooltips.remove(id) }
            .accessibilityHint(command.tooltipText(label: label, bindings: model.paneShortcuts))
    }
}
private struct HideOverlayHost: ViewModifier {
    @StateObject private var tooltips = HideTooltipController()
    func body(content: Content) -> some View {
        content
            .overlayPreferenceValue(HideAnchorPreference.self) { anchors in
                GeometryReader { geometry in
                    HideOverlayLayer(anchors: anchors, geometry: geometry)
                }
                .allowsHitTesting(false)
            }
            .environmentObject(tooltips)
            .tint(HideTheme.accent)
            .onAppear { tooltips.start() }
            .onDisappear { tooltips.stop() }
    }
}
private struct HideOverlayLayer: View {
    let anchors: [String: HideAnchor]
    let geometry: GeometryProxy
    @EnvironmentObject private var model: ShellModel
    @EnvironmentObject private var tooltips: HideTooltipController
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    private var exposed: Set<HideHintTarget> {
        guard model.shortcutHintState.revealed else { return [] }
        return HideHintTarget.exposed(among: anchors.values.filter { $0.allowsHint }.map(\.target), state: model.shortcutHintState,
            bindings: model.paneShortcuts, focusedPaneID: model.focusedPaneID,
            activeTabID: model.unifiedTabs.first(where: \.active)?.id)
    }
    var body: some View {
        let exposed = self.exposed
        let visibleID = tooltips.state.visibleID
        // The overlay hosts only balloons being shown, not an empty child for
        // every control. Resolve the hint set once, outside the per-anchor body.
        let displayedIDs = anchors.keys.filter { id in
            guard let anchor = anchors[id] else { return false }
            return id == visibleID || (!anchor.inline && exposed.contains(anchor.target))
        }.sorted()
        return ZStack(alignment: .topLeading) {
            ForEach(displayedIDs, id: \.self) { id in
                if let anchor = anchors[id] {
                    if visibleID == id {
                        PositionedHideBalloon(command: anchor.target.command, label: anchor.label, mode: .tooltip,
                            anchor: geometry[anchor.bounds], window: geometry.size)
                    } else if !anchor.inline && exposed.contains(anchor.target) {
                        PositionedHideBalloon(command: anchor.target.command, label: anchor.label, mode: .hint,
                            anchor: geometry[anchor.bounds], window: geometry.size)
                    }
                }
            }
        }
        .animation(.easeOut(duration: HideTooltipState.fadeDuration(reduceMotion: reduceMotion)), value: tooltips.state.visibleID)
        .animation(.easeOut(duration: HideTooltipState.fadeDuration(reduceMotion: reduceMotion)), value: exposed)
        .onChange(of: Set(anchors.keys), initial: true) { _, ids in tooltips.retain(ids) }
        .accessibilityHidden(true)
        .onChange(of: exposed, initial: true) { _, targets in
            #if DEBUG
            guard CommandLine.arguments.contains("--verification-ui-fixture") else { return }
            let record: [String: Any] = [
                "kind": "hide.hint.exposure",
                "uptime": ProcessInfo.processInfo.systemUptime,
                "modifiers": model.shortcutHintState.modifiers.map(\.rawValue).sorted(),
                "revealed": model.shortcutHintState.revealed,
                "focusedPaneID": model.focusedPaneID as Any? ?? NSNull(),
                "activeTabID": model.unifiedTabs.first(where: \.active)?.id as Any? ?? NSNull(),
                "targets": targets.sorted { $0.id < $1.id }.map { target -> [String: Any] in
                    let anchor = anchors[target.id]!
                    let bounds = geometry[anchor.bounds]
                    return ["id": target.id, "label": anchor.label, "inline": anchor.inline,
                            "shortcut": target.command.shortcut(bindings: model.paneShortcuts)?.displayString ?? "",
                            "paneID": target.paneID as Any? ?? NSNull(), "tabID": target.tabID as Any? ?? NSNull(),
                            "bounds": [bounds.minX, bounds.minY, bounds.width, bounds.height]]
                }
            ]
            do { VerificationReceipt.writeLine(try JSONSerialization.data(withJSONObject: record, options: [.sortedKeys])) }
            catch { preconditionFailure("Hint verification diagnostic encoding failed: \(error)") }
            #endif
        }
    }
}
private struct PositionedHideBalloon: View {
    let command: HideCommand
    let label: String
    let mode: HideBalloon.Mode
    let anchor: CGRect
    let window: CGSize
    @State private var size = CGSize.zero
    var body: some View {
        HideBalloon(command: command, label: label, mode: mode)
            .fixedSize()
            .background(GeometryReader { proxy in
                Color.clear.onAppear { size = proxy.size }.onChange(of: proxy.size) { _, value in size = value }
            })
            .position(HideBalloonPlacement.center(anchor: anchor, size: size, window: window))
            .opacity(size == .zero ? 0 : 1)
            .transition(.opacity)
    }
}
extension View {
    func hideTooltip(_ label: String, command: HideCommand? = nil, paneID: String? = nil,
                     tabID: String? = nil, inline: Bool = false, hint: Bool = true) -> some View {
        modifier(HideTooltipModifier(label: label, command: command ?? .label(label), paneID: paneID, tabID: tabID,
            inline: inline, allowsHint: hint))
    }
    func hideOverlayHost() -> some View { modifier(HideOverlayHost()) }
}
