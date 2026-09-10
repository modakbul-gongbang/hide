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
    /// Whether this row shows a collapse toggle, which is where its own line
    /// ends. A row without one has the line run on to its status mark.
    var hasToggle = false

    var body: some View {
        GeometryReader { proxy in
            let height = proxy.size.height
            let elbowY = HideTheme.lineageElbowY
            Path { path in
                // Levels above this row whose sibling run is still open pass
                // straight through, which is what keeps a nested branch from
                // breaking the line of every level above it.
                for level in guide.continuing where level < depth {
                    let x = HideTheme.lineageTrunkX(depth: level)
                    path.move(to: CGPoint(x: x, y: 0))
                    path.addLine(to: CGPoint(x: x, y: height))
                }
                if depth > 0 {
                    // This row's own join: down from the parent's column, then
                    // across into this row's status mark. A last child ends
                    // the run at that corner; an earlier one carries it down
                    // to the sibling below.
                    let parentX = HideTheme.lineageTrunkX(depth: depth - 1)
                    path.move(to: CGPoint(x: parentX, y: 0))
                    path.addLine(to: CGPoint(x: parentX, y: guide.isLastChild ? elbowY : height))
                    path.move(to: CGPoint(x: parentX, y: elbowY))
                    // Stops at the leading edge of what it points at rather
                    // than its center, so the line arrives at the glyph
                    // instead of running underneath it.
                    path.addLine(to: CGPoint(
                        x: HideTheme.lineageElbowEndX(depth: depth, hasToggle: hasToggle),
                        y: elbowY
                    ))
                }
                if guide.startsChildren {
                    // The line has to leave this row for its first child to
                    // join onto. Without it the trunk began below the parent
                    // and connected to nothing above.
                    let x = HideTheme.lineageTrunkX(depth: depth)
                    path.move(to: CGPoint(x: x, y: elbowY))
                    path.addLine(to: CGPoint(x: x, y: height))
                }
            }
            .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
        }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }
}
