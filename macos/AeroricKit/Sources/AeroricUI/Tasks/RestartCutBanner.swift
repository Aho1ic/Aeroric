import SwiftUI

/// 2026-11-14 宿主重启后续跑: above a session an Aeroric restart cut — what
/// happened, and 继续.
public struct RestartCutBanner: View {
    let onContinue: () async -> Void
    @State private var busy = false

    public init(onContinue: @escaping () async -> Void) {
        self.onContinue = onContinue
    }

    public var body: some View {
        HStack(spacing: AeroricSpace.m) {
            Image(systemName: "arrow.clockwise.circle")
                .foregroundStyle(AeroricColor.warning)
            Text(RestartCutText.banner)
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textSecondary)
                .lineLimit(2)
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
    public static let help = "用原会话接着做：先看工作区现在的状态，再从中断处继续，做完的不重做。"
}
