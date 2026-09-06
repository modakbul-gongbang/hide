import SwiftUI

struct HideKeycap: View {
    let command: HideCommand
    @EnvironmentObject private var model: ShellModel

    var emphasized = true
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        Text(command.displayString(bindings: model.paneShortcuts))
            .hideFont(size: HideTheme.Typography.micro, weight: .medium, design: .monospaced)
            .foregroundStyle(emphasized ? HideTheme.primary : HideTheme.muted)
            .padding(.horizontal, HideTheme.Hint.horizontalPadding)
            .frame(height: HideTheme.Hint.keycapHeight)
            .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall))
            .overlay {
                RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                    .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
            }
            .animation(.easeOut(duration: HideTooltipState.fadeDuration(reduceMotion: reduceMotion)), value: emphasized)
    }

}

struct HideKeycapGroup: View {
    let commands: [HideCommand]
    var body: some View {
        HStack(spacing: HideTheme.spacingXS) {
            ForEach(Array(commands.enumerated()), id: \.offset) { _, command in
                HideKeycap(command: command)
            }
        }
    }
}
