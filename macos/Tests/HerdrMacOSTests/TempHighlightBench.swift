import AppKit
import Highlightr
import Testing
@testable import HerdrMacOS

@Suite("temp bench") struct TempHighlightBench {
    @Test @MainActor func measure() throws {
        for path in [
            "/Users/hoyeonlee/projects/herdr-ide/macos/Sources/HerdrMacOS/HighlightedCodeEditor.swift",
            "/Users/hoyeonlee/projects/herdr-ide/macos/Sources/HerdrMacOS/RightPanel.swift",
            "/Users/hoyeonlee/projects/herdr-ide/macos/Sources/HerdrMacOS/ShellModel.swift",
            "/Users/hoyeonlee/projects/herdr-ide/macos/Vendor/SwiftTerm/Sources/SwiftTerm/Terminal.swift",
        ] {
            let text = try String(contentsOfFile: path, encoding: .utf8)
            let storage = HighlightedCodeEditor.makeTextStorage(language: "swift")
            let t0 = Date()
            storage.replaceCharacters(in: NSRange(location: 0, length: 0), with: text)
            let ms = Date().timeIntervalSince(t0) * 1000
            var colored = 0
            var idx = 0
            while idx < storage.length {
                var range = NSRange(location: 0, length: 0)
                let attrs = storage.attributes(at: idx, effectiveRange: &range)
                if attrs[.foregroundColor] != nil { colored += range.length }
                idx += max(1, range.length)
            }
            print(String(format: "BENCH %7d bytes %8.1f ms  colored=%d/%d  %@", text.utf8.count, ms, colored, storage.length, (path as NSString).lastPathComponent))
        }
    }
}
