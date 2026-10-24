import SwiftUI

/// Redesign 01 conversation column (Agents-Anywhere timeline width, codeg
/// structured messages). Renders host-supplied history only; it never
/// synthesises or re-renders the live TUI, which stays in the terminal.
public struct SessionView: View {
    let paginator: SessionPaginator
    var onLoadMore: (() -> Void)?
    var onExport: (() -> Void)?
    var usage: SessionUsage?

    /// Agents-Anywhere `max-w-[48rem]`.
    public static let columnWidth: CGFloat = 768

    public init(
        paginator: SessionPaginator,
        onLoadMore: (() -> Void)? = nil,
        onExport: (() -> Void)? = nil,
        usage: SessionUsage? = nil
    ) {
        self.paginator = paginator
        self.onLoadMore = onLoadMore
        self.onExport = onExport
        self.usage = usage
    }

    public var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 20) {
                    if paginator.canLoadMore {
                        Button("加载更早消息") { onLoadMore?() }
                            .buttonStyle(.borderless)
                            .font(AeroricFont.ui(AeroricFont.meta))
                            .frame(maxWidth: .infinity)
                            .accessibilityIdentifier("session.load-more")
                    }
                    if paginator.messages.isEmpty {
                        Text(paginator.isLoadingPage ? "正在读取会话历史…" : "宿主尚未提供此会话的结构化历史。")
                            .font(AeroricFont.ui(AeroricFont.meta))
                            .foregroundStyle(SessionPalette.muted)
                            .frame(maxWidth: .infinity)
                            .padding(.top, 40)
                    }
                    ForEach(Array(paginator.messages.enumerated()), id: \.offset) { index, message in
                        MessageRow(message: message)
                            .id(index)
                            .accessibilityIdentifier("session.message-row.\(index)")
                    }
                }
                .padding(.horizontal, 24)
                .padding(.top, 20)
                .padding(.bottom, 24)
                .frame(maxWidth: Self.columnWidth)
                .frame(maxWidth: .infinity)
            }
            .onChange(of: paginator.messages.count) { _, count in
                guard count > 0 else { return }
                proxy.scrollTo(count - 1, anchor: .bottom)
            }
            .accessibilityIdentifier("session.messages")
        }
        .overlay(alignment: .topTrailing) {
            if let onExport {
                NativeIconButton(systemImage: "square.and.arrow.up", label: "导出 Markdown",
                                 identifier: "session.export", action: onExport)
                    .foregroundStyle(SessionPalette.secondary)
                    .padding(8)
            }
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            if let usage { UsagePanel(usage: usage) }
        }
        .accessibilityIdentifier("session.view")
    }
}

/// P5-04 Action 1：用量面板（SessionMetrics 字段；无数据不显示假 0）。
public struct UsagePanel: View {
    let usage: SessionUsage

    public init(usage: SessionUsage) {
        self.usage = usage
    }

    public var body: some View {
        HStack(spacing: 14) {
            if let duration = usage.durationSecs {
                Text("耗时 \(String(format: "%.1fs", duration))")
            }
            if let total = usage.totalTokens {
                Text("Token \(total)")
            }
            if let context = usage.contextTokens {
                Text("上下文 \(context)")
            }
            Spacer()
        }
        .font(AeroricFont.ui(AeroricFont.meta))
        .monospacedDigit()
        .foregroundStyle(SessionPalette.muted)
        .padding(.horizontal, 24)
        .padding(.vertical, 4)
        .accessibilityIdentifier("session.usage")
    }
}

/// Continuation input under the conversation (Q24-a). Only an explicit
/// press of send writes the text into the session; `writesToTerminal`
/// labels the case where the TUI is waiting for input and the text will
/// land in whatever prompt it is showing.
public struct SessionContinuationComposer: View {
    @Binding var draft: String
    let canSend: Bool
    let writesToTerminal: Bool
    let readOnlyReason: String?
    let onSend: (String) -> Void
    @FocusState private var focused: Bool

