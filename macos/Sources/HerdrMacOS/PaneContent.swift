import Foundation

/// Herdr owns the leaf's identity and geometry; this describes what Hide draws
/// inside it. Browser identities are independent of transient CDP endpoints.
enum CorePaneContent: Decodable, Equatable, Sendable {
    case terminal
    case browser(BrowserPaneBinding)
    case unavailable(String)

    var closeConsequence: String? {
        switch self {
        case .terminal: nil
        case .browser(let binding): binding.closeConsequence
        case .unavailable:
            "Closing this pane stops its content host. Browser tabs created by that host will also close; attached existing tabs stay open."
        }
    }

    private enum CodingKeys: String, CodingKey {
        case kind, reason
    }

    init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        switch try container.decode(String.self, forKey: .kind) {
        case "terminal": self = .terminal
        case "browser": self = .browser(try BrowserPaneBinding(from: decoder))
        case "unavailable": self = .unavailable(try container.decode(String.self, forKey: .reason))
        default:
            throw DecodingError.dataCorruptedError(
                forKey: .kind, in: container, debugDescription: "Unsupported pane content kind"
            )
        }
    }
}

struct BrowserPaneBinding: Decodable, Equatable, Hashable, Sendable {
    let bindingID: String
    let profile: String
    let targetID: String
    let session: String
    let cdpPort: UInt16
    let ownsTarget: Bool

    var closeConsequence: String {
        ownsTarget
            ? "Closing this pane also closes its Chromium tab. Unsaved page input may be lost."
            : "Closing this pane detaches its viewer. The existing browser tab and session stay open."
    }

    enum CodingKeys: String, CodingKey {
        case bindingID = "binding_id"
        case profile
        case targetID = "target_id"
        case session
        case cdpPort = "cdp_port"
        case ownsTarget = "owns_target"
    }
}
