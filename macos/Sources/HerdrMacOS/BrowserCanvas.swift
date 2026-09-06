import AppKit
import SwiftUI

struct BrowserCanvas: NSViewRepresentable {
    @ObservedObject var controller: BrowserPaneController
    let paneID: String
    let onFocus: () -> Void

    func makeNSView(context: Context) -> BrowserCanvasView {
        let view = BrowserCanvasView()
        view.setAccessibilityIdentifier("browser-canvas-\(paneID)")
        view.setAccessibilityLabel("Chromium page in pane \(paneID)")
        view.setAccessibilityRole(.image)
        return view
    }

    func updateNSView(_ view: BrowserCanvasView, context: Context) {
        view.controller = controller
        controller.resizeViewport(to: view.bounds.size)
        view.onFocus = onFocus
        view.needsDisplay = true
    }
}

/// Chromium remains the DOM/input authority. AppKit only translates native
/// pointer coordinates and IME text to CDP; no page code executes in Hide.
@MainActor
final class BrowserCanvasView: NSView, @preconcurrency NSTextInputClient {
    weak var controller: BrowserPaneController?
    var onFocus: (() -> Void)?
    private var composition = NSAttributedString(string: "")
    private var compositionSelection = NSRange(location: 0, length: 0)
    private var lastPointer = CGPoint.zero

    override var isFlipped: Bool { true }
    override var acceptsFirstResponder: Bool { true }

    override func layout() {
        super.layout()
        controller?.resizeViewport(to: bounds.size)
    }

    override func draw(_ dirtyRect: NSRect) {
        HideTheme.Native.panel.setFill()
        bounds.fill()
        if let controller, let image = controller.image, let geometry = controller.geometry {
            image.draw(in: geometry.imageRect(in: bounds), from: .zero, operation: .copy, fraction: 1, respectFlipped: true, hints: nil)
        }
        if hasMarkedText() {
            let attributed = NSAttributedString(string: composition.string, attributes: [
                .font: HideTheme.nativeFont(size: HideTheme.Typography.body),
                .foregroundColor: HideTheme.Native.primary,
                .backgroundColor: HideTheme.Native.elevated,
                .underlineStyle: NSUnderlineStyle.single.rawValue,
            ])
            attributed.draw(at: compositionOrigin)
        }
    }

    private var compositionOrigin: CGPoint {
        CGPoint(x: min(lastPointer.x, max(0, bounds.width - composition.size().width)),
                y: min(lastPointer.y, max(0, bounds.height - HideTheme.compactControlSize)))
    }

    private func mouse(_ event: NSEvent, type: String, button: String) {
        lastPointer = convert(event.locationInWindow, from: nil)
        guard let controller, let point = controller.geometry?.pagePoint(lastPointer, in: bounds) else { return }
        controller.send("Input.dispatchMouseEvent", parameters: [
            "type": type, "x": point.x, "y": point.y,
            "button": button, "clickCount": event.clickCount,
            "modifiers": Self.modifiers(event.modifierFlags),
        ])
    }

    override func mouseDown(with event: NSEvent) {
        onFocus?()
        window?.makeFirstResponder(self)
        mouse(event, type: "mousePressed", button: "left")
    }
    override func mouseUp(with event: NSEvent) { mouse(event, type: "mouseReleased", button: "left") }
    override func mouseDragged(with event: NSEvent) { mouse(event, type: "mouseMoved", button: "left") }
    override func rightMouseDown(with event: NSEvent) {
        onFocus?()
        window?.makeFirstResponder(self)
        mouse(event, type: "mousePressed", button: "right")
    }
    override func rightMouseUp(with event: NSEvent) { mouse(event, type: "mouseReleased", button: "right") }
    override func scrollWheel(with event: NSEvent) {
        let point = convert(event.locationInWindow, from: nil)
        guard let controller, let page = controller.geometry?.pagePoint(point, in: bounds) else { return }
        controller.send("Input.dispatchMouseEvent", parameters: [
            "type": "mouseWheel", "x": page.x, "y": page.y,
            "deltaX": -event.scrollingDeltaX, "deltaY": -event.scrollingDeltaY,
            "modifiers": Self.modifiers(event.modifierFlags),
        ])
    }

