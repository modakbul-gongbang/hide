import AppKit
import CoreText
import Testing
@testable import HerdrMacOS

/// The bundled chrome font is what the shell's typography assumes: Inter with
/// the ss03 stylistic set, not a system fallback that measures differently.
@Suite("Hide chrome font")
struct HideChromeFontTests {
    @Test @MainActor func bundledChromeFontProvidesInterAndStylisticSetThree() throws {
        let font = HideTheme.nativeFont(size: HideTheme.Typography.body)
        #expect(font.familyName == "Inter Variable")
        let features = try #require(CTFontCopyAttribute(font as CTFont, kCTFontFeatureSettingsAttribute)
            as? [[String: Any]])
        #expect(features.contains {
            ($0[kCTFontFeatureTypeIdentifierKey as String] as? Int) == kStylisticAlternativesType
                && ($0[kCTFontFeatureSelectorIdentifierKey as String] as? Int) == kStylisticAltThreeOnSelector
        })
    }
}
