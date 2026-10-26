import AeroricClient
import Foundation
import Observation

/// What the Agent configuration page may ask of the host (G). The App
/// implements it on top of `AgentConnectionsClient`; the page never holds a
/// connection (INV-02).
@MainActor
public protocol AgentConfigBackend {
    typealias ACC = AgentConnectionsClient
    func load() async throws -> ACC.Snapshot
    func putConnection(_ value: ACC.Connection, credential: String?) async throws -> ACC.Snapshot
    func deleteConnection(id: String) async throws -> ACC.Snapshot
    func putPool(_ value: ACC.Pool) async throws -> ACC.Snapshot
    func deletePool(id: String) async throws -> ACC.Snapshot
    func putConfig(_ value: ACC.AgentConfig) async throws -> ACC.Snapshot
    func deleteConfig(id: String) async throws -> ACC.Snapshot
    func fetchModels(connectionId: String) async throws -> ACC.FetchResult
    func candidates(configId: String, model: String?) async throws -> ACC.Candidates
    func status() async throws -> [ACC.Cooldown]
    func officialStatus() async throws -> ACC.OfficialStatus
    func testConnection(connectionId: String, model: String) async throws -> ACC.Probe
}

/// Pure pieces of the page, kept apart so they can be tested.
public enum AgentConfigText {
    public typealias ACC = AgentConnectionsClient

    public static let protocols = ["responses", "messages", "chat_completions"]
    public static let harnesses = ["codex", "claude", "dsh", "omp"]
    public static let strategies = ["priority", "round_robin", "weighted_round_robin", "fill_first",
                                    "least_used", "free_first", "pace"]

    public static func harnessTitle(_ raw: String) -> String {
        switch raw {
        case "codex": "Codex"
        case "claude": "Claude Code"
        case "dsh": "DeepSeek Harness"
        case "omp": "oh-my-pi"
        default: raw
        }
    }

    public static func strategyTitle(_ raw: String) -> String {
        switch raw {
        case "priority": "按优先级"
        case "round_robin": "轮询"
        case "weighted_round_robin": "加权轮询"
        case "fill_first": "先用满"
        case "least_used": "最少使用"
        case "free_first": "免费优先"
        case "pace": "按节奏"
        default: raw
        }
    }

    /// The gateway's failure classes, as the pool page shows them.
    public static func reasonTitle(_ raw: String) -> String {
        switch raw {
        case "RateLimited": "限流"
        case "AuthFailed": "鉴权失败"
        case "PaymentRequired": "额度不足"
        case "NotFound": "模型不存在"
        case "ServerError": "服务错误"
        case "Network": "网络错误"
        case "FirstByteTimeout": "首包超时"
        default: raw
        }
    }

    public static func excludedTitle(_ code: String) -> String {
        switch code {
        case "connection_missing": "连接已删除"
        case "connection_disabled": "连接已停用"
        case "pool_missing": "号池已删除"
        case "no_protocol": "协议不兼容"
        case "gateway_not_yet_for_harness": "这个 Harness 暂不支持网关转换"
        case "model_not_offered": "没有这个模型"
        case "cooling_down": "冷却中"
        default: code
        }
    }

    public struct Route: Equatable, Sendable, Identifiable {
        public struct Channel: Equatable, Sendable, Hashable {
            public var connectionLabel: String
            public var upstreamId: String
            public var protocols: [String]
            public var enabled: Bool
        }
        public var name: String
        public var channels: [Channel]
        public var id: String { name }
    }

    /// Q25: a route is every channel whose catalog maps a model to one name.
    public static func routes(_ document: ACC.Document) -> [Route] {
        var byName: [String: [Route.Channel]] = [:]
        for connection in document.connections {
            for model in connection.models {
                let name = model.routingName
                guard !name.isEmpty else { continue }
                byName[name, default: []].append(.init(
                    connectionLabel: connection.label, upstreamId: model.upstreamId,
                    protocols: model.protocols.isEmpty ? connection.protocols : model.protocols,
                    enabled: connection.enabled && model.enabled))
            }
        }
        return byName.keys.sorted().map { Route(name: $0, channels: byName[$0] ?? []) }
    }

    /// G4: who still uses a connection or a pool (labels), so deleting it can
    /// explain why the host refuses.
    public static func references(toConnection id: String, in document: ACC.Document) -> [String] {
        document.pools.filter { $0.members.contains { $0.connectionId == id } }.map { "号池“\($0.label)”" }
            + document.configs.filter { $0.target.connectionId == id }.map { "Agent 配置“\($0.label)”" }
    }

    public static func references(toPool id: String, in document: ACC.Document) -> [String] {
        document.configs.filter { $0.target.poolId == id }.map { "Agent 配置“\($0.label)”" }
    }

