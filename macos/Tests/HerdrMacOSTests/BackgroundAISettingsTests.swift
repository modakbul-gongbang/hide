import Foundation
import Testing

@testable import HerdrMacOS

/// Pins the Swift side of the Background AI wire: the keys the core emits for
/// the operator's choice, each provider's availability and its model list, and
/// the payloads the settings group dispatches back.

@Test func theBackgroundAISectionDecodesFromTheCoresKeys() throws {
    let payload = """
    {
        "provider": "claude",
        "chosen": true,
        "providers": [
            {"id": "codex", "label": "Codex", "state": "needs_login",
             "headline": "Sign in required", "message": "Run `codex login` and check again",
             "model": "gpt-5.6-luna", "models": [], "models_unavailable_reason": "codex_not_installed"},
            {"id": "claude", "label": "Claude Code", "state": "ready",
             "headline": "Signed in", "message": null,
             "model": "sonnet", "models": ["haiku", "sonnet", "opus", "fable"],
             "models_unavailable_reason": null}
        ],
        "unavailable_reason": null
    }
    """
    let section = try JSONDecoder().decode(CoreBackgroundAI.self, from: Data(payload.utf8))
    #expect(section.provider == "claude")
    #expect(section.chosen)
    #expect(section.providers.map(\.id) == ["codex", "claude"])
    #expect(section.selected?.label == "Claude Code")
    #expect(section.selected?.model == "sonnet")
    #expect(section.selected?.models == ["haiku", "sonnet", "opus", "fable"])
    #expect(
        section.providers[0].message == "Run `codex login` and check again",
        "the provider layer's own reason reaches the screen rather than being reworded here"
    )
    #expect(
        section.providers[0].modelsUnavailableReason == "codex_not_installed",
        "an unknown model list is unknown, not an empty list"
    )
}

/// A section the core has not filled in yet still decodes, and reads as
/// unchosen defaults rather than as a failure.
@Test func aBackgroundAISectionWithNothingReadYetDecodesAsDefaults() throws {
    let section = try JSONDecoder().decode(
        CoreBackgroundAI.self,
        from: Data("{}".utf8)
    )
    #expect(section.provider == "codex")
    #expect(!section.chosen)
    #expect(section.providers.isEmpty)
    #expect(section.selected == nil)
    #expect(section.unavailableReason == nil)
}

/// The status section carries it, so the shell reads one snapshot rather than
/// asking the providers anything itself.
@Test func theStatusSnapshotCarriesTheBackgroundAISection() throws {
    let payload = """
    {
        "herdr": {"state": "connected", "message": null},
        "chromux": {"state": "idle", "profile": "hide", "current_url": null,
                    "current_title": null, "message": null, "last_checked_at_unix_ms": null},
        "background_ai": {"provider": "codex", "chosen": false,
            "providers": [{"id": "codex", "label": "Codex", "state": "unread",
                           "headline": "Not checked yet", "message": null,
                           "model": "gpt-5.6-luna", "models": [],
                           "models_unavailable_reason": null}],
            "unavailable_reason": null}
    }
    """
    let status = try JSONDecoder().decode(CoreStatusSnapshot.self, from: Data(payload.utf8))
    #expect(status.backgroundAI.providers.first?.state == "unread")
    #expect(status.backgroundAI.providers.first?.headline == "Not checked yet")
}

/// A core that does not emit the section at all is an older core, not a
/// broken one: the screen falls back to the defaults instead of failing to
/// decode the whole status.
@Test func aStatusSnapshotWithoutTheSectionStillDecodes() throws {
    let payload = """
    {
        "herdr": {"state": "connected", "message": null},
        "chromux": {"state": "idle", "profile": "hide", "current_url": null,
                    "current_title": null, "message": null, "last_checked_at_unix_ms": null}
    }
    """
    let status = try JSONDecoder().decode(CoreStatusSnapshot.self, from: Data(payload.utf8))
    #expect(status.backgroundAI.provider == "codex")
    #expect(status.backgroundAI.providers.isEmpty)
}
