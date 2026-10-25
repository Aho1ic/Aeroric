import AeroricProtocol
import Foundation

/// A14: whether the host is keeping the Mac awake for running tasks, and the
/// switch for it. Typed over the authenticated UDS (INV-02).
public struct PowerClient: Sendable {
    public static let capability = "power.keepAwake"

    public struct State: Sendable, Equatable {
        public var enabled: Bool
        public var active: Bool
        public var activeTasks: Int

        public init(enabled: Bool, active: Bool, activeTasks: Int) {
            self.enabled = enabled
            self.active = active
            self.activeTasks = activeTasks
        }
    }

    public enum MalformedReply: Error { case malformed(String) }

    let connection: HostConnection

    public init(connection: HostConnection) {
        self.connection = connection
    }

    public func state() async throws -> State {
        let frame = try await connection.request("power.keepAwake.state", params: .null)
        return try Self.decode(frame.result)
    }

    public func setEnabled(_ enabled: Bool) async throws {
        _ = try await connection.request("settings.save_keep_awake_while_running",
                                         params: .object(["enabled": .bool(enabled)]))
    }

    public static func decode(_ value: JSONValue) throws -> State {
        guard let enabled = value["enabled"]?.boolValue, let active = value["active"]?.boolValue,
              case let .number(raw)? = value["activeTasks"], let tasks = Int(raw) else {
            throw MalformedReply.malformed("power.keepAwake.state needs enabled, active and activeTasks")
        }
        return State(enabled: enabled, active: active, activeTasks: tasks)
    }
}
