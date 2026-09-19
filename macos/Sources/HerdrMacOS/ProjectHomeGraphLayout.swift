import CoreGraphics
import Foundation

/// The constellation's geometry, decided without SwiftUI.
///
/// Positions are in layout points at scale 1 around a project node fixed at
/// the origin; the view fits the whole map to its canvas. The layout is a
/// pure function of the topology and of the positions it was last at, so the
/// same input lays out the same way and a status change, which does not
/// touch the topology, never moves anything (PRD rule 1).
enum ProjectHomeGraphLayout {
    enum NodeKind: Equatable {
        case project
        case checkout
        case agent
        case child
        case pullRequest
    }

    /// What the layout knows about one node: what it is, what it hangs off,
    /// and how wide its label is, which is the one thing collision needs.
    struct Node: Equatable {
        let id: String
        let kind: NodeKind
        /// The node this one is seeded around; `nil` only for the project.
        let parentID: String?
        let radius: CGFloat
        let labelWidth: CGFloat
        /// Rank among siblings in the stable order the presentation decided.
        /// Angles come from this, never from a hash or arrival order.
        let rank: Int
        let siblingCount: Int
    }

    struct Edge: Equatable, Hashable {
        let from: String
        let to: String
    }

    struct Topology: Equatable {
        let nodes: [Node]
        let edges: [Edge]

        /// Changes exactly when a node or edge appears or disappears, which
        /// is the only time the simulation runs.
        var key: String {
            nodes.map { "\($0.id):\($0.parentID ?? "-")" }.sorted().joined(separator: ",")
                + "|" + edges.map { "\($0.from)>\($0.to)" }.sorted().joined(separator: ",")
        }
    }

    struct Result: Equatable {
        let positions: [String: CGPoint]
        /// How many ticks the simulation ran before it came to rest, so a
        /// test can say it stopped rather than hit its cap.
        let ticks: Int
    }

    /// The physics, named so a test can read what "rest" means. These are not
    /// visual tokens: nothing on screen is this number.
    enum Tuning {
        static let repulsion: CGFloat = 2600
        static let repulsionCutoff: CGFloat = 160
        static let spring: CGFloat = 0.08
        static let anchorPull: CGFloat = 0.06
        static let damping: CGFloat = 0.72
        static let maxStep: CGFloat = 12
        static let restVelocity: CGFloat = 0.05
        static let maxTicks = 240
        static let labelNudgeStep: CGFloat = .pi / 36
        static let labelNudgeRounds = 48
    }

    // MARK: Seeding

    /// The arc a parent's children need: every label side by side with a
    /// gap, so the orbit is wide enough for them before any force runs.
    /// A single child needs no more than the base orbit.
    static func orbit(base: CGFloat, spread: CGFloat, siblings: [Node]) -> CGFloat {
        guard siblings.count > 1, spread > 0 else { return base }
        let needed = siblings.reduce(CGFloat(0)) { $0 + labelFrame(for: $1, at: .zero).width }
        return max(base, needed / spread)
    }

    static let agentSpread: CGFloat = .pi * 0.9
    static let childSpread: CGFloat = .pi * 0.7

