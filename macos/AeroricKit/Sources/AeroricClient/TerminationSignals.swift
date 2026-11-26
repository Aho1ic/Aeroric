import Dispatch
import Foundation

/// 2026-11-25: a terminate signal (`kill`, launchd at logout) ends a Cocoa app
/// on the spot: `applicationWillTerminate` never runs, so the host is not
/// stopped the app's way (keep-in-background, folder access). This turns the
/// signal into a handler call on `queue`, where the app quits normally.
///
/// The signal is ignored while handled (`SIG_IGN`) so its default action does
/// not end the process first; `stop()` restores the default. Children keep
/// default handling: the host supervisor spawns with `POSIX_SPAWN_SETSIGDEF`.
public final class TerminationSignals: @unchecked Sendable {
    private let signals: [Int32]
    private let queue: DispatchQueue
    private let handler: (Int32) -> Void
    private var sources: [DispatchSourceSignal] = []

    public init(signals: [Int32] = [SIGTERM], queue: DispatchQueue = .main, handler: @escaping (Int32) -> Void) {
        self.signals = signals
        self.queue = queue
        self.handler = handler
    }

    public func start() {
        guard sources.isEmpty else { return }
        for number in signals {
            signal(number, SIG_IGN)
            let source = DispatchSource.makeSignalSource(signal: number, queue: queue)
            let handler = self.handler
            source.setEventHandler { handler(number) }
            source.resume()
            sources.append(source)
        }
    }

    public func stop() {
        for source in sources { source.cancel() }
        sources = []
        for number in signals { signal(number, SIG_DFL) }
    }

    deinit { stop() }
}
