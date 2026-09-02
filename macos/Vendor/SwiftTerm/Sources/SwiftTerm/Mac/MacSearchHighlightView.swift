//
//  MacSearchHighlightView.swift
//  SwiftTerm
//
//  Draws every match of the find bar's current term.
//
//  The terminal has one selection, so `findNext` can only show the match the
//  operator is standing on. This overlay shows the rest of them. It lives here
//  rather than in a host application because the cell geometry it needs -
//  `cellDimension` and the display buffer's scroll offset - is this module's,
//  and a host would have to be handed both to do the same arithmetic.

#if os(macOS)
import AppKit

/// A transparent, non-interactive sibling of the terminal content that fills
/// one rounded rect per match. It sits above the text and below the find bar,
/// and works under either renderer because it is a view rather than a step in
/// a draw path.
final class SearchHighlightView: NSView {
    var rects: [NSRect] = [] {
        didSet {
            guard rects != oldValue else { return }
            needsDisplay = true
        }
    }

    var color: NSColor = NSColor.systemYellow.withAlphaComponent(0.28) {
        didSet {
            guard color != oldValue else { return }
            needsDisplay = true
        }
    }

    /// The overlay never answers the pointer: a click on a highlighted match
    /// must still reach the terminal's own selection handling.
    override func hitTest(_ point: NSPoint) -> NSView? { nil }

    override func draw(_ dirtyRect: NSRect) {
        guard !rects.isEmpty else { return }
        color.setFill()
        for rect in rects where rect.intersects(dirtyRect) {
            NSBezierPath(roundedRect: rect, xRadius: 2, yRadius: 2).fill()
        }
    }
}
#endif
