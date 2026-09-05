import AppKit
import SwiftUI

/// Settings, drawn on this shell's own design system.
///
/// Every other surface in the app paints itself from `HideTheme`; this one
/// used to be a stack of SwiftUI `Form`s, so the system tab bar, grouped
/// section cards, bordered buttons, and rounded-border fields showed through
/// and made Settings read as a different application. The tokens are the same
/// ones the sidebar and the tab strip use: the four-step surface ladder,
/// hairline borders, no shadows, and the spacing and radius scales.
struct HideSettingsView: View {
    @ObservedObject var model: ShellModel
    /// The sheet has no title bar, so it carries its own close button. The
    /// Settings scene the menu bar opens has the window's own controls, and
    /// `dismiss` does not govern that window, so it does not show one.
    var showsCloseButton = false
    @Environment(\.dismiss) private var dismiss
    @State private var tab: HideSettingsTab = .general
    @State private var accentHex = HideSettingsView.fallbackAccentHex
    @State private var fontSize = HideSettingsView.fallbackFontSize

    static let fallbackAccentHex = "#B9FF66"
    static let fallbackFontSize = 13.0

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingNone) {
            header
            Rectangle()
                .fill(HideTheme.divider)
                .frame(height: HideTheme.Layout.hairlineWidth)
            HideSettingsTabBar(selection: $tab)
            Rectangle()
                .fill(HideTheme.divider)
                .frame(height: HideTheme.Layout.hairlineWidth)
            ScrollView {
                VStack(alignment: .leading, spacing: HideTheme.spacingLG) {
                    switch tab {
                    case .general: HideGeneralSettings(model: model)
                    case .appearance:
                        HideAppearanceSettings(
                            model: model,
                            accentHex: $accentHex,
                            fontSize: $fontSize
                        )
                    case .agents: HideAgentSettings()
                    case .pet: HidePetSettings(model: model)
                    case .devices: HideDeviceSettings(model: model)
                    case .shortcuts: HideShortcutSettings(model: model)
                    }
                }
                .padding(HideTheme.spacingXL)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .frame(width: HideTheme.settingsSheetSize.width, height: HideTheme.settingsSheetSize.height)
        .background(HideTheme.background)
        .preferredColorScheme(.dark)
        .onAppear {
            accentHex = model.core.snapshot?.uiState.accentHex ?? HideSettingsView.fallbackAccentHex
            fontSize = model.core.snapshot?.uiState.fontSize ?? HideSettingsView.fallbackFontSize
        }
    }

    /// The sheet had no way out but the Escape key, which is invisible. The
    /// close button is also the click target the visual check uses.
    private var header: some View {
        HStack(alignment: .top, spacing: HideTheme.spacingMD) {
            VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                Text("Settings")
                    .hideFont(size: 15, weight: .semibold)
                    .foregroundStyle(HideTheme.primary)
                Text(tab.subtitle)
                    .hideFont(size: 11)
                    .foregroundStyle(HideTheme.secondary)
            }
            Spacer(minLength: 0)
            if showsCloseButton {
                Button { dismiss() } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 9, weight: .semibold))
                }
                .buttonStyle(HideToolbarButtonStyle(isProminent: false))
                .help("Close Settings")
                .accessibilityLabel("Close Settings")
                .accessibilityIdentifier("hide-settings-close")
            }
        }
        .padding(.horizontal, HideTheme.spacingXL)
        .padding(.vertical, HideTheme.spacingLG)
    }
}

enum HideSettingsTab: String, CaseIterable, Identifiable {
    case general
    case appearance
    case agents
    case pet
    case devices
    case shortcuts

    var id: String { rawValue }

    var title: String {
        switch self {
        case .general: "General"
        case .appearance: "Appearance"
        case .agents: "Agents"
        case .pet: "Pet"
        case .devices: "Devices"
        case .shortcuts: "Shortcuts"
        }
    }

    var systemImage: String {
        switch self {
        case .general: "slider.horizontal.3"
        case .appearance: "paintbrush"
        case .agents: "sparkles"
        case .pet: "pawprint"
        case .devices: "externaldrive.connected.to.line.below"
        case .shortcuts: "command"
        }
    }

