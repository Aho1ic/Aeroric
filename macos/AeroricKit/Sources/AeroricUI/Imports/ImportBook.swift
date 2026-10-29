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
    /// The open import picker (nil when closed or still loading).
    public var pick: ImportPick?
    public private(set) var pickLoading = false
    /// The last picker run, for the settings page's one-line summary.
    public private(set) var lastApply: ImportsClient.ApplyResult?
    @ObservationIgnored private var client: ImportsClient?
    /// Set by the App: re-read projects after the picker added some.
    @ObservationIgnored public var onProjectsChanged: (@MainActor () async -> Void)?

    /// Agents this build can import (D5: DSH / omp later).
    nonisolated public static let importableAgents = ["claude", "codex", "dsh", "omp", "qoder", "cursor", "opencode"]
    /// History pages read per session (≈ 640 KB each).
    nonisolated static let maxHistoryPages = 24

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

    /// 接着聊: the task that now carries the session; the import row leaves the list.
    /// `configuration` pins the task like a new session's (its agent, default
    /// model, effort and permission); nil keeps the CLI's own login.
    public func continueSession(id: String, projectID: String? = nil,
                                configuration: SessionConfiguration? = nil) async throws -> String {
        guard let client else { throw ImportsClient.MalformedReply.malformed("imports unavailable") }
        let permissions = configuration?.options.permissionModes ?? []
        let task = try await client.continueSession(
            id: id, projectId: projectID, configurationId: configuration?.id, agent: configuration?.agent,
            model: configuration?.model ?? configuration?.options.models.first, effort: configuration?.effort,
            permissionMode: permissions.contains("ask") ? "ask" : permissions.first)
        await refresh()
        return task
    }

    public func messages(id: String) async throws -> [SessionMessage] {
        guard let client else { throw ImportsClient.MalformedReply.malformed("imports unavailable") }
        // The host pages long sessions so each reply fits one frame; walk the
        // cursor back to the start (bounded, a few MB at most).
        var page = try await client.history(id: id)
        var messages = page.messages
        var pages = 1
        while page.hasMore, let cursor = page.nextCursor, pages < Self.maxHistoryPages {
            page = try await client.history(id: id, before: cursor)
            messages.insert(contentsOf: page.messages, at: 0)
            pages += 1
        }
        return messages.map(SessionBlockMapping.message)
    }

    public func session(id: String) -> ImportsClient.ImportedSession? {
        sessions.first { $0.id == id }
    }

    public func sessions(inProject projectID: String) -> [ImportsClient.ImportedSession] {
        sessions.filter { $0.projectId == projectID && !$0.archived }
    }

    public var unassigned: [ImportsClient.ImportedSession] {
        sessions.filter { $0.projectId == nil && !$0.archived }
    }

    public var archivedSessions: [ImportsClient.ImportedSession] {
        sessions.filter(\.archived)
    }

    public func setArchived(_ id: String, _ archived: Bool) async {
        if let index = sessions.firstIndex(where: { $0.id == id }) { sessions[index].archived = archived }
        await run("archive") { try await $0.archive(id: id, archived: archived) }
    }

    /// Hides the session in Aeroric; the agent's file stays where it is.
    public func remove(_ id: String) async {
        sessions.removeAll { $0.id == id }
        await run("remove") { try await $0.remove(id: id) }
    }

    // MARK: Import picker

    /// Reads every session on disk into `pick`; nothing is saved.
    public func openPicker() async {
        guard let client else { return }
        pickLoading = true
        defer { pickLoading = false }
        do {
            pick = ImportPick(rows: try await client.preview())
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
    }

    /// Imports the ticked sessions and adds their missing projects.
    public func applyPick() async {
        guard let client, let pick else { return }
        busyAgents.insert("apply")
        defer { busyAgents.remove("apply") }
        do {
            lastApply = try await client.apply(selected: Array(pick.selected))
            self.pick = nil
            lastError = lastApply?.failedProjects.isEmpty == false
                ? "以下目录没能添加为项目：" + (lastApply?.failedProjects.joined(separator: "；") ?? "") : nil
        } catch {
            lastError = String(describing: error)
        }
        await onProjectsChanged?()
        await refresh()
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
