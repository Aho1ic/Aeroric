import Foundation
import SwiftUI

/// P7-06：同步/RAG 面的簿记与纯逻辑（五视图的数据源）。
///
/// 冲突守卫与宿主同语义：决定必须带着**冲突里那两个字段的原值**——决定和
/// 内容对不上号时下一轮会作废这条决定重新问，而不是拿旧决定覆盖用户没见过的
/// 内容（`sync::store` 的模块文档）。
@MainActor
@Observable
public final class SyncStore {
    public struct RemoteTarget: Identifiable, Equatable, Sendable {
        public var id: String
        public var kind: String
        public var root: String
        public var connectionId: String
        public var autoSync: Bool

        public init(id: String, kind: String, root: String, connectionId: String, autoSync: Bool) {
            self.id = id
            self.kind = kind
            self.root = root
            self.connectionId = connectionId
            self.autoSync = autoSync
        }
    }

    public struct Status: Equatable, Sendable {
        public var remoteId: String
        public var autoSync: Bool
        public var failures: Int
        public var dirty: Bool
        public var nextRunInMs: Int64?

        public init(remoteId: String, autoSync: Bool, failures: Int, dirty: Bool, nextRunInMs: Int64?) {
            self.remoteId = remoteId
            self.autoSync = autoSync
            self.failures = failures
            self.dirty = dirty
            self.nextRunInMs = nextRunInMs
        }
    }

    public struct PendingConflict: Identifiable, Equatable, Sendable {
        public var path: String
        public var localHash: String
        public var remoteHash: String

        public var id: String { path }

        public init(path: String, localHash: String, remoteHash: String) {
            self.path = path
            self.localHash = localHash
            self.remoteHash = remoteHash
        }
    }

    public private(set) var remotes: [RemoteTarget] = []
    public private(set) var statuses: [Status] = []
    public private(set) var pendingConflicts: [PendingConflict] = []
    /// 最近一次同步的产出（summary；凭据不出现在任何事件快照——store 无凭据字段）。
    public private(set) var lastSyncAtMs: UInt64?

    public init() {}

    public func ingest(remotes: [RemoteTarget]) {
        self.remotes = remotes
    }

    public func ingest(statuses: [Status]) {
        self.statuses = statuses
    }

    public func ingestConflicts(_ conflicts: [PendingConflict]) {
        pendingConflicts = conflicts
    }

    public func recordResolved(path: String) {
        pendingConflicts.removeAll { $0.path == path }
    }

    public func recordSyncFinished(atMs ms: UInt64) {
        lastSyncAtMs = ms
    }

    public func status(of remoteId: String) -> Status? {
        statuses.first { $0.remoteId == remoteId }
    }

    // MARK: - 纯逻辑

    /// 冲突决定是否仍然有效：local/remote hash 与用户当时看到的冲突一致。
    /// 不一致（另一轮同步改了远端）→ 决定作废重新问，不覆盖。
    public static func resolutionMatches(
        _ decision: PendingConflict, localHash: String, remoteHash: String
    ) -> Bool {
        decision.localHash == localHash && decision.remoteHash == remoteHash
    }

    /// 退避档位的人话标签（与 schedule 的 15 分钟封顶对齐）。
    public static func backoffLabel(failures: Int) -> String {
        switch failures {
        case 0: "正常"
        case 1: "失败 1 次，稍后重试"
        case 2: "失败 2 次，退避中"
        default: "连续失败 \(failures) 次，退避至多 15 分钟"
        }
    }
}

// MARK: - 1. SyncStatusPanel

/// 同步状态面板：各远端的自动同步开关、退避档位、手动「立即同步」。
public struct SyncStatusPanel: View {
    @Bindable var store: SyncStore
    public var onToggleAuto: ((_ remote: SyncStore.RemoteTarget, _ enabled: Bool) -> Void)?
    public var onRunNow: ((_ remote: SyncStore.RemoteTarget) -> Void)?

    public init(
        store: SyncStore,
        onToggleAuto: ((_ remote: SyncStore.RemoteTarget, _ enabled: Bool) -> Void)? = nil,
        onRunNow: ((_ remote: SyncStore.RemoteTarget) -> Void)? = nil
    ) {
        self.store = store
        self.onToggleAuto = onToggleAuto
        self.onRunNow = onRunNow
    }

    public var body: some View {
        List {
            ForEach(store.remotes) { remote in
                let status = store.status(of: remote.id)
                HStack {
                    VStack(alignment: .leading) {
                        Text(remote.root).font(.callout)
                        Text(SyncStore.backoffLabel(failures: status?.failures ?? 0))
                            .font(.caption)
                            .foregroundStyle((status?.failures ?? 0) > 0 ? .orange : .secondary)
                        if status?.dirty == true {
                            Label("有本地改动待同步", systemImage: "arrow.triangle.2.circlepath")
                                .font(.caption2).foregroundStyle(.orange)
                        }
                    }
                    Spacer()
                    if remote.kind == "cloud" {
                        Toggle("自动", isOn: Binding(
                            get: { remote.autoSync },
                            set: { onToggleAuto?(remote, $0) }
                        ))
                        .toggleStyle(.switch)
                        .fixedSize()
                        Button("同步") { onRunNow?(remote) }
                            .buttonStyle(.borderless)
                    } else {
                        // git 目标走 notebook.git.sync——失败模式不同，不混在一个按钮里。
                        Text("git 目标").font(.caption).foregroundStyle(.secondary)
                    }
                }
            }
        }
        .listStyle(.plain)
        .accessibilityIdentifier("notebook.sync")
    }
}

