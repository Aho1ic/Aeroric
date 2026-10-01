import Foundation
import AeroricProtocol

/// Why a pending request left the registry. A request resolves exactly once —
/// duplicates, late replies after a timeout, and answers for unknown ids are
/// protocol-legal (the reply may already be in flight when the timeout fires),
/// so they surface as `false` from [`RequestRegistry.fulfill`] instead of
/// crashing or double-resuming a continuation.
public enum RequestResolution: Error, Sendable, Equatable {
    case success(SuccessFrame)
    case failure(FailureFrame)
    case timedOut
    case cancelled
    case disconnected
}

struct RequestOutcome: Sendable, Equatable {
    var frame: SuccessFrame?
    var failure: FailureFrame?
}

/// actor-guarded id → continuation map. All mutation is actor-isolated, so a
/// plain struct entry needs no lock: the "exactly once" invariant holds because
/// every path goes through [`RequestRegistry.resolve`], which flips the entry
/// out of the dictionary before resuming its continuation.
public actor RequestRegistry {
    private struct Entry {
        let continuation: CheckedContinuation<RequestResolution, Error>
    }

    private var entries: [String: Entry] = [:]

    public init() {}

    /// Awaits the reply for `id`. Continuation is stored under actor isolation;
    /// every resolution path removes the entry first, so the continuation is
    /// resumed exactly once no matter how the races line up.
    public func awaitReply(id: String) async throws -> RequestResolution {
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                entries[id] = Entry(continuation: continuation)
            }
        } onCancel: { [weak self] in
            guard let self else { return }
            Task { await self.cancel(id: id) }
        }
    }

    /// Fulfils with a well-formed frame. Returns `false` for unknown ids
    /// (unsolicited reply) and already-resolved ids (duplicate reply).
    @discardableResult
    public func fulfill(id: String, frame: Frame) -> Bool {
        switch frame {
        case let .success(success):
            return resolve(id: id, .success(success))
        case let .failure(failure):
            return resolve(id: id, .failure(failure))
        case .event, .request:
            // An event or a host→client request is not a reply; dropping it is
            // the routing layer's job, but "false" keeps this method honest.
            return false
        }
    }

    /// Times a request out (remove-then-resume, same as every other path).
    @discardableResult
    public func timeOut(id: String) -> Bool {
        resolve(id: id, .timedOut)
    }

    /// Task cancellation path.
    @discardableResult
    public func cancel(id: String) -> Bool {
        resolve(id: id, .cancelled)
    }

    /// Fails every pending request (disconnect, stream fault). Returns the
    /// number of requests that were actually still pending.
    @discardableResult
    public func failAll(_ resolution: RequestResolution) -> Int {
        let pending = entries.values
        entries.removeAll()
        for entry in pending {
            entry.continuation.resume(returning: resolution)
        }
        return pending.count
    }

    public var pendingCount: Int { entries.count }

    private func resolve(id: String, _ resolution: RequestResolution) -> Bool {
        guard let entry = entries.removeValue(forKey: id) else { return false }
        entry.continuation.resume(returning: resolution)
        return true
    }
}
