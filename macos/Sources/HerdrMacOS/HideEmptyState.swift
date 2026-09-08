import SwiftUI

/// Shared empty and unavailable presentation, using only caller-provided content.
struct HideEmptyState<Heading: View, Detail: View>: View {
    private let heading: Heading
    private let detail: Detail
    private let emphasis: Color?

    init(emphasis: Color? = nil, @ViewBuilder label: () -> Heading, @ViewBuilder description: () -> Detail) {
        heading = label()
        detail = description()
        self.emphasis = emphasis
    }

    var body: some View {
        VStack(spacing: HideTheme.spacingMD) {
            heading
                .labelStyle(HideEmptyStateLabelStyle(emphasis: emphasis))
                .hideFont(size: HideTheme.Typography.headline, weight: .semibold)
                .foregroundStyle(emphasis ?? HideTheme.primary)
                .accessibilityAddTraits(.isHeader)
            detail
                .hideFont(size: HideTheme.Typography.subhead)
                .foregroundStyle(HideTheme.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
        .multilineTextAlignment(.center)
        .padding(HideTheme.spacingLG)
        .frame(maxWidth: .infinity)
        .accessibilityElement(children: .contain)
    }
}

extension HideEmptyState where Heading == Label<Text, Image>, Detail == Text {
    init(_ title: String, systemImage: String, description: Text = Text("")) {
        heading = Label { Text(title) } icon: { Image(systemName: systemImage) }
        detail = description
        emphasis = nil
    }
}

private struct HideEmptyStateLabelStyle: LabelStyle {
    let emphasis: Color?
    func makeBody(configuration: Configuration) -> some View {
        VStack(spacing: HideTheme.spacingMD) {
            configuration.icon
                .hideFont(size: HideTheme.Typography.display)
                .foregroundStyle(emphasis ?? HideTheme.muted)
                .accessibilityHidden(true)
            configuration.title
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}
