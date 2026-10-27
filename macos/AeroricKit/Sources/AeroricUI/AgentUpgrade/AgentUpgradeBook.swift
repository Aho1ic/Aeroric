import AeroricClient
import Foundation
import Observation

/// Agent versions, reminders and upgrade jobs (2026-10-27), shared by the session
/// page's reminder capsule and the Agent 升级 tab. Reads once when the host is
/// ready, asks the registry again every six hours, and polls each second only
/// while a job runs.
@MainActor @Observable
public final class AgentUpgradeBook {
    public typealias Harness = AgentUpgradeClient.Harness

    public private(set) var snapshot: AgentUpgradeClient.Snapshot?
    public private(set) var isAvailable = false
    public private(set) var checking = false
    public var errorText: String?

    @ObservationIgnored private var client: AgentUpgradeClient?
    @ObservationIgnored private var poll: Task<Void, Never>?
    @ObservationIgnored private var periodic: Task<Void, Never>?
    @ObservationIgnored private var waiting: [String: [() -> Void]] = [:]

    public init() {}

    public func attach(client: AgentUpgradeClient?, capabilityEnabled: Bool) {
        self.client = capabilityEnabled ? client : nil
        isAvailable = capabilityEnabled && client != nil
        periodic?.cancel()
        guard isAvailable else {
            snapshot = nil
            return
        }
        periodic = Task { [weak self] in
            await self?.refresh()
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(6 * 60 * 60))
                await self?.checkUpdates()
            }
        }
    }

    /// Test seam: show a snapshot without a host.
    public func load(_ snapshot: AgentUpgradeClient.Snapshot) {
        apply(snapshot)
    }

    public func refresh() async {
        await run { try await $0.versions() }
    }

    public func checkUpdates() async {
        checking = true
        defer { checking = false }
        await run { try await $0.checkUpdates() }
    }

    public func upgrade(_ agent: String) async {
        await run { try await $0.upgrade(agent: agent) }
    }

    public func upgradeAll() async {
        for harness in upgradable { await upgrade(harness.agent) }
    }

    public func setReminder(_ agent: String, _ mode: AgentUpgradeClient.ReminderMode) async {
        await run { try await $0.setReminder(agent: agent, mode: mode) }
    }

    /// The harness a session family belongs to (`claude_gpt55` runs the Codex binary).
    public func harness(forFamily family: String?) -> Harness? {
        guard let family else { return nil }
        let agent = family == "claude_gpt55" ? "codex" : family
        return snapshot?.agents.first { $0.agent == agent }
    }

    public var upgradable: [Harness] {
        (snapshot?.agents ?? []).filter { [.updateAvailable, .required].contains($0.status) && $0.job?.state != .running }
    }

    /// Run `action` once `agent` has upgraded successfully (a held message).
    public func whenUpgraded(_ agent: String, _ action: @escaping () -> Void) {
        waiting[agent, default: []].append(action)
    }

    private func run(_ call: (AgentUpgradeClient) async throws -> AgentUpgradeClient.Snapshot) async {
        guard let client else { return }
        do {
            apply(try await call(client))
            errorText = nil
        } catch {
            errorText = "\(error)"
        }
    }

    private func apply(_ next: AgentUpgradeClient.Snapshot) {
        let before = snapshot
        snapshot = next
        for harness in next.agents {
            let was = before?.agents.first { $0.agent == harness.agent }?.job?.state
            if was == .running, harness.job?.state == .succeeded, harness.status != .required {
                let actions = waiting.removeValue(forKey: harness.agent) ?? []
                actions.forEach { $0() }
            }
        }
        let running = next.agents.contains { $0.job?.state == .running }
        if running, poll == nil {
            poll = Task { [weak self] in
                while !Task.isCancelled {
                    try? await Task.sleep(for: .seconds(1))
                    guard let self else { return }
                    await self.refresh()
                    if !(self.snapshot?.agents.contains { $0.job?.state == .running } ?? false) { break }
                }
                self?.poll = nil
            }
        }
    }
}

/// What the session page's capsule shows for one harness (pure, tested).
public enum AgentUpdateReminder: Equatable {
    case available(title: String, latest: String, installed: String)
    case required(title: String, installed: String, minimum: String)
    case upgrading(title: String)
    case done(title: String, version: String)
    case failed(title: String, message: String)

    /// `dismissedJob`: the finish time of a result the capsule already showed.
    public static func make(_ harness: AgentUpgradeClient.Harness?, dismissedJob: UInt64?) -> AgentUpdateReminder? {
        guard let harness else { return nil }
        let title = harness.title
        if let job = harness.job {
            switch job.state {
            case .running: return .upgrading(title: title)
            case .succeeded where job.finishedAtMs != dismissedJob:
                return .done(title: title, version: harness.installed ?? harness.latest ?? "")
            case .failed where job.finishedAtMs != dismissedJob:
                return .failed(title: title, message: job.message ?? "升级没有完成")
            default: break
            }
        }
        guard harness.shouldRemind else { return nil }
        if harness.status == .required {
            return .required(title: title, installed: harness.installed ?? "", minimum: harness.minimum ?? "")
        }
        guard let latest = harness.latest else { return nil }
        return .available(title: title, latest: latest, installed: harness.installed ?? "")
    }
}
