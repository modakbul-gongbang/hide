import CoreGraphics
import Foundation
import Testing
@testable import HerdrMacOS

/// The constellation's four promises: the same topology lays out the same
/// way, one arrival moves nothing else far, labels do not cross after rest,
/// and the local graph is exactly depth two (PRD rules 1, 2 and 4).
@Suite("Project Home graph layout")
struct ProjectHomeGraphLayoutTests {
    private func topology(_ fixture: (CoreWorkspaceSnapshot, [SidebarAgent])) -> ProjectHomeGraphLayout.Topology {
        ProjectHomePresentation.build(workspace: fixture.0, agents: fixture.1, connected: true).topology
    }

    /// Pairs whose labels cross each other or the other node's disc.
    private func overlappingLabels(_ result: ProjectHomeGraphLayout.Result, _ topology: ProjectHomeGraphLayout.Topology) -> [(String, String)] {
        let nodes = topology.nodes
        var pairs: [(String, String)] = []
        for i in nodes.indices {
            for j in nodes.indices where j > i {
                let a = result.positions[nodes[i].id]!, b = result.positions[nodes[j].id]!
                let labelA = ProjectHomeGraphLayout.labelFrame(for: nodes[i], at: a)
                let labelB = ProjectHomeGraphLayout.labelFrame(for: nodes[j], at: b)
                let discA = ProjectHomeGraphLayout.discFrame(for: nodes[i], at: a)
                let discB = ProjectHomeGraphLayout.discFrame(for: nodes[j], at: b)
                if labelA.intersects(labelB) || labelA.intersects(discB) || labelB.intersects(discA) {
                    pairs.append((nodes[i].id, nodes[j].id))
                }
            }
        }
        return pairs
    }

    @Test func theSameTopologyLaysOutIdenticallyEveryTime() {
        let topology = topology(ProjectHomeFixture.fourCheckouts())
        let first = ProjectHomeGraphLayout.solve(topology)
        let second = ProjectHomeGraphLayout.solve(topology)
        #expect(first == second)
        #expect(first.positions.count == topology.nodes.count)
        #expect(first.positions[ProjectHomeModel.projectNodeID] == .zero)
        #expect(first.ticks < ProjectHomeGraphLayout.Tuning.maxTicks)
    }

    @Test func aStatusChangeDoesNotChangeTheTopologyKey() {
        let (workspace, agents) = ProjectHomeFixture.fourCheckouts()
        var moved = agents
        moved[4] = ProjectHomeFixture.agent("p-board-1", identity: "Lane board", group: "done", symbol: "✓", emphasized: true, statusLabel: "Done", lastActivity: "1789009999999")
        let before = ProjectHomePresentation.build(workspace: workspace, agents: agents, connected: true).topology
        let after = ProjectHomePresentation.build(workspace: workspace, agents: moved, connected: true).topology
        #expect(before.key == after.key)
        let cache = ProjectHomeLayoutCache()
        let first = cache.positions(for: before)
        let second = cache.positions(for: after)
        #expect(first == second)
    }

