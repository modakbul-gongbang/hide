import AppKit
import SwiftUI

/// Search owns only a selected result identity; the live projection owns rows.
struct HideSearchSelection {
    private(set) var selectedID: String?

    mutating func reconcile(_ ids: [String]) {
        if let selectedID, ids.contains(selectedID) { return }
        selectedID = ids.first
    }

    mutating func move(_ direction: MoveCommandDirection, among ids: [String]) {
        guard direction == .up || direction == .down else { return }
        guard !ids.isEmpty else { selectedID = nil; return }
        guard let selectedID, let index = ids.firstIndex(of: selectedID) else {
            self.selectedID = ids.first
            return
        }
        self.selectedID = ids[direction == .down ? min(index + 1, ids.count - 1) : max(index - 1, 0)]
    }

    func entry<Entry: Identifiable>(in entries: [Entry]) -> Entry? where Entry.ID == String {
        guard let selectedID else { return nil }
        return entries.first { $0.id == selectedID }
    }
}

/// Both search fields use the same focus, IME and selection behavior.
private struct HideSearchKeyboard: ViewModifier {
    @Binding var selection: HideSearchSelection
    let resultIDs: [String]
    let activate: () -> Void
    let dismiss: () -> Void
    let focusChanged: (Bool) -> Void
    @FocusState private var focused: Bool

    func body(content: Content) -> some View {
        content
            .focused($focused)
            .onKeyPress(keys: [.upArrow, .downArrow], phases: [.down, .repeat]) { press in
                guard press.modifiers.intersection([.command, .control, .option, .shift]).isEmpty,
                      (NSApp.keyWindow?.firstResponder as? NSTextView)?.hasMarkedText() != true else { return .ignored }
                selection.move(press.key == .downArrow ? .down : .up, among: resultIDs)
                return .handled
            }
            .onSubmit(activate)
            .onAppear {
                selection.reconcile(resultIDs)
                focused = true
            }
            .onChange(of: focused) { _, value in focusChanged(value) }
            .onChange(of: resultIDs) { _, ids in selection.reconcile(ids) }
            .onExitCommand(perform: dismiss)
    }
}

extension View {
    func hideSearchKeyboard(
        selection: Binding<HideSearchSelection>, resultIDs: [String],
        activate: @escaping () -> Void, dismiss: @escaping () -> Void,
        focusChanged: @escaping (Bool) -> Void = { _ in }
    ) -> some View {
        modifier(HideSearchKeyboard(selection: selection, resultIDs: resultIDs, activate: activate, dismiss: dismiss, focusChanged: focusChanged))
    }
}
