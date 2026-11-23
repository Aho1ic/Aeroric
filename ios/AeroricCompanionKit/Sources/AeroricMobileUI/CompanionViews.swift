import AeroricMobileTransport
import SwiftUI

/// AP5: Section tabs for the native iOS/iPadOS task companion.
public enum CompanionSection: String, CaseIterable, Hashable, Sendable {
    case tasks
    case terminal
    case filesAndGit
    case connection

    public var title: LocalizedStringKey {
        switch self {
        case .tasks: "任务与审批"
        case .terminal: "终端"
        case .filesAndGit: "文件与 Git"
        case .connection: "配对与连接"
        }
    }
}

/// AP5: Adaptive iPhone / iPad root view with explicit Mac-online/offline status banner
/// and native Liquid Glass chrome (`#available(iOS 26.0, macOS 26.0, *)`).
public struct CompanionRootView: View {
    @Bindable var store: CompanionStore
    /// D4 step 5: pairing and the live connection; nil in previews.
    public var connector: CompanionConnector?
    public var isIPadLayout: Bool
    public var reduceTransparency: Bool
    @State private var selectedSection: CompanionSection = .tasks

    public init(
        store: CompanionStore,
        connector: CompanionConnector? = nil,
        isIPadLayout: Bool = false,
        reduceTransparency: Bool = false
    ) {
        self.store = store
        self.connector = connector
        self.isIPadLayout = isIPadLayout
        self.reduceTransparency = reduceTransparency
    }

    public var body: some View {
        VStack(spacing: 0) {
            CompanionStatusBanner(state: store.connectionState, reduceTransparency: reduceTransparency,
                                  resyncing: store.resyncNeeded)
            Picker("伴随端分区", selection: $selectedSection) {
                ForEach(CompanionSection.allCases, id: \.self) { section in
                    Text(section.title).tag(section)
                }
            }
            .pickerStyle(.segmented)
            .padding(8)
            .accessibilityIdentifier("companion.section-picker")

            Divider()

            if isIPadLayout {
                NavigationSplitView {
                    List(selection: $store.selectedTaskId) {
                        ForEach(store.tasks) { task in
                            VStack(alignment: .leading) {
                                Text(task.title).font(.headline)
                                Text("\(task.agent) · \(task.status)").font(.caption)
                            }
                            .tag(Optional(task.id))
                        }
                    }
                    .navigationTitle("Aeroric iPad")
                } detail: {
                    activePane
                }
            } else {
                activePane
            }
        }
        .accessibilityIdentifier("companion.root")
        // D4 step 7: a scanned or opened pairing link shows the pairing page.
        .onChange(of: connector?.proposedPairingCode) { _, code in
            if code != nil { selectedSection = .connection }
        }
    }

    @ViewBuilder
    private var activePane: some View {
        switch selectedSection {
        case .tasks:
            CompanionTasksPane(store: store)
        case .terminal:
            CompanionTerminalPane(store: store)
        case .filesAndGit:
            CompanionFilesAndGitPane(store: store)
        case .connection:
            CompanionConnectionPane(store: store, connector: connector)
        }
    }
}

public struct CompanionStatusBanner: View {
    public let state: CompanionConnectionState
    public let reduceTransparency: Bool
    /// D4: something was missed (an event gap, an overflow, a stale refusal):
    /// what is shown may be old until the next snapshot.
    public let resyncing: Bool

    public init(state: CompanionConnectionState, reduceTransparency: Bool = false, resyncing: Bool = false) {
        self.state = state
        self.reduceTransparency = reduceTransparency
        self.resyncing = resyncing
    }

    public var statusText: String {
        switch state {
        case .unpaired:
            return "未配对 Aeroric Mac（请扫码或输入配对码）"
        case let .disconnected(reason):
            return "Mac 离线（\(reason)）— 停止提交执行与审批"
        case let .relayConnectedMacOffline(relay):
            return "已连自托管 Relay (\(relay))，但 Mac 宿主离线 — 不排队操作"
        case let .macOnline(epoch, mode):
            if resyncing { return "正在和 Mac 重新同步，之前显示的状态可能已过期" }
            return "Mac 在线 (\(mode.rawValue), \(epoch))"
        case let .revoked(reason):
            return "设备已撤销（\(reason)）— 请按 Q11 重新配对"
        }
    }

