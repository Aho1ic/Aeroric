import AeroricClient
import Foundation
import Observation

/// 设置 → 通用 → Agent 进程 (A14b, 2026-11-07): whether the host releases an
/// idle Agent's process after an hour. The host never releases one with
/// background work. Shown only once the host has answered: an older host
/// without the methods leaves the card hidden.
@MainActor @Observable
public final class IdleReleaseStore {
    public typealias Answer = (enabled: Bool, idleMinutes: Int)

    public private(set) var isAvailable = false
    public private(set) var enabled = true
    public private(set) var idleMinutes = 60
    public private(set) var lastError: String?
    @ObservationIgnored private var read: (@Sendable () async throws -> Answer)?
    @ObservationIgnored private var write: (@Sendable (Bool) async throws -> Answer)?

    public init() {}

    public func attach(client: SessionsClient?, liveEnabled: Bool) {
        guard let client, liveEnabled else {
            attach(read: nil, write: nil)
            return
        }
        attach(read: { try await client.idleRelease() },
               write: { try await client.setIdleRelease(enabled: $0) })
    }

    public func attach(read: (@Sendable () async throws -> Answer)?,
                       write: (@Sendable (Bool) async throws -> Answer)?) {
        self.read = read
        self.write = write
        isAvailable = false
        lastError = nil
    }

    public func refresh() async {
        guard let read else {
            isAvailable = false
            return
        }
        do {
            apply(try await read())
            isAvailable = true
        } catch {
            isAvailable = false
        }
    }

    /// Flips at once; a failed save puts the switch back and says why.
    public func setEnabled(_ on: Bool) async {
        guard let write else { return }
        let before = enabled
        enabled = on
        do {
            apply(try await write(on))
            lastError = nil
        } catch {
            enabled = before
            lastError = String(describing: error)
        }
    }

    private func apply(_ answer: Answer) {
        enabled = answer.enabled
        idleMinutes = answer.idleMinutes
    }
}
