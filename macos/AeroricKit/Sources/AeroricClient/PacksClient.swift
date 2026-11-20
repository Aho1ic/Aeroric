import AeroricProtocol
import Foundation

/// `packs.*` (H6, 2026-11-21): the capability packs on the 能力包 page.
public struct PacksClient: Sendable {
    let connection: HostConnection

    public init(connection: HostConnection) {
        self.connection = connection
    }

    public func list() async throws -> LivePackOverview {
        let frame = try await connection.request("packs.list", params: .object([:]))
        return LivePackOverview.decode(frame.result)
    }

    /// Add or change a server of the user's own (a pack of its own).
    public func saveServer(_ server: LivePackServer) async throws {
        _ = try await connection.request("packs.saveServer", params: .object(["server": server.json]))
    }

    /// Copy a pack folder in; it waits for the user to confirm its source.
    public func importFolder(_ path: String) async throws {
        _ = try await connection.request("packs.import", params: .object(["path": .string(path)]))
    }

    public func setEnabled(id: String, enabled: Bool) async throws {
        _ = try await connection.request("packs.setEnabled",
                                         params: .object(["id": .string(id), "enabled": .bool(enabled)]))
    }

    public func setConfirmed(id: String, confirmed: Bool) async throws {
        _ = try await connection.request("packs.setConfirmed",
                                         params: .object(["id": .string(id), "confirmed": .bool(confirmed)]))
    }

    /// Moves the pack's folder to the Trash.
    public func remove(id: String) async throws {
        _ = try await connection.request("packs.remove", params: .object(["id": .string(id)]))
    }
}
