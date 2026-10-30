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
    func clearCooldown(connectionId: String) async throws
    func loginGatewayStatus(harness: String) async throws -> ACC.LoginGatewayStatus
    func setLoginGateway(harness: String, enabled: Bool, accept: Bool) async throws -> Bool
    func officialStatus() async throws -> ACC.OfficialStatus
    func testConnection(connectionId: String, model: String) async throws -> ACC.Probe
    /// R7: connections the gateway strips replayed thinking for.
    func reasoningStripped() async throws -> [String]
    /// Fetch the model list for the unsaved editor draft.
    func fetchModels(draft: ACC.Connection, credential: String?) async throws -> ACC.DraftFetch
}

public extension AgentConfigBackend {
    func reasoningStripped() async throws -> [String] { [] }
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
    /// `冷却到 17:45 · 网络错误` (2026-10-29).
    /// 2026-10-30: the line under 「登录状态」.
    public static func loginTokenLine(_ status: AgentConnectionsClient.LoginGatewayStatus) -> String {
        if let problem = status.problem { return problem }
        guard let at = status.expiresAt else { return "过期前自动续期" }
        return "令牌有效到 \(at.formatted(date: .omitted, time: .shortened)) · 过期前自动续期"
    }

    /// D4: what the owner accepts before the login goes through the gateway.
    public static func loginGatewayConsent(_ harness: String) -> String {
        let vendor = harness == "Codex" ? "ChatGPT" : "Claude"
        let terms = harness == "Codex" ? "OpenAI" : "Anthropic"
        return "网关会读取你的 \(vendor) 登录令牌，代 \(harness) 把请求原样发给官方。\n"
            + "这可能不符合 \(terms) 的使用条款，账号有被限制的风险。\n"
            + "令牌只留在本机，随时可以关掉，回到直连。"
    }

    public static func coolingTag(until: String, reason: String) -> String {
        "冷却到 \(until) · \(reasonTitle(reason))"
    }

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

    /// R7-Q3: why a model may seem to forget its earlier train of thought here.
    public static let reasoningStrippedNote =
        "这个连接拒收过回放的思考内容，之后自动去掉。编辑连接后会重新尝试。"

    /// R8-Q4: more usually means the configuration needs rethinking.
    public static let maxFallbacks = 3

    /// R8: only harnesses that go through the gateway can fall back (Q32).
    public static func supportsFallback(_ harness: String) -> Bool {
        harness == "claude" || harness == "codex"
    }

    /// Models that can still be added as fallbacks: not the default, not chosen.
    public static func fallbackChoices(for config: ACC.AgentConfig, in document: ACC.Document) -> [String] {
        modelNames(for: config, in: document).filter { $0 != config.defaultModel && !config.fallbacks.contains($0) }
    }

    /// `list` with the entry at `index` moved by `offset` (clamped to the ends).
    public static func moving(_ list: [String], at index: Int, by offset: Int) -> [String] {
        guard list.indices.contains(index) else { return list }
        var list = list
        let item = list.remove(at: index)
        list.insert(item, at: min(max(index + offset, 0), list.count))
        return list
    }

    /// R8: the session page's downgrade badge and its explanation.
    public static func downgradeBadge(_ downgrade: ACC.Downgrade) -> String {
        "降级 · \(downgrade.served)"
    }