    /// One line under the title, so the pane says what it governs without a
    /// paragraph inside every group.
    var subtitle: String {
        switch self {
        case .general: "This instance, the Herdr runtime behind it, and where its state lives."
        case .appearance: "Accent and interface density. Dark is the only theme in this release."
        case .agents: "The agent CLIs this Mac can launch."
        case .pet: "Visibility, the global toggle shortcut, and the pet's connection."
        case .devices: "SSH targets. Authentication stays in your SSH environment."
        case .shortcuts: "Pane chords. Every other chord is listed in the menu bar."
        }
    }
}

private struct HideSettingsTabBar: View {
    @Binding var selection: HideSettingsTab

    var body: some View {
        HStack(spacing: HideTheme.spacingXXS) {
            ForEach(HideSettingsTab.allCases) { tab in
                let isSelected = tab == selection
                Button { selection = tab } label: {
                    HStack(spacing: HideTheme.spacingXS) {
                        Image(systemName: tab.systemImage)
                            .font(.system(size: 10, weight: .medium))
                        Text(tab.title)
                            .hideFont(size: 11, weight: isSelected ? .semibold : .medium)
                    }
                    .foregroundStyle(isSelected ? HideTheme.primary : HideTheme.secondary)
                    .padding(.horizontal, HideTheme.spacingMD)
                    .padding(.vertical, HideTheme.spacingSM)
                    .background(
                        isSelected ? HideTheme.elevated : .clear,
                        in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                    )
                    .overlay {
                        RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                            .stroke(
                                isSelected ? HideTheme.divider : .clear,
                                lineWidth: HideTheme.Layout.hairlineWidth
                            )
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("hide-settings-tab-\(tab.rawValue)")
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, HideTheme.spacingXL)
        .padding(.vertical, HideTheme.spacingSM)
        .background(HideTheme.sidebar)
    }
}

/// A titled card. Rows inside it are separated by hairlines rather than by
/// gaps, which is what keeps a settings pane reading as one table instead of
/// a stack of floating boxes.
struct HideSettingsGroup<Content: View>: View {
    let title: String
    var note: String?
    @ViewBuilder var content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
            Text(title)
                .hideFont(size: 11, weight: .semibold)
                .foregroundStyle(HideTheme.secondary)
            VStack(alignment: .leading, spacing: HideTheme.spacingNone) {
                content
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(HideTheme.panel, in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium))
            .overlay {
                RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
                    .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
            }
            if let note {
                Text(note)
                    .hideFont(size: 11)
                    .foregroundStyle(HideTheme.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

/// One row of a group: a label on the left, whatever the row is about on the
/// right. `showsDivider` is false on the last row so the card does not end in
/// a hairline.
struct HideSettingsRow<Content: View>: View {
    let label: String
    var showsDivider = true
    @ViewBuilder var content: Content

    var body: some View {
        VStack(spacing: HideTheme.spacingNone) {
            HStack(alignment: .firstTextBaseline, spacing: HideTheme.spacingMD) {
                Text(label)
                    .hideFont(size: 12)
                    .foregroundStyle(HideTheme.primary)
                Spacer(minLength: HideTheme.spacingSM)
                content
            }
            .padding(.horizontal, HideTheme.spacingMD)
            .padding(.vertical, HideTheme.spacingMD - HideTheme.spacingXXS)
            if showsDivider {
                Rectangle()
                    .fill(HideTheme.divider)
                    .frame(height: HideTheme.Layout.hairlineWidth)
            }
        }
    }
}

/// A row that carries only a value, and a row that carries only a message.
struct HideSettingsValue: View {
    let text: String
    var isMonospaced = true

    var body: some View {
        Text(text)
            .hideFont(size: 11, design: isMonospaced ? .monospaced : .default)
            .foregroundStyle(HideTheme.secondary)
            .multilineTextAlignment(.trailing)
            .lineLimit(2)
            .truncationMode(.middle)
    }
}

struct HideSettingsNote: View {
    let text: String
    var systemImage: String?
    var color: Color = HideTheme.secondary
    var showsDivider = true

    var body: some View {
        VStack(spacing: HideTheme.spacingNone) {
            HStack(alignment: .top, spacing: HideTheme.spacingSM) {
                if let systemImage {
                    Image(systemName: systemImage)
                        .font(.system(size: 10, weight: .medium))
                        .foregroundStyle(color)
                }
                Text(text)
                    .hideFont(size: 11)
                    .foregroundStyle(color)
                    .fixedSize(horizontal: false, vertical: true)
                Spacer(minLength: 0)
            }
            .padding(.horizontal, HideTheme.spacingMD)
            .padding(.vertical, HideTheme.spacingMD - HideTheme.spacingXXS)
            if showsDivider {
                Rectangle()
                    .fill(HideTheme.divider)
                    .frame(height: HideTheme.Layout.hairlineWidth)
            }
        }
    }
}

/// The one text-field look in Settings. `.roundedBorder` is system chrome and
/// it was the loudest system element in the old sheet.
struct HideSettingsField: View {
    let placeholder: String
    @Binding var text: String
    var width: CGFloat?
    var onSubmit: () -> Void = {}

    var body: some View {
        TextField(placeholder, text: $text)
            .textFieldStyle(.plain)
            .hideFont(size: 11, design: .monospaced)
            .foregroundStyle(HideTheme.primary)
            .labelsHidden()
            .onSubmit(onSubmit)
            .padding(.horizontal, HideTheme.spacingSM)
            .frame(width: width, height: 24)
            .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall))
            .overlay {
                RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                    .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
            }
    }
}

private struct HideGeneralSettings: View {
    @ObservedObject var model: ShellModel

    private var bundleIdentifier: String {
        Bundle.main.bundleIdentifier ?? "unknown"
    }

    var body: some View {
        HideSettingsGroup(
            title: "Instance",
            note: bundleIdentifier == CoreBridge.releaseBundleIdentifier
                ? nil
                : "This build carries a per-worktree identifier, so it keeps its own state file and runs beside the release build."
        ) {
            HideSettingsRow(label: "Bundle") {
                HideSettingsValue(text: bundleIdentifier)
            }
            HideSettingsRow(label: "Herdr") {
                HStack(spacing: HideTheme.spacingSM) {
                    Circle()
                        .fill(model.herdrIsConnected ? HideTheme.success : HideTheme.warning)
                        .frame(width: 6, height: 6)
                    HideSettingsValue(
                        text: model.herdrIsConnected ? "Connected" : "Waiting for Herdr",
                        isMonospaced: false
                    )
                }
            }
            HideSettingsRow(label: "State file", showsDivider: false) {
                HideSettingsValue(text: CoreBridge.defaultStatePath())
            }
        }

        HideSettingsGroup(title: "Herdr runtime") {
            if let selection = model.core.runtimeSelection {
                HideSettingsRow(label: "Version") {
                    HideSettingsValue(text: selection.version)
                }
                HideSettingsRow(label: "Source") {
                    HideSettingsValue(text: selection.source)
                }
                HideSettingsRow(label: "Path") {
                    HideSettingsValue(text: selection.path)
                }
                HideSettingsRow(label: "SHA-256", showsDivider: selection.guidance != nil) {
                    HideSettingsValue(text: selection.sha256 ?? "not recorded")
                }
                if let guidance = selection.guidance {
                    HideSettingsNote(
                        text: guidance,
                        systemImage: "exclamationmark.triangle.fill",
                        color: HideTheme.warning,
                        showsDivider: false
                    )
                }
            } else {
                HideSettingsNote(
                    text: "No verified Herdr runtime is available for this launch.",
                    systemImage: "exclamationmark.triangle.fill",
                    color: HideTheme.warning,
                    showsDivider: false
                )
            }
        }

        HideSettingsGroup(title: "Environment") {
            HideSettingsRow(label: "Login PATH", showsDivider: false) {
                HideSettingsValue(text: HideRuntimeEnvironment.loginShellPath() ?? "unavailable")
            }
        }

        HideSettingsGroup(title: "Authentication") {
            HideSettingsNote(
                text: "Hide delegates authentication to Herdr and the selected agent CLI.",
                systemImage: "lock.shield"
            )
            HideSettingsNote(
                text: "No credential form, secret storage, token field, or passphrase handling is provided by Hide.",
                color: HideTheme.muted,
                showsDivider: false
            )
        }
    }
}

private struct HideAppearanceSettings: View {
    @ObservedObject var model: ShellModel
    @Binding var accentHex: String
    @Binding var fontSize: Double
    @Environment(\.hideAccent) private var accent
    private let accents = ["#B9FF66", "#7DD3FC", "#C4B5FD", "#FDBA74"]

    var body: some View {
        HideSettingsGroup(
            title: "Theme",
            note: "Dark is the only product theme in this release. Accent changes stay restrained so status keeps the color."
        ) {
            HideSettingsRow(label: "Accent", showsDivider: false) {
                HStack(spacing: HideTheme.spacingSM) {
                    ForEach(accents, id: \.self) { hex in
                        Button {
                            accentHex = hex
                            model.updatePreferences(accentHex: hex)
                        } label: {
                            Circle()
                                .fill(HideTheme.color(for: hex))
                                .frame(width: 18, height: 18)
                                .overlay {
                                    Circle()
                                        .stroke(
                                            accentHex == hex ? HideTheme.primary : HideTheme.divider,
                                            lineWidth: accentHex == hex ? 2 : HideTheme.Layout.hairlineWidth
                                        )
                                }
                                .contentShape(Circle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("Accent \(hex)")
                    }
                    HideSettingsValue(text: accentHex)
                }
            }
        }

        HideSettingsGroup(title: "Density") {
            HideSettingsRow(label: "Interface font", showsDivider: false) {
                HStack(spacing: HideTheme.spacingMD) {
                    Slider(value: $fontSize, in: 11 ... 17, step: 1)
                        .labelsHidden()
                        .tint(accent)
                        .frame(width: 180)
                        .onChange(of: fontSize) { _, value in
                            model.updatePreferences(fontSize: value)
                        }
                    HideSettingsValue(text: "\(Int(fontSize)) pt")
                }
            }
        }
    }
}

private struct HideAgentSettings: View {
    var body: some View {
        HideSettingsGroup(
            title: "Installed CLIs",
            note: "Both are resolved on the login shell's PATH, which is the same PATH a launched agent inherits."
        ) {
            HideCLIStatus(name: "claude")
            HideCLIStatus(name: "codex", showsDivider: false)
        }

        HideSettingsGroup(title: "Launch safety") {
            HideSettingsNote(
                text: "Permission bypass is always off when a New Agent dialog opens, and it applies only to that one launch.",
                systemImage: "shield.lefthalf.filled",
                showsDivider: false
            )
        }
    }
}

private struct HideCLIStatus: View {
    let name: String
    var showsDivider = true

    private var isUsable: Bool { AgentCLIAvailability.isUsable(name) }

    var body: some View {
        VStack(spacing: HideTheme.spacingNone) {
            HStack(spacing: HideTheme.spacingSM) {
                Image(systemName: isUsable ? "checkmark.circle.fill" : "exclamationmark.circle")
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(isUsable ? HideTheme.success : HideTheme.warning)
                Text(name)
                    .hideFont(size: 12, weight: .semibold)
                    .foregroundStyle(HideTheme.primary)
                Spacer(minLength: HideTheme.spacingSM)
                HideSettingsValue(text: AgentCLIAvailability.executable(for: name) ?? "not found on login PATH")
            }
            .padding(.horizontal, HideTheme.spacingMD)
            .padding(.vertical, HideTheme.spacingMD - HideTheme.spacingXXS)
            if showsDivider {
                Rectangle()
                    .fill(HideTheme.divider)
                    .frame(height: HideTheme.Layout.hairlineWidth)
            }
        }
    }
}

/// The pet's own settings. The same visibility state the menu bar, the
/// shortcut, and the URL scheme write, plus the shortcut itself.
private struct HidePetSettings: View {
    @ObservedObject var model: ShellModel
    @Environment(\.hideAccent) private var accent
    @State private var capturing = false

    private var pet: CorePetSnapshot? { model.core.pet }

    var body: some View {
        HideSettingsGroup(title: "Pet") {
            HideSettingsRow(label: "Show pet") {
                Toggle(
                    "",
                    isOn: Binding(
                        get: { pet?.visible ?? true },
                        set: { model.core.setPetVisible($0) }
                    )
                )
                .toggleStyle(.switch)
                .labelsHidden()
                .tint(accent)
                .accessibilityIdentifier("pet-visible-toggle")
            }
            HideSettingsRow(label: "Toggle shortcut", showsDivider: petFootnote != nil) {
                HStack(spacing: HideTheme.spacingSM) {
                    PetShortcutCaptureField(
                        capturing: $capturing,
                        current: pet?.shortcut
                    ) { hotkey in
                        model.updatePetShortcut(hotkey.canonical)
                    }
                    .frame(width: 150, height: 24)
                    .accessibilityIdentifier("pet-shortcut-field")
                    Button(capturing ? "Cancel" : "Record") { capturing.toggle() }
                        .buttonStyle(HideToolbarButtonStyle(isProminent: false))
                    Button("Clear") {
                        capturing = false
                        model.updatePetShortcut(nil)
                    }
                    .buttonStyle(HideToolbarButtonStyle(isProminent: false))
                    .disabled(pet?.shortcut == nil)
                }
            }
            if let petFootnote {
                HideSettingsNote(
                    text: petFootnote.text,
                    systemImage: petFootnote.symbol,
                    color: petFootnote.color,
                    showsDivider: false
                )
                .accessibilityIdentifier(petFootnote.identifier)
            }
        }

        HideSettingsGroup(title: "Connection") {
            HideSettingsRow(label: "Herdr", showsDivider: false) {
                HStack(spacing: HideTheme.spacingSM) {
                    Image(
                        systemName: (pet?.isConnected ?? false)
                            ? "checkmark.circle.fill"
                            : "exclamationmark.triangle.fill"
                    )
                    .font(.system(size: 10, weight: .medium))
                    .foregroundStyle((pet?.isConnected ?? false) ? HideTheme.success : HideTheme.warning)
                    HideSettingsValue(
                        text: pet?.connectionMessage ?? (pet?.connection ?? "unknown"),
                        isMonospaced: false
                    )
                }
            }
        }
        .accessibilityIdentifier("pet-connection-status")
    }

    private struct PetFootnote {
        let text: String
        let symbol: String
        let color: Color
        let identifier: String
    }

    private var petFootnote: PetFootnote? {
        if let error = pet?.shortcutError {
            return PetFootnote(
                text: error,
                symbol: "exclamationmark.triangle.fill",
                color: HideTheme.danger,
                identifier: "pet-shortcut-error"
            )
        }
        if pet?.shortcut == nil {
            return PetFootnote(
                text: "No shortcut is registered. The menu bar, the toggle above, and herdr-ide://toggle still work.",
                symbol: "info.circle",
                color: HideTheme.secondary,
                identifier: "pet-shortcut-unset"
            )
        }
        return nil
    }
}

private struct HideDeviceSettings: View {
    @ObservedObject var model: ShellModel
    @Environment(\.hideAccent) private var accent
    @State private var showAddDevice = false

    var body: some View {
        HideSettingsGroup(
            title: "SSH targets",
            note: "Hide stores only a label and an SSH alias. No password or token is collected."
        ) {
            ForEach(Array(model.devices.enumerated()), id: \.element.id) { index, device in
                HideDeviceRow(
                    device: device,
                    accent: accent,
                    showsDivider: index < model.devices.count - 1,
                    onTest: { model.testDevice(device) },
                    onRemove: { model.removeDevice(device) }
                )
            }
            if model.devices.isEmpty {
                HideSettingsNote(
                    text: "No device is registered. This Mac is always available without one.",
                    color: HideTheme.muted,
                    showsDivider: false
                )
            }
        }

        HStack {
            Spacer(minLength: 0)
            Button("Add device") { showAddDevice = true }
                .buttonStyle(HideToolbarButtonStyle(isProminent: true))
                .accessibilityIdentifier("hide-settings-add-device")
        }

        if model.remote.phase != .idle {
            HideSettingsGroup(title: "Remote status") {
                HideSettingsRow(label: model.remote.phase.rawValue.capitalized) {
                    HStack(spacing: HideTheme.spacingSM) {
                        Image(systemName: remoteStatusSymbol)
                            .font(.system(size: 10, weight: .medium))
                            .foregroundStyle(remoteStatusColor)
                        HideSettingsValue(text: model.remote.checkedAt)
                    }
                }
                HideSettingsNote(
                    text: model.remote.message,
                    showsDivider: !model.remote.workspaces.isEmpty || canRetryRemote
                )
                ForEach(Array(model.remote.workspaces.enumerated()), id: \.element.id) { index, workspace in
                    HideSettingsRow(
                        label: workspace.label,
                        showsDivider: canRetryRemote || index < model.remote.workspaces.count - 1
                    ) {
                        HideSettingsValue(text: "\(workspace.paneCount) panes")
                    }
                }
                if canRetryRemote {
                    HStack {
                        Spacer(minLength: 0)
                        Button("Retry") { model.retryRemote() }
                            .buttonStyle(HideToolbarButtonStyle(isProminent: false))
                    }
                    .padding(.horizontal, HideTheme.spacingMD)
                    .padding(.vertical, HideTheme.spacingSM)
                }
            }
        }
    }

    private var canRetryRemote: Bool {
        model.remote.phase == .failed
            || model.remote.phase == .unavailable
            || model.remote.phase == .stale
    }

    private var remoteStatusSymbol: String {
        switch model.remote.phase {
        case .ready: "checkmark.circle.fill"
        case .loading: "arrow.triangle.2.circlepath"
        case .failed, .unavailable, .stale: "exclamationmark.triangle.fill"
        case .idle: "circle"
        }
    }

    private var remoteStatusColor: Color {
        switch model.remote.phase {
        case .ready: HideTheme.success
        case .failed, .unavailable, .stale: HideTheme.warning
        case .loading, .idle: accent
        }
    }
}

private struct HideDeviceRow: View {
    let device: CoreDeviceSnapshot
    let accent: Color
    let showsDivider: Bool
    let onTest: () -> Void
    let onRemove: () -> Void

    var body: some View {
        VStack(spacing: HideTheme.spacingNone) {
            HStack(spacing: HideTheme.spacingSM) {
                Circle()
                    .fill(device.kind == "remote" ? HideTheme.success : accent)
                    .frame(width: 6, height: 6)
                VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                    Text(device.label)
                        .hideFont(size: 12, weight: .semibold)
                        .foregroundStyle(HideTheme.primary)
                    Text(device.sshAlias ?? "local, no SSH alias")
                        .hideFont(size: 10, design: .monospaced)
                        .foregroundStyle(HideTheme.secondary)
                }
                Spacer(minLength: HideTheme.spacingSM)
                if device.kind == "remote" {
                    Button("Test", action: onTest)
                        .buttonStyle(HideToolbarButtonStyle(isProminent: false))
                    Button("Remove", action: onRemove)
                        .buttonStyle(HideDestructiveButtonStyle())
                }
            }
            .padding(.horizontal, HideTheme.spacingMD)
            .padding(.vertical, HideTheme.spacingSM)
            if showsDivider {
                Rectangle()
                    .fill(HideTheme.divider)
                    .frame(height: HideTheme.Layout.hairlineWidth)
            }
        }
    }
}

private struct HideShortcutSettings: View {
    @ObservedObject var model: ShellModel

    var body: some View {
        HideSettingsGroup(
            title: "Pane chords",
            note: "Write a chord as modifiers plus a key, joined by +: command+shift+d. Enter or Apply saves it."
        ) {
            ForEach(Array(PaneCommand.allCases.enumerated()), id: \.element.id) { index, command in
                HidePaneShortcutRow(
                    command: command,
                    model: model,
                    showsDivider: index < PaneCommand.allCases.count - 1
                        || model.shortcutDiagnostic != nil
                )
            }
            if let diagnostic = model.shortcutDiagnostic {
                HideSettingsNote(
                    text: diagnostic,
                    systemImage: "exclamationmark.triangle.fill",
                    color: HideTheme.warning,
                    showsDivider: false
                )
            }
        }

        HideSettingsGroup(title: "Direct selection") {
            HideSettingsRow(label: "Select tab 1-9") {
                HideSettingsKeycaps(labels: ["⌘1", "…", "⌘9"])
            }
            HideSettingsRow(label: "Select agent 1-9", showsDivider: false) {
                HideSettingsKeycaps(labels: ["⌃1", "…", "⌃9"])
            }
        }
    }
}

private struct HidePaneShortcutRow: View {
    let command: PaneCommand
    @ObservedObject var model: ShellModel
    let showsDivider: Bool
    @State private var draft: String

    init(command: PaneCommand, model: ShellModel, showsDivider: Bool) {
        self.command = command
        self.model = model
        self.showsDivider = showsDivider
        _draft = State(initialValue: model.shortcut(for: command).canonical)
    }

    var body: some View {
        VStack(spacing: HideTheme.spacingNone) {
            HStack(spacing: HideTheme.spacingMD) {
                Text(command.title)
                    .hideFont(size: 12)
                    .foregroundStyle(HideTheme.primary)
                Spacer(minLength: HideTheme.spacingSM)
                HideSettingsKeycaps(labels: [model.shortcut(for: command).displayString])
                HideSettingsField(
                    placeholder: "command+d",
                    text: $draft,
                    width: 168
                ) {
                    model.updateShortcut(command, raw: draft)
                }
                .accessibilityLabel("\(command.title) shortcut")
                Button("Apply") { model.updateShortcut(command, raw: draft) }
                    .buttonStyle(HideToolbarButtonStyle(isProminent: false))
            }
            .padding(.horizontal, HideTheme.spacingMD)
            .padding(.vertical, HideTheme.spacingSM)
            if let error = model.shortcutErrors[command] {
                HStack(spacing: HideTheme.spacingSM) {
                    Image(systemName: "exclamationmark.circle.fill")
                        .font(.system(size: 10, weight: .medium))
                        .foregroundStyle(HideTheme.danger)
                    Text(error)
                        .hideFont(size: 11)
                        .foregroundStyle(HideTheme.danger)
                    Spacer(minLength: 0)
                }
                .padding(.horizontal, HideTheme.spacingMD)
                .padding(.bottom, HideTheme.spacingSM)
            }
            if showsDivider {
                Rectangle()
                    .fill(HideTheme.divider)
                    .frame(height: HideTheme.Layout.hairlineWidth)
            }
        }
        .onChange(of: model.shortcut(for: command).canonical) { _, value in
            draft = value
        }
    }
}

/// The keycap the sidebar and the tab strip already draw, reused so a chord
/// looks the same everywhere it is shown.
struct HideSettingsKeycaps: View {
    let labels: [String]

    var body: some View {
        HStack(spacing: HideTheme.spacingXS) {
            ForEach(labels, id: \.self) { label in
                Text(label)
                    .hideFont(size: 10, weight: .medium, design: .monospaced)
                    .foregroundStyle(HideTheme.secondary)
                    .padding(.horizontal, HideTheme.spacingXS + 1)
                    .frame(height: 18)
                    .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall))
                    .overlay {
                        RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                            .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
                    }
            }
        }
    }
}

/// Removing a device is the one destructive action in Settings, so it is the
/// one button allowed to carry the danger color.
struct HideDestructiveButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .hideFont(size: 11, weight: .semibold)
            .foregroundStyle(HideTheme.danger)
            .padding(.horizontal, HideTheme.spacingSM)
            .padding(.vertical, 7)
            .background(
                HideTheme.danger.opacity(0.12),
                in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
            )
            .opacity(configuration.isPressed ? 0.72 : 1)
    }
}

struct AddDeviceSheet: View {
    @ObservedObject var model: ShellModel
    @Environment(\.dismiss) private var dismiss
    @State private var label = ""
    @State private var alias = ""

