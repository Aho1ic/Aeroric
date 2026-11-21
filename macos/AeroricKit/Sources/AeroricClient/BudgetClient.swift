import AeroricProtocol
import Foundation

/// `budget.*` (E6, 2026-11-21): the unattended-work budget and 暂停所有 Agent.
public struct BudgetClient: Sendable {
    let connection: HostConnection

    public init(connection: HostConnection) {
        self.connection = connection
    }

    public func status() async throws -> LiveBudget {
        LiveBudget.decode(try await connection.request("budget.status", params: .object([:])).result)
    }

    /// Save; switching the pause on stops the turns running now.
    public func set(_ budget: LiveBudget) async throws -> LiveBudget {
        LiveBudget.decode(try await connection.request(
            "budget.set", params: .object(["settings": budget.settingsJSON])).result)
    }
}
