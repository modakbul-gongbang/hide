import AppKit
import CoreText
import Foundation
import SwiftUI
import Testing
@testable import HerdrMacOS

/// The document is the independent expected value, rather than a second list
/// of expected constants. This catches drift on either side of that boundary.
@Suite("Hide design document contract")
struct HideDesignContractTests {
    private static let root = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent()

    @Test @MainActor func documentTokensMatchRenderedValues() throws {
        let document = try String(contentsOf: Self.root.appendingPathComponent("DESIGN.md"), encoding: .utf8)
        let frontmatter = try #require(document.components(separatedBy: "---").dropFirst().first)
        let product = try #require(document.components(separatedBy: "## In-Product Components").dropFirst().first)
        let colors: [String: Color] = [
            "background": HideTheme.background, "sidebar": HideTheme.sidebar,
            "panel": HideTheme.panel, "elevated": HideTheme.elevated, "balloon": HideTheme.balloon,
            "divider": HideTheme.divider, "primary": HideTheme.primary,
            "secondary": HideTheme.secondary, "muted": HideTheme.muted, "accent": HideTheme.accent,
        ]
        let spacing: [String: CGFloat] = [
            "spacingNone": HideTheme.spacingNone, "spacingXXS": HideTheme.spacingXXS,
            "spacingXS": HideTheme.spacingXS, "spacingSM": HideTheme.spacingSM,
            "spacingMD": HideTheme.spacingMD, "spacingLG": HideTheme.spacingLG,
            "spacingXL": HideTheme.spacingXL, "spacingXXL": HideTheme.spacingXXL,
            "spacingXXXL": HideTheme.spacingXXXL,
        ]
        let rounded: [String: CGFloat] = [
            "radiusExtraSmall": HideTheme.radiusExtraSmall, "radiusSmall": HideTheme.radiusSmall,
            "radiusMedium": HideTheme.radiusMedium, "radiusLarge": HideTheme.radiusLarge,
            "radiusExtraLarge": HideTheme.radiusExtraLarge,
        ]
        let typography: [String: CGFloat] = [
            "micro": HideTheme.Typography.micro, "caption": HideTheme.Typography.caption,
            "body": HideTheme.Typography.body, "subhead": HideTheme.Typography.subhead,
            "title": HideTheme.Typography.title, "headline": HideTheme.Typography.headline,
            "display": HideTheme.Typography.display,
        ]
        let known = ["colors": Set(colors.keys), "spacing": Set(spacing.keys),
                     "rounded": Set(rounded.keys), "typography": Set(typography.keys)]
        let references = try NSRegularExpression(pattern: #"\{(colors|spacing|rounded|typography)\.([A-Za-z0-9]+)\}"#)
        let text = product as NSString
        var referenced: [String: Set<String>] = [:]
        for match in references.matches(in: product, range: NSRange(location: 0, length: text.length)) {
            let section = text.substring(with: match.range(at: 1))
            let name = text.substring(with: match.range(at: 2))
            #expect(known[section]?.contains(name) == true, "Unimplemented document token: \(section).\(name)")
            referenced[section, default: []].insert(name)
        }
        for (section, names) in known {
            #expect(referenced[section] == names, "Document every native \(section) token")
        }
        let documentedColors = scalars("colors", in: frontmatter)
        for (name, color) in colors {
            let expected = try #require(documentedColors[name])
            let native = try #require(NSColor(color).usingColorSpace(.sRGB))
            let rendered = String(format: "#%02X%02X%02X",
                                  Int((native.redComponent * 255).rounded()),
                                  Int((native.greenComponent * 255).rounded()),
                                  Int((native.blueComponent * 255).rounded()))
            #expect(rendered == expected.uppercased(), "Color drift: \(name)")
        }
        for (section, values) in ["spacing": spacing, "rounded": rounded] {
            let documented = scalars(section, in: frontmatter)
            for (name, actual) in values {
                let raw = try #require(documented[name])
                let expected = try #require(Double(raw.replacingOccurrences(of: "px", with: "")))
                #expect(Double(actual) == expected, "Dimension drift: \(name)")
            }
        }
        for (name, actual) in typography {
            let marker = "| `{typography.\(name)}` |"
            let row = try #require(product.components(separatedBy: "\n").first { $0.hasPrefix(marker) })
            let value = try #require(row.split(separator: "|").dropFirst().first)
            let expected = try #require(Double(value.trimmingCharacters(in: .whitespaces)
                .replacingOccurrences(of: "px", with: "")))
            #expect(Double(actual) == expected, "Typography drift: \(name)")
        }
    }

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

    // Only read the flat scalar maps used by this contract; lint validates the
    // full YAML grammar separately. No runtime parser or dependency is added.
    private func scalars(_ section: String, in frontmatter: String) -> [String: String] {
        var inside = false
        var values: [String: String] = [:]
        for line in frontmatter.components(separatedBy: "\n") {
            if line == "\(section):" { inside = true; continue }
            guard inside else { continue }
            if !line.isEmpty && !line.hasPrefix(" ") { break }
            guard line.hasPrefix("  "), !line.hasPrefix("   "),
                  let colon = line.firstIndex(of: ":") else { continue }
            let key = line[..<colon].trimmingCharacters(in: .whitespaces)
            let value = line[line.index(after: colon)...]
                .trimmingCharacters(in: CharacterSet(charactersIn: " \""))
            values[key] = value
        }
        return values
    }
}
