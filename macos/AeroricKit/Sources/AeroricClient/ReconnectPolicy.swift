import Foundation

/// Host-restart backoff. Pure and injectable-random so tests are deterministic:
/// `delay` never reads the clock or the world, it only transforms
/// (attempt, random) → milliseconds.
///
/// Shape: exponential with a cap and ±20% jitter — after a crash the host
/// needs a moment (lock file release, socket unbind), and retrying every
/// 100 ms turns one bad build into a fork bomb. No give-up: the supervisor
/// keeps retrying with the capped delay forever, because a user who watches
/// "reconnecting (7)" with a growing delay is in a better position than one
/// whose app silently gave up.
public struct ReconnectPolicy: Sendable, Equatable {
    public var baseDelayMs: UInt64
    public var factor: Double
    public var capDelayMs: UInt64
    /// Jitter half-width as a fraction of the delay (0.2 = ±20%).
    public var jitter: Double
    /// `nil` = retry forever (the default, see above).
    public var maxAttempts: Int?

    public init(baseDelayMs: UInt64 = 500,
                factor: Double = 2,
                capDelayMs: UInt64 = 8_000,
                jitter: Double = 0.2,
                maxAttempts: Int? = nil) {
        self.baseDelayMs = baseDelayMs
        self.factor = factor
        self.capDelayMs = capDelayMs
        self.jitter = jitter
        self.maxAttempts = maxAttempts
    }

    /// Delay before attempt number `attempt` (1 = first retry). Saturates at
    /// the cap, then jitters symmetrically. `nil` once `maxAttempts` is hit.
    public func delay(afterAttempt attempt: Int,
                      randomSource: () -> Double = { Double.random(in: 0 ... 1) }) -> UInt64? {
        if let maxAttempts, attempt > maxAttempts { return nil }
        let exponent = max(0, attempt - 1)
        let raw = pow(factor, Double(exponent)) * Double(baseDelayMs)
        let capped = min(raw, Double(capDelayMs))
        let halfWidth = capped * jitter
        let offset = (randomSource() * 2 - 1) * halfWidth
        let delayed = max(0, capped + offset)
        return UInt64(delayed.rounded(.up))
    }
}
