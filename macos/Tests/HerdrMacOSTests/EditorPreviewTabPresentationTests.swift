import AppKit
import CoreText
import Foundation
import Testing
@testable import HerdrMacOS

/// The strip's side of PRD editor-preview-tab: the italic title, the
/// "Preview" name, and the Keep Open chord (D-06, D-11, B16, B17). The core
/// decides which tab is the preview; these fix what the shell does with it.
@Suite("Editor preview tab presentation")
struct EditorPreviewTabPresentationTests {
    private func editorTab(_ id: String, preview: Bool, kind: CoreEditorTabKind = .file) -> CoreEditorTabSnapshot {
        CoreEditorTabSnapshot(
            id: id, workspaceID: "w1", checkoutID: "c1", path: "/tmp/repo/\(id).md", label: "\(id).md",
            kind: kind, diffCommitted: kind == .diff ? false : nil, dirty: false, preview: preview
        )
    }

    @Test func thePreviewFlagRidesTheStripEntryOntoTheDrawnItem() {
        let items = ShellTabStrip.items(
            strip: [
                CoreStripTabSnapshot(id: "file:a", kind: .file, sourceID: "a", label: "a.md", preview: true),
                CoreStripTabSnapshot(id: "diff:b", kind: .diff, sourceID: "b", label: "b.md (working diff)"),
                CoreStripTabSnapshot(id: "herdr:t", kind: .herdr, sourceID: "t", label: "Tab 1"),
            ],
            herdrTabs: [],
            editorTabs: [editorTab("a", preview: true), editorTab("b", preview: false, kind: .diff)],
            activeHerdrTabID: nil,
            activeFileTabID: "a"
        )
        #expect(items.map(\.preview) == [true, false])
        #expect(items.map(\.active) == [true, false])
    }

    @Test func aPreviewTabIsNamedWithPreviewAndDrawnInItalicUntilPromoted() {
        #expect(EditorTabTitlePresentation.spoken(label: "notes.md", preview: true) == "notes.md · Preview")
        #expect(EditorTabTitlePresentation.spoken(label: "notes.md", preview: false) == "notes.md")
        #expect(EditorTabTitlePresentation.italic(preview: true))
        #expect(!EditorTabTitlePresentation.italic(preview: false))
    }

    @Test func aStripEntryWithoutTheFlagDecodesAsAnOrdinaryTab() throws {
        let entry = try JSONDecoder().decode(
            CoreStripTabSnapshot.self,
            from: Data(#"{"id":"file:a","kind":"file","source_id":"a","label":"a.md"}"#.utf8)
        )
        #expect(!entry.preview)
        let preview = try JSONDecoder().decode(
            CoreStripTabSnapshot.self,
            from: Data(#"{"id":"file:a","kind":"file","source_id":"a","label":"a.md","preview":true}"#.utf8)
        )
        #expect(preview.preview)
    }

    /// The italic variant is the same Inter face with the theme's slant in
    /// its matrix, so the title changes only its angle: weight, features and
    /// family stay what every other chrome label uses.
    @Test @MainActor func theItalicChromeFontIsInterSlantedByThePreviewToken() throws {
        let upright = HideTheme.nativeFont(size: HideTheme.Typography.body, weight: .semibold)
        let italic = HideTheme.nativeFont(size: HideTheme.Typography.body, weight: .semibold, italic: true)
        #expect(italic.familyName == upright.familyName)
        #expect(italic.pointSize == upright.pointSize)
        let matrix = CTFontGetMatrix(italic as CTFont)
        let expected = tan(HideTheme.Typography.previewSlant * .pi / 180)
        #expect(abs(matrix.c - expected) < 0.0001)
        #expect(matrix.a == 1 && matrix.d == 1 && matrix.b == 0)
        #expect(CTFontGetMatrix(upright as CTFont).c == 0)
        let features = try #require(CTFontCopyAttribute(italic as CTFont, kCTFontFeatureSettingsAttribute)
            as? [[String: Any]])
        #expect(features.contains {
            ($0[kCTFontFeatureSelectorIdentifierKey as String] as? Int) == kStylisticAltThreeOnSelector
        })
    }

    @Test func keepOpenIsAMenuCommandOnCommandShiftK() {
        #expect(ShellMenuCommand.keepOpen.title == "Keep Open")
        #expect(ShellMenuCommand.keepOpen.shortcut.canonical == "command+shift+k")
        #expect(ShellMenuCommand.keepOpen.displayShortcut == "⇧⌘K")
        #expect(ShellMenuCommand.keepOpen.scope == .applicationMenu)
    }

    /// The chord is pressed while the editor holds the keyboard, so the local
    /// monitor answers it the way it answers Reopen Closed Tab; a plain ⌘K
    /// (Search) and a key-up are not it.
    @MainActor
    @Test func keepOpenChordIsAnsweredRegardlessOfFocus() {
        func event(_ type: NSEvent.EventType, _ modifiers: NSEvent.ModifierFlags) -> NSEvent {
            NSEvent.keyEvent(
                with: type, location: .zero, modifierFlags: modifiers, timestamp: 0, windowNumber: 0,
                context: nil, characters: "K", charactersIgnoringModifiers: "k", isARepeat: false, keyCode: 40
            )!
        }
        #expect(KeepOpenShortcutPolicy.shouldKeepOpen(event(.keyDown, [.command, .shift])))
        #expect(!KeepOpenShortcutPolicy.shouldKeepOpen(event(.keyDown, [.command])))
        #expect(!KeepOpenShortcutPolicy.shouldKeepOpen(event(.keyUp, [.command, .shift])))
    }
}
