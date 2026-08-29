import AppKit
import CoreGraphics
import Foundation
import Testing
@testable import HerdrMacOS

/// The repository's bundled theme, found from this test file's own location
/// so the suite does not depend on the working directory.
private let repositoryRoot: URL = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent() // HerdrMacOSTests
    .deletingLastPathComponent() // Tests
    .deletingLastPathComponent() // macos
    .deletingLastPathComponent() // repository root
private let bundledThemesRoot = repositoryRoot
    .appendingPathComponent("assets/pet-theme", isDirectory: true)

@Suite("Pet theme loading")
struct PetThemeTests {
    @Test func bundledDefaultThemeCoversEveryPoseTheCoreCanReport() throws {
        let theme = try PetTheme.load(themesRoot: bundledThemesRoot, id: "default")

        #expect(theme.id == "default")
        #expect(Set(theme.states.keys) == PetTheme.requiredStates)
        for pose in PetTheme.requiredStates {
            let state = try #require(theme.state(for: pose))
            #expect(
                FileManager.default.isReadableFile(atPath: state.assetURL.path),
                "\(pose) art must exist at \(state.assetURL.path)"
            )
        }
    }

    @Test func aThemeMissingAPoseIsRejectedRatherThanLeavingThePetBlank() throws {
        let root = try temporaryThemeRoot(states: [
            "idle": ["asset": "assets/idle-fire.webp"],
        ])
        defer { try? FileManager.default.removeItem(at: root) }

        #expect(throws: PetThemeError.self) {
            try PetTheme.load(themesRoot: root, id: "default")
        }
    }

    @Test func aThemeNamingArtThatIsNotThereFailsWithThatPath() throws {
        var states = fullStateMap()
        states["error"] = ["asset": "assets/nope.png"]
        let root = try temporaryThemeRoot(states: states)
        defer { try? FileManager.default.removeItem(at: root) }

        do {
            _ = try PetTheme.load(themesRoot: root, id: "default")
            Issue.record("a missing asset must not load")
        } catch let error as PetThemeError {
            guard case let .missingAsset(pose, path) = error else {
                Issue.record("expected a missing asset error, got \(error)")
                return
            }
            #expect(pose == "error")
            #expect(path.hasSuffix("assets/nope.png"))
        }
    }

    @Test func anUnknownSchemaVersionIsRefusedInsteadOfGuessed() throws {
        let root = try temporaryThemeRoot(states: fullStateMap(), schemaVersion: 99)
        defer { try? FileManager.default.removeItem(at: root) }

        #expect(throws: PetThemeError.unsupportedSchema(99)) {
            try PetTheme.load(themesRoot: root, id: "default")
        }
    }

    @Test func theLocatorPrefersTheBundleAndFallsBackToTheCheckout() {
        #expect(
            PetThemeLocator.themesRoot(bundleResourceURL: nil, repositoryRoot: repositoryRoot)
                == bundledThemesRoot
        )
        #expect(
            PetThemeLocator.themesRoot(
                bundleResourceURL: URL(fileURLWithPath: "/nowhere"),
                repositoryRoot: URL(fileURLWithPath: "/also-nowhere")
            ) == nil,
            "no theme anywhere is reported as absent, not as an empty theme"
        )
    }

    private func fullStateMap() -> [String: [String: Any]] {
        var states: [String: [String: Any]] = [:]
        for pose in PetTheme.requiredStates {
            states[pose] = ["asset": "assets/idle-fire.webp"]
        }
        return states
    }

    private func temporaryThemeRoot(
        states: [String: [String: Any]],
        schemaVersion: Int = PetTheme.schemaVersion
    ) throws -> URL {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("herdr-pet-theme-\(UUID().uuidString)", isDirectory: true)
        let theme = root.appendingPathComponent("default/assets", isDirectory: true)
        try FileManager.default.createDirectory(at: theme, withIntermediateDirectories: true)
        try Data(
            contentsOf: bundledThemesRoot.appendingPathComponent("default/assets/idle-fire.webp")
        )
        .write(to: theme.appendingPathComponent("idle-fire.webp"))
        let manifest: [String: Any] = [
            "schemaVersion": schemaVersion,
            "name": "Fixture",
            "version": "1.0.0",
            "states": states,
        ]
        try JSONSerialization.data(withJSONObject: manifest)
            .write(to: root.appendingPathComponent("default/theme.json"))
        return root
    }
}

@Suite("Pet animation")
struct PetAnimationTests {
    /// T1's conclusion, pinned: the bundled animated art plays straight out
    /// of ImageIO with its own per-frame timing, so no conversion step is
    /// allowed to reappear.
    @Test func animatedWebpArtPlaysItsOwnFramesWithoutConversion() throws {
        let theme = try PetTheme.load(themesRoot: bundledThemesRoot, id: "default")
        let idle = try #require(theme.state(for: "idle"))
        #expect(idle.frames == nil, "an animated file declares no frame count")

        let frames = try PetAnimationLoader.frames(for: idle)
        #expect(frames.count > 1, "idle art is animated")
        #expect(frames.allSatisfy { $0.duration > 0 })
    }

