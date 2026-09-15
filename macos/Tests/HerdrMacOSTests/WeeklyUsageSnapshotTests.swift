import Foundation
import Testing
@testable import HerdrMacOS

@Suite("Weekly provider usage snapshot")
struct WeeklyUsageSnapshotTests {
    @Test func decodesTheCoreWeeklyWindowWithoutInventingValues() throws {
        let data = Data(
            #"{"provider":"codex","label":"Codex","window_minutes":10080,"state":"available","used_percent":59.0,"resets_at_unix_seconds":1788408000,"message":null,"last_checked_at_unix_ms":1788200000000}"#.utf8
        )

        let usage = try JSONDecoder().decode(CoreProviderUsageSnapshot.self, from: data)

        #expect(usage.id == "codex")
        #expect(usage.windowMinutes == 10_080)
        #expect(usage.usedPercent == 59)
        #expect(usage.resetsAtUnixSeconds == 1_788_408_000)
        #expect(usage.message == nil)
    }

    @Test func keepsUnavailableStateAndFailureMessageExplicit() throws {
        let data = Data(
            #"{"provider":"claude","label":"Claude Code","window_minutes":10080,"state":"unavailable","used_percent":null,"resets_at_unix_seconds":null,"message":"cache unavailable","last_checked_at_unix_ms":1788200000000}"#.utf8
        )

        let usage = try JSONDecoder().decode(CoreProviderUsageSnapshot.self, from: data)

        #expect(usage.state == "unavailable")
        #expect(usage.usedPercent == nil)
        #expect(usage.message == "cache unavailable")
    }

    @Test func decodesLoadingMetadataAndScopedBuckets() throws {
        let data = Data(
            #"{"provider":"claude","label":"Claude Code","window_minutes":10080,"state":"available","used_percent":43.4,"resets_at_unix_seconds":1893456000,"message":null,"last_checked_at_unix_ms":1788200000000,"last_success_at_unix_ms":1788200000000,"last_error_kind":null,"buckets":[{"label":"Fable","state":"available","used_percent":61.0,"resets_at_unix_seconds":1893521045,"message":null}]}"#.utf8
        )

        let usage = try JSONDecoder().decode(CoreProviderUsageSnapshot.self, from: data)

        #expect(usage.lastSuccessAtUnixMilliseconds == 1_788_200_000_000)
        #expect(usage.buckets.count == 1)
        #expect(usage.buckets[0].label == "Fable")
        #expect(usage.buckets[0].usedPercent == 61)
    }
}
