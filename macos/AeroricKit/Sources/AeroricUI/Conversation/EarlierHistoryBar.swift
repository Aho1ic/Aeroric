import AeroricProtocol
import SwiftUI

/// A6 (2026-11-17): the top of a transcript. Says when older messages were
/// left unread and offers them, and says when lines of the history file could
/// not be read (they used to vanish without a word).
struct EarlierHistoryBar: View {
    let hasEarlier: Bool
    let loading: Bool
    let unreadable: SessionUnreadable?
    let onLoad: () -> Void

    var body: some View {
        VStack(spacing: AeroricSpace.s) {
            if hasEarlier {
                Button(loading ? "正在载入…" : "加载更早的消息", action: onLoad)
                    .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                    .disabled(loading)
                    .help("这个会话很长，先只显示了最近的部分")
                    .accessibilityIdentifier("session.history.earlier")
            }
            if let unreadable {
                Label(unreadable.text, systemImage: "exclamationmark.triangle")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.warning)
                    .help(EarlierHistoryText.help(unreadable))
                    .accessibilityIdentifier("session.history.unreadable")
            }
        }
        .frame(maxWidth: .infinity)
    }
}

enum EarlierHistoryText {
    /// What the warning means, on hover.
    static func help(_ unreadable: SessionUnreadable) -> String {
        if unreadable.reason != nil {
            return "Aeroric 读不懂这个会话文件，所以这里没有显示它的消息；文件本身没有改动"
        }
        return "这些行不是完整的 JSON，没有显示出来；通常是 Agent 写文件时被打断。文件本身没有改动"
    }
}
