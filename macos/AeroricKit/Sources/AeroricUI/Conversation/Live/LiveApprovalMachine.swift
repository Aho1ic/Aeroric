import Foundation
import AeroricProtocol

/// Per-card approval state (Q10). A card leaves only when the host resolves it
/// (`permissionResolved`, or it disappears from `pending`); a submitted card
/// stays disabled, a failed submission can be retried.
public struct LiveApprovalMachine: Equatable, Sendable {
    public enum Phase: Equatable, Sendable {
        case pending
        /// The user is writing a denial reason.
        case denying(reason: String)
        case submitting(LiveDecision)
        case failed(LiveDecision, message: String)
    }

    public private(set) var phases: [String: Phase] = [:]

    public init() {}

    public func phase(_ id: String) -> Phase { phases[id] ?? .pending }

    public func canAct(_ id: String) -> Bool {
        switch phase(id) {
        case .pending, .denying, .failed: true
        case .submitting: false
        }
    }

    public mutating func beginDeny(_ id: String) {
        guard canAct(id) else { return }
        phases[id] = .denying(reason: "")
    }

    public mutating func editReason(_ id: String, _ reason: String) {
        if case .denying = phase(id) { phases[id] = .denying(reason: reason) }
    }

    public mutating func cancelDeny(_ id: String) {
        if case .denying = phase(id) { phases[id] = .pending }
    }

    /// Returns the reason to send (nil for allow decisions or an empty reason);
    /// nil overall when the card cannot be submitted now.
    public mutating func submit(_ id: String, _ decision: LiveDecision) -> (decision: LiveDecision, reason: String?)? {
        guard canAct(id) else { return nil }
        var reason: String?
        if decision == .deny, case let .denying(text) = phase(id) {
            let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
            reason = trimmed.isEmpty ? nil : trimmed
        }
        phases[id] = .submitting(decision)
        return (decision, reason)
    }

    public mutating func failed(_ id: String, message: String) {
        if case let .submitting(decision) = phase(id) { phases[id] = .failed(decision, message: message) }
    }

    /// Forget cards the host no longer lists.
    public mutating func retain(pending: [LivePendingPermission]) {
        let live = Set(pending.map(\.id))
        phases = phases.filter { live.contains($0.key) }
    }
}