    public var body: some View {
        let base = HStack {
            Circle()
                .fill(state.isMacOnline ? Color.green : Color.orange)
                .frame(width: 8, height: 8)
            Text(statusText)
                .font(.caption)
            Spacer()
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .accessibilityIdentifier("companion.status-banner")

        if !reduceTransparency, #available(iOS 26.0, macOS 26.0, *) {
            base.glassEffect(.regular, in: RoundedRectangle(cornerRadius: 10, style: .continuous))
        } else {
            base
        }
    }
}

public struct CompanionTasksPane: View {
    @Bindable var store: CompanionStore
    @State private var selectedAgent = "claude"
    @State private var promptDraft = ""
    @State private var errorMessage: String?

    public init(store: CompanionStore) {
        self.store = store
    }

    /// Runs `action`; a refusal shows as `errorMessage`, success clears it.
    private func attempt(_ action: () throws -> Void) {
        do {
            try action()
            errorMessage = nil
        } catch {
            errorMessage = String(describing: error)
        }
    }

    public var body: some View {
        NavigationStack {
            VStack(alignment: .leading, spacing: 10) {
                HStack {
                    Picker("Agent", selection: $selectedAgent) {
                        Text("Claude").tag("claude")
                        Text("Codex").tag("codex")
                        Text("DSH").tag("dsh")
                        Text("OMP").tag("omp")
                    }
                    .pickerStyle(.segmented)
                    TextField("输入任务指令…", text: $promptDraft)
                        .textFieldStyle(.roundedBorder)
                    Button("提交") {
                        do {
                            _ = try store.createTask(
                                projectId: store.selectedProjectId ?? "default",
                                agent: selectedAgent,
                                prompt: promptDraft
                            )
                            promptDraft = ""
                        } catch {
                            errorMessage = String(describing: error)
                        }
                    }
                    .disabled(!store.connectionState.isMacOnline || promptDraft.isEmpty)
                    .accessibilityIdentifier("companion.task.submit")
                }

                // B4-4: a refused action says why instead of doing nothing.
                if let errorMessage {
                    Text(errorMessage)
                        .font(.caption)
                        .foregroundStyle(.red)
                        .accessibilityIdentifier("companion.error")
                }

                if !store.pendingApprovals.isEmpty {
                    VStack(alignment: .leading, spacing: 6) {
                        Text("待审批请求").font(.subheadline).bold()
                        ForEach(store.pendingApprovals) { approval in
                            CompanionApprovalRow(store: store, approval: approval, attempt: attempt)
                        }
                    }
                    .padding(8)
                    .accessibilityIdentifier("companion.approvals")
                }

                HStack(spacing: 8) {
                    // B4-4: only what the Mac's adapter says it offers.
                    ForEach(store.adapterCapabilities.offeredControls, id: \.self) { ctrl in
                        Button(ctrl.rawValue) {
                            if let taskId = store.selectedTaskId ?? store.tasks.first?.id {
                                attempt { _ = try store.triggerSessionControl(ctrl, taskId: taskId) }
                            }
                        }
                        .disabled(!store.connectionState.isMacOnline)
                    }
                }
                .accessibilityIdentifier("companion.session-controls")

                // D4 (2026-11-23): a task opens its page with the session.
                List(store.tasks) { task in
                    NavigationLink(value: task.id) {
                        HStack {
                            Text(task.title)
                            Spacer()
                            Text("\(task.agent) · \(task.status)")
                                .font(.caption)
                        }
                    }
                    .accessibilityIdentifier("companion.task.\(task.id)")
                }
            }
            .padding(12)
            .navigationDestination(for: String.self) { taskId in
                CompanionTaskPage(store: store, taskId: taskId)
            }
        }
    }
}

