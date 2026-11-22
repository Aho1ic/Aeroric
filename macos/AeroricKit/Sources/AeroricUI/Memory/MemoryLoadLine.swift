import AeroricProtocol
import SwiftUI

/// E8 (2026-11-22): how much of memory a session started now would carry,
/// and what it would not; the left-out items open below the line.
struct MemoryLoadLine: View {
    let load: LiveMemoryLoad
    @State private var expanded = false

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xs) {
            HStack(spacing: AeroricSpace.s) {
                Image(systemName: "gauge.with.dots.needle.33percent")
                Text(MemoryLoadText.line(load))
                    .monospacedDigit()
                    .fixedSize(horizontal: false, vertical: true)
                if !load.leftOut.isEmpty {
                    Button(expanded ? "收起" : "看没带上的") { expanded.toggle() }
                        .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
                        .accessibilityIdentifier("memory.load.toggle")
                }
            }
            .font(.system(size: AeroricType.caption))
            .foregroundStyle(load.leftOut.isEmpty ? AeroricColor.textSecondary : AeroricColor.warning)
            if expanded {
                ForEach(load.leftOut) { item in
                    Text(MemoryLoadText.leftOut(item))
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
        .padding(.horizontal, AeroricSpace.s)
        .help("新开的会话在开场时带入的记忆：用户画像和本项目的事实，有字数上限；没带上的 Agent 仍能用 memory_recall 找到")
        .accessibilityIdentifier("memory.load")
    }
}

/// E8: the load line in words.
public enum MemoryLoadText {
    /// `新会话会带入 840 / 1200 字（8 条），另有 12 条没带上`.
    public static func line(_ load: LiveMemoryLoad) -> String {
        guard load.carried > 0 else {
            return load.leftOut.isEmpty ? "新会话不会带入记忆" : "新会话不会带入记忆，另有 \(load.leftOut.count) 条没带上"
        }
        var text = "新会话会带入 \(load.usedChars) / \(load.budgetChars) 字（\(load.carried) 条）"
        if !load.leftOut.isEmpty { text += "，另有 \(load.leftOut.count) 条没带上" }
        return text
    }

    /// `· 第 5 条项目约定（超出字数上限）`.
    public static func leftOut(_ item: LiveMemoryLoad.LeftOut) -> String {
        let why = item.why == "demoted" ? "已移出开场，只能检索到" : "超出字数上限"
        return "· \(item.text)（\(why)）"
    }
}
