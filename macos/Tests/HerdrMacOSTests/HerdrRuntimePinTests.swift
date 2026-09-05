import Foundation
import Testing

@testable import HerdrMacOS

/// The pin has to be readable from the packaged resource bundle in every build
/// that runs, including this test target, which has no .app around it.
@Test func thePinIsReadFromTheShippedManifest() throws {
    let pin = try #require(HerdrRuntimePinLoader.load())
    #expect(pin.version.split(separator: ".").count >= 2)
    #expect(pin.sha256.count == 64)
    let digestIsHex = pin.sha256.allSatisfy { $0.isHexDigit }
    #expect(digestIsHex)
}

/// The loader answers nil rather than a placeholder when the manifest is not
/// there, so a broken bundle refuses to resolve a runtime instead of trusting
/// an unverified one.
@Test func aMissingManifestYieldsNoPin() {
    #expect(HerdrRuntimePinLoader.load(bundle: nil) == nil)
}
