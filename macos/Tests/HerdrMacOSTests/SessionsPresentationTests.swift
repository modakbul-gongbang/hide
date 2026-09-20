import Foundation
import Testing
@testable import HerdrMacOS

@Suite("Sessions and Memory presentation")
struct SessionsPresentationTests {
    private func snapshot(
        query: String = "",
        loading: Bool = false,
        unavailable: String? = nil,
        rows: [CoreSessionRowSnapshot] = [],
        total: Int = 0,
        memories: [CoreMemoryRowSnapshot] = [],
        enabled: Bool = false
    ) -> CoreSessionsSnapshot {
        CoreSessionsSnapshot(
            projectID: "project-1",
            checkoutPath: "/fixture/project",
            mode: .sessions,
            providerFilter: .all,
            query: query,
            loading: loading,
            unavailableReason: unavailable,
            rows: rows,
            totalSessionCount: total,
            memories: memories,
            memoryEnabled: enabled,
            memoryDisclosureAccepted: enabled,
            memoryActiveCount: memories.count,
            memoryConflictCount: 0,
            memoryCapacityReached: false,
            analysis: .idle,
            notice: nil,
            thisTurnMemoryIDs: []
        )
    }

    private let session = CoreSessionRowSnapshot(
        id: "session-1",
        provider: "codex",
        providerLabel: "Codex",
        locator: "/fixture/session.jsonl",
        checkoutPath: "/fixture/project",
        firstHumanRequest: "프로젝트 기억을 검토해줘",
        startedAtUnixMS: 1,
        updatedAtUnixMS: 1,
        title: nil,
        unavailableReason: nil
    )

    private let memory = CoreMemoryRowSnapshot(
        id: "memory-1",
        body: "Keep Project identity canonical across worktrees.",
        lifecycle: "active",
        revision: 1,
        sourceCount: 2,
        providedSessionCount: 1,
        updatedAtUnixMS: 1
    )

    @Test func modesAndProviderFiltersUseTheExactOperatorCopy() {
        #expect(CoreSessionsMode.allCases.map(\.title) == ["Sessions", "Memory"])
        #expect(CoreSessionsProviderFilter.allCases.map(\.title) == ["All", "Codex", "Claude Code"])
    }

    @Test func sessionStatesSeparateRemoteLoadingFailureEmptyNoMatchAndRows() {
        #expect(SessionsPresentation.sessions(snapshot(), remote: true) == .remote)
        #expect(SessionsPresentation.sessions(snapshot(loading: true), remote: false) == .loading)
        #expect(SessionsPresentation.sessions(snapshot(unavailable: "Unreadable"), remote: false) == .unavailable("Unreadable"))
        #expect(SessionsPresentation.sessions(snapshot(), remote: false) == .empty)
        #expect(SessionsPresentation.sessions(snapshot(query: "none", total: 2), remote: false) == .noMatches)
        #expect(SessionsPresentation.sessions(snapshot(rows: [session], total: 1), remote: false) == .rows)
    }

    @Test func anUnreadableSessionDoesNotReplaceReadableRowsWithAFailureScreen() {
        let unavailable = CoreSessionRowSnapshot(
            id: "session-2",
            provider: "claude",
            providerLabel: "Claude Code",
            locator: "/fixture/moved.jsonl",
            checkoutPath: "/fixture/project",
            firstHumanRequest: "한글 English source를 확인해줘",
            startedAtUnixMS: 2,
            updatedAtUnixMS: 2,
            title: nil,
            unavailableReason: "Session source moved"
        )

        #expect(SessionsPresentation.sessions(
            snapshot(rows: [session, unavailable], total: 2),
            remote: false
        ) == .rows)
        #expect(SessionsPresentation.sessionAccessibility(unavailable).hasSuffix(", Session unavailable"))
        #expect(SessionsPresentation.sessionAccessibility(unavailable).contains("한글 English source를 확인해줘"))
    }

    @Test func memoryStatesSeparateOffEmptyNoMatchAndRows() {
        #expect(SessionsPresentation.memory(snapshot()) == .off)
        #expect(SessionsPresentation.memory(snapshot(enabled: true)) == .empty)
        #expect(SessionsPresentation.memory(snapshot(query: "none", enabled: true)) == .noMatches)
        #expect(SessionsPresentation.memory(snapshot(memories: [memory], enabled: true)) == .rows)
    }

    @Test func attachedAndProvidedCopyOmitZeroExactly() {
        #expect(SessionsPresentation.attachedLabel(0) == nil)
        #expect(SessionsPresentation.attachedLabel(2) == "Memory attached 2")
        #expect(SessionsPresentation.readyLabel(0) == nil)
        #expect(SessionsPresentation.readyLabel(3) == "Project Memory ready · 3")
    }

    @Test func analysisCopyDistinguishesRunningFromPausedAndCompleted() {
        #expect(SessionsPresentation.analysisLabel(.init(
            state: "analyzing", discovered: 4, analyzed: 2, failed: 0,
            message: "Analyzing 2 of 4 sessions", action: nil
        )) == "Analyzing 2 of 4 sessions")
        #expect(SessionsPresentation.analysisLabel(.init(
            state: "paused", discovered: 4, analyzed: 2, failed: 1,
            message: "Analysis paused", action: "retry"
        )) == "Analysis paused")
        #expect(SessionsPresentation.analysisLabel(.init(
            state: "complete", discovered: 4, analyzed: 3, failed: 1,
            message: "3 analyzed · 1 failed", action: "retry"
        )) == "3 analyzed · 1 failed")
    }

    @Test func normalIdleAndZeroAnalysisDoNotProduceAnActionableNotice() {
        #expect(SessionsPresentation.analysisLabel(.idle) == nil)
        #expect(SessionsPresentation.analysisLabel(.init(
            state: "complete", discovered: 0, analyzed: 0, failed: 0,
            message: "0 analyzed", action: nil
        )) == nil)
    }

    @Test func accessibilityReadsFieldsInTheContractOrder() {
        let sessionLabel = SessionsPresentation.sessionAccessibility(session)
        #expect(sessionLabel.hasPrefix("Codex, 프로젝트 기억을 검토해줘, project,"))
        #expect(sessionLabel.hasSuffix(", Available"))
        #expect(SessionsPresentation.memoryAccessibility(memory) == "Keep Project identity canonical across worktrees., 2 sources, active")
    }

    @Test func sessionAndMemoryPayloadsDecodeThisTurnConflictAndAvailability() throws {
        let data = Data("""
        {
          "project_id":"project-1","checkout_path":"/fixture/project","mode":"memory",
          "provider_filter":"claude","query":"규칙","loading":false,"unavailable_reason":null,
          "rows":[],"total_session_count":2,
          "memories":[{"id":"m1","body":"규칙","lifecycle":"conflicting","revision":2,"source_count":2,"provided_session_count":3,"updated_at_unix_ms":4}],
          "memory_enabled":true,"memory_disclosure_accepted":true,"memory_active_count":1,
          "memory_conflict_count":1,"memory_capacity_reached":false,
          "analysis":{"state":"paused","discovered":2,"analyzed":1,"failed":1,"message":"Analysis paused","action":"retry"},
          "notice":null,"this_turn_memory_ids":["m1"]
        }
        """.utf8)
        let decoded = try JSONDecoder().decode(CoreSessionsSnapshot.self, from: data)
        #expect(decoded.mode == .memory)
        #expect(decoded.providerFilter == .claude)
        #expect(decoded.thisTurnMemoryIDs == ["m1"])
        #expect(decoded.memories.first?.lifecycle == "conflicting")
        #expect(decoded.analysis.action == "retry")
    }
}
