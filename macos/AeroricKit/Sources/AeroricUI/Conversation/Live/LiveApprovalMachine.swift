import Foundation
import AeroricClient
import AeroricProtocol

/// Per-card approval state (Q10). A card leaves only when the host resolves it
/// (`permissionResolved`, or it disappears from `pending`); a submitted card
/// stays disabled, a failed submission can be retried. A2: an answer that did
/// not reach the agent is never shown as settled, and is never resent on its own.
public struct LiveApprovalMachine: Equatable, Sendable {
    public enum Phase: Equatable, Sendable {
        case pending
        /// The user is writing a denial reason.
        case denying(reason: String)
        case submitting(LiveDecision)
        /// Not sent: the call was refused, or the host could not write the
        /// answer to the agent. `decision` is nil when another end answered.
        case failed(LiveDecision?, message: String)
        /// A2: the call to the host timed out or the connection dropped, so
        /// whether the answer got there is not known. The user may send again:
        /// the host refuses a second answer to a request already answered.
        case unknown(LiveDecision, message: String)
        /// A2: another end answered first, or the request is already over;
        /// the card waits for the host to settle it.
        case settledElsewhere(message: String)
    }

    /// A2: what a failed `session.respond` call means for the card.
    public enum SubmitFailure: Equatable, Sendable {
        case notSent(String)
        case unknown(String)
        case settledElsewhere(String)

        public static func classify(_ error: Error) -> SubmitFailure {
            switch error {
            case let ConnectionError.refused(frame):
                switch frame.error.code {
                case .busy: return .settledElsewhere("另一端正在回答这条审批")
                case .notFound: return .settledElsewhere("这条审批已由另一端处理，或已经结束")
                default: return .notSent(frame.error.message)
                }
            case ConnectionError.timedOut:
                return .unknown("等宿主回复超时，不知道送到没有")
            case ConnectionError.disconnected:
                return .unknown("和宿主的连接断了，不知道送到没有")
            default:
                return .notSent("\(error)")
            }
        }
    }

    public private(set) var phases: [String: Phase] = [:]

    public init() {}

    public func phase(_ id: String) -> Phase { phases[id] ?? .pending }

    public func canAct(_ id: String) -> Bool {
        switch phase(id) {
        case .pending, .denying, .failed, .unknown: true
        case .submitting, .settledElsewhere: false
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

    /// A2: the call to the host failed; what that means depends on how.
    public mutating func submitFailed(_ id: String, _ failure: SubmitFailure) {
        guard case let .submitting(decision) = phase(id) else { return }
        switch failure {
        case let .notSent(message): phases[id] = .failed(decision, message: message)
        case let .unknown(message): phases[id] = .unknown(decision, message: message)
        case let .settledElsewhere(message): phases[id] = .settledElsewhere(message: message)
        }
    }

    /// A2: the host could not write an answer to the agent (this end's or
    /// another's). A reason being typed is left alone.
    public mutating func hostFailed(_ id: String, message: String) {
        switch phase(id) {
        case .pending, .settledElsewhere: phases[id] = .failed(nil, message: message)
        case let .submitting(decision), let .unknown(decision, _): phases[id] = .failed(decision, message: message)
        case let .failed(decision, _): phases[id] = .failed(decision, message: message)
        case .denying: break
        }
    }

    /// Forget cards the host no longer lists.
    public mutating func retain(pending: [LivePendingPermission]) {
        let live = Set(pending.map(\.id))
        phases = phases.filter { live.contains($0.key) }
    }
}
