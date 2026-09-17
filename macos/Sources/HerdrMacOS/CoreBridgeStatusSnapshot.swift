import Foundation

struct CoreStatusSnapshot: Decodable {
    let herdr: CoreHerdrStatus
    let remote: [CoreRemoteStatus]
    let chromux: CoreChromuxStatus
    let environment: [CoreEnvironmentStatus]
    let agentHooks: CoreAgentHooks
    let backgroundAI: CoreBackgroundAI
    let diagnostics: [CoreDiagnostic]
    let lastError: CoreLastError?
    let paneFocusRequest: CorePaneFocusRequest?

    enum CodingKeys: String, CodingKey {
        case herdr
        case remote
        case chromux
        case environment
        case agentHooks = "agent_hooks"
        case backgroundAI = "background_ai"
        case diagnostics
        case lastError = "last_error"
        case paneFocusRequest = "pane_focus_request"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        herdr = try container.decode(CoreHerdrStatus.self, forKey: .herdr)
        remote = try container.decodeIfPresent([CoreRemoteStatus].self, forKey: .remote) ?? []
        chromux = try container.decode(CoreChromuxStatus.self, forKey: .chromux)
        environment = try container.decodeIfPresent([CoreEnvironmentStatus].self, forKey: .environment) ?? []
        agentHooks = try container.decodeIfPresent(CoreAgentHooks.self, forKey: .agentHooks)
            ?? CoreAgentHooks()
        backgroundAI = try container.decodeIfPresent(CoreBackgroundAI.self, forKey: .backgroundAI)
            ?? CoreBackgroundAI()
        diagnostics = try container.decodeIfPresent([CoreDiagnostic].self, forKey: .diagnostics) ?? []
        lastError = try container.decodeIfPresent(CoreLastError.self, forKey: .lastError)
        paneFocusRequest = try container.decodeIfPresent(CorePaneFocusRequest.self, forKey: .paneFocusRequest)
    }
}

struct CorePaneFocusRequest: Decodable, Equatable {
    let requestID: String
    let targetPaneID: String
    let phase: String
    let message: String?
    let retryable: Bool

    enum CodingKeys: String, CodingKey {
        case requestID = "request_id"
        case targetPaneID = "target_pane_id"
        case phase
        case message
        case retryable
    }
}
/// Which agent and model the background AI features use, and what each
/// provider can do about it now.
struct CoreBackgroundAI: Decodable, Equatable {
    /// The provider a background request runs on first.
    let provider: String
    /// Whether `provider` is a saved choice rather than the default.
    let chosen: Bool
    /// One row per provider Hide can route to, in the offered order.
    let providers: [CoreBackgroundAIProvider]
    /// Why the saved choice could not be read or written; the defaults are in
    /// use while it is set.
    let unavailableReason: String?

    enum CodingKeys: String, CodingKey {
        case provider
        case chosen
        case providers
        case unavailableReason = "unavailable_reason"
    }

    init(
        provider: String = "codex",
        chosen: Bool = false,
        providers: [CoreBackgroundAIProvider] = [],
        unavailableReason: String? = nil
    ) {
        self.provider = provider
        self.chosen = chosen
        self.providers = providers
        self.unavailableReason = unavailableReason
    }

    init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        provider = try container.decodeIfPresent(String.self, forKey: .provider) ?? "codex"
        chosen = try container.decodeIfPresent(Bool.self, forKey: .chosen) ?? false
        providers = try container.decodeIfPresent([CoreBackgroundAIProvider].self, forKey: .providers) ?? []
        unavailableReason = try container.decodeIfPresent(String.self, forKey: .unavailableReason)
    }

    /// The row for the chosen provider, which is the one the model control
    /// belongs to.
    var selected: CoreBackgroundAIProvider? {
        providers.first { $0.id == provider }
    }
}

struct CoreBackgroundAIProvider: Decodable, Equatable, Identifiable {
    let id: String
    let label: String
    /// The availability class: `ready`, `needs_login`, `not_installed`,
    /// `unavailable`, `unsupported`, or `unread` before it has been asked.
    let state: String
    /// The short words beside the provider's name. The core writes them; no
    /// view builds a sentence out of `state`.
    let headline: String
    /// The provider layer's own reason, when its state carries one.
    let message: String?
    /// The model this provider is asked for.
    let model: String
    /// The models it offers, empty when they are not known.
    let models: [String]
    let modelsUnavailableReason: String?

    enum CodingKeys: String, CodingKey {
        case id
        case label
        case state
        case headline
        case message
        case model
        case models
        case modelsUnavailableReason = "models_unavailable_reason"
    }

    init(
        id: String,
        label: String,
        state: String,
        headline: String,
        message: String? = nil,
        model: String,
        models: [String] = [],
        modelsUnavailableReason: String? = nil
    ) {
        self.id = id
        self.label = label
        self.state = state
        self.headline = headline
        self.message = message
        self.model = model
        self.models = models
        self.modelsUnavailableReason = modelsUnavailableReason
    }

    init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(String.self, forKey: .id)
        label = try container.decodeIfPresent(String.self, forKey: .label) ?? id
        state = try container.decodeIfPresent(String.self, forKey: .state) ?? "unread"
        headline = try container.decodeIfPresent(String.self, forKey: .headline) ?? ""
        message = try container.decodeIfPresent(String.self, forKey: .message)
        model = try container.decodeIfPresent(String.self, forKey: .model) ?? ""
        models = try container.decodeIfPresent([String].self, forKey: .models) ?? []
        modelsUnavailableReason = try container.decodeIfPresent(String.self, forKey: .modelsUnavailableReason)
    }
}

/// What the Settings diagnosis says about agent hooks.
struct CoreHerdrStatus: Decodable {
    let state: String
    let socketPath: String?
    let message: String?
    let expectedProtocol: UInt64?
    let receivedProtocol: UInt64?
    let receivedVersion: String?

    enum CodingKeys: String, CodingKey {
        case state
        case socketPath = "socket_path"
        case message
        case expectedProtocol = "expected_protocol"
        case receivedProtocol = "received_protocol"
        case receivedVersion = "received_version"
    }
}

struct CoreChromuxStatus: Decodable {
    let state: String
    let profile: String
    let message: String?
}

struct CoreEnvironmentStatus: Decodable, Identifiable {
    var id: String { key }
    let key: String
    let required: Bool
    let format: String
    let state: String
    let absentBehavior: String
    let message: String

    enum CodingKeys: String, CodingKey {
        case key
        case required
        case format
        case state
        case absentBehavior = "absent_behavior"
        case message
    }
}

struct CoreDiagnostic: Decodable, Identifiable {
    var id: String { "\(kind)-\(occurredAt)" }
    let kind: String
    let message: String
    let occurredAt: UInt64

    enum CodingKeys: String, CodingKey {
        case kind
        case message
        case occurredAt = "occurred_at"
    }
}

struct CoreLastError: Decodable {
    let kind: String
    let message: String
    let retryable: Bool
    let occurredAt: UInt64

    enum CodingKeys: String, CodingKey {
        case kind
        case message
        case retryable
        case occurredAt = "occurred_at"
    }
}

/// Blocks local-Herdr mutations while commands target a remote device.
///
/// Only terminal events whose core handlers resolve a target-scoped session
/// from the pane ID may cross this boundary. Every other event classified as
/// a local Herdr mutation stays local and is rejected instead of accidentally
/// changing the operator's local session.
