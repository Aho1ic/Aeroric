import AppKit
import SwiftUI

/// The single reusable settings window (P4-04 Action 3). `show` re-keys the
/// existing window instead of constructing a new one (Cmd+, twice = same
/// window); `windowShouldClose` orders it out but keeps the instance so the
/// next `show` is instant (the primuse window-controller idiom,
/// MacSettingsView :6423).
@MainActor
public final class SettingsWindowController: NSObject, NSWindowDelegate {
    public static let shared = SettingsWindowController()
    private static let contentSize = NSSize(width: 560, height: 400)
    private var window: NSWindow?

    public func show(state: some SettingsStateProviding) {
        if let window {
            NSApp.activate(ignoringOtherApps: true)
            window.makeKeyAndOrderFront(nil)
            return
        }
        let win = NSWindow(
            contentRect: NSRect(origin: .zero, size: Self.contentSize),
            styleMask: [.titled, .closable],
            backing: .buffered,
            defer: false
        )
        win.title = "设置"
        win.isReleasedWhenClosed = false
        win.contentViewController = NSHostingController(
            rootView: SettingsRootView(state: state)
        )
        win.setContentSize(Self.contentSize)
        win.setFrameAutosaveName("AeroricSettings")
        win.center()
        win.delegate = self
        window = win
        NSApp.activate(ignoringOtherApps: true)
        win.makeKeyAndOrderFront(nil)
    }

    nonisolated public func windowShouldClose(_ sender: NSWindow) -> Bool {
        Task { @MainActor in self.window?.orderOut(nil) }
        return false
    }
}
