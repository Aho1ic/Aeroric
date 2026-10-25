import AeroricUI
import SwiftUI

/// A16: the menu bar extra — what is running and waiting, whether the Mac is
/// being kept awake, and a way back into the app. The host keeps running tasks
/// with every window closed; this is how the user sees that.
struct MenuBarContent: View {
    let services: AppServices

    var body: some View {
        let counts = services.activityCounts
        Text("运行中 \(counts?.running ?? 0) · 待回应 \(counts?.waiting ?? 0)")
        if services.keepingAwake {
            Text("正在阻止 Mac 睡眠")
        }
        Divider()
        if let attention = services.attentionCount {
            Button("需要我处理（\(attention)）…") { open(WorkspaceSection.attention.sidebarID) }
        }
        Button("打开 Aeroric") { open(nil) }
        Divider()
        Button("退出 Aeroric") { NSApp.terminate(nil) }
            .keyboardShortcut("q")
    }

    private func open(_ selection: String?) {
        WorkspaceActions.showMainWindow()
        if let selection { services.requestedSelection = selection }
    }
}

/// The menu bar icon: a badge when something waits on the user.
struct MenuBarLabel: View {
    let services: AppServices

    var body: some View {
        Image(systemName: (services.activityCounts?.waiting ?? 0) > 0 ? "bell.badge" : "bolt.horizontal.circle")
            .accessibilityLabel("Aeroric")
    }
}

enum MenuBarPreference {
    /// `@AppStorage` key; on by default.
    static let shownKey = "menuBarExtra.shown"
}
