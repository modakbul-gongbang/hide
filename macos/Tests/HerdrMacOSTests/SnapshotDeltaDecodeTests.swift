import Foundation
import Testing
@testable import HerdrMacOS

/// Pins the Swift side of the delta snapshot wire: the envelope keys the
/// core emits (`revision`, `terminal_sequence`, `chunks`, `chunks_dropped`)
/// and the omission semantics for caught-up sections.
@Test func chunkOnlyDeltaDecodesWithoutSections() throws {
    let payload = """
    {
        "schema_version": 2,
        "revision": 7,
        "rest": null,
        "editor": null,
        "input_generation": 3,
        "find": {
            "pane_id": null,
            "term": "",
            "index": 0,
            "total": 0,
            "truncated": false,
            "unavailable_reason": null
        },
        "terminal_sequence": 42,
        "chunks": [
            {"pane_id": "w1:p1", "sequence": 41, "bytes_base64": "aGk="},
            {"pane_id": "w1:p1", "sequence": 42, "bytes_base64": "bW8="}
        ],
        "chunks_dropped": false
    }
    """
    let decoded = try JSONDecoder().decode(
        CoreSnapshotDelta.self,
        from: Data(payload.utf8)
    )

    #expect(decoded.schemaVersion == 2)
    #expect(decoded.revision == 7)
    #expect(decoded.rest == nil)
    #expect(decoded.editor == nil)
    #expect(decoded.terminalSequence == 42)
    #expect(decoded.chunksDropped == false)
    #expect(decoded.chunks.map(\.sequence) == [41, 42])
    #expect(decoded.chunks.first?.paneID == "w1:p1")
    #expect(decoded.chunks.first?.bytesBase64 == "aGk=")
    #expect(decoded.find.term == "")
    #expect(decoded.find.total == 0)
}

@Test func laggingCursorDeltaSurfacesTheDropMarker() throws {
    let payload = """
    {
        "schema_version": 2,
        "revision": 9,
        "rest": null,
        "editor": {
            "tabs": [],
            "active_tab_id": null,
            "document": null
        },
        "input_generation": 0,
        "find": {
            "pane_id": "w1:p1",
            "term": "needle",
            "index": 2,
            "total": 7,
            "truncated": true,
            "unavailable_reason": null
        },
        "terminal_sequence": 600,
        "chunks": [],
        "chunks_dropped": true
    }
    """
    let decoded = try JSONDecoder().decode(
        CoreSnapshotDelta.self,
        from: Data(payload.utf8)
    )

    #expect(decoded.chunksDropped)
    #expect(decoded.editor != nil)
    #expect(decoded.chunks.isEmpty)
    // Find state rides the top-level per-event channel, so it arrives on a
    // response that carries no `rest` at all.
    #expect(decoded.rest == nil)
    #expect(decoded.find.term == "needle")
    #expect(decoded.find.index == 2)
    #expect(decoded.find.total == 7)
    #expect(decoded.find.truncated)
}