    @Test func spriteSheetArtIsSlicedIntoEqualDistinctFrames() throws {
        let theme = try PetTheme.load(themesRoot: bundledThemesRoot, id: "default")
        let roam = try #require(theme.state(for: "roam"))
        let declared = try #require(roam.frames)

        let frames = try PetAnimationLoader.frames(for: roam)
        #expect(frames.count == declared)
        let widths = Set(frames.map(\.image.width))
        #expect(widths.count == 1, "every sliced frame is the same width")
        let signatures = Set(frames.map { signature($0.image) })
        #expect(
            signatures.count == declared,
            "each slice is a real animation frame, not the same image repeated"
        )
        let total = frames.reduce(0) { $0 + $1.duration }
        #expect(abs(total - Double(roam.durationMilliseconds ?? 0) / 1_000) < 0.001)
    }

    @Test func aStaticPoseYieldsExactlyOneFrame() throws {
        let theme = try PetTheme.load(themesRoot: bundledThemesRoot, id: "default")
        let frames = try PetAnimationLoader.frames(for: try #require(theme.state(for: "sleeping")))
        #expect(frames.count == 1)
    }

    @Test func aSheetThatDoesNotDivideIsRefusedRatherThanRenderedSkewed() throws {
        let theme = try PetTheme.load(themesRoot: bundledThemesRoot, id: "default")
        let roam = try #require(theme.state(for: "roam"))
        let indivisible = PetThemeState(
            assetURL: roam.assetURL,
            frames: 7,
            durationMilliseconds: 800
        )
        #expect(throws: PetAnimationError.self) {
            try PetAnimationLoader.frames(for: indivisible)
        }
    }

    private func signature(_ image: CGImage) -> String {
        let side = 8
        var buffer = [UInt8](repeating: 0, count: side * side * 4)
        let context = CGContext(
            data: &buffer,
            width: side,
            height: side,
            bitsPerComponent: 8,
            bytesPerRow: side * 4,
            space: CGColorSpaceCreateDeviceRGB(),
            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        )
        context?.draw(image, in: CGRect(x: 0, y: 0, width: side, height: side))
        return buffer.map { String(format: "%02x", $0) }.joined()
    }
}

@Suite("Pet URL scheme")
struct PetURLCommandTests {
    @Test func everyCommandParsesWithOrWithoutATrailingSlash() throws {
        for command in PetURLCommand.allCases {
            for spelling in [
                "herdr-ide://\(command.rawValue)",
                "herdr-ide://\(command.rawValue)/",
                "herdr-ide://\(command.rawValue)//",
                "herdr-ide:\(command.rawValue)",
                "HERDR-IDE://\(command.rawValue.uppercased())",
            ] {
                let url = try #require(URL(string: spelling), "\(spelling) is a URL")
                #expect(
                    PetURLCommand.parse(url) == command,
                    "\(spelling) must mean \(command.rawValue)"
                )
            }
        }
    }

    @Test func theRetiredPetSchemeAndUnknownCommandsAreNotAccepted() throws {
        for rejected in [
            "herdr-pet://toggle",
            "herdr-pet://show",
            "herdr-ide://quit",
            "herdr-ide://",
            "https://example.com/toggle",
        ] {
            let url = try #require(URL(string: rejected))
            #expect(
                PetURLCommand.parse(url) == nil,
                "\(rejected) must not drive the pet"
            )
        }
    }
}

