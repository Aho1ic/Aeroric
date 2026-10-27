import AeroricClient
import AeroricProtocol
@testable import AeroricUI
import XCTest

@MainActor
private final class FakeBackend: AgentConfigBackend {
    var document = AgentConnectionsClient.Document()
    var credentials: [String: String] = [:]
    var savedCredential: String??
    var fetchCount = 0
    var stripped: [String] = []
    /// Fail `load` this many times with `.notConnected` first.
    var notConnectedLoads = 0
    func reasoningStripped() async throws -> [String] { stripped }

    private func snap() -> ACC.Snapshot { .init(document: document, credentials: credentials) }

    func load() async throws -> ACC.Snapshot {
        if notConnectedLoads > 0 {
            notConnectedLoads -= 1
            throw ConnectionError.notConnected
        }
        return snap()
    }
    func putConnection(_ value: ACC.Connection, credential: String?) async throws -> ACC.Snapshot {
        savedCredential = .some(credential)
        if let credential { credentials[value.id] = credential.isEmpty ? "missing" : "stored" }
        // Like the host: a changed model list bumps the catalog revision.
        var stored = value
        if let old = document.connections.first(where: { $0.id == value.id }) {
            stored.catalogRevision = old.models == value.models ? old.catalogRevision : old.catalogRevision + 1
        }
        document.connections.removeAll { $0.id == value.id }
        document.connections.append(stored)
        return snap()
    }
    func deleteConnection(id: String) async throws -> ACC.Snapshot {
        document.connections.removeAll { $0.id == id }
        return snap()
    }
    func putPool(_ value: ACC.Pool) async throws -> ACC.Snapshot {
        document.pools.removeAll { $0.id == value.id }
        document.pools.append(value)
        return snap()
    }
    func deletePool(id: String) async throws -> ACC.Snapshot { snap() }
    func putConfig(_ value: ACC.AgentConfig) async throws -> ACC.Snapshot {
        document.configs.removeAll { $0.id == value.id }
        document.configs.append(value)
        return snap()
    }
    func deleteConfig(id: String) async throws -> ACC.Snapshot { snap() }
    var draftFetchKey: String??
    func fetchModels(draft: ACC.Connection, credential: String?) async throws -> ACC.DraftFetch {
        fetchCount += 1
        draftFetchKey = .some(credential)
        var merged = draft
        merged.models.append(.init(upstreamId: "fetched-\(fetchCount)"))
        let listed = draft.models.map(\.upstreamId) + ["fetched-\(fetchCount)"]
        return .init(connection: merged, models: listed, fetched: listed.count, added: 1, kept: 0)
    }
    func fetchModels(connectionId: String) async throws -> ACC.FetchResult {
        fetchCount += 1
        if let index = document.connections.firstIndex(where: { $0.id == connectionId }) {
            document.connections[index].models.append(.init(upstreamId: "fetched-\(fetchCount)"))
        }
        return .init(snapshot: snap(), fetched: 1, added: 1, kept: 0)
    }
    func candidates(configId: String, model: String?) async throws -> ACC.Candidates {
        .init(models: [], model: "", channels: [], excluded: [])
    }
    func status() async throws -> [ACC.Cooldown] { [] }
    func officialStatus() async throws -> ACC.OfficialStatus {
        .init(codex: .init(installed: true, signedIn: false, mode: "none", account: nil, sources: []),
              claude: .init(installed: false, signedIn: false, mode: "none", account: nil, sources: []))
    }
    func testConnection(connectionId: String, model: String) async throws -> ACC.Probe {
        .init(protocol: "chat_completions", elapsedMs: 12, status: 200, excerpt: "pong")
    }
}

@MainActor
final class AgentConfigStoreTests: XCTestCase {
    func testADraftIsDirtyUntilSavedAndAKeyAloneCountsAsAnEdit() async {
        let backend = FakeBackend()
        let store = AgentConfigStore(backend: backend)
        await store.refresh()
        store.newConnection()
        XCTAssertTrue(store.connectionDirty, "a new connection is unsaved")
        store.connectionDraft?.label = "DeepSeek"
        store.connectionDraft?.baseUrl = " https://api.deepseek.com/v1 "
        store.credentialDraft = "sk-1"
        await store.saveConnection()
        XCTAssertFalse(store.connectionDirty)
        XCTAssertEqual(store.document.connections.first?.baseUrl, "https://api.deepseek.com/v1")
        XCTAssertEqual(backend.savedCredential, .some("sk-1"))
        XCTAssertEqual(store.credentialDraft, "", "the typed key is not kept around")
        store.credentialDraft = "sk-2"
        XCTAssertTrue(store.connectionDirty)
        store.credentialDraft = ""
        await store.saveConnection()
        XCTAssertEqual(backend.savedCredential, .some(nil), "an empty field leaves the stored key alone")
    }

