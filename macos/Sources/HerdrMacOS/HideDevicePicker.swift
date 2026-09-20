import SwiftUI

/// The snapshot owns connection and selection. Keyboard focus is only the
/// candidate row, and does not switch devices until activation.
struct HideDevicePicker: View {
    let devices: [CoreDeviceSnapshot]
    let selectedID: String
    let select: (CoreDeviceSnapshot) -> Void
    let dismiss: () -> Void
    @FocusState private var focusedID: String?

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
            Text("DEVICES")
                .hideFont(size: HideTheme.Typography.caption, weight: .bold)
                .foregroundStyle(HideTheme.muted)
                .padding(.horizontal, HideTheme.spacingSM)
            if devices.isEmpty {
                Text("No devices available")
                    .hideFont(size: HideTheme.Typography.body)
                    .foregroundStyle(HideTheme.secondary)
                    .padding(HideTheme.spacingSM)
            } else {
                ScrollView {
                    VStack(spacing: HideTheme.spacingXS) {
                        ForEach(devices) { device in
                            Button { select(device) } label: {
                                HideDevicePickerRow(device: device, selected: device.id == selectedID)
                            }
                            .buttonStyle(HideInteractiveButtonStyle())
                            .focused($focusedID, equals: device.id)
                            .accessibilityLabel(DevicePickerPresentation.spoken(device, selected: device.id == selectedID))
                            .accessibilityAddTraits(device.id == selectedID ? .isSelected : [])
                            .accessibilityIdentifier("device-picker-\(device.id)")
                        }
                    }
                }
                .frame(maxHeight: HideTheme.Layout.relationshipListMaxHeight)
                .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(HideTheme.spacingMD)
        .frame(width: HideTheme.Hint.tooltipMaxWidth)
        .background(HideTheme.elevated)
        .onAppear { reconcileFocus() }
        .onChange(of: devices.map(\.id)) { _, _ in reconcileFocus() }
        .onMoveCommand { direction in
            guard direction == .up || direction == .down,
                  let index = devices.firstIndex(where: { $0.id == focusedID }), !devices.isEmpty else { return }
            let next = direction == .down ? min(index + 1, devices.count - 1) : max(index - 1, 0)
            focusedID = devices[next].id
        }
        .onKeyPress(.return) {
            guard let device = devices.first(where: { $0.id == focusedID }) else { return .ignored }
            select(device)
            return .handled
        }
        .onExitCommand(perform: dismiss)
        .accessibilityIdentifier("device-picker")
    }

    private func reconcileFocus() {
        if let focusedID, devices.contains(where: { $0.id == focusedID }) { return }
        focusedID = devices.first(where: { $0.id == selectedID })?.id ?? devices.first?.id
    }
}

struct HideDevicePickerRow: View {
    let device: CoreDeviceSnapshot
    let selected: Bool

    var body: some View {
        HStack(spacing: HideTheme.spacingMD) {
            Image(systemName: device.kind == "remote" ? "server.rack" : "laptopcomputer")
                .hideFont(size: HideTheme.Typography.title)
                .foregroundStyle(HideTheme.secondary)
                .frame(width: HideTheme.checkoutIconWidth)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                Text(device.label)
                    .hideFont(size: HideTheme.Typography.subhead, weight: .semibold)
                    .foregroundStyle(HideTheme.primary)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Text(DevicePickerPresentation.detail(device))
                    .hideFont(size: HideTheme.Typography.caption)
                    .foregroundStyle(HideTheme.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: HideTheme.spacingSM)
            if selected {
                Image(systemName: "checkmark")
                    .hideFont(size: HideTheme.Typography.body, weight: .semibold)
                    .foregroundStyle(HideTheme.primary)
                    .accessibilityHidden(true)
            }
        }
        .padding(.horizontal, HideTheme.spacingMD)
        .padding(.vertical, HideTheme.spacingSM)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(selected ? HideTheme.accent.opacity(HideTheme.Opacity.selectedFill) : .clear,
                    in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium))
    }
}

enum DevicePickerPresentation {
    static func detail(_ device: CoreDeviceSnapshot) -> String {
        let count = "\(device.agentCount) \(device.agentCount == 1 ? "agent" : "agents")"
        guard device.kind == "remote" else { return "Local · \(count)" }
        switch device.state {
        case "ready": return "Remote · Connected · \(count)"
        case "loading", "connecting": return "Remote · Connecting…"
        case "unavailable": return "Remote · Not connected"
        default: return "Remote · \(device.state)"
        }
    }

    static func spoken(_ device: CoreDeviceSnapshot, selected: Bool) -> String {
        [device.label, detail(device), selected ? "Selected" : nil].compactMap { $0 }.joined(separator: ", ")
    }
}