    override func keyDown(with event: NSEvent) { interpretKeyEvents([event]) }

    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        guard window?.firstResponder === self, event.modifierFlags.contains(.command) else {
            return super.performKeyEquivalent(with: event)
        }
        if event.charactersIgnoringModifiers?.lowercased() == "v" {
            if let text = NSPasteboard.general.string(forType: .string) {
                controller?.send("Input.insertText", parameters: ["text": text])
            }
            return true
        }
        // Browser edit shortcuts are routed to the inspected page, while Hide
        // keeps its own pane/tab commands. Clipboard reads happen only on paste.
        if let letter = event.charactersIgnoringModifiers?.lowercased(), ["a", "z"].contains(letter) {
            let key = letter.uppercased()
            let command = letter == "a" ? "selectAll" : (event.modifierFlags.contains(.shift) ? "redo" : "undo")
            keyStroke(key: key, code: "Key\(key)", virtualKey: Int(key.utf8.first!),
                      modifiers: Self.modifiers(event.modifierFlags), commands: [command])
            return true
        }
        return super.performKeyEquivalent(with: event)
    }

    private static func modifiers(_ flags: NSEvent.ModifierFlags) -> Int {
        (flags.contains(.option) ? 1 : 0) | (flags.contains(.control) ? 2 : 0)
            | (flags.contains(.command) ? 4 : 0) | (flags.contains(.shift) ? 8 : 0)
    }

    private func keyStroke(key: String, code: String, virtualKey: Int, modifiers: Int = 0, commands: [String] = []) {
        let params: [String: Any] = ["key": key, "code": code, "windowsVirtualKeyCode": virtualKey, "modifiers": modifiers]
        var down = params
        down["type"] = key == "Enter" ? "keyDown" : "rawKeyDown"
        if key == "Enter" { down["text"] = "\r" }
        if !commands.isEmpty { down["commands"] = commands }
        controller?.send("Input.dispatchKeyEvent", parameters: down)
        controller?.send("Input.dispatchKeyEvent", parameters: params.merging(["type": "keyUp"]) { _, new in new })
    }

    func insertText(_ string: Any, replacementRange: NSRange) {
        let text = (string as? NSAttributedString)?.string ?? (string as? String ?? "")
        unmarkText()
        if !text.isEmpty { controller?.send("Input.insertText", parameters: ["text": text]) }
    }
    func setMarkedText(_ string: Any, selectedRange: NSRange, replacementRange: NSRange) {
        composition = (string as? NSAttributedString) ?? NSAttributedString(string: string as? String ?? "")
        compositionSelection = selectedRange
        needsDisplay = true
    }
    func unmarkText() { composition = NSAttributedString(string: ""); needsDisplay = true }
    func hasMarkedText() -> Bool { composition.length > 0 }
    func markedRange() -> NSRange { NSRange(location: hasMarkedText() ? 0 : NSNotFound, length: composition.length) }
    func selectedRange() -> NSRange { compositionSelection }
    func validAttributesForMarkedText() -> [NSAttributedString.Key] { [] }
    func attributedSubstring(forProposedRange range: NSRange, actualRange: NSRangePointer?) -> NSAttributedString? { nil }
    func characterIndex(for point: NSPoint) -> Int { NSNotFound }
    func firstRect(forCharacterRange range: NSRange, actualRange: NSRangePointer?) -> NSRect {
        let local = CGRect(origin: compositionOrigin, size: CGSize(width: 1, height: HideTheme.terminalBaseFontSize))
        let windowRect = convert(local, to: nil)
        return window?.convertToScreen(windowRect) ?? .zero
    }
    override func doCommand(by selector: Selector) {
        let keys: [String: (String, String, Int)] = [
            "insertNewline:": ("Enter", "Enter", 13), "insertTab:": ("Tab", "Tab", 9),
            "deleteBackward:": ("Backspace", "Backspace", 8), "deleteForward:": ("Delete", "Delete", 46),
            "moveLeft:": ("ArrowLeft", "ArrowLeft", 37), "moveRight:": ("ArrowRight", "ArrowRight", 39),
            "moveUp:": ("ArrowUp", "ArrowUp", 38), "moveDown:": ("ArrowDown", "ArrowDown", 40),
            "cancelOperation:": ("Escape", "Escape", 27),
        ]
        if let key = keys[NSStringFromSelector(selector)] {
            keyStroke(key: key.0, code: key.1, virtualKey: key.2)
        } else { super.doCommand(by: selector) }
    }
}
