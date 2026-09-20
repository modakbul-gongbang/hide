import Foundation

struct CoreRemoteStatus: Decodable, Identifiable {
    var id: String { targetID }
    let targetID: String
    let state: String
    let message: String?
    let herdrVersion: String?
    let session: CoreRemoteSessionSnapshot?
    let files: CoreRemoteFileList

    enum CodingKeys: String, CodingKey {
        case targetID = "target_id"
        case state
        case message
        case herdrVersion = "herdr_version"
        case session
        case files
    }
}
struct CoreRemoteFileList: Decodable {
    let rootPath: String?
    let state: String
    let entries: [CoreRemoteFileEntry]
    let message: String?
    let generation: UInt64

    enum CodingKeys: String, CodingKey {
        case rootPath = "root_path"
        case state
        case entries
        case message
        case generation
    }
}

struct CoreRemoteFileEntry: Decodable {
    let path: String
    let name: String
    let isDirectory: Bool
    let sizeBytes: UInt64

    enum CodingKeys: String, CodingKey {
        case path
        case name
        case isDirectory = "is_directory"
        case sizeBytes = "size_bytes"
    }
}

struct CoreRemoteSessionSnapshot: Decodable {
    let workspaces: [CoreWorkspaceSnapshot]
    let agents: [SidebarAgent]
    let activeTabIDs: [String: String]
    let focusedWorkspaceID: String?
    let focusedCheckoutID: String?
    let focusedTabID: String?
    let focusedPaneID: String?
    let paneLayouts: [RemotePaneLayoutSnapshot]

    enum CodingKeys: String, CodingKey {
        case workspaces
        case agents
        case activeTabIDs = "active_tab_ids"
        case focusedWorkspaceID = "focused_workspace_id"
        case focusedCheckoutID = "focused_checkout_id"
        case focusedTabID = "focused_tab_id"
        case focusedPaneID = "focused_pane_id"
        case paneLayouts = "pane_layouts"
    }
}
