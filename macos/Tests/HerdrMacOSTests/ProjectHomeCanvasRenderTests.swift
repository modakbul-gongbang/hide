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
        var cache = ProjectHomeLayoutCache()
        @State var focus: String?
        @State var isolated = false
        @State var hover: String?
        @State var hoverPoint: CGPoint?

        var body: some View {
            ProjectHomeCanvas(
                home: home, positions: positions, cache: cache,
                focus: $focus, isolated: $isolated, hover: $hover, hoverPoint: $hoverPoint,
                onActivate: { _ in }, onOpenPullRequest: { _ in }
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

    @Test func aTwelveCheckoutMapFitsASmallCanvasWithThinnedLabelsAndZoomsBackIn() throws {
        let (workspace, agents) = ProjectHomeFixture.twelveCheckouts()
        let home = ProjectHomePresentation.build(workspace: workspace, agents: agents, connected: true)
        let positions = ProjectHomeGraphLayout.solve(home.topology).positions
        let bounds = ProjectHomeGraphLayout.bounds(positions, nodes: home.topology.nodes)
        let size = CGSize(width: 700, height: 900)
        let fitted = ProjectHomeFit(bounds: bounds, canvas: size)
        #expect(fitted.scale < HideTheme.Home.labelThresholdScale)
        #expect(!fitted.overflows)
        let whole = try render(Host(home: home, positions: positions), size: size, name: "canvas-twelve")
        // Zoomed into the third checkout, brought to the centre by the pan.
        let zoomedCache = ProjectHomeLayoutCache()
        let hub = positions[ProjectHomeModel.checkoutNodeID("c3")]!
        zoomedCache.view = ProjectHomeViewState(zoom: 2.5, pan: CGPoint(x: bounds.midX - hub.x, y: bounds.midY - hub.y))
        let zoomed = try render(Host(home: home, positions: positions, cache: zoomedCache), size: size, name: "canvas-twelve-zoomed")
        // Zoomed in, the working agents' labels come back and the centre
        // fills: at least a tenth of the sampled pixels change.
        let sampled = Int(size.width * size.height) / 4
        #expect(differingPixels(whole, zoomed) > sampled / 10)
    }
}
