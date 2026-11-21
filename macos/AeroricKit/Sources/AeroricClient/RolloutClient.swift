import AeroricProtocol
import Foundation

/// `rollout.*` (H8, 2026-11-21): the tiers of the automatic behaviours.
public struct RolloutClient: Sendable {
    let connection: HostConnection

    public init(connection: HostConnection) {
        self.connection = connection
    }

    public func list() async throws -> [LiveRolloutFeature] {
        let frame = try await connection.request("rollout.list", params: .object([:]))
        return LiveRolloutFeature.decodeList(frame.result)
    }

    public func set(id: String, tier: String) async throws {
        _ = try await connection.request("rollout.set", params: .object(["id": .string(id), "tier": .string(tier)]))
    }

    /// Back to the tier before the last move, or one tier down.
    public func revert(id: String) async throws {
        _ = try await connection.request("rollout.revert", params: .object(["id": .string(id)]))
    }
}