/// One approval with its buttons and what became of the answer.
struct CompanionApprovalRow: View {
    @Bindable var store: CompanionStore
    let approval: CompanionApproval
    let attempt: (() throws -> Void) -> Void

    var body: some View {
        let answerable = store.connectionState.isMacOnline && approval.state.canSubmit
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(approval.summary)
                Spacer()
                Button("批准") {
                    attempt { try store.submitApproval(requestId: approval.requestId, approve: true) }
                }
                .disabled(!answerable)
                Button("拒绝") {
                    attempt { try store.submitApproval(requestId: approval.requestId, approve: false) }
                }
                .disabled(!answerable)
            }
            if let note = approval.state.note {
                Text(note)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("companion.approval.note")
            }
        }
    }
}

/// D4 (2026-11-23): one task — its open approvals and the latest messages of
/// its session, read again whenever the Mac says the task changed.
public struct CompanionTaskPage: View {
    @Bindable var store: CompanionStore
    let taskId: String
    @State private var errorMessage: String?
    @State private var draft = ""

    public init(store: CompanionStore, taskId: String) {
        self.store = store
        self.taskId = taskId
    }

    private var task: CompanionTask? { store.tasks.first { $0.id == taskId } }

    private func attempt(_ action: () throws -> Void) {
        do {
            try action()
            errorMessage = nil
        } catch {
            errorMessage = String(describing: error)
        }
    }

