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
            WorkspaceRootView(state: services, onCommand: WorkspaceActions.route(services))
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
        .commands {
            AeroricCommands(services: services)
        }
    }
}