    /// Where a node starts, and what pulls it once the simulation runs.
    ///
    /// The project is fixed at the origin. Checkouts sit on a ring at angles
    /// by rank; each agent sits on an arc facing away from the centre around
    /// its checkout, each child on a smaller arc around its parent, and a
    /// pull request just outside its checkout. Every angle is a function of
    /// rank and sibling count, and every orbit of the labels it must hold,
    /// so the seed is the same for the same topology.
    static func seeds(for topology: Topology) -> [String: CGPoint] {
        let byID = Dictionary(uniqueKeysWithValues: topology.nodes.map { ($0.id, $0) })
        let byParent = Dictionary(grouping: topology.nodes.filter { $0.kind == .agent || $0.kind == .child }, by: { $0.parentID ?? "" })
        var seeds: [String: CGPoint] = [:]
        let checkouts = topology.nodes.filter { $0.kind == .checkout }
        // The ring holds every checkout's reach side by side: its agent
        // orbit across, plus the widest label among the checkout and its
        // agents, so neighbouring constellations start clear of each other.
        // Children hang outward from their agent and take no ring room. It
        // never shrinks below the base ring widened per checkout.
        let reaches = checkouts.map { checkout -> CGFloat in
            let agents = byParent[checkout.id] ?? []
            let widest = (agents + [checkout]).map { labelFrame(for: $0, at: .zero).width }.max() ?? 0
            return orbit(base: HideTheme.Home.agentOrbitRadius, spread: agentSpread, siblings: agents) * 2 + widest
        }
        let ring = max(
            HideTheme.Home.checkoutRingRadius
                + CGFloat(max(0, checkouts.count - 3)) * HideTheme.Home.checkoutRingRadiusPerCheckout,
            reaches.reduce(0, +) / (2 * .pi)
        )

        func place(_ node: Node) -> CGPoint {
            if let cached = seeds[node.id] { return cached }
            let point: CGPoint
            switch node.kind {
            case .project:
                point = .zero
            case .checkout:
                let angle = -CGFloat.pi / 2 + CGFloat(node.rank) / CGFloat(max(1, node.siblingCount)) * 2 * .pi
                point = CGPoint(x: ring * cos(angle), y: ring * sin(angle))
            case .agent, .child, .pullRequest:
                guard let parentID = node.parentID, let parent = byID[parentID] else {
                    preconditionFailure("\(node.id) has no parent to orbit")
                }
                let centre = place(parent)
                // A child hangs above its agent on screen, clear of the
                // agent's own label below it; everything else faces away
                // from the project.
                let outward = node.kind == .child ? -CGFloat.pi / 2 : atan2(centre.y, centre.x)
                let orbit: CGFloat
                let spread: CGFloat
                switch node.kind {
                case .agent:
                    spread = agentSpread
                    orbit = Self.orbit(base: HideTheme.Home.agentOrbitRadius, spread: spread, siblings: byParent[parentID] ?? [])
                case .child:
                    spread = childSpread
                    orbit = Self.orbit(base: HideTheme.Home.childOrbitRadius, spread: spread, siblings: byParent[parentID] ?? [])
                default:
                    orbit = HideTheme.Home.pullRequestOffset
                    spread = 0
                }
                let count = CGFloat(max(1, node.siblingCount))
                let angle: CGFloat
                if node.kind == .pullRequest {
                    // The pull request is the checkout's outward edge: it sits
                    // on the far side from the project and leaves the
                    // agents the arc either side of it.
                    angle = outward
                } else {
                    let step = count > 1 ? spread / (count - 1) : 0
                    // Agents leave the outward point to the pull request and
                    // fan around it; a lone agent sits just beside it.
                    let start = outward - spread / 2
                    angle = count > 1 ? start + step * CGFloat(node.rank) : outward + .pi / 6
                }
                point = CGPoint(x: centre.x + orbit * cos(angle), y: centre.y + orbit * sin(angle))
            }
            seeds[node.id] = point
            return point
        }
        for node in topology.nodes { _ = place(node) }
        return seeds
    }

    // MARK: Simulation

