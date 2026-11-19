import SwiftUI

/// 2026-11-14 宿主重启后续跑: above a session an Aeroric restart cut — what
/// happened, and 继续.
public struct RestartCutBanner: View {
    /// E5: what the cut turn had already done outward.
    let outward: [String]
    let onContinue: () async -> Void
    @State private var busy = false

    public init(outward: [String] = [], onContinue: @escaping () async -> Void) {
        self.outward = outward
        self.onContinue = onContinue
    }

    public var body: some View {
        HStack(spacing: AeroricSpace.m) {
            Image(systemName: "arrow.clockwise.circle")
                .foregroundStyle(AeroricColor.warning)
            Text(RestartCutText.banner(outward: outward))
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textSecondary)
                .lineLimit(3)
                .help(RestartCutText.banner(outward: outward))
            Spacer(minLength: AeroricSpace.m)
            Button(busy ? "继续中…" : "继续") {
                busy = true
                Task {
                    await onContinue()
                    busy = false
                }
            }
            .buttonStyle(AeroricCapsuleButtonStyle(.tinted, compact: true))
            .fixedSize()
            .disabled(busy)
            .help(RestartCutText.help)
            .accessibilityIdentifier("session.restart.continue")
        }
        .padding(.horizontal, AeroricSpace.l)
        .padding(.vertical, AeroricSpace.s)
        .background(Capsule(style: .continuous).fill(AeroricColor.warning.opacity(0.1)))
        .padding(.horizontal, AeroricSpace.l)
        .padding(.top, AeroricSpace.s)
        .accessibilityIdentifier("session.restart.banner")
    }
}

/// The words of 宿主重启后续跑.
public enum RestartCutText {
    public static let banner = "Aeroric 重启时这个会话还在跑，上一轮被打断了。"
    public static let attention = "Aeroric 重启打断"

    /// E5: the banner, with what the cut turn already did outward.
    public static func banner(outward: [String]) -> String {
        guard !outward.isEmpty else { return banner }
        return banner + "被打断前它已经执行过 \(outward.joined(separator: "、"))，所以没有自动继续；继续前先核对，避免做两遍。"
    }

    /// E5: the attention row's detail.
    public static func attention(outward: [String]) -> String {
        outward.isEmpty ? attention : "\(attention) · 已执行过 \(outward.joined(separator: "、"))"
    }
    public static let help = "用原会话接着做：先看工作区现在的状态，再从中断处继续，做完的不重做。"
}
