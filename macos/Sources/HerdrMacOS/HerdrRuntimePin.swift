import Foundation

/// A structured line on stderr for a failure the operator has to act on but no
/// UI can describe. One writer, so every such event has the same shape.
enum HideDiagnostic {
    static func emit(component: String, kind: String, message: String) {
        let payload = ["component": component, "kind": kind, "message": message]
        guard let data = try? JSONSerialization.data(withJSONObject: payload),
              var line = String(data: data, encoding: .utf8)
        else { return }
        line.append("\n")
        FileHandle.standardError.write(Data(line.utf8))
    }
}

/// The pinned Herdr runtime, as shipped in `herdr-bundle.json`.
struct HerdrRuntimePin: Equatable, Sendable {
    let version: String
    let sha256: String
}

/// Reads the pin from the manifest the app ships rather than from a constant
/// compiled beside it. The manifest lives in this target's `Resources/`, so
/// SwiftPM packages it into the resource bundle the app and `swift test` both
/// load. `herdr-bundle.json` is the single source: `build.rs`
/// generates the core's `BUNDLED_HERDR_VERSION` from the same file and
/// `scripts/build-app.sh` downloads and verifies the binary against it.
enum HerdrRuntimePinLoader {
    static let pinned: HerdrRuntimePin? = load()

    /// Returns nil rather than a default pin. A pin that fell back to a
    /// placeholder would silently accept an unverified runtime binary, so the
    /// caller must refuse to resolve one instead.
    static func load(bundle: Bundle? = PackagedResourceBundle.app) -> HerdrRuntimePin? {
        func fail(_ message: String) -> HerdrRuntimePin? {
            HideDiagnostic.emit(
                component: "runtime_pin",
                kind: "manifest.unreadable",
                message: message
            )
            HideLaunchTrace.mark("runtime_pin.failed", detail: "manifest_unreadable")
            return nil
        }

        guard let url = bundle?.url(forResource: "herdr-bundle", withExtension: "json") else {
            return fail("herdr-bundle.json is not present in the packaged resources")
        }
        guard let data = try? Data(contentsOf: url) else {
            return fail("herdr-bundle.json could not be read at \(url.path)")
        }
        guard let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            return fail("herdr-bundle.json is not a JSON object")
        }
        guard let version = (object["version"] as? String)?.trimmingCharacters(in: .whitespaces),
              !version.isEmpty
        else {
            return fail("herdr-bundle.json has no version string")
        }
        guard let sha256 = (object["sha256"] as? String)?.trimmingCharacters(in: .whitespaces),
              sha256.count == 64
        else {
            return fail("herdr-bundle.json has no 64-character sha256 string")
        }
        return HerdrRuntimePin(version: version, sha256: sha256)
    }
}
