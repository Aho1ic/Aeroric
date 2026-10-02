import SwiftUI

/// P5-04 Action 1：单条消息行。文本块走 markdown 显示（Action 1：不执行
/// HTML/脚本——这里用原生 Text 显示明文，链接 scheme 校验在 ContentBlock）；
/// tool/thinking/attachment 块委托 ContentBlock。
public struct MessageRow: View {
    let message: SessionMessage

    public init(message: SessionMessage) {
        self.message = message
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(message.role == "user" ? "用户" : "Assistant")
                .font(.caption)
                .foregroundStyle(Color.dsTextMuted)
            ForEach(Array(message.content.enumerated()), id: \.offset) { _, block in
                ContentBlock(block: block)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.vertical, 4)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("session.message.\(message.messageId ?? message.role)")
    }
}

/// P5-04 Action 1：内容块。Markdown 渲染显示内容**不得执行 HTML/脚本**
/// （原生 Text，永不走 HTML 渲染）；点击 URL 校验 scheme（http/https/mailto
/// 之外一律不打开）。
public struct ContentBlock: View {
    let block: SessionContent
    @State private var thinkingExpanded = false
    @State private var toolExpanded = false

    public init(block: SessionContent) {
        self.block = block
    }

    public var body: some View {
        switch block {
        case let .text(text):
            // 原生 Text＝纯明文显示：markdown 语法的字符原样呈现，HTML/
            // 脚本不会被执行。链接由系统 DataDetector 处理且仅 http/https。
            Text(text)
                .textSelection(.enabled)
                .accessibilityIdentifier("session.content.text")
        case let .thinking(thinking):
            VStack(alignment: .leading, spacing: 4) {
                Button(thinkingExpanded ? "收起思考" : "展开思考") {
                    thinkingExpanded.toggle()
                }
                .font(.caption)
                .accessibilityIdentifier("session.content.thinking-toggle")
                if thinkingExpanded {
                    Text(thinking)
                        .font(.caption)
                        .foregroundStyle(Color.dsTextSecondary)
                        .textSelection(.enabled)
                }
            }
            .padding(6)
            .background(Color.dsSurface.opacity(0.5))
        case let .toolUse(id, name, input):
            VStack(alignment: .leading, spacing: 4) {
                Button {
                    toolExpanded.toggle()
                } label: {
                    HStack {
                        Image(systemName: "wrench.fill")
                            .font(.caption2)
                        Text(name)
                            .font(.caption)
                        Spacer()
                    }
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("session.tool.\(id)")
                if toolExpanded {
                    Text(input)
                        .font(.caption2.monospaced())
                        .foregroundStyle(Color.dsTextSecondary)
                        .textSelection(.enabled)
                }
            }
            .padding(6)
            .background(Color.dsSurface.opacity(0.5))
        case let .toolResult(id, output):
            ToolCallRow(id: id, output: output)
        case let .attachment(name, mediaType, _):
            HStack(spacing: 4) {
                Image(systemName: "paperclip")
                    .font(.caption2)
                Text("\(name)（\(mediaType)）")
                    .font(.caption)
            }
            .foregroundStyle(Color.dsTextSecondary)
            .accessibilityIdentifier("session.content.attachment")
        }
    }

    /// URL scheme 校验（Action 1：点击 URL 校验 scheme）。http/https/mailto
    /// 之外的 scheme（javascript:/file:/自定义）一律不打开。
    public static func isAllowedLinkScheme(_ url: URL) -> Bool {
        switch url.scheme?.lowercased() {
        case "http", "https", "mailto":
            return true
        default:
            return false
        }
    }
}

/// P5-04 Action 1：tool 结果行（稳定 ID 去重后渲染；重复 tool_end 不重复
/// row 由 SessionHistoryLogic.deduplicateToolBlocks 保证）。
public struct ToolCallRow: View {
    let id: String
    let output: String

    public init(id: String, output: String) {
        self.id = id
        self.output = output
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text("结果 · \(id)")
                .font(.caption2)
                .foregroundStyle(Color.dsTextMuted)
            Text(output)
                .font(.caption2.monospaced())
                .foregroundStyle(Color.dsTextSecondary)
                .textSelection(.enabled)
                .lineLimit(12)
        }
        .padding(6)
        .background(Color.dsSurface.opacity(0.5))
        .accessibilityIdentifier("session.tool-result.\(id)")
    }
}
