import CoreGraphics
import Foundation
import ImageIO

struct PetFrame: Equatable {
    let image: CGImage
    let duration: TimeInterval

    static func == (left: PetFrame, right: PetFrame) -> Bool {
        left.image === right.image && left.duration == right.duration
    }
}

/// Turns one theme state into the frames the renderer plays.
///
/// Two sources, one output. An animated webp carries its own per-frame delay
/// and ImageIO decodes it directly - no conversion step (T1 spike). A sprite
/// sheet is one image sliced into equal horizontal frames, which is how the
/// walk, carrying, juggling, and waking art ship.
enum PetAnimationLoader {
    static let fallbackFrameDuration: TimeInterval = 0.1

    static func frames(for state: PetThemeState) throws -> [PetFrame] {
        guard let source = CGImageSourceCreateWithURL(state.assetURL as CFURL, nil) else {
            throw PetAnimationError.undecodable(state.assetURL.path)
        }
        if let declared = state.frames {
            return try sheetFrames(source: source, state: state, count: declared)
        }
        return try sequenceFrames(source: source, path: state.assetURL.path)
    }

    private static func sheetFrames(
        source: CGImageSource,
        state: PetThemeState,
        count: Int
    ) throws -> [PetFrame] {
        guard let sheet = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
            throw PetAnimationError.undecodable(state.assetURL.path)
        }
        let frameWidth = sheet.width / count
        guard frameWidth > 0, frameWidth * count == sheet.width else {
            throw PetAnimationError.sheetNotDivisible(
                path: state.assetURL.path,
                width: sheet.width,
                frames: count
            )
        }
        let total = TimeInterval(state.durationMilliseconds ?? 800) / 1_000
        let perFrame = total / TimeInterval(count)
        return try (0..<count).map { index in
            let rect = CGRect(
                x: index * frameWidth,
                y: 0,
                width: frameWidth,
                height: sheet.height
            )
            guard let cropped = sheet.cropping(to: rect) else {
                throw PetAnimationError.undecodable(state.assetURL.path)
            }
            return PetFrame(image: cropped, duration: perFrame)
        }
    }

    private static func sequenceFrames(source: CGImageSource, path: String) throws -> [PetFrame] {
        let count = CGImageSourceGetCount(source)
        guard count > 0 else {
            throw PetAnimationError.undecodable(path)
        }
        var frames: [PetFrame] = []
        frames.reserveCapacity(count)
        for index in 0..<count {
            guard let image = CGImageSourceCreateImageAtIndex(source, index, nil) else {
                throw PetAnimationError.undecodable(path)
            }
            frames.append(
                PetFrame(
                    image: image,
                    duration: count == 1
                        ? 0
                        : delay(source: source, index: index) ?? fallbackFrameDuration
                )
            )
        }
        return frames
    }

    private static func delay(source: CGImageSource, index: Int) -> TimeInterval? {
        guard
            let properties = CGImageSourceCopyPropertiesAtIndex(source, index, nil)
                as? [CFString: Any]
        else { return nil }
        let containers: [CFString] = [
            kCGImagePropertyWebPDictionary,
            kCGImagePropertyGIFDictionary,
            kCGImagePropertyPNGDictionary,
        ]
        let delayKeys: [CFString] = [
            kCGImagePropertyWebPUnclampedDelayTime,
            kCGImagePropertyWebPDelayTime,
            kCGImagePropertyGIFUnclampedDelayTime,
            kCGImagePropertyGIFDelayTime,
            kCGImagePropertyAPNGUnclampedDelayTime,
            kCGImagePropertyAPNGDelayTime,
        ]
        for container in containers {
            guard let dictionary = properties[container] as? [CFString: Any] else { continue }
            for key in delayKeys {
                if let value = dictionary[key] as? Double, value > 0 {
                    return value
                }
            }
        }
        return nil
    }
}

enum PetAnimationError: Error, Equatable, LocalizedError {
    case undecodable(String)
    case sheetNotDivisible(path: String, width: Int, frames: Int)

    var errorDescription: String? {
        switch self {
        case let .undecodable(path):
            "The pet art at \(path) could not be decoded."
        case let .sheetNotDivisible(path, width, frames):
            "The sprite sheet at \(path) is \(width)px wide, which does not divide into \(frames) frames."
        }
    }
}
