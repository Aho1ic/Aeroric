import AeroricClient
import Foundation
import Observation

/// Window-independent state for imported sessions (M4). Talks to the host
/// only through `ImportsClient` (INV-02). When the host lacks the `imports`
/// capability, `isAvailable` stays false and every entry point hides.
@MainActor @Observable
public final class ImportBook {
    public private(set) var isAvailable = false
    public private(set) var status: ImportsClient.Status?
    public private(set) var sessions: [ImportsClient.ImportedSession] = []
    public private(set) var detected: [ImportsClient.DetectedAgent] = []
    public private(set) var estimates: [String: ImportsClient.Estimate] = [:]
    public private(set) var busyAgents: Set<String> = []
    public private(set) var lastError: String?
    @ObservationIgnored private var client: ImportsClient?

    /// Agents this build can import (D5: DSH / omp later).
    nonisolated public static let importableAgents = ["claude", "codex"]

    public init() {}

    public func attach(client: ImportsClient?, capabilityEnabled: Bool) {
        self.client = capabilityEnabled ? client : nil
        isAvailable = capabilityEnabled && client != nil
        if !isAvailable {
            status = nil
            sessions = []
            estimates = [:]
        }
    }

    public func refresh() async {
        guard let client else { return }
        do {
            status = try await client.status()
            sessions = try await client.list()
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
    }

    public func detectAgents() async {
        guard let client else { return }
        detected = (try? await client.detectAgents()) ?? []
    }

    public func estimate(_ agent: String) async {
        guard let client else { return }
        if let estimate = try? await client.estimate(agent: agent) { estimates[agent] = estimate }
    }

    public func grant(_ agent: String) async {
        await run(agent) { try await $0.grant(agent: agent) }
    }

    public func revoke(_ agent: String) async {
        await run(agent) { try await $0.revoke(agent: agent) }
    }

    public func rescan() async {
        await run("rescan") { try await $0.rescan() }
    }

    public func messages(id: String) async throws -> [SessionMessage] {
        guard let client else { throw ImportsClient.MalformedReply.malformed("imports unavailable") }
        return try await client.history(id: id).messages.map(SessionBlockMapping.message)
    }

    public func session(id: String) -> ImportsClient.ImportedSession? {
        sessions.first { $0.id == id }
    }

    public func sessions(inProject projectID: String) -> [ImportsClient.ImportedSession] {
        sessions.filter { $0.projectId == projectID }
    }

    public var unassigned: [ImportsClient.ImportedSession] {
        sessions.filter { $0.projectId == nil }
    }

    public func agentStatus(_ agent: String) -> ImportsClient.AgentStatus? {
        status?.agents.first { $0.agent == agent }
    }

    private func run(_ key: String, _ body: (ImportsClient) async throws -> Void) async {
        guard let client else { return }
        busyAgents.insert(key)
        defer { busyAgents.remove(key) }
        do {
            try await body(client)
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
        await refresh()
    }
}

/// Presentation helpers shared by onboarding and settings.
@MainActor
public enum ImportPresentation {
    public static func agentTitle(_ agent: String) -> String {
        AgentGlyph.displayName(for: agent)
    }

    /// "约 12 个会话 · 3.4 MB" / "未找到会话目录".
    public static func estimateText(_ estimate: ImportsClient.Estimate?) -> String {
        guard let estimate else { return "正在估算…" }
        guard estimate.rootExists else { return "未找到会话目录" }
        let size = ByteCountFormatter.string(fromByteCount: Int64(clamping: estimate.bytes), countStyle: .file)
        return "\(estimate.sessions) 个会话 · \(size)"
    }

    public static func abbreviated(_ path: String, home: String = NSHomeDirectory()) -> String {
        path.hasPrefix(home) ? "~" + path.dropFirst(home.count) : path
    }
}