// MARK: - 2. SyncConflictSheet

/// 同步冲突弹窗：决定带着冲突原值（local/remote hash）入库；撤回＝resolution 传空。
/// fork 目标在此处挡空串，写入侧还有范围/穿越/symlink 三道。
public struct SyncConflictSheet: View {
    public var conflict: SyncStore.PendingConflict
    public var onKeepLocal: () -> Void
    public var onKeepRemote: () -> Void
    public var onWithdraw: () -> Void
    public var onCancel: () -> Void

    public init(
        conflict: SyncStore.PendingConflict,
        onKeepLocal: @escaping () -> Void,
        onKeepRemote: @escaping () -> Void,
        onWithdraw: @escaping () -> Void,
        onCancel: @escaping () -> Void
    ) {
        self.conflict = conflict
        self.onKeepLocal = onKeepLocal
        self.onKeepRemote = onKeepRemote
        self.onWithdraw = onWithdraw
        self.onCancel = onCancel
    }

    public var body: some View {
        VStack(spacing: 12) {
            Label("同步冲突：\(conflict.path)", systemImage: "exclamationmark.triangle.fill")
                .font(.headline)
            Text("本地与远端都有改动。决定只入库，由下一轮同步执行；撤回后重新询问。")
                .font(.callout)
                .multilineTextAlignment(.leading)
                .frame(maxWidth: 340, alignment: .leading)
            HStack {
                Button("取消", action: onCancel)
                    .keyboardShortcut(.cancelAction)
                Button("撤回决定", action: onWithdraw)
                Button("保留远端", action: onKeepRemote)
                Button("保留本地", action: onKeepLocal)
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(minWidth: 400)
    }
}

// MARK: - 3. RAGSearchView

/// RAG 检索面板：结果列表 + 未建索引提示。检索要走 embedding provider
/// （可能把查询文本发往外网）——按钮触发，不因打开面板自动发送。
public struct RAGSearchView: View {
    public struct Hit: Identifiable, Equatable, Sendable {
        public var path: String
        public var snippet: String
        public var score: Double

        public var id: String { "\(path)#\(snippet.hashValue)" }

        public init(path: String, snippet: String, score: Double) {
            self.path = path
            self.snippet = snippet
            self.score = score
        }
    }

    @State private var query = ""
    @State private var hits: [Hit] = []
    @State private var vectorsMissing = false
    @State private var searched = false
    public var onSearch: ((_ query: String, _ deliver: @escaping ([Hit], Bool) -> Void) -> Void)?

    public init(
        onSearch: ((_ query: String, _ deliver: @escaping ([Hit], Bool) -> Void) -> Void)? = nil
    ) {
        self.onSearch = onSearch
    }

    public var body: some View {
        VStack(spacing: 0) {
            HStack {
                TextField("在笔记库里问一句…", text: $query)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit(runSearch)
                Button("检索", action: runSearch)
                    .disabled(query.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            .padding(10)
            if vectorsMissing {
                ContentUnavailableView(
                    "还没有索引", systemImage: "brain",
                    description: Text("先在设置里建索引——检索只会命中已索引的笔记"))
            } else {
                List(hits) { hit in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(hit.snippet).font(.callout).lineLimit(3)
                        HStack {
                            Text(hit.path).font(.caption).foregroundStyle(.secondary)
                            Spacer()
                            Text(String(format: "%.3f", hit.score))
                                .font(.caption2).foregroundStyle(.tertiary)
                        }
                    }
                }
                .listStyle(.plain)
                .overlay {
                    if searched && hits.isEmpty {
                        ContentUnavailableView("没有命中", systemImage: "magnifyingglass")
                    }
                }
            }
        }
        .accessibilityIdentifier("notebook.rag")
    }

    private func runSearch() {
        let trimmed = query.trimmingCharacters(in: .whitespaces)
        guard !trimmed.isEmpty else { return }
        onSearch?(trimmed) { results, missing in
            hits = results
            vectorsMissing = missing
            searched = true
        }
    }
}

// MARK: - 4. EmbeddingSettingsView

/// embedding 设置：provider/model/endpoint + key（key 存 OS 钥匙串，
/// **不回明文**——只有「已设置」状态）。成本与外网发送内容明确提示；
/// 默认不因打开面板而发送任何内容。
public struct EmbeddingSettingsView: View {
    public struct Config: Equatable, Sendable {
        public var provider: String
        public var baseURL: String
        public var model: String

        public init(provider: String, baseURL: String, model: String) {
            self.provider = provider
            self.baseURL = baseURL
            self.model = model
        }
    }

    @State private var provider = "ollama"
    @State private var baseURL = "http://127.0.0.1:11434"
    @State private var model = ""
    @State private var apiKey = ""
    @State private var keyIsSet = false
    public var onTest: ((_ config: Config, _ key: String, _ deliver: @escaping (Int?) -> Void) -> Void)?
    public var onKeyChange: ((_ key: String?) -> Void)?

    public init(
        onTest: ((_ config: Config, _ key: String, _ deliver: @escaping (Int?) -> Void) -> Void)? = nil,
        onKeyChange: ((_ key: String?) -> Void)? = nil
    ) {
        self.onTest = onTest
        self.onKeyChange = onKeyChange
    }

    public var body: some View {
        Form {
            Section("Embedding 提供方") {
                Picker("类型", selection: $provider) {
                    Text("Ollama（本机）").tag("ollama")
                    Text("OpenAI 兼容").tag("openai")
                }
                .onChange(of: provider) { _, newValue in
                    baseURL = newValue == "ollama" ? "http://127.0.0.1:11434" : "https://api.openai.com/v1"
                }
                TextField("端点", text: $baseURL)
                TextField("模型", text: $model)
            }
            if provider == "openai" {
                Section("API Key") {
                    SecureField("API Key（存入钥匙串，不回显）", text: $apiKey)
                    HStack {
                        Text(keyIsSet ? "已设置" : "未设置")
                            .font(.caption)
                            .foregroundStyle(keyIsSet ? .green : .secondary)
                        Button("保存 Key") {
                            onKeyChange?(apiKey.trimmingCharacters(in: .whitespaces))
                            keyIsSet = !apiKey.trimmingCharacters(in: .whitespaces).isEmpty
                            apiKey = ""
                        }
                        .disabled(apiKey.trimmingCharacters(in: .whitespaces).isEmpty)
                        Button("清除 Key") {
                            onKeyChange?(nil)
                            keyIsSet = false
                        }
                    }
                }
            }
            Section {
                Button("测试连接（发送一次探测请求）") {
                    onTest?(Config(provider: provider, baseURL: baseURL, model: model), apiKey) { _ in }
                }
                .disabled(model.trimmingCharacters(in: .whitespaces).isEmpty)
                Text("测试会向上方端点发送一次模型探测请求；建索引会把笔记**全文**发送给该端点。本机 Ollama 不出网。")
                    .font(.caption).foregroundStyle(.secondary)
            }
        }
        .padding()
        .frame(minWidth: 420)
        .accessibilityIdentifier("notebook.embedding")
    }
}

// MARK: - 5. StorageConnectionEditor

/// 存储连接编辑器：公开配置与凭据分字段（凭据传一次后清内存）。
/// 必填键由服务端按协议校验——保存失败不污染旧配置。
public struct StorageConnectionEditor: View {
    public var protocols: [String]
    @State private var name = ""
    @State private var protocolName = "webdavHttps"
    @State private var endpoint = ""
    @State private var username = ""
    @State private var password = ""
    public var onSave: ((_ name: String, _ protocolName: String, _ config: [String: String], _ secrets: [String: String]) -> Void)?
    public var onCancel: () -> Void

    public init(
        protocols: [String] = ["s3", "webdavHttps", "dropbox", "box", "baiduNetdisk", "smb"],
        onSave: ((_ name: String, _ protocolName: String, _ config: [String: String], _ secrets: [String: String]) -> Void)? = nil,
        onCancel: @escaping () -> Void = {}
    ) {
        self.protocols = protocols
        self.onSave = onSave
        self.onCancel = onCancel
    }

    public var body: some View {
        Form {
            TextField("连接名称", text: $name)
            Picker("协议", selection: $protocolName) {
                ForEach(protocols, id: \.self) { Text($0).tag($0) }
            }
            TextField("端点 / 主机", text: $endpoint)
            if protocolName != "s3" {
                TextField("用户名", text: $username)
                SecureField("密码 / 应用密码（传一次后清内存）", text: $password)
            }
            HStack {
                Button("取消", action: onCancel)
                    .keyboardShortcut(.cancelAction)
                Button("保存") {
                    var config: [String: String] = [:]
                    config["endpoint"] = endpoint
                    config["host"] = endpoint
                    var secrets: [String: String] = [:]
                    secrets["username"] = username
                    secrets["password"] = password
                    onSave?(name, protocolName, config, secrets)
                    password = ""
                }
                .keyboardShortcut(.defaultAction)
                .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            Text("凭据单独存放（0600），不会出现在连接清单里。")
                .font(.caption).foregroundStyle(.secondary)
        }
        .padding()
        .frame(minWidth: 400)
        .accessibilityIdentifier("notebook.storage")
    }
}
