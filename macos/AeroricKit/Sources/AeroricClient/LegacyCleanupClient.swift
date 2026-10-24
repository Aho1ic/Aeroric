import AeroricProtocol
import Foundation

/// What the legacy cleanup screen needs from the host. The store depends on this,
/// not on a connection, so its rules are testable without a socket.
public protocol LegacyCleanupServing: Sendable {
    func plan() async throws -> LegacyCleanupReport
    func apply(sourceIDs: [String]) async throws -> LegacyCleanupReport
}

/// `agentConnections.legacyCleanup.*` over the authenticated UDS. Every decision
/// (eligibility, order, partial/resume) is the host's; this only sends the user's
/// explicit selection and decodes the reply strictly.
public struct LegacyCleanupClient: LegacyCleanupServing {
    let connection: HostConnection

    public init(connection: HostConnection) {
        self.connection = connection
    }

    public func plan() async throws -> LegacyCleanupReport {
        let frame = try await connection.request(LegacyCleanupContract.planMethod, params: .object([:]))
        return try Self.decode(frame.result)
    }

    public func apply(sourceIDs: [String]) async throws -> LegacyCleanupReport {
        let ids = try Self.validateSourceIDs(sourceIDs)
        let frame = try await connection.request(
            LegacyCleanupContract.applyMethod, params: legacyCleanupApplyParams(ids)
        )
        return try Self.decode(frame.result)
    }

    /// Same rules the host enforces, checked before anything is sent: non-empty,
    /// `custom:<id>` only, duplicates dropped in order, bounded.
    public static func validateSourceIDs(_ ids: [String]) throws -> [String] {
        guard !ids.isEmpty else { throw InvalidSelection("select at least one legacy Agent") }
        var unique: [String] = []
        for id in ids {
            let name = id.hasPrefix(LegacyCleanupContract.sourcePrefix)
                ? id.dropFirst(LegacyCleanupContract.sourcePrefix.count) : ""
            guard !name.trimmingCharacters(in: .whitespaces).isEmpty else {
                throw InvalidSelection("\(id) is not a custom Agent source")
            }
            if !unique.contains(id) { unique.append(id) }
        }
        guard unique.count <= LegacyCleanupContract.maxSourceIDs else {
            throw InvalidSelection("at most \(LegacyCleanupContract.maxSourceIDs) Agents per cleanup")
        }
        return unique
    }

    static func decode(_ value: JSONValue) throws -> LegacyCleanupReport {
        do {
            return try LegacyCleanupReport.decode(value)
        } catch let error as LegacyCleanupDecodeError {
            throw WorkspaceClient.MalformedReply("invalid legacy cleanup reply: \(error.reason)")
        }
    }

    public struct InvalidSelection: Error, Equatable {
        public let reason: String
        public init(_ reason: String) { self.reason = reason }
    }
}
