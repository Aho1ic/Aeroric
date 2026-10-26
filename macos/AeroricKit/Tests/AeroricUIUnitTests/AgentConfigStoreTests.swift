import AeroricClient
@testable import AeroricUI
import XCTest

@MainActor
private final class FakeBackend: AgentConfigBackend {
    var document = AgentConnectionsClient.Document()
    var credentials: [String: String] = [:]
    var savedCredential: String??
    var fetchCount = 0

    private func snap() -> ACC.Snapshot { .init(document: document, credentials: credentials) }

    func load() async throws -> ACC.Snapshot { snap() }
    func putConnection(_ value: ACC.Connection, credential: String?) async throws -> ACC.Snapshot {
        savedCredential = .some(credential)
        if let credential { credentials[value.id] = credential.isEmpty ? "missing" : "stored" }
        document.connections.removeAll { $0.id == value.id }
        document.connections.append(value)
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
        backend.document.connections = [.init(id: "c1", label: "甲")]
        let store = AgentConfigStore(backend: backend)
        await store.refresh()
        store.selectConnection("c1")
        await store.fetchModels()
        XCTAssertEqual(store.connectionDraft?.models.map(\.upstreamId), ["fetched-1"])
        XCTAssertFalse(store.connectionDirty)
        await store.testConnection(model: "fetched-1")
        XCTAssertEqual(store.probe, "成功 · Chat Completions · 12 ms · pong")
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
        store.configDraft?.target = .init(kind: "connection", connectionId: "c1", poolId: "stale")
        await store.saveConfig()
        XCTAssertEqual(backend.document.configs.first?.target, .init(kind: "connection", connectionId: "c1"))
        XCTAssertFalse(store.configDirty)
    }
}