    @Test func oneArrivalMovesNoExistingNodeFar() {
        let (workspace, agents) = ProjectHomeFixture.fourCheckouts()
        let before = ProjectHomePresentation.build(workspace: workspace, agents: agents, connected: true).topology
        let rested = ProjectHomeGraphLayout.solve(before)
        // A new agent lands on the busiest checkout, beside three others.
        var withNewcomer = workspace.checkouts
        let home = withNewcomer.firstIndex { $0.id == "home" }!
        withNewcomer[home] = ProjectHomeFixture.checkout(
            "home", branch: "prd/home-graph", panes: ["p-home-1", "p-home-2", "p-home-3", "p-home-4"],
            ahead: 4, changed: 2, dirty: true, pullRequest: ProjectHomeFixture.pullRequest(110, draft: true, checks: .pending)
        )
        let newcomer = ProjectHomeFixture.agent("p-home-4", identity: "Newcomer", group: "working", symbol: "●", activity: "working", lastActivity: "1789000900000")
        let after = ProjectHomePresentation.build(workspace: ProjectHomeFixture.workspace(withNewcomer), agents: agents + [newcomer], connected: true).topology
        #expect(before.key != after.key)
        let cache = ProjectHomeLayoutCache()
        _ = cache.positions(for: before)
        let settled = cache.positions(for: after)
        var largest: CGFloat = 0
        for (id, point) in rested.positions {
            let now = settled[id]!
            largest = max(largest, hypot(now.x - point.x, now.y - point.y))
        }
        #expect(largest <= HideTheme.Home.agentOrbitRadius / 2, "an existing node moved \(largest) points")
        let arrived = settled[ProjectHomeModel.agentNodeID("p-home-4")]!
        let hub = settled[ProjectHomeModel.checkoutNodeID("home")]!
        #expect(hypot(arrived.x - hub.x, arrived.y - hub.y) < HideTheme.Home.agentOrbitRadius * 2)
    }

    @Test func labelsDoNotCrossOnTwelveCheckoutsAndThirtyAgents() {
        let topology = topology(ProjectHomeFixture.twelveCheckouts())
        #expect(topology.nodes.filter { $0.kind == .checkout }.count == 12)
        #expect(topology.nodes.filter { $0.kind == .agent || $0.kind == .child }.count == 30)
        let result = ProjectHomeGraphLayout.solve(topology)
        let crossings = overlappingLabels(result, topology)
        #expect(crossings.isEmpty, "\(crossings)")
        #expect(result.ticks < ProjectHomeGraphLayout.Tuning.maxTicks)
    }

    @Test func labelsDoNotCrossOnTheFourCheckoutFixtureEither() {
        let topology = topology(ProjectHomeFixture.fourCheckouts())
        let result = ProjectHomeGraphLayout.solve(topology)
        #expect(overlappingLabels(result, topology).isEmpty)
    }

    @Test func nodesDoNotSitOnOneAnother() {
        let topology = topology(ProjectHomeFixture.twelveCheckouts())
        let result = ProjectHomeGraphLayout.solve(topology)
        for i in topology.nodes.indices {
            for j in topology.nodes.indices where j > i {
                let a = result.positions[topology.nodes[i].id]!, b = result.positions[topology.nodes[j].id]!
                let gap = hypot(a.x - b.x, a.y - b.y)
                #expect(gap >= topology.nodes[i].radius + topology.nodes[j].radius, "\(topology.nodes[i].id) and \(topology.nodes[j].id) are \(gap) apart")
            }
        }
    }

    @Test func theCacheRunsTheSimulationOnlyWhenTheTopologyChanges() {
        let cache = ProjectHomeLayoutCache()
        let topology = topology(ProjectHomeFixture.fourCheckouts())
        let first = cache.positions(for: topology)
        let ticks = cache.lastTicks
        #expect(ticks > 0)
        let again = cache.positions(for: topology)
        #expect(again == first)
        #expect(cache.lastTicks == ticks)
        // A node that leaves drops out of the cache.
        let smaller = ProjectHomeGraphLayout.Topology(
            nodes: topology.nodes.filter { $0.id != ProjectHomeModel.agentNodeID("p-labels-3") },
            edges: topology.edges.filter { $0.to != ProjectHomeModel.agentNodeID("p-labels-3") }
        )
        let shrunk = cache.positions(for: smaller)
        #expect(shrunk[ProjectHomeModel.agentNodeID("p-labels-3")] == nil)
        #expect(shrunk.count == smaller.nodes.count)
    }

