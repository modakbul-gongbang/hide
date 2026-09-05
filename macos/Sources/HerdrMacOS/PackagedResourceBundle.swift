import Foundation

private final class PackagedResourceMarker: NSObject {}

enum PackagedResourceBundle {
    static let app: Bundle? = {
        let name = "HerdrMacOS_HerdrMacOS.bundle"
        let containers = [Bundle.main, Bundle(for: PackagedResourceMarker.self)]
            + Bundle.allBundles
            + Bundle.allFrameworks
        for container in containers {
            var roots = [
                container.bundleURL,
                container.bundleURL.deletingLastPathComponent(),
            ]
            if let resourceURL = container.resourceURL {
                roots.insert(resourceURL, at: 0)
            }
            for root in roots {
                if let bundle = Bundle(url: root.appendingPathComponent(name, isDirectory: true)) {
                    return bundle
                }
            }
        }

        HideDiagnostic.emit(
            component: "app_resources",
            kind: "resource_bundle.missing",
            message: "\(name) is not readable from the packaged app or executable directory"
        )
        return nil
    }()
}