    public static func downgradeHelp(_ downgrade: ACC.Downgrade) -> String {
        let why = downgrade.reason.map { "（\(reasonTitle($0))）" } ?? ""
        return "主模型 \(downgrade.requested) 不可用\(why)，本轮由 \(downgrade.served) 应答；主模型恢复后自动切回。"
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
    public enum Tab: String, CaseIterable, Sendable { case connections, pools, routes, configs, upgrades }

    public var tab: Tab = .connections
    public private(set) var snapshot: ACC.Snapshot?
    public private(set) var cooldowns: [ACC.Cooldown] = []
    /// R7: connections that refused replayed thinking (in the gateway's memory).
    public private(set) var reasoningStripped: Set<String> = []
    public private(set) var official: ACC.OfficialStatus?
    /// 2026-10-30: per harness (`codex` / `claude`), the login through the gateway.
    public private(set) var loginGateway: [String: ACC.LoginGatewayStatus] = [:]
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

    /// The App can reopen straight onto this page before the host connection
    /// is up (2026-10-26: "ConnectionError error 4"). Not being connected yet
    /// is waited out quietly; only a real failure reaches the banner.
    public func refresh(retryWhileConnecting attempts: Int = 20,
                        every interval: Duration = .milliseconds(500)) async {
        for attempt in 0..<max(1, attempts) {
            do {
                let snapshot = try await backend.load()
                apply(snapshot)
                errorText = nil
                cooldowns = (try? await backend.status()) ?? []
                reasoningStripped = Set((try? await backend.reasoningStripped()) ?? [])
                return
            } catch ConnectionError.notConnected where attempt + 1 < attempts {
                try? await Task.sleep(for: interval)
                if Task.isCancelled { return }
            } catch {
                errorText = "读取 Agent 配置失败：\(Self.describe(error))"
                return
            }
        }
    }

    /// 立即恢复 (2026-10-29).
    public func clearCooldown(connectionId: String) async {
        await run("解除冷却失败") { [self] in try await backend.clearCooldown(connectionId: connectionId) }
        await refreshStatus()
    }

    public func refreshStatus() async {
        if let latest = try? await backend.status() { cooldowns = latest }
        if let latest = try? await backend.reasoningStripped() { reasoningStripped = Set(latest) }
    }

    public func refreshLoginGateway(_ harness: String) async {
        if let latest = try? await backend.loginGatewayStatus(harness: harness) { loginGateway[harness] = latest }
    }

    /// Returns true when the owner must see the terms-and-risk notice first.
    public func setLoginGateway(_ harness: String, enabled: Bool, accept: Bool = false) async -> Bool {
        var needsConsent = false
        await run(enabled ? "开启经网关使用失败" : "关闭经网关使用失败") { [self] in
            needsConsent = try await backend.setLoginGateway(harness: harness, enabled: enabled, accept: accept)
        }
        await refreshLoginGateway(harness)
        return needsConsent
    }

    public func detectOfficial() async {
        await run("检测本机官方配置失败") { [self] in official = try await backend.officialStatus() }
    }

    /// Called when the stored document changed (a save, delete or a refresh
    /// that found edits): the App reloads the new-session picker so a new
    /// connection shows up at once (2026-10-27).
    @ObservationIgnored public var onDocumentChange: (() -> Void)?

    private func apply(_ snapshot: ACC.Snapshot) {
        let changed = self.snapshot != nil && self.snapshot?.document != snapshot.document
        self.snapshot = snapshot
        if changed { onDocumentChange?() }
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
            errorText = "\(failure)：\(Self.describe(error))"
        }
    }

