import AeroricProtocol
import Foundation

/// `github.*` (D1, 2026-11-21): PR snapshots and their read-only reviews.
/// Nothing here writes to GitHub.
public struct GitHubClient: Sendable {
    let connection: HostConnection

    public init(connection: HostConnection) {
        self.connection = connection
    }

    /// Fetch the PR now and pin it (the same head again returns the one kept).
    public func pullSnapshot(projectId: String, url: String) async throws -> LivePullSnapshot? {
        let frame = try await connection.request(
            "github.pullSnapshot", params: .object(["projectId": .string(projectId), "url": .string(url)]))
        return frame.result["snapshot"].flatMap(LivePullSnapshot.decode)
    }

    public func snapshots(projectId: String) async throws -> [LivePullSnapshot] {
        let frame = try await connection.request("github.snapshots", params: .object(["projectId": .string(projectId)]))
        return (frame.result["snapshots"]?.arrayValue ?? []).compactMap(LivePullSnapshot.decode)
    }

    public func reviewStart(projectId: String, snapshotId: String, agent: String) async throws {
        _ = try await connection.request(
            "github.reviewStart",
            params: .object(["projectId": .string(projectId), "snapshotId": .string(snapshotId),
                             "agent": .string(agent)]))
    }

    public func reviews(projectId: String) async throws -> [LivePullReview] {
        let frame = try await connection.request("github.reviews", params: .object(["projectId": .string(projectId)]))
        return (frame.result["reviews"]?.arrayValue ?? []).compactMap(LivePullReview.decode)
    }
}
