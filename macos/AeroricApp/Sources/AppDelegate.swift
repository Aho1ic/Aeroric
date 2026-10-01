import AppKit

/// Window lifecycle policy for the preview app (P4-01 Action 6, real
/// multi-window behaviour P4-04):
/// * Dock reopen re-shows the main window — SwiftUI's WindowGroup recreation
///   handles it, so `applicationShouldHandleReopen` keeps the default
///   handling (returns true).
/// * Cmd+Q terminates through the standard NSApplication path (no override).
/// * Closing the last window does NOT terminate the app: it stays in the Dock
///   so reopen can bring the window back (macOS convention for
///   document-less apps).
/// * On termination the supervisor is stopped: the spawned `aeroric-cli
///   serve` child must die WITH the app. An orphaned serve keeps holding the
///   data-dir `.write.lock` (the lock module reports its holder but never
///   kills by pid, by design), so every later launch's serve loses lock
///   acquisition, exits instantly, and the supervisor reconnect-loops
///   forever — measured 2026-10-01: one quit-without-stop left the lock held
///   for every launch after it. `applicationWillTerminate` has no async
///   context, so the stop is awaited on a bounded semaphore; the SIGTERM
///   lands in the first moments and bookkeeping past that is cut short by
///   process exit.
final class AppDelegate: NSObject, NSApplicationDelegate {
    weak var services: AppServices?

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        true
    }

    func applicationWillTerminate(_ notification: Notification) {
        guard let services else { return }
        let stopped = DispatchSemaphore(value: 0)
        Task { [services] in
            await services.stopSupervisor()
            stopped.signal()
        }
        _ = stopped.wait(timeout: .now() + .seconds(2))
    }
}
