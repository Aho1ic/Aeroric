import SwiftUI
import AeroricProtocol

/// Session page for a structured (native) Agent session (ADR-043, N4): the
/// whole page is the conversation — history, the live turn on top of it,
/// inline approvals, a composer that can change model / permission / effort
/// for the next turn, and the usage line. No terminal below (Q25).
public struct LiveSessionView: View {
    @State private var store: LiveSessionStore
    let facts: LiveSessionFacts
    /// Scroll to this approval card when it changes (a clicked notification).
    let focusApproval: String?
    @State private var draft = ""
    @State private var infoCollapsed: Bool
    @AppStorage(SessionInfoLayout.widthKey) private var infoWidth = Double(SessionInfoLayout.defaultWidth)
    @State private var dragStartWidth: Double?
    @State private var infoPopover = false
    @State private var confirmEnd = false
    @State private var confirmFullAccess = false
    @State private var modelPickerRequest = 0
    @State private var permissionPickerRequest = 0
    @State private var atBottom = true
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    static let infoCollapsedKey = "aeroric.session.infoCollapsed"
    private static let bottomID = "live.bottom"

    public init(store: LiveSessionStore, facts: LiveSessionFacts, focusApproval: String? = nil) {
        _store = State(initialValue: store)
        self.facts = facts
        self.focusApproval = focusApproval
        _infoCollapsed = State(initialValue: UserDefaults.standard.bool(forKey: Self.infoCollapsedKey))
    }

    private var historyItems: [TranscriptItem] {
        TranscriptBuilder.items(from: store.history, taskRunning: false)
    }

    private var liveItems: [LiveItem] { store.model?.items ?? [] }
    private var codex: Bool { store.family == "codex" }

    public var body: some View {
        GeometryReader { outer in
            let showsColumn = SessionInfoLayout.showsColumn(collapsed: infoCollapsed, centreWidth: outer.size.width)
            HStack(spacing: 0) {
                conversation
                if showsColumn {
                    infoResizeHandle
                    VStack(spacing: 0) {
                        HStack { Spacer(); moreMenu }
                            .padding(.horizontal, AeroricSpace.m)
                            .padding(.top, AeroricSpace.m)
                        SessionInfoCard(state: store.state, model: store.model, facts: facts,
                                        collapsed: $infoCollapsed, asColumn: true)
                    }
                    .frame(width: SessionInfoLayout.clampWidth(infoWidth))
                    .frame(maxHeight: .infinity)
                    .background(AeroricColor.layer1)
                }
            }
            .overlay(alignment: .topTrailing) {
                if !showsColumn { foldedInfo }
            }
        }
        .onChange(of: infoCollapsed) { _, value in UserDefaults.standard.set(value, forKey: Self.infoCollapsedKey) }
        .task(id: store.taskID) { await store.run() }
        .modifier(SessionDialogs(store: store, confirmEnd: $confirmEnd, confirmFullAccess: $confirmFullAccess,
                                 configure: { configure(permission: $0) }))
        .accessibilityIdentifier("session.live.view")
    }

    /// Info folded into a button (by the user, or because the window is narrow).
    private var foldedInfo: some View {
        HStack(alignment: .top, spacing: AeroricSpace.s) {
            Button {
                if infoCollapsed { infoCollapsed = false } else { infoPopover = true }
            } label: {
                Label("会话信息", systemImage: "info.circle").font(.system(size: AeroricType.secondary, weight: .medium))
            }
            .buttonStyle(AeroricCapsuleButtonStyle(.ghost, compact: true))
            .popover(isPresented: $infoPopover, arrowEdge: .bottom) {
                SessionInfoCard(state: store.state, model: store.model, facts: facts, collapsed: .constant(false))
            }
            .accessibilityIdentifier("session.info.open")
            moreMenu
        }
        .padding(AeroricSpace.m)
    }

