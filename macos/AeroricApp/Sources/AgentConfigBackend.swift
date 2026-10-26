import AeroricClient
import AeroricUI
import Foundation

/// G: the Agent configuration page's host calls. The client is looked up per
/// call because the connection is replaced when the host restarts.
@MainActor
struct HostAgentConfigBackend: AgentConfigBackend {
    weak var services: AppServices?

    private func client() throws -> AgentConnectionsClient {
        guard let client = services?.agentConnectionsClient else { throw AppServices.ProjectError.notConnected }
        return client
    }

    func load() async throws -> ACC.Snapshot { try await client().load() }
    func putConnection(_ value: ACC.Connection, credential: String?) async throws -> ACC.Snapshot {
        try await client().putConnection(value, credential: credential)
    }
    func deleteConnection(id: String) async throws -> ACC.Snapshot { try await client().deleteConnection(id: id) }
    func putPool(_ value: ACC.Pool) async throws -> ACC.Snapshot { try await client().putPool(value) }
    func deletePool(id: String) async throws -> ACC.Snapshot { try await client().deletePool(id: id) }
    func putConfig(_ value: ACC.AgentConfig) async throws -> ACC.Snapshot { try await client().putConfig(value) }
    func deleteConfig(id: String) async throws -> ACC.Snapshot { try await client().deleteConfig(id: id) }
    func fetchModels(connectionId: String) async throws -> ACC.FetchResult {
        try await client().fetchModels(connectionId: connectionId)
    }
    func candidates(configId: String, model: String?) async throws -> ACC.Candidates {
        try await client().candidates(configId: configId, model: model)
    }
    func status() async throws -> [ACC.Cooldown] { try await client().status() }
    func officialStatus() async throws -> ACC.OfficialStatus { try await client().officialStatus() }
    func testConnection(connectionId: String, model: String) async throws -> ACC.Probe {
        try await client().testConnection(connectionId: connectionId, model: model)
    }
}
