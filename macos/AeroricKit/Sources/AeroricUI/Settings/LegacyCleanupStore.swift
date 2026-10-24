import AeroricClient
import AeroricProtocol
import Foundation
import Observation

/// View state for the legacy custom Agent cleanup sheet (B1, Q10/Q11).
///
/// Every decision is the host's: eligibility, the fixed step order, partial and
/// resume. This store only caches replies, forwards the user's explicit selection
/// and remembers errors. Nothing here runs without a click: no apply on attach,
/// on reload or on opening the sheet.
@MainActor
@Observable
public final class LegacyCleanupStore {
    public enum Availability: Equatable, Sendable {
        case notConnected
        case unsupported(String)
        case available
    }

    public private(set) var availability: Availability = .notConnected
    public private(set) var report: LegacyCleanupReport?
    public private(set) var selection: Set<String> = []
    /// Q5: the user states they launched the selected Agents through their new
    /// configuration. Reset after every apply.
    public var launchConfirmed = false
    public private(set) var isBusy = false
    public private(set) var lastError: String?
    /// Batch directory of the last apply that created one.
    public private(set) var lastBatch: String?

    private var client: (any LegacyCleanupServing)?

    public init() {}

    /// Called when a connection is (re)established. `capabilityEnabled` comes from
    /// the handshake; a host that does not advertise cleanup gets a reason, not a
    /// button that is doomed to fail.
    public func attach(client: (any LegacyCleanupServing)?, capabilityEnabled: Bool) {
        self.client = client
        report = nil
        selection = []
        launchConfirmed = false
        lastError = nil
        lastBatch = nil
        if client == nil {
            availability = .notConnected
        } else if !capabilityEnabled {
            availability = .unsupported("当前宿主未提供旧版 Agent 清理（agentConnections.legacyCleanup）。")
        } else {
            availability = .available
        }
    }

    public var unavailableReason: String? {
        switch availability {
        case .available: nil
        case .notConnected: "尚未连接到宿主。"
        case let .unsupported(reason): reason
        }
    }

    public var canApply: Bool {
        availability == .available && !selection.isEmpty && launchConfirmed && !isBusy
    }

    public func isSelectable(_ candidate: LegacyCleanupCandidate) -> Bool {
        candidate.state.isActionable && !isBusy
    }

    public func toggle(_ source: String) {
        guard let candidate = report?.candidates.first(where: { $0.source == source }),
              isSelectable(candidate) else { return }
        if selection.contains(source) {
            selection.remove(source)
        } else {
            selection.insert(source)
        }
    }

    public func reload() async {
        guard availability == .available, let client, !isBusy else { return }
        isBusy = true
        defer { isBusy = false }
        do {
            adopt(try await client.plan())
            lastError = nil
        } catch {
            // Keep what was shown; a failed refresh is not an empty list.
            lastError = "读取清理计划失败：\(error)"
        }
    }

    /// Sends exactly the confirmed selection, in list order.
    public func apply() async {
        guard canApply, let client else { return }
        let ordered = (report?.candidates ?? []).map(\.source).filter(selection.contains)
        isBusy = true
        do {
            let outcome = try await client.apply(sourceIDs: ordered)
            adopt(outcome)
            if let batch = outcome.quarantineBatch { lastBatch = batch }
            lastError = nil
            selection = []
            launchConfirmed = false
        } catch {
            lastError = "清理失败：\(error)"
            isBusy = false
            return
        }
        isBusy = false
        // The apply reply already shows each outcome; reload so the list reflects
        // the host's current verification (D4). A partial entry is never retried
        // automatically.
        await reload()
    }

    private func adopt(_ next: LegacyCleanupReport) {
        report = next
        let actionable = Set(next.candidates.filter(\.state.isActionable).map(\.source))
        selection.formIntersection(actionable)
    }
}
