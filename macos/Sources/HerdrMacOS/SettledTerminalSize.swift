/// A grid is settled after two consecutive display ticks report the same
/// positive size. Reports inside one tick replace the candidate, never the PTY.
struct SettledTerminalSize {
    struct Grid: Equatable {
        let cols: Int
        let rows: Int
    }
    private var candidate: Grid?
    private var sampled: Grid?
    private var delivered: Grid?

    mutating func report(cols: Int, rows: Int) {
        guard cols > 0, rows > 0 else {
            candidate = nil
            sampled = nil
            return
        }
        let next = Grid(cols: cols, rows: rows)
        // A transient grid invalidates the view's frame baseline even when
        // layout returns to the previously delivered size. That final size
        // still needs one settled repaint from the server.
        if next != candidate { delivered = nil }
        candidate = next
    }

    mutating func displayTick() -> Grid? {
        defer { sampled = candidate }
        guard let candidate, candidate == sampled, candidate != delivered else { return nil }
        delivered = candidate
        return candidate
    }
}
