import Foundation

struct CoreAgentHooks: Decodable, Equatable {
    /// One row per runtime Hide has an adapter for. A runtime that is not on
    /// this Mac is still a row: "not here" and "not installed" differ.
    let runtimes: [CoreAgentHookRuntime]
    /// Panes running a session that started before the hook was installed.
    /// These are the ones a restart would fix.
    let sessionsPredatingInstall: [CoreAgentHookPane]
    /// The last hook report Herdr did not take, when the most recent one
    /// failed. While this is set a restart is not the fix.
    let lastReportFailure: String?

    enum CodingKeys: String, CodingKey {
        case runtimes
        case sessionsPredatingInstall = "sessions_predating_install"
        case lastReportFailure = "last_report_failure"
    }

    init(
        runtimes: [CoreAgentHookRuntime] = [],
        sessionsPredatingInstall: [CoreAgentHookPane] = [],
        lastReportFailure: String? = nil
    ) {
        self.runtimes = runtimes
        self.sessionsPredatingInstall = sessionsPredatingInstall
        self.lastReportFailure = lastReportFailure
    }

    init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        runtimes = try container.decodeIfPresent([CoreAgentHookRuntime].self, forKey: .runtimes) ?? []
        sessionsPredatingInstall =
            try container.decodeIfPresent([CoreAgentHookPane].self, forKey: .sessionsPredatingInstall) ?? []
        lastReportFailure = try container.decodeIfPresent(String.self, forKey: .lastReportFailure)
    }
}
struct CoreAgentHookRuntime: Decodable, Equatable, Identifiable {
    let id: String
    let label: String
    /// The configuration file this row describes, so the operator can look.
    let path: String
    /// The short word beside the runtime's name. The core writes it; no view
    /// builds a sentence out of a status name.
    let headline: String
    let installed: Bool
    /// Whether the operator can be offered an install. Hide never reinstalls
    /// on its own after the first run.
    let offersInstall: Bool

    enum CodingKeys: String, CodingKey {
        case id
        case label
        case path
        case headline
        case installed
        case offersInstall = "offers_install"
    }

    init(
        id: String,
        label: String,
        path: String,
        headline: String,
        installed: Bool = false,
        offersInstall: Bool = false
    ) {
        self.id = id
        self.label = label
        self.path = path
        self.headline = headline
        self.installed = installed
        self.offersInstall = offersInstall
    }
}

struct CoreAgentHookPane: Decodable, Equatable, Identifiable {
    var id: String { paneID }
    let paneID: String
    let label: String
    let message: String

    enum CodingKeys: String, CodingKey {
        case paneID = "pane_id"
        case label
        case message
    }

    init(paneID: String, label: String, message: String) {
        self.paneID = paneID
        self.label = label
        self.message = message
    }
}
