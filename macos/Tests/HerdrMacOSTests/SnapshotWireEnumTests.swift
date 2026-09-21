import Foundation
import Testing
@testable import HerdrMacOS

/// The core is the writer of every string enum in the snapshot and this shell
/// decodes them strictly, so one value the shell does not know fails the whole
/// snapshot decode and freezes the shell on its last good frame. The values
/// the core emits are pinned in `contracts/snapshot-wire-enums.json` by the
/// core's own test; this test proves every pinned value decodes here and that
/// no Swift case exists without a pinned value behind it.
@Suite struct SnapshotWireEnumTests {
    private static let contract: [String: [String]] = {
        let repositoryRoot = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
            .deletingLastPathComponent()
        let url = repositoryRoot.appendingPathComponent("contracts/snapshot-wire-enums.json")
        let data = try! Data(contentsOf: url)
        return try! JSONDecoder().decode([String: [String]].self, from: data)
    }()

    private func decodeEach<T: Decodable & CaseIterable & Equatable>(
        _ key: String,
        as type: T.Type,
        sourceLocation: SourceLocation = #_sourceLocation
    ) throws {
        let values = try #require(Self.contract[key], "\(key) is listed in the contract", sourceLocation: sourceLocation)
        var decoded: [T] = []
        for value in values {
            let payload = Data("\"\(value)\"".utf8)
            decoded.append(try JSONDecoder().decode(T.self, from: payload))
        }
        #expect(
            decoded.count == T.allCases.count,
            "\(key): the contract lists \(values.count) values, Swift has \(T.allCases.count) cases",
            sourceLocation: sourceLocation
        )
        for expected in T.allCases {
            #expect(decoded.contains(expected), "\(key): no contract value decodes to \(expected)", sourceLocation: sourceLocation)
        }
    }

    @Test func everyPinnedValueDecodesAndNoCaseIsUnpinned() throws {
        try decodeEach("changed_file_status", as: CoreChangedFileStatus.self)
        try decodeEach("checkout_purpose_origin", as: CoreCheckoutPurpose.Origin.self)
        try decodeEach("document_kind", as: CoreDocumentKind.self)
        try decodeEach("editor_tab_kind", as: CoreEditorTabKind.self)
        try decodeEach("pull_request_badge", as: CorePullRequestBadge.self)
        try decodeEach("pull_request_checks", as: CorePullRequestChecks.self)
        try decodeEach("review_decision", as: CoreReviewDecision.self)
        try decodeEach("right_panel_section", as: RightPanelSection.self)
        try decodeEach("strip_tab_kind", as: CoreStripTabSnapshot.Kind.self)
        let pinned = Set(Self.contract.keys)
        let checked: Set<String> = [
            "changed_file_status", "checkout_purpose_origin", "document_kind", "editor_tab_kind",
            "pull_request_badge", "pull_request_checks", "review_decision", "right_panel_section",
            "strip_tab_kind",
        ]
        #expect(pinned == checked, "contract lists enums this test does not decode: \(pinned.subtracting(checked).sorted())")
    }
}
