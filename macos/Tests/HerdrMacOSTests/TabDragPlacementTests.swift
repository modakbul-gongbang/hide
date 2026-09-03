import CoreGraphics
import Testing

@testable import HerdrMacOS

/// Where a dragged tab lands. Tabs are as wide as their labels, so the answer
/// has to come from the widths beside the one being carried, not from a fixed
/// step. The strip in these cases is three tabs of 80, 120, and 60 points.
@Suite("Tab drag placement")
struct TabDragPlacementTests {
    private let widths: [CGFloat] = [80, 120, 60]

    @Test func aDragThatHasNotClearedHalfTheNeighbourStaysPut() {
        #expect(TabDragPlacement.destinationIndex(from: 0, translation: 0, widths: widths) == 0)
        #expect(TabDragPlacement.destinationIndex(from: 0, translation: 59, widths: widths) == 0)
        #expect(TabDragPlacement.destinationIndex(from: 2, translation: -29, widths: widths) == 2)
    }

    @Test func aTabTakesTheSlotOfEachNeighbourItHasPassedTheMiddleOf() {
        // Past half of the 120-wide neighbour, the first tab takes its slot.
        #expect(TabDragPlacement.destinationIndex(from: 0, translation: 60, widths: widths) == 1)
        // Past half of the 60-wide tab beyond it, it takes the last slot.
        #expect(TabDragPlacement.destinationIndex(from: 0, translation: 150, widths: widths) == 2)
        // Dragging left works the same way from the other end.
        #expect(TabDragPlacement.destinationIndex(from: 2, translation: -60, widths: widths) == 1)
        #expect(TabDragPlacement.destinationIndex(from: 2, translation: -160, widths: widths) == 0)
    }

    @Test func aDragPastTheEndStopsAtTheLastSlot() {
        #expect(TabDragPlacement.destinationIndex(from: 0, translation: 4000, widths: widths) == 2)
        #expect(TabDragPlacement.destinationIndex(from: 2, translation: -4000, widths: widths) == 0)
    }

    @Test func aStripThatChangedUnderTheDragIsLeftAlone() {
        #expect(TabDragPlacement.destinationIndex(from: 3, translation: 200, widths: widths) == 3)
        #expect(TabDragPlacement.destinationIndex(from: 0, translation: 200, widths: []) == 0)
    }
}
