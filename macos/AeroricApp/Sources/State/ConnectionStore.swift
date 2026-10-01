import Foundation
import Observation
import AeroricClient
import AeroricProtocol

/// UI mirror of the supervisor + reducer state. All mutations land on the
/// main actor; the supervisor's `onChange` and the connection's event sink
/// hop here via `Task { @MainActor … }`.
@MainActor
@Observable
final class ConnectionStore {
    struct ReplayGap: Equatable {
        var expected: UInt64
        var received: UInt64
    }

    private(set) var phase: SupervisorPhase = .idle
    private(set) var readyInfo: ReadyInfo?
    private(set) var lastGap: ReplayGap?
    /// Human-readable note about the last protocol anomaly the reducer saw
    /// (`nil` while the stream is clean).
    private(set) var anomaly: String?
    private(set) var needsFullRefreshReason: String?

    /// Live count of applied events since the epoch began (reducer watermark).
    var watermarkText: String {
        String(reducer.watermark.uint64Value)
    }

    var phaseText: String {
        switch phase {
        case .idle: "idle"
        case .connecting: "connecting"
        case .ready: "ready"
        case let .degraded(reason): "degraded: \(reason)"
        case let .reconnecting(attempt, nextDelayMs):
            "reconnecting (attempt \(attempt), next in \(nextDelayMs) ms)"
        case let .incompatible(reason): "incompatible: \(reason)"
        case .stopped: "stopped"
        }
    }

    private var reducer = EventReducer()
    var connection: HostConnection?
    /// Used by `syncReplay` for the epoch check. AppServices supplies it in
    /// `attach` (after `start()`, so the value is already assigned).
    private var supervisorRef: HostSupervisor?
    private var replaySyncTask: Task<Void, Never>?

    func attach(connection: HostConnection, supervisor: HostSupervisor?) {
        self.connection = connection
        self.supervisorRef = supervisor
    }

    /// Unwinds on stop: drops the channel refs and cancels an in-flight
    /// replay sync (the phase reset to `.stopped` clears reducer state).
    func detach() {
        replaySyncTask?.cancel()
        replaySyncTask = nil
        connection = nil
        supervisorRef = nil
    }

    func applyPhase(_ phase: SupervisorPhase) {
        self.phase = phase
        switch phase {
        case .ready:
            beginReplaySync()
        case .stopped:
            readyInfo = nil
            reducer = EventReducer()
        default:
            break
        }
    }

    /// Live event from the connection's event sink.
    func ingest(_ frame: EventFrame) {
        for effect in reducer.apply(frame) {
            record(effect)
        }
    }

    // MARK: - Replay sync

    /// On `.ready` (authenticated), backfills from `events.since {after:
    /// watermark}` so the store starts from the host's snapshot, then rides
    /// live events. Runs at most once per ready-phase; a newer `.ready`
    /// cancels the previous attempt.
    private func beginReplaySync() {
        replaySyncTask?.cancel()
        replaySyncTask = Task { [weak self] in
            await self?.syncReplay()
        }
    }

    private func syncReplay() async {
        guard let connection, let supervisorRef else { return }
        let ready = await supervisorRef.currentReadyInfo
        guard let ready, !ready.epoch.isEmpty else { return }
        readyInfo = ready
        do {
            let after = reducer.watermark.uint64Value
            let reply = try await connection.request(
                "events.since",
                params: .object(["after": .number(String(after))])
            )
            guard let batch = ReplayBatch.decode(reply.result) else {
                anomaly = "replay batch decode failed"
                return
            }
            for effect in reducer.apply(batch, epoch: ready.epoch) {
                record(effect)
            }
        } catch let error as ConnectionError {
            anomaly = describe(error)
        } catch {
            anomaly = String(describing: error)
        }
    }

    private func record(_ effect: ReducerEffect) {
        switch effect {
        case .applied:
            break
        case let .duplicateDropped(seq):
            anomaly = "duplicate dropped at seq \(seq.uint64Value)"
        case let .replayGap(expected, received):
            lastGap = ReplayGap(expected: expected.uint64Value, received: received.uint64Value)
            anomaly = "replay gap: expected \(expected.uint64Value), got \(received.uint64Value)"
        case let .epochChanged(from, to):
            anomaly = "epoch changed \(from) → \(to)"
        case let .needsFullRefresh(reason):
            needsFullRefreshReason = reason
            anomaly = reason
        }
    }

    private func describe(_ error: ConnectionError) -> String {
        switch error {
        case let .io(reason): "io: \(reason)"
        case let .transport(fault): "transport: \(fault)"
        case let .authDenied(message): "auth denied: \(message)"
        case .disconnected: "disconnected"
        case .timedOut: "timed out"
        case .cancelled: "cancelled"
        case .notConnected: "not connected"
        case let .refused(failure): "refused: \(failure.error.message)"
        }
    }
}
