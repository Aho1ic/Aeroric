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
    public var isIPadLayout: Bool
    public var reduceTransparency: Bool
    @State private var selectedSection: CompanionSection = .tasks

    public init(
        store: CompanionStore,
        isIPadLayout: Bool = false,
        reduceTransparency: Bool = false
    ) {
        self.store = store
        self.isIPadLayout = isIPadLayout
        self.reduceTransparency = reduceTransparency
    }

    public var body: some View {
        VStack(spacing: 0) {
            CompanionStatusBanner(state: store.connectionState, reduceTransparency: reduceTransparency)
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
            CompanionConnectionPane(store: store)
        }
    }
}

public struct CompanionStatusBanner: View {
    public let state: CompanionConnectionState
    public let reduceTransparency: Bool

    public init(state: CompanionConnectionState, reduceTransparency: Bool = false) {
        self.state = state
        self.reduceTransparency = reduceTransparency
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

    public var body: some View {
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

            if !store.pendingApprovals.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    Text("待审批请求").font(.subheadline).bold()
                    ForEach(store.pendingApprovals) { approval in
                        HStack {
                            Text(approval.summary)
                            Spacer()
                            Button("批准") {
                                try? store.submitApproval(requestId: approval.requestId, approve: true)
                            }
                            .disabled(!store.connectionState.isMacOnline)
                            Button("拒绝") {
                                try? store.submitApproval(requestId: approval.requestId, approve: false)
                            }
                            .disabled(!store.connectionState.isMacOnline)
                        }
                    }
                }
                .padding(8)
                .accessibilityIdentifier("companion.approvals")
            }

            HStack(spacing: 8) {
                ForEach(CompanionSessionControl.allCases, id: \.self) { ctrl in
                    Button(ctrl.rawValue) {
                        if let taskId = store.selectedTaskId ?? store.tasks.first?.id {
                            try? store.triggerSessionControl(ctrl, taskId: taskId)
                        }
                    }
                    .disabled(!store.connectionState.isMacOnline)
                }
            }
            .accessibilityIdentifier("companion.session-controls")

            List(store.tasks) { task in
                HStack {
                    Text(task.title)
                    Spacer()
                    Text("\(task.agent) · \(task.status)")
                        .font(.caption)
                }
            }
        }
        .padding(12)
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

public struct CompanionFilesAndGitPane: View {
    @Bindable var store: CompanionStore

    public init(store: CompanionStore) {
        self.store = store
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("项目文件").font(.headline)
            List(store.files) { entry in
                HStack {
                    Image(systemName: entry.isDirectory ? "folder" : "doc.text")
                    Text(entry.name)
                }
            }
            Text("Git 变更").font(.headline)
            List(store.gitChanges) { change in
                HStack {
                    Text(change.status).font(.caption).bold()
                    Text(change.path)
                }
            }
        }
        .padding(12)
    }
}

public struct CompanionConnectionPane: View {
    @Bindable var store: CompanionStore
    @State private var hostName = "My-Aeroric-Mac"
    @State private var relayURL = ""

    public init(store: CompanionStore) {
        self.store = store
    }

    public var body: some View {
        Form {
            TextField("Mac 主机名称", text: $hostName)
            TextField("自托管 Relay URL (可选，无官方默认)", text: $relayURL)
        }
        .padding(12)
    }
}
