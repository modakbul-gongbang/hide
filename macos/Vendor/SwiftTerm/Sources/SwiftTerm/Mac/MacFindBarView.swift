//
//  MacFindBarView.swift
//  SwiftTerm
//

#if os(macOS)
import AppKit

/// The result counter's text for a match position. A search with a term but
/// no match says so rather than leaving the field looking inert, which is the
/// whole difference between "found nothing" and "did nothing".
public func terminalFindBarSummary (term: String, index: Int, total: Int) -> String {
    if term.isEmpty {
        return ""
    }
    if total == 0 {
        return "No matches"
    }
    if index == 0 {
        return "\(total) matches"
    }
    return "\(index)/\(total)"
}

public final class TerminalFindBarView: NSVisualEffectView, NSSearchFieldDelegate {
    var onSearchChanged: ((String) -> Void)?
    var onFindNext: (() -> Void)?
    var onFindPrevious: (() -> Void)?
    var onClose: (() -> Void)?
    var onOptionsChanged: ((SearchOptions) -> Void)?

    private let searchField = NSSearchField()
    private let summaryLabel = NSTextField(labelWithString: "")
    private let previousButton = NSButton()
    private let nextButton = NSButton()
    private let closeButton = NSButton()
    /// The three search options live behind one pull-down rather than as three
    /// labelled checkboxes in the row. Spelled out they were wider than the
    /// search field itself, which is what made this bar span the pane instead
    /// of floating over a corner of it.
    private let optionsButton = NSPopUpButton(frame: .zero, pullsDown: true)
    private let caseSensitiveItem = NSMenuItem(title: "Case Sensitive", action: nil, keyEquivalent: "")
    private let regexItem = NSMenuItem(title: "Regular Expression", action: nil, keyEquivalent: "")
    private let wholeWordItem = NSMenuItem(title: "Whole Word", action: nil, keyEquivalent: "")

    public var searchText: String {
        get { searchField.stringValue }
        set { searchField.stringValue = newValue }
    }

    /// The match counter, or the reason there is none.
    public var summary: String {
        get { summaryLabel.stringValue }
        set { summaryLabel.stringValue = newValue }
    }

    /// The field, the counter, and the controls, so a host can restyle the bar
    /// into its own design system. SwiftTerm has no access to that system, so
    /// it exposes the parts rather than guessing at colors.
    public var styleTargets: (field: NSSearchField, summary: NSTextField, buttons: [NSButton]) {
        (searchField, summaryLabel, [previousButton, nextButton, closeButton, optionsButton])
    }

    public var options: SearchOptions {
        SearchOptions(
            caseSensitive: caseSensitiveItem.state == .on,
            regex: regexItem.state == .on,
            wholeWord: wholeWordItem.state == .on
        )
    }

    public override init(frame frameRect: NSRect) {
        super.init(frame: frameRect)
        setup()
    }

    public required init?(coder: NSCoder) {
        super.init(coder: coder)
        setup()
    }

    public func focus() {
        window?.makeFirstResponder(searchField)
    }

    private func setup() {
        wantsLayer = true
        material = .popover
        blendingMode = .withinWindow
        state = .active
        layer?.cornerRadius = 6
        layer?.masksToBounds = true

        searchField.placeholderString = "Find"
        searchField.delegate = self
        searchField.translatesAutoresizingMaskIntoConstraints = false
        searchField.sendsSearchStringImmediately = true
        searchField.target = self
        searchField.action = #selector(searchFieldAction)
        searchField.controlSize = .small
        searchField.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        searchField.setContentHuggingPriority(.defaultLow, for: .horizontal)

        configureButton(previousButton, symbol: "chevron.up", tooltip: "Previous")
        previousButton.target = self
        previousButton.action = #selector(previousTapped)

        configureButton(nextButton, symbol: "chevron.down", tooltip: "Next")
        nextButton.target = self
        nextButton.action = #selector(nextTapped)

        configureButton(closeButton, symbol: "xmark", tooltip: "Close")
        closeButton.target = self
        closeButton.action = #selector(closeTapped)

        configureOptionsButton()

        summaryLabel.font = NSFont.systemFont(ofSize: 11)
        summaryLabel.textColor = .secondaryLabelColor
        summaryLabel.alignment = .right
        summaryLabel.setContentHuggingPriority(.defaultHigh, for: .horizontal)

        let stack = NSStackView(views: [
            searchField,
            summaryLabel,
            previousButton,
            nextButton,
            optionsButton,
            closeButton
        ])
        stack.orientation = .horizontal
        stack.alignment = .centerY
        stack.spacing = 4
        stack.translatesAutoresizingMaskIntoConstraints = false

        addSubview(stack)

        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: leadingAnchor, constant: 6),
            stack.trailingAnchor.constraint(equalTo: trailingAnchor, constant: -6),
            stack.topAnchor.constraint(equalTo: topAnchor, constant: 4),
            stack.bottomAnchor.constraint(equalTo: bottomAnchor, constant: -4),
            // A definite width, not a floor: the bar is a fixed compact field
            // that floats over the pane, so it must not grow with its content.
            searchField.widthAnchor.constraint(equalToConstant: 150)
        ])
    }

    private func configureButton(_ button: NSButton, symbol: String, tooltip: String) {
        button.translatesAutoresizingMaskIntoConstraints = false
        button.bezelStyle = .texturedRounded
        button.setButtonType(.momentaryPushIn)
        button.controlSize = .small
        button.image = NSImage(systemSymbolName: symbol, accessibilityDescription: tooltip)
        button.toolTip = tooltip
    }

    private func configureOptionsButton() {
        let menu = NSMenu()
        // A pull-down spends its first item on the button face, never showing
        // it in the list.
        let face = NSMenuItem()
        face.image = NSImage(
            systemSymbolName: "ellipsis.circle",
            accessibilityDescription: "Search options"
        )
        menu.addItem(face)
        for item in [caseSensitiveItem, regexItem, wholeWordItem] {
            item.target = self
            item.action = #selector(optionToggled(_:))
            menu.addItem(item)
        }
        optionsButton.menu = menu
        optionsButton.translatesAutoresizingMaskIntoConstraints = false
        optionsButton.bezelStyle = .texturedRounded
        optionsButton.controlSize = .small
        optionsButton.imagePosition = .imageOnly
        optionsButton.toolTip = "Search options"
    }

    @objc private func optionToggled(_ sender: NSMenuItem) {
        sender.state = sender.state == .on ? .off : .on
        onOptionsChanged?(options)
    }

    @objc private func searchFieldAction() {
        if let event = NSApp.currentEvent, event.modifierFlags.contains(.shift) {
            onFindPrevious?()
        } else {
            onFindNext?()
        }
    }

    @objc private func previousTapped() {
        onFindPrevious?()
    }

    @objc private func nextTapped() {
        onFindNext?()
    }

    @objc private func closeTapped() {
        onClose?()
    }

    public func controlTextDidChange(_ obj: Notification) {
        onSearchChanged?(searchField.stringValue)
    }

    public func control(_ control: NSControl, textView: NSTextView, doCommandBy commandSelector: Selector) -> Bool {
        if commandSelector == #selector(NSResponder.cancelOperation(_:)) {
            onClose?()
            return true
        }
        if commandSelector == #selector(NSResponder.insertNewline(_:)) {
            searchFieldAction()
            return true
        }
        return false
    }
}
#endif