    /// Plain words instead of "ConnectionError error 4".
    static func describe(_ error: Error) -> String {
        switch error as? ConnectionError {
        case .refused(let frame)?: frame.error.message
        case .notConnected?, .disconnected?: "还没连上 Aeroric 宿主，稍后点右上角刷新"
        case .timedOut?: "宿主没有及时回应"
        case .io(let message)?: message
        default: error.localizedDescription
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

    /// Append a placeholder model to the connection draft. The name is worked
    /// out before the write: reading `connectionDraft` inside its own mutation
    /// is an exclusivity violation that aborts the app.
    public func addManualModel() {
        guard let draft = connectionDraft else { return }
        let taken = Set(draft.models.map(\.upstreamId))
        var number = draft.models.count + 1
        while taken.contains("model-\(number)") { number += 1 }
        connectionDraft?.models.append(ACC.ModelEntry(upstreamId: "model-\(number)"))
    }

    public func newConnection() {
        selectedConnection = nil
        // The name starts empty: the user names it (2026-10-26).
        var draft = ACC.Connection(id: AgentConfigText.newID("conn"), label: "")
        draft.protocols = ["chat_completions"]
        connectionDraft = draft
        credentialDraft = ""
        probe = nil
    }

    public func saveConnection() async {
        guard var draft = connectionDraft else { return }
        draft.label = draft.label.trimmingCharacters(in: .whitespaces)
        draft.baseUrl = draft.baseUrl.trimmingCharacters(in: .whitespaces)
        guard !draft.label.isEmpty else {
            errorText = "先给连接起个名字"
            return
        }
        guard !draft.protocols.isEmpty else {
            errorText = "至少选一个协议"
            return
        }
        let credential = credentialDraft.isEmpty ? nil : credentialDraft
        await run("保存连接失败") { [self] in
            let snapshot = try await backend.putConnection(draft, credential: credential)
            credentialDraft = ""
            selectedConnection = draft.id
            // Adopt what the host stored: it bumps `catalog_revision` when the
            // models change, so the pre-save copy never equals the saved one and
            // the save bar stayed up forever (2026-10-26).
            connectionDraft = snapshot.document.connections.first { $0.id == draft.id } ?? draft
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

    /// Ready once a base URL and a key (typed, or already stored) are there;
    /// saving first is not needed (cc-switch / magpie fetch from the form).
    public var canFetchModels: Bool {
        guard !busy, let draft = connectionDraft,
              !draft.baseUrl.trimmingCharacters(in: .whitespaces).isEmpty else { return false }
        let typed = !credentialDraft.trimmingCharacters(in: .whitespaces).isEmpty
        let stored = selectedConnection.flatMap { snapshot?.credentials[$0] } == "stored"
        return typed || stored
    }

    /// The fetched list waiting for the user to pick from (2026-10-26).
    public struct ModelPick: Equatable, Sendable {
        public var available: [String]
        /// Already in the draft: shown ticked, not added twice.
        public var existing: Set<String>
        public var selected: Set<String> = []
        public var query = ""

        public init(available: [String], existing: Set<String>) {
            var seen = Set<String>()
            self.available = available.filter { !$0.isEmpty && seen.insert($0).inserted }
            self.existing = existing
        }

        /// What the search shows, in the endpoint's order.
        public var visible: [String] { available.filter { AgentConfigText.fuzzyMatch(query, $0) } }
        public var addable: [String] { visible.filter { !existing.contains($0) } }

        /// 全选 / 全不选 act on what the search shows.
        public mutating func selectAllVisible() { selected.formUnion(addable) }
        public mutating func selectNoneVisible() { selected.subtract(visible) }
        public mutating func toggle(_ id: String) {
            guard !existing.contains(id) else { return }
            if selected.contains(id) { selected.remove(id) } else { selected.insert(id) }
        }
    }

    public var modelPick: ModelPick?

    /// Fetch with what is typed (no save needed), then let the user pick.
    public func fetchModels() async {
        guard canFetchModels, let draft = connectionDraft else {
            errorText = "先填 Base URL 和 API Key"
            return
        }
        let key = credentialDraft.trimmingCharacters(in: .whitespaces)
        await run("拉取模型列表失败") { [self] in
            let result = try await backend.fetchModels(draft: draft, credential: key.isEmpty ? nil : key)
            let ids = result.models.isEmpty ? result.connection.models.map(\.upstreamId) : result.models
            modelPick = ModelPick(available: ids,
                                  existing: Set(connectionDraft?.models.map(\.upstreamId) ?? []))
        }
    }

    /// Add the picked models to the draft (an unsaved edit; the save bar shows).
    public func applyModelPick() {
        guard let pick = modelPick else { return }
        modelPick = nil
        let new = pick.available.filter { pick.selected.contains($0) && !pick.existing.contains($0) }
        guard !new.isEmpty, connectionDraft != nil else { return }
        let entries = new.map { id -> ACC.ModelEntry in
            var entry = ACC.ModelEntry(upstreamId: id)
            entry.source = "fetched"
            return entry
        }
        connectionDraft?.models.append(contentsOf: entries)
        notice = "已添加 \(new.count) 个模型；记得保存"
    }

    public func testConnection(model: String) async {
        guard let id = selectedConnection else { return }
        probe = AgentConfigText.probeRunning
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
        poolDraft = ACC.Pool(id: AgentConfigText.newID("pool"), label: "")
    }

    public func savePool() async {
        guard var draft = poolDraft else { return }
        guard !draft.label.trimmingCharacters(in: .whitespaces).isEmpty else {
            errorText = "先给号池起个名字"
            return
        }
        draft.label = draft.label.trimmingCharacters(in: .whitespaces)
        for index in draft.members.indices {
            draft.members[index].weight = min(max(draft.members[index].weight, 1), 100)
        }
        await run("保存号池失败") { [self] in
            let snapshot = try await backend.putPool(draft)
            selectedPool = draft.id
            poolDraft = snapshot.document.pools.first { $0.id == draft.id } ?? draft
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
        configDraft = ACC.AgentConfig(id: AgentConfigText.newID("cfg"), label: "",
                                      harness: "codex", target: .init(kind: "route"))
    }

    public func saveConfig() async {
        guard var draft = configDraft else { return }
        guard !draft.label.trimmingCharacters(in: .whitespaces).isEmpty else {
            errorText = "先给配置起个名字"
            return
        }
        draft.label = draft.label.trimmingCharacters(in: .whitespaces)
        switch draft.target.kind {
        case "connection": draft.target.poolId = nil
        case "pool": draft.target.connectionId = nil
        default: draft.target.connectionId = nil
        }
        await run("保存 Agent 配置失败") { [self] in
            let snapshot = try await backend.putConfig(draft)
            selectedConfig = draft.id
            configDraft = snapshot.document.configs.first { $0.id == draft.id } ?? draft
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

extension AgentConfigBackend {
    /// Backends without the gateway (previews, tests) have nothing to clear.
    public func clearCooldown(connectionId: String) async throws {}
    public func loginGatewayStatus(harness: String) async throws -> ACC.LoginGatewayStatus { .init() }
    public func setLoginGateway(harness: String, enabled: Bool, accept: Bool) async throws -> Bool { false }
}
