import Foundation
import Testing
@testable import HerdrMacOS

@Test func devicePickerNamesLocationConnectionAndOnlyAvailableCounts() {
    func device(_ kind: String, _ state: String, _ count: UInt32) -> CoreDeviceSnapshot {
        CoreDeviceSnapshot(id: kind == "local" ? "local" : "mini", label: "작업용 Mac",
            kind: kind, state: state, message: nil, sshAlias: nil, agentCount: count, test: nil)
    }
    #expect(DevicePickerPresentation.detail(device("local", "ready", 0)) == "Local · 0 agents")
    #expect(DevicePickerPresentation.detail(device("remote", "ready", 1)) == "Remote · Connected · 1 agent")
    #expect(DevicePickerPresentation.detail(device("remote", "unavailable", 8)) == "Remote · Not connected")
    #expect(DevicePickerPresentation.detail(device("remote", "loading", 8)) == "Remote · Connecting…")
    #expect(DevicePickerPresentation.spoken(device("remote", "ready", 7), selected: true)
        == "작업용 Mac, Remote · Connected · 7 agents, Selected")
}
