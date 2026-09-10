import SwiftUI

/// The connector that ties an agent row to the one that spawned it.
///
/// It is drawn as one overlay across the row's whole leading gutter rather
/// than as a stub per row, because a stub cannot join anything: the trunk has
/// to run the full height of every row between a parent and its last child,
/// including the rows of deeper branches in between. Those pass-through
/// levels are what `SidebarGrouping.LineageGuide.continuing` carries.
///
/// Geometry comes from `HideTheme.lineageTrunkX`, the same function the row's
/// own inset is built from, so the elbow lands on the child's status mark
/// instead of near it (design principle 7: structure is drawn, not narrated).
struct LineageGuideView: View {
    let depth: Int
    let guide: SidebarGrouping.LineageGuide

    /// The trunk stops at the mark's center on the last child and runs the
    /// whole row otherwise, which is what makes a sibling run read as one
    /// line and its end read as an end.
    private var elbowFraction: CGFloat { 0.5 }

    var body: some View {
        GeometryReader { proxy in
            let height = proxy.size.height
            let elbowY = height * elbowFraction
            Path { path in
                for level in guide.continuing where level < depth {
                    let x = HideTheme.lineageTrunkX(depth: level)
                    path.move(to: CGPoint(x: x, y: 0))
                    path.addLine(to: CGPoint(x: x, y: height))
                }
                guard depth > 0 else { return }
                let x = HideTheme.lineageTrunkX(depth: depth - 1)
                path.move(to: CGPoint(x: x, y: 0))
                path.addLine(to: CGPoint(x: x, y: guide.isLastChild ? elbowY : height))
                path.move(to: CGPoint(x: x, y: elbowY))
                path.addLine(to: CGPoint(x: HideTheme.lineageTrunkX(depth: depth), y: elbowY))
            }
            .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }
}
