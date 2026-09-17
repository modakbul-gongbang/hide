import SwiftUI

/// The chat composer: three chips on one line, a message, and Send.
///
/// It replaces the form this shell used to open. That form asked four
/// questions before the
/// operator could type anything and refused to start without a checkout; this
/// asks one - what do you want - and answers the other three with defaults
/// already filled in.
struct ChatComposerSheet: View {
    @EnvironmentObject private var model: ShellModel
    @Environment(\.dismiss) private var dismiss
    @State private var message = ""
    @State private var provider: AgentProvider = .claude
    @State private var bypassWarnings = false
    @FocusState private var messageFocused: Bool

    private var agentIsInstalled: Bool {
        AgentCLIAvailability.isUsable(provider.rawValue)
    }

    private var canSend: Bool {
        ChatComposerPolicy.canSend(
            message: message,
            agentIsInstalled: agentIsInstalled,
            isSubmitting: model.composerSubmitting
        )
    }

    private var whereLabel: String {
        guard let checkout = model.composerCheckout else { return model.scratch.label }
        return checkout.label
    }

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingMD) {
            chips
            messageField
            footer
        }
        .padding(HideTheme.spacingLG)
        .frame(width: HideTheme.composerSheetSize.width, height: HideTheme.composerSheetSize.height)
        .background(HideTheme.panel)
        .preferredColorScheme(.dark)
        .accessibilityIdentifier("hide-chat-composer")
        .onAppear {
            provider = AgentProvider(rawValue: model.core.snapshot?.uiState.lastAgentKind ?? "")
                ?? .claude
            bypassWarnings = model.core.snapshot?.uiState.lastAgentBypass ?? false
            messageFocused = true
        }
        .onExitCommand {
            guard !model.composerSubmitting else { return }
            dismiss()
        }
    }

    /// Where, Run on, Agent - one line, each already answered.
    private var chips: some View {
        HStack(spacing: HideTheme.spacingSM) {
            Menu {
                Button(model.scratch.label) { model.composerCheckoutID = nil }
                ForEach(model.composerCheckouts, id: \.checkout.id) { item in
                    Button("\(item.workspace.repoName) / \(item.checkout.label)") {
                        model.composerCheckoutID = item.checkout.id
                    }
                }
            } label: {
                HideMenuChipLabel(title: whereLabel, image: Image(systemName: "tray"))
            }
            .menuStyle(.borderlessButton)
            .fixedSize()
            .accessibilityIdentifier("hide-composer-where")

            Menu {
                ForEach(model.devices) { device in
                    Button(device.label) { model.composerDeviceID = device.id }
                }
            } label: {
                HideMenuChipLabel(
                    title: model.devices.first(where: { $0.id == model.composerDeviceID })?.label
                        ?? "This Mac",
                    image: Image(systemName: "desktopcomputer")
                )
            }
            .menuStyle(.borderlessButton)
            .fixedSize()
            .accessibilityIdentifier("hide-composer-device")

            Menu {
                ForEach(AgentProvider.allCases, id: \.rawValue) { candidate in
                    Button(candidate.rawValue.capitalized) { provider = candidate }
                }
                Divider()
                Toggle("Pass the CLI bypass flag", isOn: $bypassWarnings)
            } label: {
                if let mark = AgentMark.image(for: provider.rawValue, side: HideTheme.agentMarkWidth) {
                    HideMenuChipLabel(
                        title: provider.rawValue.capitalized,
                        image: Image(nsImage: mark)
                    )
                } else {
                    HideMenuChipLabel(
                        title: provider.rawValue.capitalized,
                        image: Image(systemName: "sparkles")
                    )
                }
            }
            .menuStyle(.borderlessButton)
            .fixedSize()
            .accessibilityIdentifier("hide-composer-agent")

            if bypassWarnings {
                Label(
                    "Bypass flag on",
                    systemImage: "exclamationmark.triangle.fill"
                )
                .hideFont(size: HideTheme.Typography.caption, weight: .medium)
                .foregroundStyle(HideTheme.warning)
                .accessibilityIdentifier("hide-composer-bypass-warning")
            }
            Spacer(minLength: 0)
        }
    }

    private var messageField: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
            TextEditor(text: $message)
                .focused($messageFocused)
                .scrollContentBackground(.hidden)
                .hideInputSurface(focused: messageFocused)
                .overlay(alignment: .topLeading) {
                    if message.isEmpty {
                        Text("Ask anything")
                            .hideFont(size: HideTheme.Typography.title)
                            .foregroundStyle(HideTheme.muted)
                            .padding(.horizontal, HideTheme.spacingMD)
                            .padding(.vertical, HideTheme.spacingLG)
                            .allowsHitTesting(false)
                    }
                }
                .disabled(model.composerSubmitting)
                .accessibilityIdentifier("hide-composer-message")

            if !agentIsInstalled {
                VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                    Label(
                        "\(provider.rawValue) is not on the login-shell PATH.",
                        systemImage: "exclamationmark.triangle"
                    )
                    .hideFont(size: HideTheme.Typography.caption)
                    .foregroundStyle(HideTheme.warning)
                    Link("Install \(provider.rawValue)", destination: provider == .claude
                        ? URL(string: "https://docs.anthropic.com/en/docs/claude-code/overview")!
                        : URL(string: "https://developers.openai.com/codex/")!)
                        .hideFont(size: HideTheme.Typography.caption)
                }
                .accessibilityIdentifier("hide-composer-agent-missing")
            }
        }
    }

    private var footer: some View {
        HStack(spacing: HideTheme.spacingSM) {
            Spacer(minLength: 0)
            Button(action: send) {
                HStack(spacing: HideTheme.spacingXS + 2) {
                    if model.composerSubmitting {
                        ProgressView()
                            .controlSize(.small)
                        Text("Starting")
                            .hideFont(size: HideTheme.Typography.body, weight: .semibold)
                    } else {
                        Text("Send")
                            .hideFont(size: HideTheme.Typography.body, weight: .semibold)
                        HideKeycap(command: .label(PaneShortcut(key: "↩", modifiers: [.command]).displayString), emphasized: false)
                            .opacity(HideTheme.Opacity.secondary)
                    }
                }
            }
            .buttonStyle(HideTextButtonStyle(appearance: .prominent))
            .disabled(!canSend)
            .keyboardShortcut(.return, modifiers: .command)
            .accessibilityIdentifier("hide-composer-send")
        }
    }

    private func send() {
        guard canSend else { return }
        model.sendComposerMessage(
            provider: provider,
            message: message,
            bypassWarnings: bypassWarnings
        )
    }
}
