import AppKit
import Foundation
import SwiftUI
import Testing
@testable import HerdrMacOS

/// The canvas hosted offscreen: the hover card is a drawing-time decision,
/// and a window that is not key never sees the pointer, so the state is
/// injected and the render compared. With `HIDE_HOME_RENDER_DIR` set the
/// frames are also written out as PNGs for the run directory.
@Suite("Project Home canvas render")
@MainActor
struct ProjectHomeCanvasRenderTests {
    private struct Host: View {
        let home: ProjectHomeModel
        let positions: [String: CGPoint]
        @State var focus: String?
        @State var hover: String?
        @State var hoverPoint: CGPoint?

        var body: some View {
            ProjectHomeCanvas(
                home: home, positions: positions,
                focus: $focus, hover: $hover, hoverPoint: $hoverPoint, onActivate: { _ in }
            )
            .background(HideTheme.background)
        }
    }

    private func render(_ view: Host, size: CGSize, name: String) throws -> NSBitmapImageRep {
        let window = NSWindow(contentRect: NSRect(origin: .zero, size: size),
                              styleMask: [.borderless], backing: .buffered, defer: false)
        let host = NSHostingView(rootView: view)
        host.frame = NSRect(origin: .zero, size: size)
        window.contentView = host
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.2))
        host.layoutSubtreeIfNeeded()
        let rep = try #require(host.bitmapImageRepForCachingDisplay(in: host.bounds))
        host.cacheDisplay(in: host.bounds, to: rep)
        if let dir = ProcessInfo.processInfo.environment["HIDE_HOME_RENDER_DIR"] {
            let url = URL(fileURLWithPath: dir).appendingPathComponent("\(name).png")
            try #require(rep.representation(using: .png, properties: [:])).write(to: url, options: .atomic)
        }
        return rep
    }

    private func differingPixels(_ a: NSBitmapImageRep, _ b: NSBitmapImageRep) -> Int {
        var count = 0
        for y in stride(from: 0, to: a.pixelsHigh, by: 2) {
            for x in stride(from: 0, to: a.pixelsWide, by: 2) where a.colorAt(x: x, y: y) != b.colorAt(x: x, y: y) {
                count += 1
            }
        }
        return count
    }

    @Test func hoveringAnAgentDrawsItsCardBesideTheNode() throws {
        let (workspace, agents) = ProjectHomeFixture.fourCheckouts()
        let home = ProjectHomePresentation.build(workspace: workspace, agents: agents, connected: true)
        let positions = ProjectHomeGraphLayout.solve(home.topology).positions
        let size = CGSize(width: 900, height: 640)
        let fit = ProjectHomeFit(bounds: ProjectHomeGraphLayout.bounds(positions, nodes: home.topology.nodes), canvas: size)
        let hovered = ProjectHomeModel.agentNodeID("p-home-1")
        let plain = try render(Host(home: home, positions: positions, focus: hovered), size: size, name: "canvas-selected")
        let withCard = try render(
            Host(home: home, positions: positions, focus: hovered, hover: hovered, hoverPoint: fit.canvasPoint(positions[hovered]!)),
            size: size, name: "canvas-hover"
        )
        #expect(plain.pixelsWide == withCard.pixelsWide)
        let changed = differingPixels(plain, withCard)
        // A card of cardWidth by at least two text lines.
        let cardArea = Int(HideTheme.Home.cardWidth * HideTheme.Home.labelHeight * 2) / 4
        #expect(changed > cardArea, "only \(changed) sampled pixels changed")
    }

    @Test func aTwelveCheckoutMapScrollsInASmallCanvasAndFitsALargeOne() throws {
        let (workspace, agents) = ProjectHomeFixture.twelveCheckouts()
        let home = ProjectHomePresentation.build(workspace: workspace, agents: agents, connected: true)
        let positions = ProjectHomeGraphLayout.solve(home.topology).positions
        let bounds = ProjectHomeGraphLayout.bounds(positions, nodes: home.topology.nodes)
        let small = ProjectHomeFit(bounds: bounds, canvas: CGSize(width: 900, height: 640))
        #expect(small.scale == HideTheme.Home.minScale)
        #expect(small.scrolls)
        let large = CGSize(width: 1100, height: 1000)
        let fitted = ProjectHomeFit(bounds: bounds, canvas: large)
        #expect(!fitted.scrolls)
        _ = try render(Host(home: home, positions: positions), size: large, name: "canvas-twelve")
    }
}
