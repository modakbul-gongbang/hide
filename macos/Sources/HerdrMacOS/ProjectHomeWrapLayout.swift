import SwiftUI

/// Lays a lane's items (its header parts, its card groups) out left to right
/// and wraps at the lane's width.
///
/// Every group keeps its own ideal size and its place in the sequence, so a
/// status change repaints a card without moving its neighbours; only a change
/// in the number or size of groups reflows the lane (PRD home-board, rule 3).
/// The arithmetic is a pure function so a test can read the rows it makes
/// without a view host.
struct ProjectHomeWrapLayout: Layout {
    var horizontalSpacing: CGFloat = HideTheme.spacingSM
    var verticalSpacing: CGFloat = HideTheme.spacingSM

    /// Where each size lands in a container `width` wide. A group wider than
    /// the container takes a row of its own at the container's width, so a
    /// truncating label truncates instead of overflowing; an unbounded width
    /// lays everything on one row.
    static func arrange(
        sizes: [CGSize],
        width: CGFloat,
        horizontalSpacing: CGFloat,
        verticalSpacing: CGFloat
    ) -> (frames: [CGRect], size: CGSize) {
        var frames: [CGRect] = []
        var x: CGFloat = 0
        var y: CGFloat = 0
        var rowHeight: CGFloat = 0
        var widest: CGFloat = 0
        for ideal in sizes {
            let size = CGSize(width: min(ideal.width, width), height: ideal.height)
            if x > 0, x + size.width > width {
                x = 0
                y += rowHeight + verticalSpacing
                rowHeight = 0
            }
            frames.append(CGRect(origin: CGPoint(x: x, y: y), size: size))
            x += size.width + horizontalSpacing
            rowHeight = max(rowHeight, size.height)
            widest = max(widest, x - horizontalSpacing)
        }
        let height = sizes.isEmpty ? 0 : y + rowHeight
        return (frames, CGSize(width: widest, height: height))
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let sizes = subviews.map { $0.sizeThatFits(.unspecified) }
        let arranged = Self.arrange(
            sizes: sizes, width: proposal.width ?? .infinity,
            horizontalSpacing: horizontalSpacing, verticalSpacing: verticalSpacing
        )
        return CGSize(width: proposal.width ?? arranged.size.width, height: arranged.size.height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let sizes = subviews.map { $0.sizeThatFits(.unspecified) }
        let arranged = Self.arrange(
            sizes: sizes, width: bounds.width,
            horizontalSpacing: horizontalSpacing, verticalSpacing: verticalSpacing
        )
        for (subview, frame) in zip(subviews, arranged.frames) {
            subview.place(
                at: CGPoint(x: bounds.minX + frame.minX, y: bounds.minY + frame.minY),
                anchor: .topLeading,
                proposal: ProposedViewSize(frame.size)
            )
        }
    }
}
