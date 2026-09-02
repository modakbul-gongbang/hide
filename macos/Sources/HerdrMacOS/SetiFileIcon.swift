import AppKit
import CoreText
import SwiftUI

struct SetiFileIcon: Equatable, Sendable {
    let glyph: String
    let colorHex: String
    let fallbackSystemImage: String
}

enum SetiFileIconCatalog {
    /// The one deliberate generic: a plain document. Every name the specific
    /// rules do not claim lands here, at the same size and colour as every
    /// other row, rather than on a missing glyph or an empty cell.
    static let fallback = SetiFileIcon(
        glyph: "\u{E023}",
        colorHex: HideTheme.fileIconDocumentHex,
        fallbackSystemImage: "doc"
    )

    /// Names carrying no extension, or nothing but a leading-dot name, used to
    /// reach the generic silently: `URL.pathExtension` is empty for `.env` and
    /// for `CODEOWNERS` alike, so the extension table never saw them. Whole
    /// names are resolved first, and only what no rule claims falls through.
    private static let namedIcons: [String: SetiFileIcon] = {
        var table: [String: SetiFileIcon] = [:]
        func assign(_ names: [String], _ icon: SetiFileIcon) {
            for name in names { table[name] = icon }
        }
        assign(
            [".gitignore", ".gitattributes", ".gitmodules", ".gitconfig", ".gitkeep", ".mailmap"],
            icon("\u{E034}", HideTheme.fileIconNeutralHex, "point.3.connected.trianglepath.dotted")
        )
        assign(
            [".dockerignore"],
            icon("\u{E025}", "#519ABA", "shippingbox")
        )
        assign(
            [
                "makefile", "gnumakefile", "cmakelists.txt", "justfile",
                "rakefile", "gemfile", "procfile", "brewfile",
            ],
            icon("\u{E05F}", "#E37933", "hammer")
        )
        assign(
            [
                ".zshrc", ".zshenv", ".zprofile", ".zlogin", ".zlogout",
                ".bashrc", ".bash_profile", ".bash_logout", ".profile",
                ".inputrc", ".hushlogin", ".envrc",
            ],
            icon("\u{E089}", "#8DC149", "terminal")
        )
        assign(
            [
                ".editorconfig", ".npmrc", ".nvmrc", ".yarnrc", ".tool-versions",
                ".ruby-version", ".node-version", ".python-version",
                ".npmignore", ".eslintignore", ".prettierignore", ".swiftformat",
            ],
            icon("\u{E019}", HideTheme.fileIconNeutralHex, "gearshape")
        )
        assign(
            ["package-lock.json", "yarn.lock", "pnpm-lock.yaml"],
            icon("\u{E05D}", "#8DC149", "lock")
        )
        return table
    }()

    static func icon(for url: URL) -> SetiFileIcon {
        icon(fileName: url.lastPathComponent)
    }

    static func icon(fileName: String) -> SetiFileIcon {
        let lowercased = fileName.lowercased()

        if let named = namedIcons[lowercased] {
            return named
        }
        if lowercased.hasPrefix("readme") {
            return icon("\u{E04D}", "#519ABA", "info.circle")
        }
        if ["license", "licence", "copying"].contains(where: lowercased.hasPrefix) {
            return icon("\u{E05A}", "#CBCB41", "doc.text")
        }
        if lowercased.hasPrefix("dockerfile") {
            return icon("\u{E025}", "#519ABA", "shippingbox")
        }
        if lowercased.hasPrefix(".env") {
            return icon("\u{E019}", HideTheme.fileIconNeutralHex, "gearshape")
        }
        // A dot-name ending in `rc` is a tool's run-control file whatever the
        // tool is, so the class is claimed rather than each new tool's name.
        if lowercased.hasPrefix("."), lowercased.hasSuffix("rc") {
            return icon("\u{E019}", HideTheme.fileIconNeutralHex, "gearshape")
        }
        if lowercased.hasSuffix(".lock") {
            return icon("\u{E05D}", "#8DC149", "lock")
        }

        return switch URL(fileURLWithPath: fileName).pathExtension.lowercased() {
        case "swift": icon("\u{E092}", "#E37933", "swift")
        case "rs": icon("\u{E082}", HideTheme.fileIconNeutralHex, "gearshape.2")
        case "ts": icon("\u{E099}", "#519ABA", "t.square")
        case "tsx", "jsx": icon("\u{E07D}", "#519ABA", "atom")
        case "js", "mjs", "cjs": icon("\u{E051}", "#CBCB41", "j.square")
        case "md", "markdown": icon("\u{E060}", "#519ABA", "text.book.closed")
        case "json", "jsonc", "jsonl": icon("\u{E055}", "#CBCB41", "curlybraces")
        case "toml", "ini", "cfg", "conf", "config", "env", "plist":
            icon("\u{E019}", HideTheme.fileIconNeutralHex, "gearshape")
        case "yaml", "yml": icon("\u{E0A7}", "#A074C4", "list.bullet.indent")
        case "sh", "bash", "zsh", "fish": icon("\u{E089}", "#8DC149", "terminal")
        case "py", "pyw": icon("\u{E07B}", "#519ABA", "chevron.left.forwardslash.chevron.right")
        case "png", "jpg", "jpeg", "gif", "webp", "tiff", "heic", "avif":
            icon("\u{E04C}", "#A074C4", "photo")
        case "svg": icon("\u{E091}", "#A074C4", "scribble.variable")
        case "html", "htm": icon("\u{E048}", "#E37933", "chevron.left.forwardslash.chevron.right")
        case "css", "scss", "sass", "less": icon("\u{E01D}", "#519ABA", "paintbrush")
        case "txt", "log": icon("\u{E023}", HideTheme.fileIconDocumentHex, "doc.text")
        default: fallback
        }
    }

