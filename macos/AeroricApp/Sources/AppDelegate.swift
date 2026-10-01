import AppKit

/// Window lifecycle policy for the preview app (P4-01 Action 6).
///
/// P4-04 wires the real multi-window behaviour (main + settings windows);
/// until then the policy is fixed here so `pnpm native:dev` and the UI test
/// see deterministic behaviour:
/// * Dock reopen re-shows the main window — SwiftUI's WindowGroup recreation
///   handles it, so `applicationShouldHandleReopen` keeps the default
///   handling (returns true).
/// * Cmd+Q terminates through the standard NSApplication path (no override).
/// * Closing the last window does NOT terminate the app: it stays in the Dock
///   so reopen can bring the window back (macOS convention for
///   document-less apps).
final class AppDelegate: NSObject, NSApplicationDelegate {
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        true
    }
}
