import SwiftUI

/// What the session page may ask of the host. The App implements it on top
/// of `AeroricClient`; the view never holds a connection (INV-02).
@MainActor
public protocol SessionRouteActions {
    func loadHistory(taskID: String) async throws -> [SessionMessage]
    func send(taskID: String, text: String) async throws
    /// Ends the task (`task.cancel`); there is no per-turn interrupt yet (D3).
    func stop(taskID: String) async
    func respond(eventID: String, outcome: PendingRequestStore.Outcome) async throws
    func export(taskID: String) async throws
}

/// Session page: transcript + inline approvals + the shared composer.
/// Model, permission and configuration are read-only here (INV-04).
public struct SessionRouteView: View {
    let task: TaskStore.TaskEntry
    let actions: any SessionRouteActions
    let canSend: Bool
    /// When set, the composer is replaced by this notice (imported sessions).
    let readOnlyNotice: String?
    @State private var messages: [SessionMessage] = []
    @State private var loading = false
    @State private var draft = ""
    @State private var store: PendingRequestStore
    @State private var errorText: String?

    public init(task: TaskStore.TaskEntry, actions: any SessionRouteActions, canSend: Bool,
                pendingStore: PendingRequestStore = PendingRequestStore(), readOnlyNotice: String? = nil) {
        self.task = task
        self.actions = actions
        self.canSend = canSend
        self.readOnlyNotice = readOnlyNotice
        _store = State(initialValue: pendingStore)
    }

    private var activity: ActivityState { ActivityState(wireStatus: task.status) }
    private var taskRunning: Bool { activity == .running || activity == .waiting }
    private var items: [TranscriptItem] { TranscriptBuilder.items(from: messages, taskRunning: taskRunning) }

    public var body: some View {
        TranscriptView(items: items, isLoading: loading, pending: store.pending, store: store,
                       onRespond: respond) {
            if let readOnlyNotice {
                Label(readOnlyNotice, systemImage: "lock")
                    .font(.system(size: AeroricType.secondary))
                    .foregroundStyle(AeroricColor.textSecondary)
                    .padding(.horizontal, AeroricSpace.l)
                    .frame(maxWidth: .infinity, minHeight: 40, alignment: .leading)
                    .glassSurface(.floatingCard, in: AeroricRadius.shape(AeroricRadius.l))
                    .accessibilityIdentifier("session.read-only")
            } else {
            VStack(alignment: .leading, spacing: AeroricSpace.s) {
                if task.status == "input_required" && canSend {
                    Label("Agent 正在等待输入：发送的文字将原样输入到终端", systemImage: "exclamationmark.circle")
                        .font(.system(size: AeroricType.secondary))
                        .foregroundStyle(AeroricColor.warning)
                        .accessibilityIdentifier("session.continue.terminal-hint")
                }
                ComposerCard(
                    text: $draft, placeholder: canSend ? "继续对话…" : "当前只读；离开任务面后不会自动恢复输入或审批。",
                    isEditable: canSend, sendState: sendState, identifierPrefix: "session.continue",
                    onSend: send, onStop: stop
                ) {
                    HStack(spacing: AeroricSpace.s) {
                        AgentGlyph(agent: task.agent, size: 14)
                        Text(AgentGlyph.displayName(for: task.agent))
                            .font(.system(size: AeroricType.secondary))
                            .foregroundStyle(AeroricColor.textSecondary)
                        StatusDot(activity, label: task.status)
                    }
                    .padding(.horizontal, AeroricSpace.s)
                    .help("会话中不可更改 Agent、模型或权限")
                }
            }
            }
        }
        .overlay(alignment: .topTrailing) {
            Menu {
                Button("导出 Markdown", systemImage: "square.and.arrow.up") { export() }
                Button("重新读取历史", systemImage: "arrow.clockwise") { load() }
            } label: {
                Image(systemName: "ellipsis").frame(width: 26, height: 26)
            }
            .menuStyle(.borderlessButton)
            .menuIndicator(.hidden)
            .fixedSize()
            .padding(AeroricSpace.m)
            .accessibilityLabel("会话操作")
            .accessibilityIdentifier("session.more")
        }
        .alert("操作失败", isPresented: Binding(get: { errorText != nil }, set: { if !$0 { errorText = nil } })) {
            Button("好", role: .cancel) {}
        } message: {
            Text(errorText ?? "")
        }
        .task(id: task.id) { load() }
        .accessibilityIdentifier("session.view")
    }

    private var sendState: ComposerSendState {
        if taskRunning && task.status != "input_required" { return .running }
        return canSend ? .ready : .blocked("当前只读")
    }

    private func load() {
        loading = true
        Task { @MainActor in
            defer { loading = false }
            do { messages = try await actions.loadHistory(taskID: task.id) } catch { errorText = "读取历史失败：\(error)" }
        }
    }

    private func send() {
        let text = draft
        guard canSend, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        draft = ""
        Task { @MainActor in
            do { try await actions.send(taskID: task.id, text: text) } catch {
                draft = text
                errorText = "发送失败：\(error)"
            }
        }
    }

    private func stop() {
        Task { @MainActor in await actions.stop(taskID: task.id) }
    }

    private func respond(_ eventID: String, _ outcome: PendingRequestStore.Outcome) {
        guard canSend else { return }
        store.markSubmitted(eventId: eventID)
        Task { @MainActor in
            do {
                try await actions.respond(eventID: eventID, outcome: outcome)
                store.resolve(eventId: eventID)
            } catch {
                errorText = "提交失败：\(error)"
            }
        }
    }

    private func export() {
        Task { @MainActor in
            do { try await actions.export(taskID: task.id) } catch { errorText = "导出失败：\(error)" }
        }
    }
}
