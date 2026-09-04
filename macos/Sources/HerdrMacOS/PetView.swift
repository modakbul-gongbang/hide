import AppKit
import SwiftUI

/// Plays one pose's frames.
///
/// Frame timing runs on a `.common`-mode run loop timer rather than a
/// SwiftUI animation clock: the pet window is deliberately never key, and the
/// pose has to keep moving while the user works in another app.
@MainActor
final class PetAnimator: ObservableObject {
    /// Holds the run loop timer so teardown can invalidate it from a
    /// nonisolated deinit, the same way the pet window holds its observers.
    private final class Lifecycle: @unchecked Sendable {
        var timer: Timer?

        deinit {
            timer?.invalidate()
        }
    }

    @Published private(set) var frame: CGImage?

    private var frames: [PetFrame] = []
    private var index = 0
    private var pose: String?
    private let lifecycle = Lifecycle()

    /// Switches to `pose`. Re-showing the pose already playing keeps the
    /// current frame instead of restarting the loop, so a snapshot that
    /// repeats the same pose does not make the pet stutter.
    func show(pose: String, frames: [PetFrame]) {
        guard self.pose != pose else { return }
        self.pose = pose
        self.frames = frames
        index = 0
        frame = frames.first?.image
        lifecycle.timer?.invalidate()
        lifecycle.timer = nil
        scheduleNextFrame()
    }

    func clear(reason: String) {
        pose = "\u{1}unavailable:\(reason)"
        frames = []
        frame = nil
        lifecycle.timer?.invalidate()
        lifecycle.timer = nil
    }

    private func scheduleNextFrame() {
        guard frames.count > 1 else { return }
        let duration = max(frames[index].duration, 1.0 / 60.0)
        let timer = Timer(timeInterval: duration, repeats: false) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, !self.frames.isEmpty else { return }
                self.index = (self.index + 1) % self.frames.count
                self.frame = self.frames[self.index].image
                self.scheduleNextFrame()
            }
        }
        RunLoop.main.add(timer, forMode: .common)
        lifecycle.timer = timer
    }
}

/// The badge row: one capsule per non-zero group, in the sidebar's own order -
/// what needs the operator, what finished unseen, what is still running - then
/// the ambient counts.
struct PetBadgeRow: View {
    let badges: CorePetBadges

    private struct Badge: Identifiable {
        let id: String
        let count: Int
        let color: Color
        let label: String
        let symbol: String?
    }

    private var badgeList: [Badge] {
        var result: [Badge] = [
            Badge(id: "needs-you", count: badges.needsYou, color: .yellow, label: "needs you", symbol: nil),
            Badge(id: "done", count: badges.done, color: .green, label: "finished, not yet seen", symbol: nil),
            Badge(id: "working", count: badges.working, color: .blue, label: "working", symbol: nil),
        ]
        result.append(
            Badge(
                id: "subagents",
                count: Int(badges.subagentsActive),
                color: .purple,
                label: "active subagents",
                symbol: "person.2.fill"
            )
        )
        result.append(
            Badge(
                id: "background",
                count: Int(badges.backgroundRunning),
                color: .teal,
                label: "running background tasks",
                symbol: "clock.arrow.circlepath"
            )
        )
        result.append(
            Badge(
                id: "background-failed",
                count: Int(badges.backgroundFailed),
                color: .orange,
                label: "failed background tasks",
                symbol: "exclamationmark.triangle.fill"
            )
        )
        return result.filter { $0.count > 0 }
    }

    var body: some View {
        HStack(spacing: 3) {
            ForEach(badgeList) { badge in
                HStack(spacing: 2) {
                    if let symbol = badge.symbol {
                        Image(systemName: symbol).font(.system(size: 7, weight: .bold))
                    }
                    Text("\(badge.count)")
                        .font(.system(size: 9, weight: .bold, design: .rounded))
                        .monospacedDigit()
                }
                .foregroundStyle(.white)
                .padding(.horizontal, 4)
                .padding(.vertical, 1)
                .background(badge.color, in: Capsule())
                .overlay(Capsule().stroke(.black.opacity(0.25), lineWidth: 0.5))
                .accessibilityLabel("\(badge.count) \(badge.label)")
                .accessibilityIdentifier("pet-badge-\(badge.id)")
            }
        }
        .shadow(color: .black.opacity(0.35), radius: 1.5, y: 0.5)
        .accessibilityIdentifier("pet-badge-row")
    }
}

struct PetView: View {
    @ObservedObject var animator: PetAnimator
    let badges: CorePetBadges
    /// Set when the theme could not be loaded. The pet never falls back to a
    /// blank window: an unrenderable theme says so where the pet would be.
    let unavailableReason: String?

    var body: some View {
        ZStack(alignment: .topTrailing) {
            Color.clear
            if let unavailableReason {
                VStack(spacing: 2) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .font(.system(size: 20, weight: .semibold))
                    Text("Pet art\nunavailable")
                        .font(.system(size: 8, weight: .semibold))
                        .multilineTextAlignment(.center)
                }
                .foregroundStyle(.orange)
                .padding(4)
                .background(.black.opacity(0.55), in: RoundedRectangle(cornerRadius: 6))
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .help(unavailableReason)
                .accessibilityLabel("Pet art unavailable: \(unavailableReason)")
                .accessibilityIdentifier("pet-art-unavailable")
            } else if let frame = animator.frame {
                Image(decorative: frame, scale: 1)
                    .resizable()
                    .interpolation(.high)
                    .scaledToFit()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .accessibilityIdentifier("pet-art")
            }
            PetBadgeRow(badges: badges)
                .padding(.top, 1)
                .padding(.trailing, 1)
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("pet-window")
    }
}
