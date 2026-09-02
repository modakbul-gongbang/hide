//
//  TerminalViewSearch.swift
//  SwiftTerm
//
//  Search integration for TerminalView.
//

#if os(macOS) || os(iOS) || os(visionOS)
import Foundation

/// One match's place in the terminal buffer: an absolute buffer row, the
/// starting column, and how many cells it covers.
public struct SearchMatchPosition: Equatable {
    public let row: Int
    public let col: Int
    public let length: Int

    public init (row: Int, col: Int, length: Int) {
        self.row = row
        self.col = col
        self.length = length
    }
}

extension TerminalView {
    /// Finds the next match for `term`, selects it, and optionally scrolls it into view.
    /// - Parameters:
    ///   - term: The search term.
    ///   - options: Search options (case sensitivity, regex, whole word).
    ///   - scrollToResult: Whether to scroll the result into view.
    /// - Returns: `true` if a match was found.
    @discardableResult
    public func findNext (_ term: String, options: SearchOptions = SearchOptions(), scrollToResult: Bool = true) -> Bool {
        guard let search = search, let selection = selection else {
            return false
        }

        search.updateLastSelection(currentSearchSelection(selection))
        if let result = search.findNext(term: term, options: options) {
            return applySearchResult(result, selection: selection, scrollToResult: scrollToResult)
        }

        selection.selectNone()
        return false
    }

    /// Finds the previous match for `term`, selects it, and optionally scrolls it into view.
    /// - Parameters:
    ///   - term: The search term.
    ///   - options: Search options (case sensitivity, regex, whole word).
    ///   - scrollToResult: Whether to scroll the result into view.
    /// - Returns: `true` if a match was found.
    @discardableResult
    public func findPrevious (_ term: String, options: SearchOptions = SearchOptions(), scrollToResult: Bool = true) -> Bool {
        guard let search = search, let selection = selection else {
            return false
        }

        search.updateLastSelection(currentSearchSelection(selection))
        if let result = search.findPrevious(term: term, options: options) {
            return applySearchResult(result, selection: selection, scrollToResult: scrollToResult)
        }

        selection.selectNone()
        return false
    }


    /// Position of the current match among all matches for `term`: a 1-based
    /// `index` (0 when there is no current match) and the `total` match count
    /// (capped at `limit`). Drives a "2/14" style counter in a search UI.
    public func searchMatchSummary (_ term: String, options: SearchOptions = SearchOptions(), limit: Int = 1000) -> (index: Int, total: Int) {
        guard let search = search else {
            return (0, 0)
        }
        let all = search.findAll(term: term, options: options, limit: limit)
        guard let last = search.lastResult,
              let i = all.firstIndex(where: { $0.row == last.row && $0.col == last.col }) else {
            return (0, all.count)
        }
        return (i + 1, all.count)
    }

    /// Every match for `term` as a buffer position and a length, in buffer
    /// order and capped at `limit`. A host that wants to highlight all matches
    /// needs their positions, which `findNext` alone cannot give: the terminal
    /// has one selection, so it can only show the current match.
    public func searchMatchPositions (_ term: String, options: SearchOptions = SearchOptions(), limit: Int = 1000) -> [SearchMatchPosition] {
        guard let search = search else {
            return []
        }
        return search.findAll(term: term, options: options, limit: limit).map { result in
            SearchMatchPosition(row: result.row, col: result.col, length: max(result.size, 0))
        }
    }

    /// Clears the current search state and selection.
    public func clearSearch () {
        search?.reset()
        selection?.selectNone()
    }

    private func applySearchResult (_ result: SearchResult, selection: SelectionService, scrollToResult: Bool) -> Bool {
        let range = search?.selectionRange(for: result) ?? (start: Position(col: result.col, row: result.row),
                                                           end: Position(col: result.col, row: result.row))
        selection.setSelection(start: range.start, end: range.end)
        if scrollToResult {
            scrollToReveal(row: result.row)
        }
        return true
    }

    private func currentSearchSelection (_ selection: SelectionService) -> SearchSelection? {
        guard selection.active else {
            return nil
        }
        let start = selection.start
        let end = selection.end
        switch Position.compare(start, end) {
        case .before, .equal:
            return SearchSelection(start: start, end: end)
        case .after:
            return SearchSelection(start: end, end: start)
        }
    }

    private func scrollToReveal (row: Int) {
        let displayBuffer = terminal.displayBuffer
        let rows = displayBuffer.rows
        guard rows > 0 else {
            return
        }
        if terminal.isDisplayBufferAlternate {
            return
        }

        let upperVisible = displayBuffer.yDisp
        let lowerVisible = displayBuffer.yDisp + rows - 1
        if row >= upperVisible && row <= lowerVisible {
            return
        }

        let maxScrollback = max(0, displayBuffer.lines.count - rows)
        var target = row - rows / 2
        if target < 0 {
            target = 0
        }
        if target > maxScrollback {
            target = maxScrollback
        }
        scrollTo(row: target)
    }
}
#endif
