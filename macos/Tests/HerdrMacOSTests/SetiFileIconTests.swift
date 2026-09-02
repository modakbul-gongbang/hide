import AppKit
import CoreText
import Testing
@testable import HerdrMacOS

/// Names with no extension and names beginning with a dot reached the generic
/// document icon silently, because `URL.pathExtension` is empty for both and
/// the catalog only ever consulted the extension. These assert the whole-name
/// coverage, and that no name the catalog claims points at a codepoint the
/// bundled subset does not carry - the class of failure behind an icon cell
/// that renders as nothing at all.
@Suite("File icon resolution")
struct SetiFileIconTests {
    private static let reportedNames = [
        ".gitignore", ".gitattributes", ".env", ".zshrc",
        "CODEOWNERS", "Procfile", "Dockerfile.dev", "LICENSE",
    ]

    @Test func everyReportedNameResolvesToANonEmptyGlyph() {
        for name in Self.reportedNames {
            let icon = SetiFileIconCatalog.icon(fileName: name)
            #expect(!icon.glyph.isEmpty, "\(name) resolved to an empty glyph")
            #expect(!icon.fallbackSystemImage.isEmpty, "\(name) has no system fallback")
        }
    }

    @Test func dotNamesResolveToTheirOwnCategoryRatherThanTheGeneric() {
        let generic = SetiFileIconCatalog.fallback.glyph
        for name in [".gitignore", ".gitattributes", ".env", ".env.local", ".zshrc", ".npmrc", "Dockerfile.dev", "Procfile"] {
            #expect(
                SetiFileIconCatalog.icon(fileName: name).glyph != generic,
                "\(name) still falls through to the generic document icon"
            )
        }
    }

    @Test func aNameNoRuleClaimsGetsTheOneDeliberateGeneric() {
        for name in ["CODEOWNERS", "notes", "AUTHORS"] {
            #expect(SetiFileIconCatalog.icon(fileName: name) == SetiFileIconCatalog.fallback)
        }
    }

    @Test func resolutionIsCaseInsensitive() {
        #expect(
            SetiFileIconCatalog.icon(fileName: "DOCKERFILE").glyph
                == SetiFileIconCatalog.icon(fileName: "Dockerfile").glyph
        )
    }

    /// A glyph the catalog can return but the bundled subset does not carry
    /// renders as a blank box, which is indistinguishable from a broken icon
    /// column. The subset carries 22 codepoints; the catalog may only name
    /// those.
    @Test func everyGlyphTheCatalogCanReturnExistsInTheBundledFont() throws {
        try #require(SetiIconFont.isAvailable, "the bundled Seti subset did not register")
        let font = try #require(NSFont(name: SetiIconFont.postScriptName, size: 12))
        for glyph in SetiFileIconCatalog.allGlyphs {
            let scalars = Array(glyph.unicodeScalars)
            #expect(scalars.count == 1, "an icon glyph must be one codepoint")
            var characters = Array(String(scalars[0]).utf16)
            var glyphs = [CGGlyph](repeating: 0, count: characters.count)
            let resolved = CTFontGetGlyphsForCharacters(
                font, &characters, &glyphs, characters.count
            )
            #expect(
                resolved && glyphs[0] != 0,
                "U+\(String(scalars[0].value, radix: 16, uppercase: true)) is not in seti-subset.woff"
            )
        }
    }
}
