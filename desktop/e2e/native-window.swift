// Capture one background candidate, including when legacy window capture is
// unavailable. Never activate a window or request a new system permission.
import AppKit
import ScreenCaptureKit
import UniformTypeIdentifiers

@main
struct NativeWindowCapture {
    static func main() async {
        do {
            guard CommandLine.arguments.count == 4,
                  let windowID = UInt32(CommandLine.arguments[1]),
                  let ownerPID = Int32(CommandLine.arguments[2]) else {
                throw CaptureError.invalidArguments
            }
            guard CGPreflightScreenCaptureAccess() else {
                throw CaptureError.permissionUnavailable
            }
            guard #available(macOS 14, *) else {
                throw CaptureError.systemUnavailable
            }
            let content = try await SCShareableContent.excludingDesktopWindows(true, onScreenWindowsOnly: true)
            let matches = content.windows.filter { $0.windowID == windowID && $0.owningApplication?.processID == ownerPID }
            guard matches.count == 1, let window = matches.first else {
                throw CaptureError.candidateUnavailable
            }
            let filter = SCContentFilter(desktopIndependentWindow: window)
            let config = SCStreamConfiguration()
            config.width = Int(filter.contentRect.width * CGFloat(filter.pointPixelScale))
            config.height = Int(filter.contentRect.height * CGFloat(filter.pointPixelScale))
            config.showsCursor = false
            let image = try await SCScreenshotManager.captureImage(contentFilter: filter, configuration: config)
            let file = URL(fileURLWithPath: CommandLine.arguments[3])
            guard let destination = CGImageDestinationCreateWithURL(file as CFURL, UTType.png.identifier as CFString, 1, nil) else {
                throw CaptureError.destinationUnavailable
            }
            CGImageDestinationAddImage(destination, image, nil)
            guard CGImageDestinationFinalize(destination) else {
                throw CaptureError.writeFailed
            }
            print("ScreenCaptureKit window=\(windowID) pid=\(ownerPID) size=\(image.width)x\(image.height)")
        } catch {
            FileHandle.standardError.write(Data("Exact-window capture failed: \(error)\n".utf8))
            exit(1)
        }
    }

    enum CaptureError: Error {
        case invalidArguments, permissionUnavailable, systemUnavailable
        case candidateUnavailable, destinationUnavailable, writeFailed
    }
}