    /// Every mapped name a config could use, for its default-model menu.
    public static func modelNames(for config: ACC.AgentConfig, in document: ACC.Document) -> [String] {
        let members: Set<String>? = switch config.target.kind {
        case "connection": config.target.connectionId.map { Set([$0]) }
        case "pool": document.pools.first { $0.id == config.target.poolId }.map { Set($0.members.map(\.connectionId)) }
        default: nil
        }
        let names = document.connections
            .filter { $0.enabled && (members?.contains($0.id) ?? true) }
            .flatMap { $0.models.filter(\.enabled).map(\.routingName) }
        return Array(Set(names)).filter { !$0.isEmpty }.sorted()
    }

    public static func newID(_ prefix: String) -> String {
        "\(prefix)-\(UUID().uuidString.prefix(8).lowercased())"
    }
}

/// The Agent configuration page's state: one snapshot of the host document,
/// one draft per tab, and the live status around them. Saving is explicit
/// (G3): a draft that differs from what is saved is "dirty".
@Observable
@MainActor
public final class AgentConfigStore {
    public typealias ACC = AgentConnectionsClient
    public enum Tab: String, CaseIterable, Sendable { case connections, pools, routes, configs }

    public var tab: Tab = .connections
    public private(set) var snapshot: ACC.Snapshot?
    public private(set) var cooldowns: [ACC.Cooldown] = []
    public private(set) var official: ACC.OfficialStatus?
    public private(set) var busy = false
    public var errorText: String?
    public var notice: String?

    public private(set) var selectedConnection: String?
    public var connectionDraft: ACC.Connection?
    /// Typed key; empty leaves the stored one alone.
    public var credentialDraft = ""
    public private(set) var probe: String?

    public private(set) var selectedPool: String?
    public var poolDraft: ACC.Pool?

    public private(set) var selectedConfig: String?
    public var configDraft: ACC.AgentConfig?
    public private(set) var candidates: ACC.Candidates?

    @ObservationIgnored private let backend: any AgentConfigBackend

    public init(backend: any AgentConfigBackend) { self.backend = backend }

    public var document: ACC.Document { snapshot?.document ?? .init() }

    // MARK: Loading

    public func refresh() async {
        await run("读取 Agent 配置失败") { [self] in
            apply(try await backend.load())
            cooldowns = (try? await backend.status()) ?? []
        }
    }

    public func refreshStatus() async {
        if let latest = try? await backend.status() { cooldowns = latest }
    }

    public func detectOfficial() async {
        await run("检测本机官方配置失败") { [self] in official = try await backend.officialStatus() }
    }

    private func apply(_ snapshot: ACC.Snapshot) {
        self.snapshot = snapshot
        // Keep drafts that are being edited; refresh clean ones.
        if let id = selectedConnection, !connectionDirty {
            connectionDraft = snapshot.document.connections.first { $0.id == id }
        }
        if let id = selectedPool, !poolDirty { poolDraft = snapshot.document.pools.first { $0.id == id } }
        if let id = selectedConfig, !configDirty { configDraft = snapshot.document.configs.first { $0.id == id } }
    }

    private func run(_ failure: String, _ body: () async throws -> Void) async {
        busy = true
        defer { busy = false }
        do {
            try await body()
            errorText = nil
        } catch {
            errorText = "\(failure)：\(error.localizedDescription)"
        }
    }

    // MARK: Connections

    public var savedConnection: ACC.Connection? {
        selectedConnection.flatMap { id in document.connections.first { $0.id == id } }
    }

    public var connectionDirty: Bool {
        guard let draft = connectionDraft else { return false }
        return draft != savedConnection || !credentialDraft.isEmpty
    }

    public func selectConnection(_ id: String?) {
        selectedConnection = id
        connectionDraft = id.flatMap { id in document.connections.first { $0.id == id } }
        credentialDraft = ""
        probe = nil
    }

    public func newConnection() {
        selectedConnection = nil
        var draft = ACC.Connection(id: AgentConfigText.newID("conn"), label: "新连接")
        draft.protocols = ["chat_completions"]
        connectionDraft = draft
        credentialDraft = ""
        probe = nil
    }

    public func saveConnection() async {
        guard var draft = connectionDraft else { return }
        draft.label = draft.label.trimmingCharacters(in: .whitespaces)
        draft.baseUrl = draft.baseUrl.trimmingCharacters(in: .whitespaces)
        guard !draft.label.isEmpty, !draft.protocols.isEmpty else {
            errorText = "名称和至少一个协议不能为空"
            return
        }
        let credential = credentialDraft.isEmpty ? nil : credentialDraft
        await run("保存连接失败") { [self] in
            let snapshot = try await backend.putConnection(draft, credential: credential)
            credentialDraft = ""
            selectedConnection = draft.id
            connectionDraft = draft
            apply(snapshot)
            notice = "已保存“\(draft.label)”"
        }
    }

