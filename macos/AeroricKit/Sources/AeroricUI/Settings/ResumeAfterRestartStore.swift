import AeroricClient
import Foundation
import Observation

/// 设置 → 通用 → Agent 进程 (2026-11-14): whether sessions nobody is
/// watching (opened by a 定时任务, or delegated) are continued on their own
/// after an Aeroric restart cut them. Off by default; the user's own
/// sessions only ever get the 继续 button.
@MainActor @Observable
public final class ResumeAfterRestartStore {
    public private(set) var isAvailable = false
    public private(set) var enabled = false
    public private(set) var lastError: String?
    @ObservationIgnored private var read: (@Sendable () async throws -> Bool)?
    @ObservationIgnored private var write: (@Sendable (Bool) async throws -> Bool)?

    public init() {}

    public func attach(client: SessionsClient?, liveEnabled: Bool) {
        guard let client, liveEnabled else {
            attach(read: nil, write: nil)
            return
        }
        attach(read: { try await client.resumeAfterRestart() },
               write: { try await client.setResumeAfterRestart(enabled: $0) })
    }

    public func attach(read: (@Sendable () async throws -> Bool)?, write: (@Sendable (Bool) async throws -> Bool)?) {
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
            enabled = try await read()
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
            enabled = try await write(on)
            lastError = nil
        } catch {
            enabled = before
            lastError = String(describing: error)
        }
    }
}
