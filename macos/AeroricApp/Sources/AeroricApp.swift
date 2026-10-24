import AeroricUI
import SwiftUI

/// SwiftUI entry point (P4-01 bootstrap shell, P4-04 real workspace shell).
/// The main window renders the shell from `WorkspaceRootView`; scene state
/// (selection, palette) is per-window `@State` inside that view, and the
/// menu commands (`AeroricCommands`) route through the focused window's
/// focused values, so with two windows open an operation lands on the one
/// the user is actually looking at.
@main
struct AeroricNativeApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @Environment(\.scenePhase) private var scenePhase
    @State private var services: AppServices

    init() {
        AppFontRegistration.registerBundledFonts()
        // App.init runs on the main actor, so resolving the injected preview
        // configuration here is the MainActor-safe construction point.
        let services = AppServices.resolvingInjectedPreviewConfiguration()
        _services = State(initialValue: services)
        // The delegate stops the supervisor on termination (the serve child
        // must not outlive the app — see AppDelegate.applicationWillTerminate).
        appDelegate.services = services
    }

    var body: some Scene {
        WindowGroup(id: "main") {
            mainContent
        }
        .windowToolbarStyle(.unifiedCompact)
        .defaultSize(width: NativePanelGeometry.mainShell.defaultWindowWidth,
                     height: NativePanelGeometry.mainShell.defaultWindowHeight)
        .commands {
            AeroricCommands(services: services)
        }
    }

    /// The ordinary product shell, consuming the approved shared primitives.
    private var mainContent: some View {
        WorkspaceRootView(
            state: services,
            onCommand: WorkspaceActions.route(services),
            initialSelection: SessionNavigation.allTasks,
            // P5-03 接线：宿主 ready 后详情列切任务工作区（列表/详情/
            // 草稿/终端挂载）。侧栏选择由 shell 持有，这里按它决定渲染
            // 哪个工作面——shell 不知道行 id 的含义（INV-02 不变）。
            readyContent: { selection in
                AnyView(TaskWorkspaceView(services: services, selection: selection))
            }
        )
            // The supervisor owns its own run loop; startSupervisor is
            // idempotent, so re-activation after sleep/logout is a no-op
            // while it is already up.
            .task {
                await services.startSupervisor()
                await services.openInjectedTestProjectsIfRequested()
            }
            .onChange(of: scenePhase) { _, phase in
                guard phase == .active else { return }
                Task { await services.startSupervisor() }
            }
            // Captures `openWindow` once so menu commands can recreate the
            // main window after every window is closed (red light).
            .background(OpenWindowHook())
    }
}