    public var body: some View {
        List {
            if let task {
                Section {
                    Text("\(task.agent) · \(task.status)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            let approvals = store.pendingApprovals.filter { $0.taskId == taskId }
            if !approvals.isEmpty {
                Section("待审批") {
                    ForEach(approvals) { approval in
                        CompanionApprovalRow(store: store, approval: approval, attempt: attempt)
                    }
                    if let errorMessage {
                        Text(errorMessage).font(.caption).foregroundStyle(.red)
                    }
                }
            }
            Section("发给这个任务") { messageSection }
            if let changes = store.taskChanges, changes.taskId == taskId {
                Section("改动") { changesSection(changes) }
            }
            Section("会话") { transcriptSection }
        }
        .navigationTitle(task?.title ?? "任务")
        .accessibilityIdentifier("companion.task-page")
        .onAppear { store.openTask(taskId) }
        .onDisappear { store.closeTask(taskId) }
        .sheet(isPresented: Binding(
            get: { store.taskChanges?.taskId == taskId && store.taskChanges?.document != nil },
            set: { if !$0 { store.closeTaskDiff() } })) {
            if let document = store.taskChanges?.document {
                CompanionDocumentView(document: document) { store.closeTaskDiff() }
            }
        }
    }

    /// D4 (2026-11-23): a message to the task, and 停止 for its current turn.
    @ViewBuilder
    private var messageSection: some View {
        let online = store.connectionState.isMacOnline
        TextField("写给 Agent 的话…", text: $draft, axis: .vertical)
            .lineLimit(1...5)
            .disabled(!online)
            .accessibilityIdentifier("companion.task.message")
        HStack {
            Button("发送") {
                attempt {
                    try store.sendMessage(taskId: taskId, text: draft)
                    draft = ""
                }
            }
            .disabled(!online || draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            .accessibilityIdentifier("companion.task.send")
            Spacer()
            Button("停止这一轮", role: .destructive) {
                attempt { try store.stopTask(taskId: taskId) }
            }
            .disabled(!online)
            .accessibilityIdentifier("companion.task.stop")
        }
        .buttonStyle(.borderless)
        if let action = store.taskActions[taskId] {
            Text(action.note)
                .font(.caption)
                .foregroundStyle(action.state.isFailure ? Color.red : Color.secondary)
                .accessibilityIdentifier("companion.task.action-note")
        }
        if !online {
            Text("连上 Mac 后才能发").font(.caption).foregroundStyle(.secondary)
        }
    }

    /// D4 (2026-11-23): what the task changed — in its own copy, or, when it
    /// works in the project folder, everything uncommitted there.
    @ViewBuilder
    private func changesSection(_ changes: CompanionTaskChanges) -> some View {
        switch changes.scope {
        case .worktree:
            Text("这个任务在自己的副本里改，下面是它的改动")
                .font(.caption).foregroundStyle(.secondary)
        case .project:
            Text("这个任务直接在项目目录里改，下面是项目里全部没提交的改动，可能有别的任务或你自己的")
                .font(.caption).foregroundStyle(.secondary)
                .accessibilityIdentifier("companion.task.changes.project-scope")
        case nil:
            EmptyView()
        }
        switch changes.changes {
        case .loading:
            ProgressView()
        case let .loaded(list) where list.isEmpty:
            Text("没有未提交的改动").foregroundStyle(.secondary)
        case let .loaded(list):
            ForEach(list) { change in
                Button { store.openTaskDiff(change) } label: {
                    HStack {
                        Text(change.status)
                            .font(.system(.caption, design: .monospaced))
                            .bold()
                            .frame(minWidth: 24, alignment: .leading)
                        Text(change.path).lineLimit(2)
                        Spacer()
                        if change.staged { Text("已暂存").font(.caption2).foregroundStyle(.secondary) }
                    }
                }
                .accessibilityIdentifier("companion.task.change.\(change.id)")
            }
        case let .unavailable(reason):
            Text(reason).foregroundStyle(.secondary)
        case let .failed(message):
            Text("没读到：\(message)").foregroundStyle(.red)
        }
    }

    @ViewBuilder
    private var transcriptSection: some View {
        switch store.transcript {
        case .closed, .loading:
            HStack {
                ProgressView()
                Text("正在从 Mac 读会话…").foregroundStyle(.secondary)
            }
        case let .shown(transcript, refreshing):
            if transcript.messages.isEmpty {
                Text("会话里还没有消息").foregroundStyle(.secondary)
            }
            if transcript.omitted > 0 {
                Text("只显示最近 \(transcript.messages.count) 条，较早的 \(transcript.omitted) 条在 Mac 上看")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .accessibilityIdentifier("companion.transcript.omitted")
            }
            ForEach(transcript.messages) { message in
                CompanionMessageView(message: message)
            }
            if refreshing {
                Text("正在更新…").font(.caption).foregroundStyle(.secondary)
            }
        case let .unavailable(_, reason):
            Text(reason)
                .foregroundStyle(.secondary)
                .accessibilityIdentifier("companion.transcript.unavailable")
        case let .failed(_, message):
            VStack(alignment: .leading, spacing: 6) {
                Text("没读到会话：\(message)").foregroundStyle(.red)
                Button("重试") { store.openTask(taskId) }
                    .disabled(!store.connectionState.isMacOnline)
            }
            .accessibilityIdentifier("companion.transcript.failed")
        }
    }
}

struct CompanionMessageView: View {
    let message: CompanionMessage

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(message.role == "user" ? "你" : "Agent")
                .font(.caption)
                .bold()
                .foregroundStyle(message.role == "user" ? Color.accentColor : Color.secondary)
            ForEach(Array(message.lines.enumerated()), id: \.offset) { _, line in
                lineView(line)
                if line.truncated {
                    Text("（太长，只显示了前面一部分）").font(.caption2).foregroundStyle(.secondary)
                }
            }
        }
        .padding(.vertical, 2)
    }

    @ViewBuilder
    private func lineView(_ line: CompanionMessageLine) -> some View {
        switch line.kind {
        case .text:
            Text(line.text).textSelection(.enabled)
        case .thinking:
            Text(line.text).font(.callout).italic().foregroundStyle(.secondary).lineLimit(4)
        case .tool:
            Text("工具 · \(line.text)")
                .font(.system(.caption, design: .monospaced))
                .lineLimit(3)
        case .toolResult:
            Text(line.text)
                .font(.system(.caption2, design: .monospaced))
                .foregroundStyle(.secondary)
                .lineLimit(6)
        case .failedToolResult:
            Text(line.text)
                .font(.system(.caption2, design: .monospaced))
                .foregroundStyle(.red)
                .lineLimit(6)
        case .attachment:
            Text("附件 · \(line.text)").font(.caption).foregroundStyle(.secondary)
        }
    }
}

/// AP5: Pure native terminal view with IME input and touch accessory keys (no WebView/JS).
public struct CompanionTerminalPane: View {
    @Bindable var store: CompanionStore
    @State private var imeInput = ""

    public static let accessoryKeys: [(label: String, sequence: String)] = [
        ("Esc", "\u{1b}"),
        ("Tab", "\t"),
        ("Ctrl+C", "\u{03}"),
        ("↑", "\u{1b}[A"),
        ("↓", "\u{1b}[B"),
    ]

    public init(store: CompanionStore) {
        self.store = store
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            ScrollView {
                Text(store.terminalBuffer.isEmpty ? "等待 Mac 宿主终端流…" : store.terminalBuffer)
                    .font(.system(.body, design: .monospaced))
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(8)
            }
            .accessibilityIdentifier("companion.terminal.viewport")

            HStack(spacing: 6) {
                ForEach(Self.accessoryKeys, id: \.label) { key in
                    Button(key.label) {
                        _ = try? store.sendTerminalInput(streamId: 1, text: key.sequence)
                    }
                    .disabled(!store.connectionState.isMacOnline)
                }
            }
            .accessibilityIdentifier("companion.terminal.accessory-bar")

            HStack {
                TextField("终端输入（支持中文 IME）…", text: $imeInput)
                    .textFieldStyle(.roundedBorder)
                    .disabled(!store.connectionState.isMacOnline)
                Button("发送") {
                    guard !imeInput.isEmpty else { return }
                    _ = try? store.sendTerminalInput(streamId: 1, text: imeInput + "\r")
                    imeInput = ""
                }
                .disabled(!store.connectionState.isMacOnline || imeInput.isEmpty)
            }
        }
        .padding(12)
    }
}

/// D4 (2026-11-23): a project's Git changes and files, read from the Mac
/// and read again whenever the Mac's state changes. Read only.
public struct CompanionFilesAndGitPane: View {
    @Bindable var store: CompanionStore

