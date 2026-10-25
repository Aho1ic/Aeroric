import SwiftUI

/// Under the composer (Q28): context share and token counts; over 80% turns
/// amber and suggests /compact.
struct UsageBar: View {
    let usage: UsageSummary

    var body: some View {
        if !usage.isEmpty {
            HStack(spacing: AeroricSpace.s) {
                if let fraction = usage.fraction {
                    ProgressView(value: fraction)
                        .progressViewStyle(.linear)
                        .tint(usage.isHigh ? AeroricColor.warning : AeroricColor.textTertiary)
                        .frame(width: 64)
                }
                Text(usage.text)
                    .monospacedDigit()
                if usage.isHigh {
                    Text("上下文快满了，可用 /compact 压缩").foregroundStyle(AeroricColor.warning)
                }
                Spacer(minLength: 0)
            }
            .font(.system(size: AeroricType.caption))
            .foregroundStyle(usage.isHigh ? AeroricColor.warning : AeroricColor.textTertiary)
            .padding(.horizontal, AeroricSpace.m)
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("session.live.usage")
        }
    }
}
