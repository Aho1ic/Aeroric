import AeroricProtocol
import Foundation

/// `github.*` (D1, 2026-11-21): PR snapshots and their read-only reviews.
/// Only `publish` writes to GitHub (D2), and only on the user's click.
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

    /// D2: the review's publishing draft (one per review).
    public func writeDraft(projectId: String, reviewId: String) async throws {
        _ = try await connection.request(
            "github.writeDraft", params: .object(["projectId": .string(projectId), "reviewId": .string(reviewId)]))
    }

    public func writes(projectId: String) async throws -> [LiveWriteIntent] {
        let frame = try await connection.request("github.writes", params: .object(["projectId": .string(projectId)]))
        return (frame.result["writes"]?.arrayValue ?? []).compactMap(LiveWriteIntent.decode)
    }

    public func writeEdit(projectId: String, id: String, itemId: String, body: String?, included: Bool?) async throws {
        var params: [String: JSONValue] = ["projectId": .string(projectId), "id": .string(id), "itemId": .string(itemId)]
        if let body { params["body"] = .string(body) }
        if let included { params["included"] = .bool(included) }
        _ = try await connection.request("github.writeEdit", params: .object(params))
    }

    /// The only write to GitHub; the user's click.
    public func publish(projectId: String, id: String, resendUnknown: Bool) async throws {
        _ = try await connection.request(
            "github.publish",
            params: .object(["projectId": .string(projectId), "id": .string(id), "resendUnknown": .bool(resendUnknown)]))
    }

    public func reviews(projectId: String) async throws -> [LivePullReview] {
        let frame = try await connection.request("github.reviews", params: .object(["projectId": .string(projectId)]))
        return (frame.result["reviews"]?.arrayValue ?? []).compactMap(LivePullReview.decode)
    }
}