    public init(store: CompanionStore) {
        self.store = store
    }

    private var projectId: String? {
        store.browse?.projectId ?? store.selectedProjectId ?? store.projects.first?.id
    }

    public var body: some View {
        List {
            if store.projects.isEmpty {
                Text(store.connectionState.isMacOnline ? "Mac 上还没有项目" : "连上 Mac 后在这里看项目的文件和改动")
                    .foregroundStyle(.secondary)
            } else {
                Section {
                    Picker("项目", selection: Binding(
                        get: { projectId ?? "" },
                        set: { id in
                            store.selectedProjectId = id
                            store.openBrowse(projectId: id)
                        })) {
                        ForEach(store.projects) { project in
                            Text(project.name).tag(project.id)
                        }
                    }
                    .accessibilityIdentifier("companion.browse.project")
                }
                if let browse = store.browse {
                    Section("Git 改动") { changesSection(browse.changes) }
                    Section(folderTitle(browse)) { filesSection(browse) }
                }
            }
        }
        .accessibilityIdentifier("companion.browse")
        .onAppear { if let projectId { store.openBrowse(projectId: projectId) } }
        .onDisappear { store.closeBrowse() }
        .onChange(of: store.projects.first?.id) { _, _ in
            if store.browse == nil, let projectId { store.openBrowse(projectId: projectId) }
        }
        .sheet(isPresented: Binding(
            get: { store.browse?.document != nil },
            set: { if !$0 { store.closeDocument() } })) {
            if let document = store.browse?.document {
                CompanionDocumentView(document: document) { store.closeDocument() }
            }
        }
    }

    private func folderTitle(_ browse: CompanionBrowse) -> String {
        guard let directory = browse.directory else { return "文件" }
        return "文件 · \((directory as NSString).lastPathComponent)"
    }

