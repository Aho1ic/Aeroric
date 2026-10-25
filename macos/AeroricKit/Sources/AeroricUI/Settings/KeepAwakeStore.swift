import AeroricClient
import Foundation
import Observation

/// Window-independent state for "keep the Mac awake while tasks run" (A14).
/// Unavailable (no capability) hides every entry point.
@MainActor @Observable
public final class KeepAwakeStore {
    public private(set) var isAvailable = false
    public private(set) var state: PowerClient.State?
    public private(set) var lastError: String?
    @ObservationIgnored private var client: PowerClient?

    public init() {}

    public func attach(client: PowerClient?, capabilityEnabled: Bool) {
        self.client = capabilityEnabled ? client : nil
        isAvailable = capabilityEnabled && client != nil
        if !isAvailable { state = nil }
    }

    /// The host is holding the assertion right now.
    public var active: Bool { state?.active == true }

    public func refresh() async {
        guard let client else { return }
        if let state = try? await client.state() { self.state = state }
    }

    public func setEnabled(_ enabled: Bool) async {
        guard let client else { return }
        do {
            try await client.setEnabled(enabled)
            lastError = nil
        } catch {
            lastError = String(describing: error)
        }
        await refresh()
    }
}