    public func deleteConnection() async {
        guard let id = selectedConnection else { connectionDraft = nil; return }
        let users = AgentConfigText.references(toConnection: id, in: document)
        guard users.isEmpty else {
            errorText = "还在被使用，不能删除：" + users.joined(separator: "、")
            return
        }
        await run("删除连接失败") { [self] in
            let snapshot = try await backend.deleteConnection(id: id)
            selectConnection(nil)
            apply(snapshot)
        }
    }

    public func fetchModels() async {
        guard let id = selectedConnection else {
            errorText = "先保存连接，再拉取模型列表"
            return
        }
        await run("拉取模型列表失败") { [self] in
            let result = try await backend.fetchModels(connectionId: id)
            connectionDraft = nil
            apply(result.snapshot)
            connectionDraft = result.snapshot.document.connections.first { $0.id == id }
            notice = "拉取到 \(result.fetched) 个模型，新增 \(result.added) 个，保留 \(result.kept) 个"
        }
    }

    public func testConnection(model: String) async {
        guard let id = selectedConnection else { return }
        probe = "测试中…"
        do {
            let result = try await backend.testConnection(connectionId: id, model: model)
            let verdict = (200..<300).contains(Int(result.status)) ? "成功" : "失败（HTTP \(result.status)）"
            probe = "\(verdict) · \(AgentConnectionsClient.protocolTitle(result.protocol)) · \(result.elapsedMs) ms"
                + (result.excerpt.isEmpty ? "" : " · \(result.excerpt)")
        } catch {
            probe = "失败：\(error.localizedDescription)"
        }
    }

    public func cooldowns(forConnection id: String) -> [ACC.Cooldown] {
        let now = UInt64(Date().timeIntervalSince1970)
        return cooldowns.filter { $0.connectionId == id && $0.untilEpochS > now }
    }

    // MARK: Pools

    public var poolDirty: Bool {
        guard let draft = poolDraft else { return false }
        return draft != selectedPool.flatMap { id in document.pools.first { $0.id == id } }
    }

    public func selectPool(_ id: String?) {
        selectedPool = id
        poolDraft = id.flatMap { id in document.pools.first { $0.id == id } }
    }

    public func newPool() {
        selectedPool = nil
        poolDraft = ACC.Pool(id: AgentConfigText.newID("pool"), label: "新号池")
    }

    public func savePool() async {
        guard var draft = poolDraft else { return }
        draft.label = draft.label.trimmingCharacters(in: .whitespaces)
        for index in draft.members.indices {
            draft.members[index].weight = min(max(draft.members[index].weight, 1), 100)
        }
        await run("保存号池失败") { [self] in
            let snapshot = try await backend.putPool(draft)
            selectedPool = draft.id
            poolDraft = draft
            apply(snapshot)
            notice = "已保存“\(draft.label)”"
        }
    }

    public func deletePool() async {
        guard let id = selectedPool else { poolDraft = nil; return }
        let users = AgentConfigText.references(toPool: id, in: document)
        guard users.isEmpty else {
            errorText = "还在被使用，不能删除：" + users.joined(separator: "、")
            return
        }
        await run("删除号池失败") { [self] in
            let snapshot = try await backend.deletePool(id: id)
            selectPool(nil)
            apply(snapshot)
        }
    }

    // MARK: Configs

    public var configDirty: Bool {
        guard let draft = configDraft else { return false }
        return draft != selectedConfig.flatMap { id in document.configs.first { $0.id == id } }
    }

    public func selectConfig(_ id: String?) async {
        selectedConfig = id
        configDraft = id.flatMap { id in document.configs.first { $0.id == id } }
        candidates = nil
        if let id {
            candidates = try? await backend.candidates(configId: id, model: nil)
        }
    }

    public func newConfig() {
        selectedConfig = nil
        candidates = nil
        configDraft = ACC.AgentConfig(id: AgentConfigText.newID("cfg"), label: "新 Agent 配置",
                                      harness: "codex", target: .init(kind: "route"))
    }

    public func saveConfig() async {
        guard var draft = configDraft else { return }
        draft.label = draft.label.trimmingCharacters(in: .whitespaces)
        switch draft.target.kind {
        case "connection": draft.target.poolId = nil
        case "pool": draft.target.connectionId = nil
        default: draft.target.connectionId = nil
        }
        await run("保存 Agent 配置失败") { [self] in
            let snapshot = try await backend.putConfig(draft)
            selectedConfig = draft.id
            configDraft = draft
            apply(snapshot)
            candidates = try? await backend.candidates(configId: draft.id, model: nil)
            notice = "已保存“\(draft.label)”"
        }
    }

    public func deleteConfig() async {
        guard let id = selectedConfig else { configDraft = nil; return }
        await run("删除 Agent 配置失败") { [self] in
            let snapshot = try await backend.deleteConfig(id: id)
            await selectConfig(nil)
            apply(snapshot)
        }
    }
}

extension AgentConnectionsClient: AgentConfigBackend {}