    @ViewBuilder
    private func changesSection(_ load: CompanionLoad<[CompanionGitChange]>) -> some View {
        switch load {
        case .loading:
            ProgressView()
        case let .loaded(changes) where changes.isEmpty:
            Text("没有未提交的改动").foregroundStyle(.secondary)
        case let .loaded(changes):
            ForEach(changes) { change in
                Button { store.openDiff(change) } label: {
                    HStack {
                        Text(change.status)
                            .font(.system(.caption, design: .monospaced))
                            .bold()
                            .frame(minWidth: 24, alignment: .leading)
                        Text(change.path).lineLimit(2)
                        Spacer()
                        if change.staged { Text("已暂存").font(.caption2).foregroundStyle(.secondary) }
                    }
                }
                .accessibilityIdentifier("companion.change.\(change.id)")
            }
        case let .unavailable(reason):
            Text(reason).foregroundStyle(.secondary)
        case let .failed(message):
            Text("没读到：\(message)").foregroundStyle(.red)
        }
    }

    @ViewBuilder
    private func filesSection(_ browse: CompanionBrowse) -> some View {
        if !browse.folders.isEmpty {
            Button { store.leaveFolder() } label: {
                Label("上一级", systemImage: "chevron.left")
            }
            .accessibilityIdentifier("companion.browse.up")
        }
        switch browse.entries {
        case .loading:
            ProgressView()
        case let .loaded(entries) where entries.isEmpty:
            Text("这个文件夹是空的").foregroundStyle(.secondary)
        case let .loaded(entries):
            ForEach(entries) { entry in
                Button { store.openFile(entry) } label: {
                    HStack {
                        Image(systemName: entry.isDirectory ? "folder" : "doc.text")
                        Text(entry.name)
                    }
                }
            }
        case let .unavailable(reason):
            Text(reason).foregroundStyle(.secondary)
        case let .failed(message):
            Text("没读到：\(message)").foregroundStyle(.red)
        }
    }
}

/// A file or a diff, as text; a diff's added and removed lines in color.
struct CompanionDocumentView: View {
    let document: CompanionDocument
    let close: () -> Void

    var body: some View {
        NavigationStack {
            Group {
                switch document.content {
                case .loading:
                    ProgressView("正在从 Mac 读…")
                case let .loaded(text):
                    ScrollView([.vertical, .horizontal]) {
                        VStack(alignment: .leading, spacing: 0) {
                            if text.truncated {
                                Text("文件太大，只显示了前面一部分")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .padding(.bottom, 6)
                            }
                            if text.text.isEmpty {
                                Text(isDiff ? "没有差异" : "空文件").foregroundStyle(.secondary)
                            }
                            ForEach(Array(text.text.split(separator: "\n", omittingEmptySubsequences: false)
                                .enumerated()), id: \.offset) { _, line in
                                Text(String(line).isEmpty ? " " : String(line))
                                    .font(.system(.caption, design: .monospaced))
                                    .foregroundStyle(color(String(line)))
                                    .fixedSize(horizontal: true, vertical: false)
                            }
                        }
                        .padding(12)
                        .textSelection(.enabled)
                    }
                case let .unavailable(reason):
                    Text(reason).foregroundStyle(.secondary).padding()
                case let .failed(message):
                    Text("没读到：\(message)").foregroundStyle(.red).padding()
                }
            }
            .navigationTitle((document.path as NSString).lastPathComponent)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) { Button("关闭", action: close) }
            }
        }
        .accessibilityIdentifier("companion.document")
    }

    private var isDiff: Bool {
        if case .diff = document.kind { return true }
        return false
    }

    private func color(_ line: String) -> Color {
        guard isDiff else { return .primary }
        if line.hasPrefix("+++") || line.hasPrefix("---") { return .secondary }
        if line.hasPrefix("+") { return .green }
        if line.hasPrefix("-") { return .red }
        if line.hasPrefix("@@") { return .blue }
        return .primary
    }
}

