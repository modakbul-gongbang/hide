import CoreGraphics
import Testing

@testable import HerdrMacOS

@Suite("Adaptive tab strip presentation")
struct AdaptiveTabStripPresentationTests {
    private let tabIDs = (0..<8).map { "tab-\($0)" }

    @Test func tabsStartAtThePreferredWidth() {
        let presentation = make(width: 360, count: 2, active: 0)

        #expect(presentation.density == .standard)
        #expect(presentation.slotWidth == 180)
        #expect(presentation.visibleRange == 0..<2)
        #expect(presentation.hiddenIndices.isEmpty)
    }

    @Test func allTabsShrinkEquallyWhileTitlesStillFit() {
        let presentation = make(width: 450, count: 3, active: 1)

        #expect(presentation.density == .compressed)
        #expect(presentation.slotWidth == 150)
        #expect(presentation.visibleRange == 0..<3)
        #expect(!presentation.showsOverflow)
    }

    @Test func titlesCollapseOnlyBelowTheirMinimumWidth() {
        let titled = make(width: 416, count: 4, active: 2)
        let icons = make(width: 412, count: 4, active: 2)

        #expect(titled.density == .compressed)
        #expect(titled.slotWidth == 104)
        #expect(icons.density == .icon)
        #expect(icons.slotWidth == 103)
        #expect(icons.visibleRange == 0..<4)
    }

    @Test func allIconTabsRemainVisibleAtTheMinimumWidth() {
        let presentation = make(width: 320, count: 5, active: 4)

        #expect(presentation.density == .icon)
        #expect(presentation.slotWidth == 64)
        #expect(presentation.visibleRange == 0..<5)
        #expect(!presentation.showsOverflow)
    }

    @Test func overflowKeepsAContiguousRangeContainingTheActiveTab() {
        let presentation = make(width: 320, count: 6, active: 4)

        #expect(presentation.density == .icon)
        #expect(presentation.slotWidth == 73)
        #expect(presentation.visibleRange == 2..<6)
        #expect(presentation.hiddenIndices == [0, 1])
        #expect(presentation.showsOverflow)
    }

    @Test func selectingAHiddenTabMovesTheWindowWithoutReorderingTabs() {
        let first = make(width: 320, count: 8, active: 0)
        let middle = make(width: 320, count: 8, active: 5)
        let last = make(width: 320, count: 8, active: 7)

        #expect(first.visibleRange == 0..<4)
        #expect(first.hiddenIndices == [4, 5, 6, 7])
        #expect(middle.visibleRange == 3..<7)
        #expect(middle.hiddenIndices == [0, 1, 2, 7])
        #expect(last.visibleRange == 4..<8)
        #expect(last.hiddenIndices == [0, 1, 2, 3])
    }

    @Test func wideningRestoresIconTitleAndPreferredStatesInReverse() {
        let overflow = make(width: 320, count: 6, active: 4)
        let icons = make(width: 480, count: 6, active: 4)
        let titles = make(width: 720, count: 6, active: 4)
        let preferred = make(width: 1_080, count: 6, active: 4)

        #expect(overflow.showsOverflow)
        #expect(icons.density == .icon)
        #expect(icons.visibleRange == 0..<6)
        #expect(titles.density == .compressed)
        #expect(preferred.density == .standard)
        #expect(preferred.slotWidth == 180)
    }

    @Test func dragUsesTheSameVisibleSlotsAndCannotCrossHiddenTabs() {
        let presentation = make(width: 320, count: 8, active: 5)

        #expect(presentation.visibleRange == 3..<7)
        #expect(presentation.tabIDs == tabIDs)
        #expect(presentation.destinationIndex(from: 3, translation: 36) == 3)
        #expect(presentation.destinationIndex(from: 3, translation: 37) == 4)
        #expect(presentation.destinationIndex(from: 3, translation: 4000) == 6)
        #expect(presentation.destinationIndex(from: 6, translation: -4000) == 3)
        #expect(presentation.destinationIndex(from: 2, translation: 4000) == 2)
    }

    @Test func anEmptyStripHasNoSlotsOrOverflow() {
        let presentation = AdaptiveTabStripPresentation(
            availableWidth: 400,
            tabIDs: [],
            activeTabID: nil
        )

        #expect(presentation.visibleRange.isEmpty)
        #expect(presentation.hiddenIndices.isEmpty)
        #expect(!presentation.showsOverflow)
        #expect(presentation.slotWidth == 0)
    }

    private func make(width: CGFloat, count: Int, active: Int) -> AdaptiveTabStripPresentation {
        AdaptiveTabStripPresentation(
            availableWidth: width,
            tabIDs: Array(tabIDs.prefix(count)),
            activeTabID: tabIDs[active]
        )
    }
}