    func testAConnectionInUseCannotBeDeletedAndSaysWhy() async {
        let backend = FakeBackend()
        backend.document.connections = [.init(id: "c1", label: "甲")]
        var pool = AgentConnectionsClient.Pool(id: "p1", label: "池")
        pool.members = [.init(connectionId: "c1")]
        backend.document.pools = [pool]
        let store = AgentConfigStore(backend: backend)
        await store.refresh()
        store.selectConnection("c1")
        await store.deleteConnection()
        XCTAssertEqual(store.errorText, "还在被使用，不能删除：号池“池”")
        XCTAssertEqual(backend.document.connections.count, 1)
    }

    func testFetchingModelsRefreshesTheDraftAndTesting() async {
        let backend = FakeBackend()
        var saved = AgentConnectionsClient.Connection(id: "c1", label: "甲")
        saved.baseUrl = "https://api.example.com/v1"
        backend.document.connections = [saved]
        backend.credentials = ["c1": "stored"]
        let store = AgentConfigStore(backend: backend)
        await store.refresh()
        store.selectConnection("c1")
        XCTAssertTrue(store.canFetchModels, "a stored key is enough")
        await store.fetchModels()
        XCTAssertEqual(store.modelPick?.available, ["fetched-1"])
        store.modelPick?.selectAllVisible()
        store.applyModelPick()
        XCTAssertNil(store.modelPick)
        XCTAssertEqual(store.connectionDraft?.models.map(\.upstreamId), ["fetched-1"])
        XCTAssertEqual(store.connectionDraft?.models.first?.source, "fetched")
        XCTAssertEqual(backend.draftFetchKey, .some(nil), "empty field: the host uses the stored key")
        XCTAssertTrue(store.connectionDirty, "fetched models are an unsaved edit")
        await store.testConnection(model: "fetched-1")
        XCTAssertEqual(store.probe, "成功 · Chat Completions · 12 ms · pong")
    }

    /// 2026-10-26: 拉取列表 is ready as soon as a base URL and a key are typed,
    /// without saving first.
    func testANewConnectionCanFetchBeforeItIsSaved() async {
        let backend = FakeBackend()
        let store = AgentConfigStore(backend: backend)
        await store.refresh()
        store.newConnection()
        XCTAssertFalse(store.canFetchModels)
        store.connectionDraft?.baseUrl = "https://api.example.com/v1"
        XCTAssertFalse(store.canFetchModels, "no key yet")
        store.credentialDraft = "sk-typed"
        XCTAssertTrue(store.canFetchModels)
        await store.fetchModels()
        XCTAssertEqual(backend.draftFetchKey, .some("sk-typed"))
        XCTAssertEqual(store.connectionDraft?.models, [], "nothing is added until the user picks")
        store.modelPick?.toggle("fetched-1")
        store.applyModelPick()
        XCTAssertEqual(store.connectionDraft?.models.map(\.upstreamId), ["fetched-1"])
        XCTAssertTrue(backend.document.connections.isEmpty, "nothing was saved")
    }

    func testRoutesGroupChannelsByMappedName() {
        var a = AgentConnectionsClient.Connection(id: "a", label: "甲")
        a.protocols = ["chat_completions"]
        var model = AgentConnectionsClient.ModelEntry(upstreamId: "ds-chat")
        model.mappedName = "deepseek-v4.1-flash"
        a.models = [model, .init(upstreamId: "other")]
        var b = AgentConnectionsClient.Connection(id: "b", label: "乙")
        b.protocols = ["messages"]
        var bm = AgentConnectionsClient.ModelEntry(upstreamId: "deepseek-v4.1-flash")
        bm.protocols = ["responses"]
        b.models = [bm]
        var document = AgentConnectionsClient.Document()
        document.connections = [a, b]
        let routes = AgentConfigText.routes(document)
        XCTAssertEqual(routes.map(\.name), ["deepseek-v4.1-flash", "other"])
        XCTAssertEqual(routes[0].channels.map(\.connectionLabel), ["甲", "乙"])
        XCTAssertEqual(routes[0].channels[1].protocols, ["responses"], "a model override wins")
    }