/// D4 step 5: pair from the Mac's link, see what the connection is doing,
/// forget the Mac. D4 step 7: scan the Mac's QR code, or take a link that
/// opened the app — either only fills the field; pairing is the user's tap.
public struct CompanionConnectionPane: View {
    @Bindable var store: CompanionStore
    var connector: CompanionConnector?
    @State private var pairingCode = ""
    @State private var deviceName = "iPhone"
    @State private var pairing = false
    @State private var scanning = false

    public init(store: CompanionStore, connector: CompanionConnector? = nil) {
        self.store = store
        self.connector = connector
    }

    public var body: some View {
        Form {
            if let connector {
                if let credentials = connector.credentials {
                    Section("已配对") {
                        LabeledContent("Mac", value: credentials.hostName)
                        Text(connector.activity)
                            .accessibilityIdentifier("companion.connection.activity")
                        if let error = connector.lastError, pairingCode.isEmpty {
                            Text(error).font(.caption).foregroundStyle(.secondary)
                        }
                        Button(connector.isRunning ? "断开" : "连接") {
                            Task {
                                if connector.isRunning { await connector.stop() } else { connector.start() }
                            }
                        }
                        Button("取消配对", role: .destructive) {
                            Task { await connector.forget() }
                        }
                        .accessibilityIdentifier("companion.connection.forget")
                    }
                }
                if connector.credentials == nil || !pairingCode.isEmpty {
                    pairingSection(connector)
                }
            } else {
                Text("这个预览没有连接。")
            }
        }
        .padding(12)
        .onAppear { takeOffered() }
        .onChange(of: connector?.proposedPairingCode) { _, _ in takeOffered() }
        #if os(iOS) && canImport(VisionKit)
        .sheet(isPresented: $scanning) {
            CompanionPairingScanner { text in
                scanning = false
                connector?.offerPairingLink(text)
            }
            .ignoresSafeArea()
        }
        #endif
    }

    @ViewBuilder
    private func pairingSection(_ connector: CompanionConnector) -> some View {
        Section(connector.credentials == nil ? "和 Mac 配对" : "换一台 Mac") {
            Text("在 Mac 上的 Aeroric 打开远程访问，生成配对二维码：扫码，或复制链接粘贴到这里。10 分钟内有效。")
                .font(.caption)
                .foregroundStyle(.secondary)
            if let name = connector.proposedHostName, !pairingCode.isEmpty {
                Text("链接里的 Mac：\(name)。确认是你的 Mac 再点“配对”。")
                    .accessibilityIdentifier("companion.pairing.offered-host")
            }
            if let current = connector.credentials {
                Text("配对会替换现在的 \(current.hostName)")
                    .foregroundStyle(.orange)
                    .accessibilityIdentifier("companion.pairing.replaces")
            }
            #if os(iOS) && canImport(VisionKit)
            if let reason = CompanionPairingScanner.unavailableReason {
                Text(reason).font(.caption).foregroundStyle(.secondary)
            } else {
                Button("扫描 Mac 上的二维码") { scanning = true }
                    .accessibilityIdentifier("companion.pairing.scan")
            }
            #endif
            TextField("aeroric://pair?code=…", text: $pairingCode, axis: .vertical)
                .lineLimit(3)
                .autocorrectionDisabled()
                .accessibilityIdentifier("companion.pairing.code")
            TextField("这台设备的名称", text: $deviceName)
                .accessibilityIdentifier("companion.pairing.device-name")
            Button(pairing ? "正在配对…" : "配对") {
                pairing = true
                Task {
                    try? await connector.pair(code: pairingCode, deviceName: deviceName)
                    pairing = false
                    if connector.lastError == nil { pairingCode = "" }
                }
            }
            .disabled(pairing || pairingCode.isEmpty || deviceName.isEmpty)
            .accessibilityIdentifier("companion.pairing.submit")
            if let error = connector.lastError {
                Text(error)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .accessibilityIdentifier("companion.pairing.error")
            }
        }
    }

    private func takeOffered() {
        if let code = connector?.takeProposedPairingCode() { pairingCode = code }
    }
}
