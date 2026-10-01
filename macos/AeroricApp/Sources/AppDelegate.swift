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
/// * On termination the supervisor's serve child must die WITH the app. An
///   orphaned serve keeps holding the data-dir `.write.lock` (the lock module
///   reports its holder but never kills by pid, by design), so every later
///   launch's serve loses lock acquisition, exits instantly, and the
///   supervisor reconnect-loops forever. The teardown is a SYNCHRONOUS kill
///   through `terminateChildForProcessExit()` — the first version awaited
///   `stopSupervisor()` on a bounded semaphore, which deadlocked: the Task
///   inherits the main actor and the main thread was the one blocked, so
///   every quit orphaned the serve (measured 2026-10-01, one quit poisoned
///   the shared scratch for every launch after it).
final class AppDelegate: NSObject, NSApplicationDelegate {
    weak var services: AppServices?

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        true
    }

    func applicationWillTerminate(_ notification: Notification) {
        services?.terminateHostForProcessExit()
    }
}
