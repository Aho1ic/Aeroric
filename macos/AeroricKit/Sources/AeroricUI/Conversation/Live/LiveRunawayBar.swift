import SwiftUI

/// D 批: the runaway reminder above the composer — the most telling one,
/// with how many more; they unfold on a click.
struct LiveRunawayBar: View {
    let alerts: [LiveRunawayAlert]
    let summarize: () -> Void
    let dismiss: (String) -> Void
    @State private var unfolded = false

    var body: some View {
        if let first = alerts.first {
            VStack(alignment: .leading, spacing: AeroricSpace.xs) {
                HStack(spacing: AeroricSpace.s) {
                    Image(systemName: "gauge.with.dots.needle.67percent")
                    Text(first.text)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .help(first.text)
                    Spacer(minLength: 0)
                    if alerts.count > 1 {
                        Button { unfolded.toggle() } label: {
                            Text("还有 \(alerts.count - 1) 条")
                                .padding(.horizontal, AeroricSpace.s)
                                .overlay(Capsule(style: .continuous).strokeBorder(AeroricColor.warning.opacity(0.5),
                                                                                  lineWidth: 0.5))
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("session.live.runaway.more")
                    }
                    Button("请它先总结", action: summarize)
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .fixedSize()
                        .help("让 Agent 先总结进度、还剩什么、卡在哪里，再决定要不要继续")
                        .accessibilityIdentifier("session.live.runaway.summarize")
                    Button { dismiss(first.id) } label: { Image(systemName: "xmark") }
                        .buttonStyle(.plain)
                        .help("知道了")
                        .accessibilityLabel("知道了")
                        .accessibilityIdentifier("session.live.runaway.dismiss")
                }
                .padding(.horizontal, AeroricSpace.l)
                .frame(height: 30)
                .background(Capsule(style: .continuous).fill(AeroricColor.warning.opacity(0.12)))
                if unfolded {
                    VStack(alignment: .leading, spacing: AeroricSpace.xs) {
                        ForEach(alerts) { alert in
                            Label(alert.text, systemImage: "circle.fill")
                                .labelStyle(BulletLabel())
                        }
                    }
                    .padding(AeroricSpace.m)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(AeroricColor.layer1, in: AeroricRadius.shape(AeroricRadius.m))
                    .accessibilityIdentifier("session.live.runaway.list")
                }
            }
            .font(.system(size: AeroricType.caption))
            .foregroundStyle(AeroricColor.warning)
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("session.live.runaway")
        }
    }

    private struct BulletLabel: LabelStyle {
        func makeBody(configuration: Configuration) -> some View {
            HStack(alignment: .firstTextBaseline, spacing: AeroricSpace.s) {
                configuration.icon.font(.system(size: 4))
                configuration.title
            }
        }
    }
}