    func testANewConfigKeepsOnlyTheFieldsItsTargetUses() async {
        let backend = FakeBackend()
        let store = AgentConfigStore(backend: backend)
        await store.refresh()
        store.newConfig()
        XCTAssertEqual(store.configDraft?.label, "", "the user names it")
        store.configDraft?.label = "我的配置"
        store.configDraft?.target = .init(kind: "connection", connectionId: "c1", poolId: "stale")
        await store.saveConfig()
        XCTAssertEqual(backend.document.configs.first?.target, .init(kind: "connection", connectionId: "c1"))
        XCTAssertFalse(store.configDirty)
    }

    // MARK: - R8 fallback models

    func testFallbackChoicesSkipTheDefaultAndChosenModels() throws {
        var a = AgentConnectionsClient.Connection(id: "a", label: "甲")
        a.models = ["main", "b1", "b2"].map { AgentConnectionsClient.ModelEntry(upstreamId: $0) }
        var document = AgentConnectionsClient.Document()
        document.connections = [a]
        var config = AgentConnectionsClient.AgentConfig(id: "c", label: "c", harness: "claude", target: .init(kind: "route"))
        config.defaultModel = "main"
        config.fallbacks = ["b2"]
        XCTAssertEqual(AgentConfigText.fallbackChoices(for: config, in: document), ["b1"])
        XCTAssertTrue(AgentConfigText.supportsFallback("codex"))
        XCTAssertFalse(AgentConfigText.supportsFallback("omp"))

        config.fallbacks = []
        XCTAssertNil(config.fallbackModels, "an empty list is omitted on the wire")
        let wire = try AgentConnectionsClient.encode(config)
        XCTAssertNil(wire["fallback_models"])
        config.fallbacks = ["b1"]
        XCTAssertEqual(try AgentConnectionsClient.encode(config)["fallback_models"], .array([.string("b1")]))
    }

    func testFallbacksReorderWithinBounds() {
        XCTAssertEqual(AgentConfigText.moving(["a", "b", "c"], at: 2, by: -1), ["a", "c", "b"])
        XCTAssertEqual(AgentConfigText.moving(["a", "b", "c"], at: 0, by: -1), ["a", "b", "c"])
        XCTAssertEqual(AgentConfigText.moving(["a", "b", "c"], at: 1, by: 5), ["a", "c", "b"])
    }

    func testTheDowngradeBadgeNamesBothModelsAndTheReason() {
        let downgrade = AgentConnectionsClient.Downgrade(taskId: "t", requested: "sonnet", served: "deepseek-chat",
                                                         reason: "RateLimited", sinceEpochS: 1)
        XCTAssertEqual(AgentConfigText.downgradeBadge(downgrade), "降级 · deepseek-chat")
        XCTAssertEqual(AgentConfigText.downgradeHelp(downgrade),
                       "主模型 sonnet 不可用（限流），本轮由 deepseek-chat 应答；主模型恢复后自动切回。")
    }

    // MARK: - R7

    func testTheStoreKnowsWhichConnectionsGetReasoningStripped() async {
        let backend = FakeBackend()
        backend.stripped = ["a"]
        let store = AgentConfigStore(backend: backend)
        await store.refresh()
        XCTAssertEqual(store.reasoningStripped, ["a"])
        backend.stripped = []
        await store.refreshStatus()
        XCTAssertTrue(store.reasoningStripped.isEmpty)
    }

    /// Crash 2026-10-26: "新建连接 → 手动添加" read the draft inside its own
    /// mutation and aborted with an exclusivity conflict.
    func testAddingAModelByHandToANewConnection() async {
        let store = AgentConfigStore(backend: FakeBackend())
        await store.refresh()
        store.newConnection()
        store.addManualModel()
        store.addManualModel()
        store.connectionDraft?.models.remove(at: 0)
        store.addManualModel()
        XCTAssertEqual(store.connectionDraft?.models.map(\.upstreamId), ["model-2", "model-3"])
    }

    // MARK: - Visual polish (2026-10-26)

    func testConnectionStatusReadsWorstFirst() {
        XCTAssertEqual(AgentConfigText.connectionStatus(enabled: false, credential: "stored", cooling: true), .disabled)
        XCTAssertEqual(AgentConfigText.connectionStatus(enabled: true, credential: "missing", cooling: true), .missingKey)
        XCTAssertEqual(AgentConfigText.connectionStatus(enabled: true, credential: "stored", cooling: true), .cooling)
        XCTAssertEqual(AgentConfigText.connectionStatus(enabled: true, credential: "stored", cooling: false), .ready)
    }

