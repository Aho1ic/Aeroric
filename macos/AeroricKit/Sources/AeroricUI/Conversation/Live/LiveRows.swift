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

/// Live rows with consecutive tool calls folded into one group (2026-10-27:
/// "已执行 n 个指令", collapsed by default; expand for the list, expand a
/// line for its output).
enum LiveRowGrouping {
    enum Row: Identifiable, Equatable {
        case item(LiveItem)
        case tools(id: String, [LiveTool])

        var id: String {
            switch self {
            case let .item(item): item.id
            case let .tools(id, _): "tools:\(id)"
            }
        }
    }

    static func rows(_ items: [LiveItem]) -> [Row] {
        var rows: [Row] = []
        var run: [LiveTool] = []
        func flush() {
            if run.count == 1 { rows.append(.item(.tool(run[0]))) }
            else if !run.isEmpty { rows.append(.tools(id: run[0].id, run)) }
            run = []
        }
        for item in items {
            if case let .tool(tool) = item { run.append(tool) } else { flush(); rows.append(.item(item)) }
        }
        flush()
        return rows
    }

    /// "已执行 3 个指令" when every call is a command, else "已调用 3 个工具";
    /// while one still runs, "正在执行第 2 / 3 个".
    static func title(_ tools: [LiveTool]) -> String {
        let commands = tools.allSatisfy { $0.kind == .command }
        if let running = tools.firstIndex(where: { $0.status == .running }) {
            return "正在执行第 \(running + 1) / \(tools.count) 个\(commands ? "指令" : "工具")"
        }
        return commands ? "已执行 \(tools.count) 个指令" : "已调用 \(tools.count) 个工具"
    }
}

extension LiveTool {
    var activity: ActivityState {
        switch status {
        case .running: .running
        case .completed: .done
        case .failed: .failed
        case .declined: .idle
        }
    }

    var statusText: String {
        switch status {
        case .running: "运行中"
        case .completed: "完成"
        case .failed: "失败"
        case .declined: "已拒绝"
        }
    }

    var symbol: String {
        switch kind {
        case .command: "terminal"
        case .fileChange: "pencil"
        case .mcp: "puzzlepiece.extension"
        case .webSearch: "globe"
        case .subagent: "person.2"
        case .other: "wrench.and.screwdriver"
        }
    }
}

/// One tool call as a quiet grey line (no border), collapsed by default;
/// expanded it shows the output, streaming in while it runs (Q25).
struct LiveToolLine: View {
    let tool: LiveTool
    @State private var expanded = false
    @State private var showAll = false
    @State private var hovered = false

    static let collapsedLines = 12

    private var lines: [Substring] { tool.output.split(separator: "\n", omittingEmptySubsequences: false) }

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xs) {
            Button { expanded.toggle() } label: {
                HStack(spacing: AeroricSpace.m) {
                    StatusDot(tool.activity, label: tool.statusText)
                    Image(systemName: tool.symbol)
                        .font(.system(size: 11))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .frame(width: 14)
                    Text(tool.title.isEmpty ? "工具" : tool.title)
                        .font(AeroricType.mono(AeroricType.secondary))
                        .foregroundStyle(AeroricColor.textSecondary)
                        .lineLimit(1).truncationMode(.middle)
                    Spacer(minLength: AeroricSpace.s)
                    if tool.status != .completed {
                        Text(tool.statusText)
                            .font(.system(size: AeroricType.caption))
                            .foregroundStyle(tool.status == .failed ? AeroricColor.danger : AeroricColor.textTertiary)
                    }
                    Image(systemName: "chevron.right")
                        .font(.system(size: 9, weight: .semibold))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .rotationEffect(.degrees(expanded ? 90 : 0))
                }
                .padding(.horizontal, AeroricSpace.m)
                .frame(height: 26)
                .background(Capsule(style: .continuous).fill(hovered ? AeroricColor.hover : .clear))
                .contentShape(Capsule(style: .continuous))
            }
            .buttonStyle(.plain)
            .onHover { hovered = $0 }
            if expanded {
                VStack(alignment: .leading, spacing: AeroricSpace.xs) {
                    Text(tool.title)
                        .font(AeroricType.mono(AeroricType.caption))
                        .foregroundStyle(AeroricColor.textSecondary)
                        .textSelection(.enabled)
                    if !tool.output.isEmpty {
                        Text(showAll ? tool.output : lines.suffix(Self.collapsedLines).joined(separator: "\n"))
                            .font(AeroricType.mono(AeroricType.caption))
                            .foregroundStyle(AeroricColor.textTertiary)
                            .textSelection(.enabled)
                            .frame(maxWidth: .infinity, alignment: .leading)
                        if lines.count > Self.collapsedLines {
                            Button(showAll ? "收起" : "显示全部 \(lines.count) 行") { showAll.toggle() }
                                .buttonStyle(.link)
                                .font(.system(size: AeroricType.caption))
                        }
                    } else if tool.status == .running {
                        Text("等待输出…").font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.textTertiary)
                    }
                }
                .padding(.leading, AeroricSpace.xxxl + AeroricSpace.s)
                .padding(.trailing, AeroricSpace.m)
                .padding(.bottom, AeroricSpace.xs)
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("session.live.tool.\(tool.id)")
    }
}

