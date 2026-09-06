import Foundation

/// The bytes held for a pane whose terminal view has not registered yet.
///
/// A pane in a tab the operator has not opened has no view, so its chunks pile
/// up here until one appears. The core keeps 512 chunks per snapshot and no
/// more, so holding more than that here can never draw anything the core could
/// still send: it only grows for the life of the process. The buffer is
/// emptied when the pane's session is released or the pane goes away, because
/// the next visit redraws from Herdr's own full frame.
struct PendingTerminalBuffer<Chunk> {
    /// The same bound the core keeps its retained chunks at
    /// (`RETAINED_TERMINAL_CHUNKS`). Holding more here would buffer frames the
    /// core has already forgotten.
    static var chunkLimit: Int { 512 }

    private var chunks: [String: [Chunk]] = [:]

    /// Appends one chunk and reports how many of the pane's oldest chunks were
    /// dropped to stay inside the bound.
    @discardableResult
    mutating func append(_ bytes: Chunk, for paneID: String) -> Int {
        var held = chunks[paneID] ?? []
        held.append(bytes)
        let dropped = max(0, held.count - Self.chunkLimit)
        if dropped > 0 {
            held.removeFirst(dropped)
        }
        chunks[paneID] = held
        return dropped
    }

    /// Removes and returns a pane's held chunks, oldest first.
    mutating func take(_ paneID: String) -> [Chunk]? {
        chunks.removeValue(forKey: paneID)
    }

    mutating func clear(_ paneID: String) {
        chunks.removeValue(forKey: paneID)
    }

    /// Drops every pane that is not in `paneIDs`, and reports which went.
    @discardableResult
    mutating func retain(paneIDs: Set<String>) -> [String] {
        let leaving = chunks.keys.filter { !paneIDs.contains($0) }
        for paneID in leaving {
            chunks.removeValue(forKey: paneID)
        }
        return leaving
    }

    func count(for paneID: String) -> Int {
        chunks[paneID]?.count ?? 0
    }

    var paneIDs: Set<String> { Set(chunks.keys) }
}

/// Which panes' held bytes are still worth keeping (R8).
///
/// R8 empties a pane's buffer when its session is released or the pane leaves
/// the session. Those are two different questions and they have two different
/// answers in the snapshot.
///
/// Whether a pane still exists is Herdr's layouts. It is *not* the transport
/// projection: the core empties `terminal.panes` while a selection is in
/// progress (`clear_terminal_projection`) and deliberately leaves the layouts
/// alone, so a tick taken in that moment lists no pane at all. Reading
/// existence from the projection threw away a pane's full frame while its view
/// was still being built - after a zoom, or on a switch to a tab holding
/// several panes - and the next small delta then drew on an empty grid.
enum PendingTerminalRetention {
    /// The panes to keep, or `nil` when the snapshot says nothing about which
    /// panes exist and therefore cannot justify dropping anything.
    static func keep(
        layouts: [CorePaneLayoutSnapshot],
        transportPanes: [CoreTerminalPaneSnapshot]
    ) -> Set<String>? {
        let live = Set(layouts.flatMap { $0.root.paneIDs })
        guard !live.isEmpty else { return nil }
        let released = Set(
            transportPanes
                .filter { $0.transportState == "released" }
                .map(\.paneID)
        )
        return live.subtracting(released)
    }
}