    func testListAndRowTextsAreShortAndReadable() {
        var connection = AgentConnectionsClient.Connection(id: "c", label: "本机假服务")
        XCTAssertEqual(AgentConfigText.connectionSubtitle(connection), "还没有模型")
        connection.protocols = ["messages"]
        connection.models = [.init(upstreamId: "a"), .init(upstreamId: "b")]
        XCTAssertEqual(AgentConfigText.connectionSubtitle(connection), "Messages · 2 个模型")

        var model = AgentConnectionsClient.ModelEntry(upstreamId: "m")
        XCTAssertEqual(AgentConfigText.modelBadges(model), [])
        model.contextWindow = 128_000
        model.protocols = ["responses"]
        XCTAssertEqual(AgentConfigText.modelBadges(model), ["128K", "Responses"])
        XCTAssertEqual(AgentConfigText.contextTitle(1_000_000), "1M")
        XCTAssertTrue(AgentConfigText.probeSucceeded("成功 · Chat Completions · 12 ms · pong"))
        XCTAssertFalse(AgentConfigText.probeSucceeded("失败 · 401"))
    }

    /// 2026-10-26: reopening the App on this page loaded before the host was
    /// connected and showed "ConnectionError error 4".
    func testOpeningBeforeTheHostIsConnectedWaitsInsteadOfFailing() async {
        let backend = FakeBackend()
        backend.notConnectedLoads = 2
        backend.document.connections = [.init(id: "c1", label: "甲")]
        let store = AgentConfigStore(backend: backend)
        await store.refresh(every: .milliseconds(1))
        XCTAssertNil(store.errorText)
        XCTAssertEqual(store.document.connections.map(\.id), ["c1"])

        backend.notConnectedLoads = 5
        await store.refresh(retryWhileConnecting: 2, every: .milliseconds(1))
        XCTAssertEqual(store.errorText, "读取 Agent 配置失败：还没连上 Aeroric 宿主，稍后点右上角刷新")
    }

    func testANewConnectionStartsWithoutAName() async {
        let store = AgentConfigStore(backend: FakeBackend())
        await store.refresh()
        store.newConnection()
        XCTAssertEqual(store.connectionDraft?.label, "")
        store.connectionDraft?.baseUrl = "https://x/v1"
        await store.saveConnection()
        XCTAssertEqual(store.errorText, "先给连接起个名字")
    }

    // MARK: - Model picker (2026-10-26)

    func testThePickerSearchesFuzzilyAndSelectsWhatItShows() {
        var pick = AgentConfigStore.ModelPick(
            available: ["deepseek-v4.1-flash", "deepseek-v4.1-pro", "glm-5.3", "glm-5.3", "kimi-k3"],
            existing: ["glm-5.3"])
        XCTAssertEqual(pick.available.count, 4, "duplicates collapse")
        pick.query = "ds4fl"
        XCTAssertEqual(pick.visible, ["deepseek-v4.1-flash"])
        pick.query = "deepseek pro"
        XCTAssertEqual(pick.visible, ["deepseek-v4.1-pro"])
        pick.query = "glm"
        pick.selectAllVisible()
        XCTAssertTrue(pick.selected.isEmpty, "an already added model is not selected again")
        pick.query = ""
        pick.selectAllVisible()
        XCTAssertEqual(pick.selected, ["deepseek-v4.1-flash", "deepseek-v4.1-pro", "kimi-k3"])
        pick.query = "deep"
        pick.selectNoneVisible()
        XCTAssertEqual(pick.selected, ["kimi-k3"], "全不选 only clears what the search shows")
        pick.toggle("glm-5.3")
        XCTAssertEqual(pick.selected, ["kimi-k3"])
    }

    func testFuzzyMatch() {
        XCTAssertTrue(AgentConfigText.fuzzyMatch("", "anything"))
        XCTAssertTrue(AgentConfigText.fuzzyMatch("GPT55", "gpt-5.5"))
        XCTAssertFalse(AgentConfigText.fuzzyMatch("claude", "gpt-5.5"))
    }

    /// 2026-10-26: after adding models and saving, the save bar never went
    /// away — the host bumps `catalog_revision`, so the pre-save draft never
    /// matched the saved connection.
    func testSavingModelsClearsTheUnsavedState() async {
        let backend = FakeBackend()
        var saved = AgentConnectionsClient.Connection(id: "c1", label: "cline")
        saved.protocols = ["chat_completions"]
        backend.document.connections = [saved]
        let store = AgentConfigStore(backend: backend)
        await store.refresh()
        store.selectConnection("c1")
        store.addManualModel()
        XCTAssertTrue(store.connectionDirty)
        await store.saveConnection()
        XCTAssertNil(store.errorText)
        XCTAssertEqual(backend.document.connections.first?.catalogRevision, 1)
        XCTAssertFalse(store.connectionDirty, "saved: the bar must go away")
        XCTAssertEqual(store.connectionDraft?.models.map(\.upstreamId), ["model-1"])
    }
}