    /// Every glyph the catalog can return, for the coverage check that keeps a
    /// resolved name from ever pointing at a codepoint the bundled subset does
    /// not carry.
    static var allGlyphs: Set<String> {
        var glyphs = Set(namedIcons.values.map(\.glyph))
        let probes = [
            "README.md", "LICENSE", "Dockerfile.dev", ".envrc", ".prettierrc",
            "Cargo.lock", "main.swift", "lib.rs", "app.ts", "App.tsx", "app.js",
            "notes.md", "data.json", "Cargo.toml", "ci.yaml", "run.sh",
            "main.py", "logo.png", "logo.svg", "index.html", "app.css",
            "notes.txt", "CODEOWNERS",
        ]
        for probe in probes { glyphs.insert(icon(fileName: probe).glyph) }
        glyphs.insert(fallback.glyph)
        return glyphs
    }

    private static func icon(_ glyph: String, _ colorHex: String, _ fallback: String) -> SetiFileIcon {
        SetiFileIcon(glyph: glyph, colorHex: colorHex, fallbackSystemImage: fallback)
    }
}

enum SetiIconFont {
    static let postScriptName = "seti"
    static var resourceURL: URL? {
        PackagedResourceBundle.app?.url(forResource: "seti-subset", withExtension: "woff")
    }

    static let isAvailable: Bool = {
        if NSFont(name: postScriptName, size: 12) != nil {
            return true
        }
        guard let url = resourceURL else {
            reportFailure("seti_font.resource_missing", "seti-subset.woff is not bundled")
            return false
        }
        var registrationError: Unmanaged<CFError>?
        let registered = CTFontManagerRegisterFontsForURL(url as CFURL, .process, &registrationError)
        if NSFont(name: postScriptName, size: 12) != nil {
            return true
        }
        let message = registrationError?.takeRetainedValue().localizedDescription
            ?? (registered ? "registered font cannot be resolved by its PostScript name" : "font registration failed")
        reportFailure("seti_font.registration_failed", message)
        return false
    }()

    private static func reportFailure(_ kind: String, _ message: String) {
        let payload: [String: String] = [
            "component": "explorer",
            "kind": kind,
            "message": message,
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: payload),
              var line = String(data: data, encoding: .utf8)
        else { return }
        line.append("\n")
        FileHandle.standardError.write(Data(line.utf8))
    }
}

struct SetiFileIconView: View {
    let url: URL
    var size: CGFloat = 12

    private var icon: SetiFileIcon { SetiFileIconCatalog.icon(for: url) }

    var body: some View {
        Group {
            if SetiIconFont.isAvailable {
                Text(icon.glyph)
                    .font(.custom(SetiIconFont.postScriptName, size: size))
            } else {
                Image(systemName: icon.fallbackSystemImage)
                    .font(.system(size: size - 1))
            }
        }
        .foregroundStyle(HideTheme.color(for: icon.colorHex))
        .frame(width: 16, height: 16)
        .accessibilityHidden(true)
    }
}