    /// Runs the simulation to rest.
    ///
    /// Every node is pulled to an anchor: where it was last time when the
    /// caller has that, its seed otherwise. That is what keeps a map still
    /// when one node arrives - the ones already placed are drawn back to
    /// where the operator learned them - and what lets a new node settle
    /// next to its parent instead of reshuffling the rest.
    static func solve(_ topology: Topology, previous: [String: CGPoint] = [:]) -> Result {
        let seeds = seeds(for: topology)
        let order = topology.nodes.map(\.id)
        let nodes = Dictionary(uniqueKeysWithValues: topology.nodes.map { ($0.id, $0) })
        var anchors: [String: CGPoint] = [:]
        var positions: [String: CGPoint] = [:]
        for id in order {
            let anchor = previous[id] ?? seeds[id]!
            anchors[id] = anchor
            positions[id] = anchor
        }
        var velocities: [String: CGVector] = Dictionary(uniqueKeysWithValues: order.map { ($0, CGVector.zero) })
        let restLength: [Edge: CGFloat] = Dictionary(uniqueKeysWithValues: topology.edges.map { edge in
            let a = seeds[edge.from]!, b = seeds[edge.to]!
            return (edge, hypot(a.x - b.x, a.y - b.y))
        })

        var ticks = 0
        while ticks < Tuning.maxTicks {
            ticks += 1
            var forces: [String: CGVector] = Dictionary(uniqueKeysWithValues: order.map { ($0, CGVector.zero) })
            // Repulsion between every pair inside the cutoff. O(n²) at a
            // scale of fifty nodes is a few thousand subtractions per tick.
            for i in order.indices {
                for j in order.indices where j > i {
                    let a = positions[order[i]]!, b = positions[order[j]]!
                    var dx = a.x - b.x, dy = a.y - b.y
                    var distance = hypot(dx, dy)
                    if distance < 0.01 {
                        // Two nodes on one point push apart along a direction
                        // decided by their order, never by a random draw.
                        dx = 1; dy = CGFloat(i - j); distance = hypot(dx, dy)
                    }
                    guard distance < Tuning.repulsionCutoff else { continue }
                    let minimum = nodes[order[i]]!.radius + nodes[order[j]]!.radius
                    let strength = Tuning.repulsion / max(distance * distance, minimum * minimum)
                    let fx = dx / distance * strength, fy = dy / distance * strength
                    forces[order[i]]!.dx += fx; forces[order[i]]!.dy += fy
                    forces[order[j]]!.dx -= fx; forces[order[j]]!.dy -= fy
                }
            }
            for edge in topology.edges {
                let a = positions[edge.from]!, b = positions[edge.to]!
                let dx = b.x - a.x, dy = b.y - a.y
                let distance = max(0.01, hypot(dx, dy))
                let stretch = distance - restLength[edge]!
                let fx = dx / distance * stretch * Tuning.spring, fy = dy / distance * stretch * Tuning.spring
                forces[edge.from]!.dx += fx; forces[edge.from]!.dy += fy
                forces[edge.to]!.dx -= fx; forces[edge.to]!.dy -= fy
            }
            var fastest: CGFloat = 0
            for id in order {
                guard nodes[id]!.kind != .project else { continue }
                let position = positions[id]!, anchor = anchors[id]!
                var force = forces[id]!
                force.dx += (anchor.x - position.x) * Tuning.anchorPull
                force.dy += (anchor.y - position.y) * Tuning.anchorPull
                var velocity = velocities[id]!
                velocity.dx = (velocity.dx + force.dx) * Tuning.damping
                velocity.dy = (velocity.dy + force.dy) * Tuning.damping
                let speed = hypot(velocity.dx, velocity.dy)
                if speed > Tuning.maxStep {
                    velocity.dx *= Tuning.maxStep / speed
                    velocity.dy *= Tuning.maxStep / speed
                }
                velocities[id] = velocity
                positions[id] = CGPoint(x: position.x + velocity.dx, y: position.y + velocity.dy)
                fastest = max(fastest, min(speed, Tuning.maxStep))
            }
            if fastest < Tuning.restVelocity { break }
        }
        positions = separateLabels(positions, topology: topology)
        return Result(positions: positions, ticks: ticks)
    }

    // MARK: Labels

    /// How much wider a label can be than its layout width once drawn: the
    /// fit shrinks the map to `minScale` but the type only to
    /// `labelMinFontScale`, so at the smallest fit a label covers this much
    /// more of the map than its points say.
    static let labelSlack = HideTheme.Home.labelMinFontScale / HideTheme.Home.minScale

    /// The room a label claims below a node, in layout points: its width at
    /// the worst fit, a gap either side, and the label height.
    static func labelFrame(for node: Node, at point: CGPoint) -> CGRect {
        let width = node.labelWidth * labelSlack + HideTheme.Home.labelGap * 2
        return CGRect(
            x: point.x - width / 2,
            y: point.y + node.radius + HideTheme.Home.labelGap,
            width: width,
            height: HideTheme.Home.labelHeight
        )
    }

    /// The disc a node occupies, in layout points.
    static func discFrame(for node: Node, at point: CGPoint) -> CGRect {
        CGRect(x: point.x - node.radius, y: point.y - node.radius, width: node.radius * 2, height: node.radius * 2)
    }