    public init(draft: Binding<String>, canSend: Bool, writesToTerminal: Bool,
                readOnlyReason: String? = nil, onSend: @escaping (String) -> Void) {
        _draft = draft
        self.canSend = canSend
        self.writesToTerminal = writesToTerminal
        self.readOnlyReason = readOnlyReason
        self.onSend = onSend
    }

    private var trimmed: String { draft.trimmingCharacters(in: .whitespacesAndNewlines) }

    public var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(alignment: .bottom, spacing: 8) {
                TextField(canSend ? "继续对话…" : (readOnlyReason ?? "当前只读"),
                          text: $draft, axis: .vertical)
                    .textFieldStyle(.plain)
                    .font(AeroricFont.ui(AeroricFont.body))
                    .lineLimit(1...6)
                    .focused($focused)
                    .disabled(!canSend)
                    .onKeyPress(.return, phases: .down) { press in
                        guard press.modifiers.contains(.command) else { return .ignored }
                        send()
                        return .handled
                    }
                    .accessibilityLabel("续聊输入")
                    .accessibilityIdentifier("session.continue.input")
                Button(action: send) {
                    Image(systemName: "arrow.up")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(Color.white)
                        .frame(width: 28, height: 28)
                        .background(canSend && !trimmed.isEmpty ? SessionPalette.accent : SessionPalette.muted.opacity(0.5),
                                    in: Circle())
                }
                .buttonStyle(.plain)
                .disabled(!canSend || trimmed.isEmpty)
                .help("发送（⌘↩）")
                .accessibilityLabel("发送")
                .accessibilityIdentifier("session.continue.send")
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 10)
            .background(SessionPalette.input, in: RoundedRectangle(cornerRadius: ShellRadius.composer, style: .continuous))
            .overlay {
                RoundedRectangle(cornerRadius: ShellRadius.composer, style: .continuous)
                    .strokeBorder(focused ? SessionPalette.accent.opacity(0.6) : SessionPalette.edge, lineWidth: 1)
                    .allowsHitTesting(false)
            }
            if writesToTerminal && canSend {
                Label("Agent 正在等待输入：发送的文字将原样输入到终端", systemImage: "exclamationmark.circle")
                    .font(AeroricFont.ui(AeroricFont.meta))
                    .foregroundStyle(SessionPalette.amber)
                    .accessibilityIdentifier("session.continue.terminal-hint")
            }
        }
        .padding(.horizontal, 24)
        .padding(.vertical, 10)
        .frame(maxWidth: SessionView.columnWidth + 48)
        .frame(maxWidth: .infinity)
    }

    private func send() {
        guard canSend, !trimmed.isEmpty else { return }
        onSend(draft)
        draft = ""
    }
}

/// P5-04 Action 5：会话操作（export/search/resume/adopt/handoff 的纯
/// 决策面；跨协议动作显示新会话提示）。
public enum SessionActions {
    /// 跨协议 resume/adopt 是否需要「新会话」提示（旧历史仍能打开）。
    /// 判定：目标 agent family 与会话 transcript 格式不一致 → 需要提示。
    public static func requiresNewSessionHint(
        currentFamily: String,
        targetAgent: String
    ) -> Bool {
        let targetFamily: String
        switch targetAgent {
        case "dsh":
            targetFamily = "dsh"
        case "omp":
            targetFamily = "omp"
        case "codex":
            targetFamily = "codex"
        default:
            targetFamily = "claude"
        }
        return currentFamily != targetFamily
    }

    /// 搜索结果过滤（session.search 的客户端镜像；query 空白＝不过滤）。
    public static func matchesQuery(_ message: SessionMessage, query: String) -> Bool {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return true }
        return message.content.contains { block in
            switch block {
            case let .text(text):
                return text.localizedCaseInsensitiveContains(trimmed)
            case let .toolUse(_, name, input):
                return name.localizedCaseInsensitiveContains(trimmed)
                    || input.localizedCaseInsensitiveContains(trimmed)
            case let .toolResult(_, output):
                return output.localizedCaseInsensitiveContains(trimmed)
            case let .thinking(thinking):
                return thinking.localizedCaseInsensitiveContains(trimmed)
            case let .attachment(name, _, _):
                return name.localizedCaseInsensitiveContains(trimmed)
            }
        }
    }
}
