import AppKit
import SwiftUI
import AeroricUI

/// Shared command execution (P4-04 Action 4). Both the menu commands and
/// the command palette route through here, so a command id has exactly one
/// behaviour regardless of which surface invoked it. Commands that target a
/// project bind through the focused window's `workspaceSelection`
/// (`@FocusedValue`), so with multiple windows open an operation lands on
/// the project the user is actually looking at — never a global default.
@MainActor
enum WorkspaceActions {
    static func route(_ services: AppServices) -> (PaletteCommand) -> Void {
        { command in
            switch command.id {
            // The shell handles task.new using its window-local selection.
            case "project.open": openProjectPanel(services: services)
            case "settings.show": SettingsWindowController.shared.show(state: services)
            default:
                break
            }
        }
    }

    /// Directory picker → `project.open` → sidebar upsert → focused window
    /// selects the new project. Cancelling the panel creates nothing
    /// (P4-04 Action 5).
    static func openProjectPanel(services: AppServices, selection: Binding<String?>? = nil) {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.prompt = "打开"
        panel.message = "选择要打开的项目目录"
        guard panel.runModal() == .OK, let url = panel.url else { return }
        Task { @MainActor in
            do {
                let entry = try await services.authorizeAndOpenProject(url: url)
                // Without a window binding (palette, sidebar button) the
                // focused window picks the request up.
                if let selection { selection.wrappedValue = entry.id } else { services.requestedSelection = entry.id }
            } catch {
                presentOpenProjectFailure(error)
            }
        }
    }

    /// Bring an existing main window to front, or recreate one through the
    /// SwiftUI `openWindow(id:)` bridge when the user has closed every
    /// window (red light). Settings windows are excluded by their autosave
    /// name so "Show Main Window" never keys the settings panel (the
    /// primuse lesson, MacMenuBarController :157).
    static func showMainWindow() {
        NSApp.activate(ignoringOtherApps: true)
        if let main = NSApp.windows.first(where: { window in
            guard window.canBecomeMain, !(window is NSPanel) else { return false }
            return window.frameAutosaveName != "AeroricSettings"
        }) {
            if main.isMiniaturized { main.deminiaturize(nil) }
            main.makeKeyAndOrderFront(nil)
            return
        }
        AeroricWindowOpener.openMain()
    }

    /// Opens one more main-window scene. The WindowGroup already exists, so
    /// this never creates a second data writer — every window shares the
    /// single supervisor/connection owned by `AppServices`.
    static func openAnotherMainWindow() {
        AeroricWindowOpener.openMain()
    }

    private static func presentOpenProjectFailure(_ error: Error) {
        let alert = NSAlert()
        alert.messageText = "无法打开项目"
        alert.informativeText = String(describing: error)
        alert.runModal()
    }
}

/// Menu-bar commands (P4-04 Action 4): New Task / Open Project / Settings /
/// Close Window / Show Main Window / New Window / Command Palette.
struct AeroricCommands: Commands {
    let services: AppServices

    @FocusedValue(\.palettePresentation) private var palettePresentation
    @FocusedValue(\.workspaceSelection) private var workspaceSelection
    @FocusedValue(\.workspaceActivity) private var workspaceActivity
    @FocusedValue(\.sidebarCollapse) private var sidebarCollapse
    @FocusedValue(\.newSessionAction) private var newSessionAction

    var body: some Commands {
        CommandGroup(replacing: .newItem) {
            Button("新建会话") { newSessionAction?() }
            .keyboardShortcut("n")
            .disabled(newSessionAction == nil)

            Button("打开项目…") {
                WorkspaceActions.openProjectPanel(
                    services: services,
                    selection: workspaceSelection
                )
            }
            .keyboardShortcut("o")
            .disabled(workspaceActivity != true)

            Button("新建窗口") {
                WorkspaceActions.openAnotherMainWindow()
            }
            .keyboardShortcut("n", modifiers: [.command, .shift])

            Divider()

            Button("设置…") {
                SettingsWindowController.shared.show(state: services)
            }
            .keyboardShortcut(",")

            Button("显示主窗口") {
                WorkspaceActions.showMainWindow()
            }

            Button("关闭窗口") {
                NSApp.keyWindow?.performClose(nil)
            }
            .keyboardShortcut("w")
        }

        CommandGroup(after: .toolbar) {
            Button("命令面板…") {
                palettePresentation?.wrappedValue = true
            }
            .keyboardShortcut("k")
            .disabled(palettePresentation == nil)

            Divider()

            // ⌃⌘S is the system's sidebar-toggle chord (Finder, Mail, Notes
            // all use it), so it is not a new convention to learn. The
            // command targets the focused window through the same
            // focused-value channel as the palette, so with several windows
            // open it collapses the one being looked at.
            Button(sidebarCollapse?.wrappedValue == true ? "展开侧栏" : "折叠侧栏") {
                sidebarCollapse?.wrappedValue.toggle()
            }
            .keyboardShortcut("s", modifiers: [.command, .control])
            .disabled(sidebarCollapse == nil)
        }
    }
}

/// Bridges `openWindow(id:)` from non-view code: the environment action can
/// only be read inside a view, so the scene content registers it once at
/// first paint (`.background(OpenWindowHook())` in the WindowGroup) and
/// command handlers forward through here.
@MainActor
enum AeroricWindowOpener {
    private static var environment: OpenWindowAction?

    static func register(_ environment: OpenWindowAction) {
        self.environment = environment
    }

    static func openMain() {
        environment?(id: "main")
    }
}

/// Tiny view that captures `@Environment(\.openWindow)` for the opener.
struct OpenWindowHook: View {
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Color.clear
            .frame(width: 0, height: 0)
            .onAppear {
                AeroricWindowOpener.register(openWindow)
            }
    }
}
