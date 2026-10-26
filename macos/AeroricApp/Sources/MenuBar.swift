import AeroricUI
import AppKit
import Observation

/// A16: the menu bar item — what is running and waiting, whether the Mac is
/// being kept awake, and a way back into the app. The host keeps running tasks
/// with every window closed; this is how the user sees that.
///
/// An AppKit status item rather than SwiftUI's `MenuBarExtra(isInserted:)`:
/// next to this app's main window that scene re-rendered the whole window
/// graph forever (main thread at 100%, the app never responded), even with a
/// static label. Measured 2026-10-26; a plain `MenuBarExtra` without
/// `isInserted` did not loop, but then the Settings switch could not hide it.
@MainActor
final class MenuBarController: NSObject, NSMenuDelegate {
    private let services: AppServices
    private var item: NSStatusItem?
    private var defaultsObserver: NSObjectProtocol?

    init(services: AppServices) {
        self.services = services
    }

    func start() {
        reconcile()
        defaultsObserver = NotificationCenter.default.addObserver(
            forName: UserDefaults.didChangeNotification, object: nil, queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.reconcile() }
        }
    }

    /// Show or hide the item to match Settings → 通用 → 菜单栏.
    private func reconcile() {
        if MenuBarPreference.isShown {
            guard item == nil else { return }
            let item = NSStatusBar.system.statusItem(withLength: NSStatusItem.squareLength)
            let menu = NSMenu()
            menu.delegate = self
            item.menu = menu
            self.item = item
            trackIcon()
        } else if let item {
            NSStatusBar.system.removeStatusItem(item)
            self.item = nil
        }
    }

    /// The icon carries a badge while something waits on the user; re-arms
    /// itself on every change it observed.
    private func trackIcon() {
        guard let button = item?.button else { return }
        let waiting = withObservationTracking {
            (services.activityCounts?.waiting ?? 0) > 0
        } onChange: { [weak self] in
            Task { @MainActor in self?.trackIcon() }
        }
        button.image = NSImage(systemSymbolName: waiting ? "bell.badge" : "bolt.horizontal.circle",
                               accessibilityDescription: "Aeroric")
    }

    /// Built fresh each time the menu opens, so it never shows stale counts.
    func menuNeedsUpdate(_ menu: NSMenu) {
        menu.removeAllItems()
        let counts = services.activityCounts
        menu.addItem(info("运行中 \(counts?.running ?? 0) · 待回应 \(counts?.waiting ?? 0)"))
        if services.keepingAwake {
            menu.addItem(info("正在阻止 Mac 睡眠"))
        }
        menu.addItem(.separator())
        if let attention = services.attentionCount {
            menu.addItem(action("需要我处理（\(attention)）…", #selector(openAttention)))
        }
        menu.addItem(action("打开 Aeroric", #selector(openMain)))
        menu.addItem(.separator())
        let quit = action("退出 Aeroric", #selector(quit))
        quit.keyEquivalent = "q"
        menu.addItem(quit)
    }

    private func info(_ title: String) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
        item.isEnabled = false
        return item
    }

    private func action(_ title: String, _ selector: Selector) -> NSMenuItem {
        let item = NSMenuItem(title: title, action: selector, keyEquivalent: "")
        item.target = self
        return item
    }

    @objc private func openAttention() {
        WorkspaceActions.showMainWindow()
        services.requestedSelection = WorkspaceSection.attention.sidebarID
    }

    @objc private func openMain() {
        WorkspaceActions.showMainWindow()
    }

    @objc private func quit() {
        NSApp.terminate(nil)
    }
}

enum MenuBarPreference {
    /// Shared with `MenuBarSettingsCard`'s `@AppStorage`; on by default.
    static let shownKey = "menuBarExtra.shown"

    static var isShown: Bool {
        UserDefaults.standard.object(forKey: shownKey) as? Bool ?? true
    }
}
