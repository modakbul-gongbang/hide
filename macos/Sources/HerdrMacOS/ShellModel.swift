import Combine
import Foundation

enum ShellSurface: String, CaseIterable, Hashable {
    case agents
    case terminal
    case workbench

    var title: String {
        switch self {
        case .agents:
            "Agents"
        case .terminal:
            "Terminal"
        case .workbench:
            "Workbench"
        }
    }
}

@MainActor
final class ShellModel: ObservableObject {
    @Published var activeSurface: ShellSurface = .terminal
    let core: CoreBridge
    private var coreSubscription: AnyCancellable?

    init(core: CoreBridge = CoreBridge()) {
        self.core = core
        coreSubscription = core.objectWillChange.sink { [weak self] _ in
            self?.objectWillChange.send()
        }
    }

    func focus(_ surface: ShellSurface) {
        activeSurface = surface
    }
}
