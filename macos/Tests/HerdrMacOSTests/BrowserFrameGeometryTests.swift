import Foundation
import Testing
@testable import HerdrMacOS

@Suite("Browser frame geometry")
struct BrowserFrameGeometryTests {
    @Test func aWideBrowserIsLetterboxedWithoutChangingItsViewport() {
        let geometry = BrowserFrameGeometry(deviceWidth: 1280, deviceHeight: 720, pageScaleFactor: 1, offsetTop: 0)
        let bounds = CGRect(x: 0, y: 0, width: 640, height: 480)
        #expect(geometry.imageRect(in: bounds) == CGRect(x: 0, y: 60, width: 640, height: 360))
        #expect(geometry.pagePoint(CGPoint(x: 320, y: 240), in: bounds) == CGPoint(x: 640, y: 360))
        #expect(geometry.pagePoint(CGPoint(x: 320, y: 30), in: bounds) == nil)
        #expect(geometry.pagePoint(CGPoint(x: 640, y: 240), in: bounds) == nil)
    }

    @Test func pageScaleAndTopInsetAreRemovedFromInputCoordinates() {
        let geometry = BrowserFrameGeometry(deviceWidth: 1280, deviceHeight: 720, pageScaleFactor: 2, offsetTop: 20)
        let bounds = CGRect(x: 0, y: 0, width: 640, height: 360)
        #expect(geometry.pagePoint(CGPoint(x: 320, y: 180), in: bounds) == CGPoint(x: 320, y: 170))
        #expect(geometry.pagePoint(CGPoint(x: 320, y: 5), in: bounds) == nil)
    }

    @Test func aMalformedEndpointCannotBecomeANetworkRequest() {
        for target in ["", "../browser", "other/target", "target?host=remote"] {
            let binding = BrowserPaneBinding(bindingID: "qa", profile: "work", targetID: target, session: "qa", cdpPort: 9300, ownsTarget: false)
            #expect(throws: BrowserConnectionError.self) { try BrowserCDPSession(binding: binding) }
        }
        let zeroPort = BrowserPaneBinding(bindingID: "qa", profile: "work", targetID: "ABC", session: "qa", cdpPort: 0, ownsTarget: false)
        #expect(throws: BrowserConnectionError.self) { try BrowserCDPSession(binding: zeroPort) }
    }
}