    /// After rest, a label may still cross another label or another node's
    /// disc. The later node in the stable order is turned a step around its
    /// parent, away from whatever it crossed, until it clears every earlier
    /// node; a bounded number of rounds keeps a crowded arc from looping.
    /// The project never moves.
    private static func separateLabels(_ positions: [String: CGPoint], topology: Topology) -> [String: CGPoint] {
        var positions = positions
        let nodes = topology.nodes.filter { $0.kind != .project }
        let project = topology.nodes.first { $0.kind == .project }
        func crosses(_ node: Node, _ other: Node) -> Bool {
            let a = positions[node.id]!, b = positions[other.id]!
            let label = labelFrame(for: node, at: a), disc = discFrame(for: node, at: a)
            let otherLabel = labelFrame(for: other, at: b), otherDisc = discFrame(for: other, at: b)
            return label.intersects(otherLabel) || label.intersects(otherDisc)
                || disc.intersects(otherLabel) || disc.intersects(otherDisc)
        }
        for _ in 0..<Tuning.labelNudgeRounds {
            var moved = false
            for (index, node) in nodes.enumerated() {
                guard let parentID = node.parentID, let centre = positions[parentID] else { continue }
                let blocker = nodes[..<index].first { crosses(node, $0) }
                    ?? project.flatMap { crosses(node, $0) ? $0 : nil }
                guard let blocker else { continue }
                let point = positions[node.id]!
                let dx = point.x - centre.x, dy = point.y - centre.y
                let orbit = hypot(dx, dy)
                let angle = atan2(dy, dx)
                if blocker.id == parentID {
                    // Crossing the parent itself: no turn clears it, so the
                    // node steps outward instead.
                    let grown = orbit + HideTheme.Home.labelHeight / 2
                    positions[node.id] = CGPoint(x: centre.x + grown * cos(angle), y: centre.y + grown * sin(angle))
                    moved = true
                    continue
                }
                let blockerPoint = positions[blocker.id]!
                var away = angle - atan2(blockerPoint.y - centre.y, blockerPoint.x - centre.x)
                away = atan2(sin(away), cos(away))
                // Turn away from what was crossed; a blocker dead ahead or on
                // the parent itself turns by the node's own parity so two
                // siblings never chase each other round the ring.
                let sign: CGFloat = away > 0.001 ? 1 : away < -0.001 ? -1 : (index.isMultiple(of: 2) ? 1 : -1)
                let turned = angle + sign * Tuning.labelNudgeStep
                positions[node.id] = CGPoint(x: centre.x + orbit * cos(turned), y: centre.y + orbit * sin(turned))
                moved = true
            }
            if !moved { break }
        }
        return positions
    }

    // MARK: Queries

    /// The ids within `depth` steps of `id` over undirected edges, `id`
    /// included. This is the local graph (PRD rule 2).
    static func neighborhood(of id: String, depth: Int, edges: [Edge]) -> Set<String> {
        var adjacency: [String: [String]] = [:]
        for edge in edges {
            adjacency[edge.from, default: []].append(edge.to)
            adjacency[edge.to, default: []].append(edge.from)
        }
        var seen: Set<String> = [id]
        var frontier: [String] = [id]
        for _ in 0..<depth {
            var next: [String] = []
            for node in frontier {
                for neighbour in adjacency[node] ?? [] where seen.insert(neighbour).inserted {
                    next.append(neighbour)
                }
            }
            frontier = next
        }
        return seen
    }

    /// The bounds of every node and label, so the view can fit the map.
    static func bounds(_ positions: [String: CGPoint], nodes: [Node]) -> CGRect {
        var rect: CGRect?
        for node in nodes {
            guard let point = positions[node.id] else { continue }
            let disc = CGRect(x: point.x - node.radius, y: point.y - node.radius, width: node.radius * 2, height: node.radius * 2)
            let frame = disc.union(labelFrame(for: node, at: point))
            rect = rect.map { $0.union(frame) } ?? frame
        }
        return rect ?? .zero
    }

    /// The node under a point, nearest centre first, within its radius plus
    /// a grab margin. Pure, so a click and a hover ask the same question.
    static func hit(_ point: CGPoint, positions: [String: CGPoint], nodes: [Node], margin: CGFloat) -> String? {
        var best: (id: String, distance: CGFloat)?
        for node in nodes {
            guard let centre = positions[node.id] else { continue }
            let distance = hypot(point.x - centre.x, point.y - centre.y)
            guard distance <= node.radius + margin else { continue }
            if best == nil || distance < best!.distance { best = (node.id, distance) }
        }
        return best?.id
    }
}

/// The session's node positions, keyed by node id and kept across snapshots
/// and across the overlay opening and closing.
///
/// The simulation runs only when the topology key changes; a snapshot that
/// changes status alone reads the same positions back. A node that left
/// drops out of the cache, one that stayed anchors where it was, and one
/// that arrived is seeded beside its parent (PRD rule 1).
final class ProjectHomeLayoutCache {
    private(set) var key = ""
    private(set) var positions: [String: CGPoint] = [:]
    private(set) var lastTicks = 0

    func positions(for topology: ProjectHomeGraphLayout.Topology) -> [String: CGPoint] {
        let nextKey = topology.key
        guard nextKey != key else { return positions }
        let retained = Set(topology.nodes.map(\.id))
        let result = ProjectHomeGraphLayout.solve(topology, previous: positions.filter { retained.contains($0.key) })
        key = nextKey
        positions = result.positions
        lastTicks = result.ticks
        return positions
    }
}