    private var canAdd: Bool {
        !label.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            && !alias.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingLG) {
            SheetHeader(
                title: "Add device",
                subtitle: "Use an existing SSH alias. Hide never asks for credentials."
            )
            HideSettingsGroup(title: "Device") {
                HideSettingsRow(label: "Label") {
                    HideSettingsField(placeholder: "mini", text: $label, width: 210)
                }
                HideSettingsRow(label: "SSH alias", showsDivider: false) {
                    HideSettingsField(placeholder: "my-mac-mini", text: $alias, width: 210)
                }
            }
            .padding(.horizontal, HideTheme.spacingXL)
            Spacer(minLength: 0)
            HStack(spacing: HideTheme.spacingSM) {
                Spacer(minLength: 0)
                Button("Cancel") { dismiss() }
                    .buttonStyle(HideToolbarButtonStyle(isProminent: false))
                Button("Add") {
                    model.addDevice(label: label, alias: alias)
                    dismiss()
                }
                .buttonStyle(HideToolbarButtonStyle(isProminent: true))
                .keyboardShortcut(.defaultAction)
                .disabled(!canAdd)
                .opacity(canAdd ? 1 : 0.5)
            }
            .padding(.horizontal, HideTheme.spacingXL)
            .padding(.bottom, HideTheme.spacingXL)
        }
        .frame(width: HideTheme.addDeviceSheetSize.width, height: HideTheme.addDeviceSheetSize.height)
        .background(HideTheme.background)
        .preferredColorScheme(.dark)
    }
}
