import SwiftUI
import AeroricProtocol

/// Thinking, folded by default as "思考 · n 字" (Q13); Codex shows its summary.
struct LiveThoughtRow: View {
    let id: String
    let text: String
    let summary: Bool
    @State private var expanded = false

    var body: some View {
        FlatDisclosureRow(symbol: "brain", title: "思考 · \(text.count) 字",
                          summary: summary ? "推理摘要" : "", expanded: $expanded,
                          identifier: "session.live.thought.\(id)") {
            Text(text)
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textSecondary)
                .textSelection(.enabled)
        }
    }
}

/// A tool call with its output streaming in (Q25: no terminal below).
struct LiveToolCard: View {
    let tool: LiveTool
    @State private var expanded = false

    static let collapsedLines = 12

    private var activity: ActivityState {
        switch tool.status {
        case .running: .running
        case .completed: .done
        case .failed: .failed
        case .declined: .idle
        }
    }

    private var statusText: String {
        switch tool.status {
        case .running: "运行中"
        case .completed: "完成"
        case .failed: "失败"
        case .declined: "已拒绝"
        }
    }

    private var symbol: String {
        switch tool.kind {
        case .command: "terminal"
        case .fileChange: "pencil"
        case .mcp: "puzzlepiece.extension"
        case .webSearch: "globe"
        case .subagent: "person.2"
        case .other: "wrench.and.screwdriver"
        }
    }

    private var lines: [Substring] { tool.output.split(separator: "\n", omittingEmptySubsequences: false) }

    var body: some View {
        let shape = AeroricRadius.shape(AeroricRadius.m)
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            HStack(spacing: AeroricSpace.s) {
                Image(systemName: symbol).foregroundStyle(AeroricColor.textSecondary)
                Text(tool.title.isEmpty ? "工具" : tool.title)
                    .font(AeroricType.mono(AeroricType.code))
                    .lineLimit(1).truncationMode(.middle)
                    .textSelection(.enabled)
                Spacer(minLength: AeroricSpace.s)
                StatusDot(activity, label: statusText)
                Text(statusText)
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.textTertiary)
            }
            if !tool.output.isEmpty {
                let shown = expanded ? tool.output
                    : lines.suffix(Self.collapsedLines).joined(separator: "\n")
                Text(shown)
                    .font(AeroricType.mono(AeroricType.code))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                if lines.count > Self.collapsedLines {
                    Button(expanded ? "收起" : "显示全部 \(lines.count) 行") { expanded.toggle() }
                        .buttonStyle(.link)
                        .font(.system(size: AeroricType.caption))
                }
            }
        }
        .padding(AeroricSpace.m)
        .background(AeroricColor.layer1, in: shape)
        .overlay { shape.strokeBorder(AeroricColor.separator2, lineWidth: 1) }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("session.live.tool.\(tool.id)")
    }
}

struct LiveNoticeRow: View {
    let message: String
    let recoverable: Bool

    var body: some View {
        Label(message, systemImage: recoverable ? "info.circle" : "exclamationmark.triangle")
            .font(.system(size: AeroricType.secondary))
            .foregroundStyle(recoverable ? AeroricColor.textSecondary : AeroricColor.danger)
            .textSelection(.enabled)
            .accessibilityIdentifier("session.live.notice")
    }
}

/// A05: "上一轮改动 +12 −3 · 2 个文件", expandable to the per-file counts.
struct LiveTurnChangesRow: View {
    let changes: LiveTurnChanges
    @State private var expanded = false

    var body: some View {
        DisclosureGroup(isExpanded: $expanded) {
            VStack(alignment: .leading, spacing: 2) {
                ForEach(changes.files, id: \.path) { file in
                    HStack(spacing: AeroricSpace.m) {
                        Text(file.path)
                            .font(.system(size: AeroricType.caption, design: .monospaced))
                            .lineLimit(1)
                            .truncationMode(.middle)
                            .textSelection(.enabled)
                        Spacer(minLength: AeroricSpace.m)
                        if let added = file.additions, let removed = file.deletions {
                            Text("+\(added)").foregroundStyle(AeroricColor.success)
                            Text("−\(removed)").foregroundStyle(AeroricColor.danger)
                        } else {
                            Text("二进制").foregroundStyle(AeroricColor.textTertiary)
                        }
                    }
                    .font(.system(size: AeroricType.caption))
                }
                if changes.truncated {
                    Text("文件过多，只列出前 \(changes.files.count) 个；合计行数包含全部文件。")
                        .font(.system(size: AeroricType.caption))
                        .foregroundStyle(AeroricColor.textTertiary)
                }
            }
            .padding(.top, AeroricSpace.xs)
        } label: {
            Label("上一轮改动 \(changes.summary)", systemImage: "plusminus")
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(AeroricColor.textSecondary)
        }
        .padding(.horizontal, AeroricSpace.l)
        .accessibilityIdentifier("session.live.turn-changes")
    }
}
