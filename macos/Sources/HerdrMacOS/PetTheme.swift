import Foundation

/// One pose's art. `frames` is set only for a horizontal sprite sheet; an
/// animated file (webp with an ANIM chunk) carries its own frame timing and
/// declares nothing here.
struct PetThemeState: Equatable {
    let assetURL: URL
    let frames: Int?
    let durationMilliseconds: Int?
}

/// `theme.json` is the source of truth for the pose-to-asset mapping.
///
/// Every pose the core can report must be present: a theme that cannot draw
/// a pose is rejected at load rather than leaving the pet blank when that
/// pose first occurs.
struct PetTheme: Equatable {
    let id: String
    let name: String
    let version: String
    let states: [String: PetThemeState]

    /// Exactly the poses `herdr_core`'s pet block can emit.
    static let requiredStates: Set<String> = [
        "idle",
        "working",
        "carrying",
        "juggling",
        "notification",
        "error",
        "disconnected",
        "roam",
        "waking",
        "yawning",
        "dozing",
        "collapsing",
        "sleeping",
    ]

    static let schemaVersion = 2

    func state(for pose: String) -> PetThemeState? {
        states[pose]
    }

    static func load(themesRoot: URL, id: String) throws -> PetTheme {
        let root = themesRoot.appendingPathComponent(id, isDirectory: true)
        let manifestURL = root.appendingPathComponent("theme.json")
        let data: Data
        do {
            data = try Data(contentsOf: manifestURL)
        } catch {
            throw PetThemeError.manifestUnreadable(manifestURL.path)
        }
        guard let manifest = try? JSONDecoder().decode(Manifest.self, from: data) else {
            throw PetThemeError.manifestUnreadable(manifestURL.path)
        }
        guard manifest.schemaVersion == schemaVersion else {
            throw PetThemeError.unsupportedSchema(manifest.schemaVersion)
        }

        let missing = requiredStates.subtracting(manifest.states.keys).sorted()
        guard missing.isEmpty else {
            throw PetThemeError.missingStates(missing)
        }

        var states: [String: PetThemeState] = [:]
        for (pose, entry) in manifest.states {
            let assetURL = root.appendingPathComponent(entry.asset)
            guard FileManager.default.isReadableFile(atPath: assetURL.path) else {
                throw PetThemeError.missingAsset(pose: pose, path: assetURL.path)
            }
            if let frames = entry.frames, frames < 1 {
                throw PetThemeError.invalidFrameCount(pose: pose, frames: frames)
            }
            states[pose] = PetThemeState(
                assetURL: assetURL,
                frames: entry.frames,
                durationMilliseconds: entry.durationMs
            )
        }
        return PetTheme(
            id: id,
            name: manifest.name,
            version: manifest.version,
            states: states
        )
    }

    private struct Manifest: Decodable {
        let schemaVersion: Int
        let name: String
        let version: String
        let states: [String: Entry]

        struct Entry: Decodable {
            let asset: String
            let frames: Int?
            let durationMs: Int?
        }
    }
}

enum PetThemeError: Error, Equatable, LocalizedError {
    case manifestUnreadable(String)
    case unsupportedSchema(Int)
    case missingStates([String])
    case missingAsset(pose: String, path: String)
    case invalidFrameCount(pose: String, frames: Int)

    var errorDescription: String? {
        switch self {
        case let .manifestUnreadable(path):
            "The pet theme manifest at \(path) could not be read."
        case let .unsupportedSchema(version):
            "The pet theme declares schema version \(version); this build reads version \(PetTheme.schemaVersion)."
        case let .missingStates(states):
            "The pet theme is missing art for: \(states.joined(separator: ", "))."
        case let .missingAsset(pose, path):
            "The pet theme's \(pose) art is missing at \(path)."
        case let .invalidFrameCount(pose, frames):
            "The pet theme's \(pose) sprite sheet declares \(frames) frames."
        }
    }
}

/// Where the bundled themes live.
///
/// The installed bundle carries them beside its other resources; a `swift
/// run` build from the checkout has no bundle, so the repository copy is the
/// fallback and a failure to find either is stated rather than silently
/// drawing nothing.
enum PetThemeLocator {
    static func themesRoot(
        bundleResourceURL: URL? = Bundle.main.resourceURL,
        repositoryRoot: URL? = nil,
        fileManager: FileManager = .default
    ) -> URL? {
        var candidates: [URL] = []
        if let bundleResourceURL {
            candidates.append(bundleResourceURL.appendingPathComponent("pet-theme", isDirectory: true))
        }
        if let repositoryRoot {
            candidates.append(
                repositoryRoot
                    .appendingPathComponent("assets", isDirectory: true)
                    .appendingPathComponent("pet-theme", isDirectory: true)
            )
        }
        return candidates.first { candidate in
            fileManager.isReadableFile(atPath: candidate.appendingPathComponent("default/theme.json").path)
        }
    }
}