    @Test func theNeighbourhoodIsExactlyDepthTwo() {
        let topology = topology(ProjectHomeFixture.fourCheckouts())
        let child = ProjectHomeModel.agentNodeID("p-home-2")
        let two = ProjectHomeGraphLayout.neighborhood(of: child, depth: 2, edges: topology.edges)
        #expect(two == [
            child,
            ProjectHomeModel.agentNodeID("p-home-1"),
            ProjectHomeModel.agentNodeID("p-home-3"),
            ProjectHomeModel.checkoutNodeID("home"),
        ])
        let one = ProjectHomeGraphLayout.neighborhood(of: child, depth: 1, edges: topology.edges)
        #expect(one == [child, ProjectHomeModel.agentNodeID("p-home-1")])
        let zero = ProjectHomeGraphLayout.neighborhood(of: child, depth: 0, edges: topology.edges)
        #expect(zero == [child])
        let fromCheckout = ProjectHomeGraphLayout.neighborhood(of: ProjectHomeModel.checkoutNodeID("home"), depth: 2, edges: topology.edges)
        #expect(fromCheckout.contains(ProjectHomeModel.projectNodeID))
        #expect(fromCheckout.contains(ProjectHomeModel.checkoutNodeID("board")))
        #expect(fromCheckout.contains(ProjectHomeModel.pullRequestNodeID("home")))
        #expect(fromCheckout.contains(child))
        #expect(!fromCheckout.contains(ProjectHomeModel.agentNodeID("p-board-1")))
    }

    @Test func hitTestingFindsTheNearestNodeWithinItsRadiusAndMargin() {
        let topology = topology(ProjectHomeFixture.fourCheckouts())
        let result = ProjectHomeGraphLayout.solve(topology)
        let hub = ProjectHomeModel.checkoutNodeID("home")
        let centre = result.positions[hub]!
        let node = topology.nodes.first { $0.id == hub }!
        #expect(ProjectHomeGraphLayout.hit(centre, positions: result.positions, nodes: topology.nodes, margin: 0) == hub)
        let edge = CGPoint(x: centre.x + node.radius + 2, y: centre.y)
        #expect(ProjectHomeGraphLayout.hit(edge, positions: result.positions, nodes: topology.nodes, margin: 0) == nil)
        #expect(ProjectHomeGraphLayout.hit(edge, positions: result.positions, nodes: topology.nodes, margin: 4) == hub)
        let far = CGPoint(x: 10_000, y: 10_000)
        #expect(ProjectHomeGraphLayout.hit(far, positions: result.positions, nodes: topology.nodes, margin: 100) == nil)
    }

    @Test func theFitGrowsASmallMapAndScrollsALargeOne() {
        let small = CGRect(x: -100, y: -100, width: 200, height: 200)
        let grown = ProjectHomeFit(bounds: small, canvas: CGSize(width: 800, height: 600))
        #expect(grown.scale == HideTheme.Home.maxScale)
        #expect(grown.labelFontScale == 1)
        #expect(!grown.scrolls)
        #expect(grown.canvasPoint(.zero) == CGPoint(x: 400, y: 300))
        let snug = CGRect(x: -400, y: -300, width: 800, height: 600)
        let shrunk = ProjectHomeFit(bounds: snug, canvas: CGSize(width: 700, height: 600))
        #expect(shrunk.scale < 1 && shrunk.scale >= HideTheme.Home.minScale)
        #expect(!shrunk.scrolls)
        #expect(shrunk.labelFontScale == HideTheme.Home.labelMinFontScale)
        let large = CGRect(x: -500, y: -400, width: 1000, height: 800)
        let scrolled = ProjectHomeFit(bounds: large, canvas: CGSize(width: 400, height: 300))
        #expect(scrolled.scale == HideTheme.Home.minScale)
        #expect(scrolled.scrolls)
        #expect(scrolled.content.width == 1000 * HideTheme.Home.minScale + HideTheme.Home.canvasInset * 2)
        let back = scrolled.layoutPoint(scrolled.canvasPoint(CGPoint(x: 12, y: -34)))
        #expect(abs(back.x - 12) < 0.001 && abs(back.y + 34) < 0.001)
    }
}