/// Consecutive calls folded into one grey line until expanded.
struct LiveToolGroup: View {
    let id: String
    let tools: [LiveTool]
    @State private var expanded = false
    @State private var hovered = false

    private var activity: ActivityState {
        if tools.contains(where: { $0.status == .running }) { return .running }
        if tools.contains(where: { $0.status == .failed }) { return .failed }
        return .done
    }

    var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.xxs) {
            Button { expanded.toggle() } label: {
                HStack(spacing: AeroricSpace.m) {
                    StatusDot(activity, label: LiveRowGrouping.title(tools))
                    Image(systemName: "terminal")
                        .font(.system(size: 11))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .frame(width: 14)
                    Text(LiveRowGrouping.title(tools))
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.textSecondary)
                    let failed = tools.filter { $0.status == .failed }.count
                    if failed > 0 {
                        Text("\(failed) 个失败").font(.system(size: AeroricType.caption)).foregroundStyle(AeroricColor.danger)
                    }
                    Spacer(minLength: AeroricSpace.s)
                    Image(systemName: "chevron.right")
                        .font(.system(size: 9, weight: .semibold))
                        .foregroundStyle(AeroricColor.textTertiary)
                        .rotationEffect(.degrees(expanded ? 90 : 0))
                }
                .padding(.horizontal, AeroricSpace.m)
                .frame(height: 26)
                .background(Capsule(style: .continuous).fill(hovered ? AeroricColor.hover : .clear))
                .contentShape(Capsule(style: .continuous))
            }
            .buttonStyle(.plain)
            .onHover { hovered = $0 }
            .accessibilityIdentifier("session.live.tool-group.\(id)")
            if expanded {
                ForEach(tools, id: \.id) { tool in
                    LiveToolLine(tool: tool).padding(.leading, AeroricSpace.l)
                }
            }
        }
    }
}

struct LiveNoticeRow: View {
    let message: String
    let recoverable: Bool

    /// Codex's "Model metadata for `x` not found. Defaulting to fallback
    /// metadata…" fires for every model it does not ship (any third-party
    /// one). Codex's own TUI keeps it as a quiet footer note; here it becomes
    /// one grey line in plain words (2026-10-27).
    static func fallbackModel(in message: String) -> String? {
        let prefix = "Model metadata for `"
        guard message.hasPrefix(prefix), message.contains("Defaulting to fallback metadata"),
              let end = message.dropFirst(prefix.count).firstIndex(of: "`") else { return nil }
        return String(message[message.index(message.startIndex, offsetBy: prefix.count)..<end])
    }

    /// D3 (2026-10-30): an error that means the local login cannot serve
    /// right now — spent quota or rate limit, or a login that needs signing
    /// in again; the page then offers 「换到第三方模型」.
    static func loginTrouble(in message: String) -> String? {
        let text = message.lowercased()
        let spent = ["usage limit", "rate limit", "quota", "429", "额度", "限流"]
        let auth = ["401", "unauthorized", "本机登录已失效", "sign in again", "log in again"]
        if spent.contains(where: text.contains) { return "本机登录额度已用完或被限流" }
        if auth.contains(where: text.contains) { return "本机登录需要重新登录" }
        return nil
    }