@Suite("Pet global shortcut")
struct PetHotkeyTests {
    @Test func anAcceleratorIsBuiltFromThePhysicalKeyNotTheTypedCharacter() throws {
        // Option+P reports "π" as the character on macOS. Binding that would
        // produce a shortcut that never fires, so identity is the key code.
        let optionP = try #require(
            NSEvent.keyEvent(
                with: .keyDown,
                location: .zero,
                modifierFlags: [.option, .command],
                timestamp: 0,
                windowNumber: 0,
                context: nil,
                characters: "\u{3c0}",
                charactersIgnoringModifiers: "\u{3c0}",
                isARepeat: false,
                keyCode: 35 // kVK_ANSI_P
            )
        )
        let captured = try #require(PetHotkey.capture(from: optionP))
        #expect(captured.canonical == "command+option+p")
        #expect(captured.keyCode == 35)
        #expect(captured.displayGlyphs == "⌥⌘P")
    }

    @Test func modifierOnlyAndBareKeyPressesAreNotCompleteBindings() throws {
        let bare = try #require(
            NSEvent.keyEvent(
                with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0,
                windowNumber: 0, context: nil, characters: "p",
                charactersIgnoringModifiers: "p", isARepeat: false, keyCode: 35
            )
        )
        #expect(
            PetHotkey.capture(from: bare) == nil,
            "a bare key would swallow that key system-wide"
        )

        let unknownKey = try #require(
            NSEvent.keyEvent(
                with: .keyDown, location: .zero, modifierFlags: [.command], timestamp: 0,
                windowNumber: 0, context: nil, characters: "", charactersIgnoringModifiers: "",
                isARepeat: false, keyCode: 250
            )
        )
        #expect(PetHotkey.capture(from: unknownKey) == nil)
    }

    @Test func storedAcceleratorsRoundTripAndInvalidOnesAreNamed() throws {
        let parsed = try PetHotkey.parse("command+option+p")
        #expect(parsed.canonical == "command+option+p")
        #expect(parsed.keyCode == PetHotkey.keyCode(for: "p"))

        #expect(throws: PetHotkeyError.invalidFormat) { try PetHotkey.parse("p") }
        #expect(throws: PetHotkeyError.invalidFormat) { try PetHotkey.parse("command+command+p") }
        #expect(throws: PetHotkeyError.unsupportedKey) { try PetHotkey.parse("command+\u{3c0}") }
    }

    @MainActor
    @Test func anEmptyAcceleratorRegistersNothingAndReportsNoError() {
        let registrar = PetHotkeyRegistrar {}
        for blank in [nil, "", "   "] {
            #expect(registrar.apply(accelerator: blank) == nil)
            #expect(registrar.registered == nil, "nothing is registered for a blank binding")
            #expect(registrar.lastError == nil)
        }
    }

    @MainActor
    @Test func anUnparsableStoredAcceleratorSurfacesInsteadOfSilentlyNotFiring() {
        let registrar = PetHotkeyRegistrar {}
        let failure = registrar.apply(accelerator: "not-a-chord")
        #expect(failure != nil)
        #expect(registrar.registered == nil)
        #expect(registrar.lastError != nil)
    }
}

@Suite("Pet placement and gestures")
struct PetPlacementTests {
    /// The recorded incident: `~/.config/herdr-pet/window.json` held
    /// `[542720, 163840]`, `show()` succeeded, and the user saw nothing.
    @Test func theRecordedOffscreenIncidentCoordinateComesBackOnScreen() {
        let visible = CGRect(x: 0, y: 25, width: 1_440, height: 875)
        let resolved = PetPlacement.clampedOrigin(
            requested: CGPoint(x: 542_720, y: 163_840),
            windowSize: PetPlacement.windowSize,
            visibleFrames: [visible]
        )
        #expect(visible.contains(resolved))
        #expect(resolved == CGPoint(x: 1_312, y: 772))
    }

    @Test func anOriginOnAScreenThatIsNoLongerConnectedMovesToOneThatIs() {
        let remaining = CGRect(x: 0, y: 25, width: 1_440, height: 875)
        let onUnpluggedDisplay = CGPoint(x: -1_800, y: 900)
        let resolved = PetPlacement.clampedOrigin(
            requested: onUnpluggedDisplay,
            windowSize: PetPlacement.windowSize,
            visibleFrames: [remaining]
        )
        #expect(remaining.contains(resolved))
    }

    @Test func clampingIsStableWhenRepeated() {
        let visible = CGRect(x: -1_920, y: 0, width: 1_920, height: 1_080)
        let first = PetPlacement.clampedOrigin(
            requested: CGPoint(x: -200, y: 100),
            windowSize: PetPlacement.windowSize,
            visibleFrames: [visible]
        )
        let second = PetPlacement.clampedOrigin(
            requested: first,
            windowSize: PetPlacement.windowSize,
            visibleFrames: [visible]
        )
        #expect(first == second)
    }

    @Test func aReleaseWithinTheThresholdIsAClickAndFurtherIsADrag() {
        let start = CGPoint(x: 100, y: 100)
        #expect(!PetGesture.isDrag(from: start, to: start))
        #expect(!PetGesture.isDrag(from: start, to: CGPoint(x: 102, y: 100)))
        #expect(PetGesture.isDrag(from: start, to: CGPoint(x: 110, y: 100)))
    }

    @Test func draggingKeepsTheCursorAtTheSamePointOnThePet() {
        let anchor = CGSize(width: 30, height: 40)
        // A long drag stays anchored: the origin is always cursor minus the
        // offset captured at press time, never an accumulated delta.
        for cursor in [CGPoint(x: 500, y: 500), CGPoint(x: 1_400, y: 80)] {
            let origin = PetGesture.anchoredOrigin(cursor: cursor, anchor: anchor)
            #expect(cursor.x - origin.x == anchor.width)
            #expect(cursor.y - origin.y == anchor.height)
        }
    }
}
