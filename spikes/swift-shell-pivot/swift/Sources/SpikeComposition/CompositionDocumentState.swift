import Foundation

public struct CompositionDocumentState: Equatable, Sendable {
    public struct Snapshot: Equatable, Sendable {
        public let anchor: Int?
        public let markedLength: Int
        public let relativeSelection: NSRange

        public var isActive: Bool {
            anchor != nil && markedLength > 0
        }

        public var markedRange: NSRange {
            guard let anchor, markedLength > 0 else {
                return NSRange(location: NSNotFound, length: 0)
            }
            return NSRange(location: anchor, length: markedLength)
        }

        public var selectedRange: NSRange? {
            guard let anchor, markedLength > 0,
                  relativeSelection.location != NSNotFound,
                  relativeSelection.location <= Int.max - anchor
            else { return nil }
            return NSRange(
                location: anchor + relativeSelection.location,
                length: relativeSelection.length
            )
        }
    }

    public struct Transition: Equatable, Sendable {
        public let operation: String
        public let before: Snapshot
        public let after: Snapshot
        public let failure: String?

        public var succeeded: Bool { failure == nil }
    }

    public struct RangeTranslation: Equatable, Sendable {
        public let documentRange: NSRange
        public let localRange: NSRange
    }

    private var anchor: Int?
    private var markedLength = 0
    private var relativeSelection = NSRange(location: NSNotFound, length: 0)

    public init() {}

    public var snapshot: Snapshot {
        Snapshot(
            anchor: anchor,
            markedLength: markedLength,
            relativeSelection: relativeSelection
        )
    }

    public mutating func receiveMarkedText(
        utf16Length: Int,
        selectedRange: NSRange,
        replacementRange: NSRange,
        baseSelection: NSRange
    ) -> Transition {
        let before = snapshot
        guard utf16Length > 0 else {
            clearStorage()
            return Transition(operation: "set-empty-marked-text", before: before, after: snapshot, failure: nil)
        }
        guard Self.isValid(range: selectedRange, upperBound: utf16Length) else {
            return Transition(
                operation: "set-marked-text",
                before: before,
                after: before,
                failure: "selectedRange is outside the incoming marked string"
            )
        }

        let nextAnchor: Int
        if replacementRange.location != NSNotFound {
            guard Self.isNonnegative(range: replacementRange) else {
                return Transition(
                    operation: "set-marked-text",
                    before: before,
                    after: before,
                    failure: "replacementRange is invalid"
                )
            }
            nextAnchor = replacementRange.location
        } else if let anchor {
            nextAnchor = anchor
        } else {
            guard baseSelection.location != NSNotFound,
                  Self.isNonnegative(range: baseSelection)
            else {
                return Transition(
                    operation: "set-marked-text",
                    before: before,
                    after: before,
                    failure: "base selection has no document location"
                )
            }
            nextAnchor = baseSelection.location
        }

        anchor = nextAnchor
        markedLength = utf16Length
        relativeSelection = selectedRange
        return Transition(operation: "set-marked-text", before: before, after: snapshot, failure: nil)
    }

    public mutating func clear(operation: String) -> Transition {
        let before = snapshot
        clearStorage()
        return Transition(operation: operation, before: before, after: snapshot, failure: nil)
    }

    public func translateDocumentRange(_ proposedRange: NSRange) -> RangeTranslation? {
        let marked = snapshot.markedRange
        guard marked.location != NSNotFound,
              proposedRange.location != NSNotFound,
              Self.isNonnegative(range: proposedRange),
              proposedRange.length > 0,
              marked.location <= Int.max - marked.length,
              proposedRange.location <= Int.max - proposedRange.length
        else { return nil }

        let lowerBound = max(marked.location, proposedRange.location)
        let upperBound = min(
            marked.location + marked.length,
            proposedRange.location + proposedRange.length
        )
        guard lowerBound < upperBound else { return nil }
        return RangeTranslation(
            documentRange: NSRange(location: lowerBound, length: upperBound - lowerBound),
            localRange: NSRange(location: lowerBound - marked.location, length: upperBound - lowerBound)
        )
    }

    public func resolveReplacementRange(
        _ replacementRange: NSRange,
        fallbackSelection: NSRange
    ) -> NSRange? {
        if replacementRange.location != NSNotFound {
            return Self.isNonnegative(range: replacementRange) ? replacementRange : nil
        }
        if snapshot.isActive {
            return snapshot.markedRange
        }
        return Self.isNonnegative(range: fallbackSelection) ? fallbackSelection : nil
    }

    public func translateReplacementRangeToMarkedStorage(_ documentRange: NSRange) -> NSRange? {
        let marked = snapshot.markedRange
        guard marked.location != NSNotFound,
              documentRange.location != NSNotFound,
              Self.isNonnegative(range: documentRange),
              documentRange.location >= marked.location,
              documentRange.location <= Int.max - documentRange.length,
              marked.location <= Int.max - marked.length,
              documentRange.location + documentRange.length <= marked.location + marked.length
        else { return nil }
        return NSRange(
            location: documentRange.location - marked.location,
            length: documentRange.length
        )
    }

    private mutating func clearStorage() {
        anchor = nil
        markedLength = 0
        relativeSelection = NSRange(location: NSNotFound, length: 0)
    }

    private static func isValid(range: NSRange, upperBound: Int) -> Bool {
        guard isNonnegative(range: range), range.location <= upperBound else { return false }
        return range.length <= upperBound - range.location
    }

    private static func isNonnegative(range: NSRange) -> Bool {
        range.location != NSNotFound && range.location >= 0 && range.length >= 0
    }
}