    var body: some View {
        if Self.fallbackModel(in: message) != nil {
            // 2026-10-29: not shown. It fires for every third-party model and
            // says nothing the user can act on; Codex runs on its defaults.
            EmptyView()
        } else {
            Label(message, systemImage: recoverable ? "info.circle" : "exclamationmark.triangle")
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(recoverable ? AeroricColor.textSecondary : AeroricColor.danger)
                .textSelection(.enabled)
                .accessibilityIdentifier("session.live.notice")
        }
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

/// A05: the project's checks after the last turn; failures expand to the
/// failing cases and the output's tail, and can be handed back to the agent.
struct LiveAutoCheckRow: View {
    let check: LiveAutoCheck
    let stale: Bool
    let onSendFailures: (String) -> Void
    @State private var expanded = false

    private var tint: Color {
        if stale { return AeroricColor.textTertiary }
        switch check.state {
        case "passed": return AeroricColor.success
        case "failed": return AeroricColor.danger
        default: return AeroricColor.textSecondary
        }
    }

    private var icon: String {
        switch check.state {
        case "running": "hourglass"
        case "passed": "checkmark.seal"
        case "failed": "xmark.seal"
        default: "minus.circle"
        }
    }

    var body: some View {
        DisclosureGroup(isExpanded: $expanded) {
            VStack(alignment: .leading, spacing: AeroricSpace.s) {
                ForEach(Array(check.items.enumerated()), id: \.offset) { _, item in
                    VStack(alignment: .leading, spacing: 2) {
                        Text("\(item.label)：\(item.summary)")
                            .font(.system(size: AeroricType.caption, weight: .medium))
                            .foregroundStyle(item.state == "passed" ? AeroricColor.textSecondary : AeroricColor.danger)
                        ForEach(Array(item.failures.enumerated()), id: \.offset) { _, failure in
                            Text(failure)
                                .font(.system(size: AeroricType.caption, design: .monospaced))
                                .lineLimit(3)
                                .textSelection(.enabled)
                        }
                        if item.state != "passed", item.failures.isEmpty, !item.outputTail.isEmpty {
                            ScrollView {
                                Text(item.outputTail)
                                    .font(.system(size: AeroricType.caption, design: .monospaced))
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .textSelection(.enabled)
                            }
                            .frame(maxHeight: 160)
                        }
                    }
                }
                if !stale, let report = check.failureReport {
                    Button("把失败发给 Agent") { onSendFailures(report) }
                        .controlSize(.small)
                        .accessibilityIdentifier("session.live.auto-check.send")
                }
            }
            .padding(.top, AeroricSpace.xs)
        } label: {
            Label(stale ? "\(check.title)（之后又有改动，结果已过时）" : check.title, systemImage: icon)
                .font(.system(size: AeroricType.secondary))
                .foregroundStyle(tint)
        }
        .disabled(check.items.isEmpty)
        .padding(.horizontal, AeroricSpace.l)
        .accessibilityIdentifier("session.live.auto-check")
    }
}

/// A05: per-project switch for the checks, shown beside the turn's changes.
struct LiveAutoCheckMenu: View {
    let selection: LiveAutoCheckSelection
    let onChange: (LiveAutoCheckSelection) -> Void

    var body: some View {
        Menu {
            Toggle("改动后运行测试", isOn: Binding(get: { selection.tests }, set: { value in
                onChange(LiveAutoCheckSelection(tests: value, diagnostics: selection.diagnostics))
            }))
            Toggle("改动后运行诊断（cargo check / tsc / ruff）", isOn: Binding(get: { selection.diagnostics }, set: { value in
                onChange(LiveAutoCheckSelection(tests: selection.tests, diagnostics: value))
            }))
            Text("对这个项目生效；不会自动让 Agent 修复")
        } label: {
            Label(selection.isOn ? "自动检查：开" : "自动检查：关", systemImage: "checklist")
                .font(.system(size: AeroricType.caption))
        }
        .menuStyle(.borderlessButton)
        .fixedSize()
        .accessibilityIdentifier("session.live.auto-check.menu")
    }
}
