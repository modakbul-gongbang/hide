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
}

@Test func laggingCursorDeltaSurfacesTheDropMarker() throws {
    let payload = """
    {
        "schema_version": 2,
        "revision": 9,
        "rest": null,
        "editor": {
            "path": null,
            "language": null,
            "contents_utf8": null,
            "opened_modified_at_unix_ms": null,
            "dirty": false,
            "conflict": null,
            "diff": null
        },
        "input_generation": 0,
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
}
