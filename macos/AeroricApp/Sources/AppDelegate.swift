import AppKit
import AeroricUI

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

    /// Light / dark / system from Settings → 外观 (M3), before any window shows.
    func applicationDidFinishLaunching(_ notification: Notification) {
        AppTheme.applyStored()
    }

    /// Default reopen handling (P4-01 policy): returning true lets SwiftUI's
    /// WindowGroup recreate the main window on Dock reopen. NOTE (G41): an
    /// XCUITest launch can deliver a reopen before the scene connects, which
    /// creates a second shell window — the tests tolerate that with
    /// firstMatch queries and value-based selection assertions instead of
    /// suppressing reopen here (refusing it during startup raced the
    /// automation-session setup and produced zero-window launches wholesale,
    /// measured 2026-10-02).
    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        true
    }

    /// UI-test launches wipe SwiftUI's persisted scene storage: a hard-killed
    /// instance's saved window would otherwise be RESTORED next to the fresh
    /// WindowGroup window — two shell scenes per launch, which broke every
    /// window-count assertion (measured 2026-10-02). Gated on the injected
    /// test environment; plain launches keep restoration. (.restorationBehavior
    /// would be the scene-level switch but needs macOS 15; target is 14.)
    func applicationWillFinishLaunching(_ notification: Notification) {
        guard let environment = ProcessInfo.processInfo.environment["AERORIC_ENV"],
              !environment.isEmpty,
              let bundleIdentifier = Bundle.main.bundleIdentifier else { return }
        UserDefaults.standard.removePersistentDomain(forName: bundleIdentifier)
    }

    func applicationWillTerminate(_ notification: Notification) {
        services?.terminateHostForProcessExit()
    }
}
