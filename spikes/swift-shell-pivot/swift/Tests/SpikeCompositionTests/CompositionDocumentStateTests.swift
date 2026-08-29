import Foundation
import Testing
@testable import SpikeComposition

struct CompositionDocumentStateTests {
    @Test func anchorsFirstMarkedTextAtTheDocumentSelection() {
        var state = CompositionDocumentState()

        let transition = state.receiveMarkedText(
            utf16Length: 1,
            selectedRange: NSRange(location: 1, length: 0),
            replacementRange: NSRange(location: NSNotFound, length: 0),
            baseSelection: NSRange(location: 198, length: 0)
        )

        #expect(transition.succeeded)
        #expect(state.snapshot.markedRange == NSRange(location: 198, length: 1))
        #expect(state.snapshot.selectedRange == NSRange(location: 199, length: 0))
    }

    @Test func preservesAnchorAcrossMarkedTextUpdates() {
        var state = CompositionDocumentState()
        _ = state.receiveMarkedText(
            utf16Length: 1,
            selectedRange: NSRange(location: 1, length: 0),
            replacementRange: NSRange(location: NSNotFound, length: 0),
            baseSelection: NSRange(location: 198, length: 0)
        )

        let transition = state.receiveMarkedText(
            utf16Length: 2,
            selectedRange: NSRange(location: 1, length: 1),
            replacementRange: NSRange(location: NSNotFound, length: 0),
            baseSelection: NSRange(location: 500, length: 0)
        )

        #expect(transition.succeeded)
        #expect(state.snapshot.markedRange == NSRange(location: 198, length: 2))
        #expect(state.snapshot.selectedRange == NSRange(location: 199, length: 1))
    }

    @Test func usesExplicitDocumentReplacementAsTheNewAnchor() {
        var state = CompositionDocumentState()

        let transition = state.receiveMarkedText(
            utf16Length: 2,
            selectedRange: NSRange(location: 2, length: 0),
            replacementRange: NSRange(location: 41, length: 3),
            baseSelection: NSRange(location: 9, length: 0)
        )

        #expect(transition.succeeded)
        #expect(state.snapshot.markedRange == NSRange(location: 41, length: 2))
        #expect(state.snapshot.selectedRange == NSRange(location: 43, length: 0))
    }

    @Test func translatesOnlyTheMarkedIntersectionToStorageCoordinates() {
        var state = CompositionDocumentState()
        _ = state.receiveMarkedText(
            utf16Length: 4,
            selectedRange: NSRange(location: 2, length: 0),
            replacementRange: NSRange(location: NSNotFound, length: 0),
            baseSelection: NSRange(location: 100, length: 0)
        )

        let translation = state.translateDocumentRange(NSRange(location: 98, length: 5))

        #expect(translation?.documentRange == NSRange(location: 100, length: 3))
        #expect(translation?.localRange == NSRange(location: 0, length: 3))
        #expect(state.translateDocumentRange(NSRange(location: 104, length: 2)) == nil)
    }

    @Test func resolvesImplicitCommitAgainstTheActiveDocumentMark() {
        var state = CompositionDocumentState()
        _ = state.receiveMarkedText(
            utf16Length: 2,
            selectedRange: NSRange(location: 2, length: 0),
            replacementRange: NSRange(location: NSNotFound, length: 0),
            baseSelection: NSRange(location: 72, length: 0)
        )

        let documentRange = state.resolveReplacementRange(
            NSRange(location: NSNotFound, length: 0),
            fallbackSelection: NSRange(location: 99, length: 0)
        )

        #expect(documentRange == NSRange(location: 72, length: 2))
        #expect(state.translateReplacementRangeToMarkedStorage(documentRange!) == NSRange(location: 0, length: 2))
    }

    @Test func clearingACompositionRemovesEveryCoordinate() {
        var state = CompositionDocumentState()
        _ = state.receiveMarkedText(
            utf16Length: 1,
            selectedRange: NSRange(location: 1, length: 0),
            replacementRange: NSRange(location: NSNotFound, length: 0),
            baseSelection: NSRange(location: 7, length: 0)
        )

        let transition = state.clear(operation: "commit")

        #expect(transition.before.isActive)
        #expect(!transition.after.isActive)
        #expect(state.snapshot.markedRange.location == NSNotFound)
        #expect(state.snapshot.selectedRange == nil)
    }

    @Test func invalidRelativeSelectionFailsWithoutCorruptingActiveState() {
        var state = CompositionDocumentState()
        _ = state.receiveMarkedText(
            utf16Length: 2,
            selectedRange: NSRange(location: 1, length: 0),
            replacementRange: NSRange(location: NSNotFound, length: 0),
            baseSelection: NSRange(location: 30, length: 0)
        )
        let before = state.snapshot

        let transition = state.receiveMarkedText(
            utf16Length: 1,
            selectedRange: NSRange(location: 2, length: 0),
            replacementRange: NSRange(location: NSNotFound, length: 0),
            baseSelection: NSRange(location: 300, length: 0)
        )

        #expect(!transition.succeeded)
        #expect(state.snapshot == before)
        #expect(transition.failure != nil)
    }
}