    /// Drag the column's left edge; 260–340 points, remembered.
    private var infoResizeHandle: some View {
        Rectangle()
            .fill(AeroricColor.separator1)
            .frame(width: 1)
            .padding(.horizontal, 3)
            .contentShape(Rectangle())
            .onHover { inside in if inside { NSCursor.resizeLeftRight.push() } else { NSCursor.pop() } }
            .gesture(DragGesture(minimumDistance: 1)
                .onChanged { value in
                    let start = dragStartWidth ?? infoWidth
                    dragStartWidth = start
                    infoWidth = Double(SessionInfoLayout.clampWidth(CGFloat(start) - value.translation.width))
                }
                .onEnded { _ in dragStartWidth = nil })
            .accessibilityLabel("调整会话信息宽度")
    }

    private var conversation: some View {
        GeometryReader { proxy in
            let column = TranscriptGeometry.columnWidth(for: proxy.size.width)
            ScrollViewReader { scroller in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: AeroricSpace.l) {
                        if historyItems.isEmpty && liveItems.isEmpty && store.pending.isEmpty {
                            AeroricEmptyState(systemImage: store.loading ? "hourglass" : "text.bubble",
                                              title: store.loading ? "正在打开会话…" : "发一条消息开始",
                                              message: nil)
                                .padding(.top, 60)
                        }
                        ForEach(Array(historyItems.enumerated()), id: \.element.id) { offset, item in
                            historyRow(item, offset: offset, column: column)
                        }
                        ForEach(liveItems) { item in liveRow(item, column: column) }
                        ForEach(store.pending, id: \.id) { request in
                            approvalCard(request).id("approval.\(request.id)")
                        }
                        if store.turnActive {
                            ProgressView().controlSize(.small).accessibilityIdentifier("session.live.working")
                        }
                        Color.clear.frame(height: 1).id(Self.bottomID)
                            .onAppear { atBottom = true }
                            .onDisappear { atBottom = false }
                    }
                    .frame(width: column)
                    .padding(.vertical, AeroricSpace.xxxl)
                    .frame(maxWidth: .infinity)
                }
                .safeAreaInset(edge: .bottom, spacing: 0) {
                    footer.frame(width: column).padding(.bottom, AeroricSpace.m).frame(maxWidth: .infinity)
                }
                .onChange(of: liveItems) { _, _ in if atBottom { scroll(scroller, to: Self.bottomID) } }
                .onChange(of: store.pending.count) { _, count in if count > 0 { scroll(scroller, to: Self.bottomID) } }
                .onChange(of: focusApproval) { _, id in if let id { scroll(scroller, to: "approval.\(id)") } }
                .onAppear { scroller.scrollTo(Self.bottomID, anchor: .bottom) }
            }
        }
        .background(AeroricColor.canvas)
    }

    // MARK: - Rows

    @ViewBuilder private func historyRow(_ item: TranscriptItem, offset: Int, column: CGFloat) -> some View {
        switch item {
        case let .message(_, role, content):
            MessageBubble(role: role, content: content, columnWidth: column)
        case let .tool(call):
            ToolCallLine(call: call, index: offset)
        case let .toolGroup(_, calls):
            ToolGroupLine(calls: calls, firstIndex: offset)
        }
    }

    @ViewBuilder private func liveRow(_ item: LiveItem, column: CGFloat) -> some View {
        switch item {
        case let .userEcho(_, text):
            MessageBubble(role: "user", content: [.text(text: text)], columnWidth: column)
        case let .message(_, text):
            MessageBubble(role: "assistant", content: [.text(text: text)], columnWidth: column)
        case let .thought(id, text, summary):
            LiveThoughtRow(id: id, text: text, summary: summary)
        case let .tool(tool):
            LiveToolCard(tool: tool)
        case let .notice(_, message, recoverable):
            LiveNoticeRow(message: message, recoverable: recoverable)
        case let .unknown(_, kind):
            LiveNoticeRow(message: "未识别的内容：\(kind)", recoverable: true)
        }
    }

    private func approvalCard(_ request: LivePendingPermission) -> some View {
        LiveApprovalCard(
            request: request, phase: store.approvals.phase(request.id), enabled: true,
            onDecide: { decision in Task { await store.respond(request.id, decision) } },
            onBeginDeny: { store.beginDeny(request.id) },
            onReason: { store.editReason(request.id, $0) },
            onCancelDeny: { store.cancelDeny(request.id) }
        )
    }

    // MARK: - Footer

    private var suggestions: [SlashCommand.Suggestion] { SlashCommand.suggestions(for: draft, family: store.family) }

    private var footer: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.s) {
            if let problem = store.connectionProblem {
                Label(problem, systemImage: "wifi.exclamationmark")
                    .font(.system(size: AeroricType.caption))
                    .foregroundStyle(AeroricColor.warning)
            }
            if let blocked = store.state?.resumeBlocked, store.state?.live == false {
                Label(blocked, systemImage: "lock")
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .textSelection(.enabled)
                    .padding(.horizontal, AeroricSpace.l)
                    .frame(maxWidth: .infinity, minHeight: 40, alignment: .leading)
                    .glassSurface(.floatingCard, in: AeroricRadius.shape(AeroricRadius.l))
                    .accessibilityIdentifier("session.live.resume-blocked")
            } else {
                composer
            }
            if store.turnActive, let text = LiveLoop.of(store.model?.turnToolSignatures ?? []).text {
                Label(text, systemImage: "arrow.triangle.2.circlepath")
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.warning)
                    .padding(.horizontal, AeroricSpace.l)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .accessibilityIdentifier("session.live.loop")
            }
            TimelineView(.periodic(from: .now, by: 30)) { context in
                let stall = LiveStall.of(turnActive: store.turnActive, waitingOnUser: !store.pending.isEmpty,
                                         lastActivityAtMs: store.model?.lastActivityAtMs, since: store.openedAtMs,
                                         nowMs: UInt64(context.date.timeIntervalSince1970 * 1000))
                if let text = stall.text {
                    Label(text, systemImage: "hourglass")
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(stall.isSevere ? AeroricColor.danger : AeroricColor.warning)
                        .padding(.horizontal, AeroricSpace.l)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .accessibilityIdentifier("session.live.stall")
                }
            }
            if !store.turnActive, let changes = store.model?.lastTurnChanges {
                HStack(alignment: .firstTextBaseline, spacing: AeroricSpace.s) {
                    LiveTurnChangesRow(changes: changes)
                    if let selection = store.autoCheck {
                        LiveAutoCheckMenu(selection: selection) { next in
                            Task { await store.setAutoCheck(next) }
                        }
                        .padding(.trailing, AeroricSpace.l)
                    }
                }
            }
            if let check = store.model?.lastAutoCheck {
                LiveAutoCheckRow(check: check, stale: check.isStale(given: store.model?.lastTurnChanges)) { report in
                    Task {
                        do { _ = try await store.send(report) } catch { store.errorText = "发送失败：\(error)" }
                    }
                }
            }
            UsageBar(usage: store.model?.usage ?? UsageSummary(contextUsed: nil, contextLimit: nil,
                                                                inputTokens: nil, outputTokens: nil))
        }
    }

    @ViewBuilder private var composer: some View {
        if !suggestions.isEmpty { suggestionList }
        ComposerCard(
            text: $draft, placeholder: store.turnActive ? "补充说明会发给正在进行的这一轮…" : "继续对话，或输入 / 使用命令",
            isEditable: store.state != nil, sendState: sendState, identifierPrefix: "session.live",
            onSend: send, onStop: { Task { await store.interrupt() } }
        ) {
            HStack(spacing: AeroricSpace.xs) {
                modelPicker
                permissionPicker
                effortPicker
            }
        }
    }

    private var sendState: ComposerSendState {
        if store.state == nil { return .blocked("正在打开会话") }
        return store.turnActive && draft.isEmpty ? .running : .ready
    }

    private var suggestionList: some View {
        VStack(alignment: .leading, spacing: 2) {
            ForEach(suggestions) { suggestion in
                Button {
                    draft = suggestion.command
                    send()
                } label: {
                    HStack {
                        Text(suggestion.command).font(AeroricType.mono(AeroricType.code))
                        Text(suggestion.detail).foregroundStyle(AeroricColor.textTertiary)
                        Spacer()
                    }
                    .font(.system(size: AeroricType.secondary))
                    .padding(.horizontal, AeroricSpace.m)
                    .frame(height: 26)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .disabled(!suggestion.enabled)
            }
        }
        .padding(AeroricSpace.s)
        .glassSurface(.popover, in: AeroricRadius.shape(AeroricRadius.m))
        .accessibilityIdentifier("session.live.slash")
    }

    private var modelPicker: some View {
        ComposerPicker(title: store.model?.config.model ?? "模型", systemImage: "cpu",
                       menu: LivePickerOptions.models(facts.models, current: store.model?.config.model),
                       identifier: "session.live.model") { configure(model: $0) }
            .id("model-\(modelPickerRequest)")
    }

    private var permissionPicker: some View {
        let current = store.model?.config.permission
        return ComposerPicker(title: current.map(ComposerPickerOptions.permissionTitle) ?? "权限",
                              systemImage: "lock.shield", menu: LivePickerOptions.permissions(current: current),
                              identifier: "session.live.permission") { id in
            guard let permission = LivePermission(rawValue: id), id != current else { return }
            if permission == .fullAccess { confirmFullAccess = true } else { configure(permission: permission) }
        }
        .id("permission-\(permissionPickerRequest)")
    }

    @ViewBuilder private var effortPicker: some View {
        let current = store.model?.config.effort
        if let menu = LivePickerOptions.efforts(facts.efforts, current: current, editable: codex) {
            ComposerPicker(title: current.map(ComposerPickerOptions.effortTitle) ?? "推理强度", systemImage: "brain",
                           menu: menu, identifier: "session.live.effort") { configure(effort: $0) }
        }
    }

    private var moreMenu: some View {
        Menu {
            Button("导出 Markdown", systemImage: "square.and.arrow.up") { Task { await store.export() } }
            Button("重新读取", systemImage: "arrow.clockwise") { Task { await store.open() } }
            Divider()
            Button("结束会话…", systemImage: "stop.circle", role: .destructive) { confirmEnd = true }
        } label: {
            Image(systemName: "ellipsis").frame(width: 26, height: 26)
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .accessibilityLabel("会话操作")
        .accessibilityIdentifier("session.live.more")
    }

    // MARK: - Actions

    private func send() {
        let text = draft
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        draft = ""
        Task { @MainActor in
            do {
                switch try await store.send(text) {
                case .sent, .ranCommand, .clearedToNewSession: break
                case .openModelPicker: modelPickerRequest += 1
                case .openPermissionPicker: permissionPickerRequest += 1
                }
            } catch {
                draft = text
                store.errorText = "发送失败：\(error)"
            }
        }
    }

    private func configure(model: String? = nil, permission: LivePermission? = nil, effort: String? = nil) {
        Task { @MainActor in
            do { try await store.configure(model: model, permission: permission, effort: effort) } catch {
                store.errorText = "修改失败：\(error)"
            }
        }
    }

    private func scroll(_ scroller: ScrollViewProxy, to id: String) {
        withAnimation(AeroricMotion.animation(AeroricMotion.standard, reduceMotion: reduceMotion)) {
            scroller.scrollTo(id, anchor: id == Self.bottomID ? .bottom : .center)
        }
    }
}

/// The session page's confirmations and error alert, kept out of `body`.
private struct SessionDialogs: ViewModifier {
    let store: LiveSessionStore
    @Binding var confirmEnd: Bool
    @Binding var confirmFullAccess: Bool
    let configure: (LivePermission) -> Void

    func body(content: Content) -> some View {
        content
        .alert("结束这个会话？", isPresented: $confirmEnd) {
            Button("结束会话", role: .destructive) { Task { await store.endSession() } }
            Button("取消", role: .cancel) {}
        } message: {
            Text("会停止当前的一轮并释放 Agent 进程。任务和会话记录都会保留，之后发消息会接着这个会话继续。")
        }
        .alert("切换到完全访问？", isPresented: $confirmFullAccess) {
            Button("切换", role: .destructive) { configure(.fullAccess) }
            Button("取消", role: .cancel) {}
        } message: {
            Text("下一轮起 Agent 执行命令和改文件都不再询问你。")
        }
        .alert("操作失败", isPresented: Binding(get: { store.errorText != nil }, set: { if !$0 { store.errorText = nil } })) {
            Button("好", role: .cancel) {}
        } message: {
            Text(store.errorText ?? "")
        }
    }
}
