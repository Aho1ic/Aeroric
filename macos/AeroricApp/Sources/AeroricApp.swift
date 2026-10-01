import SwiftUI

/// SwiftUI entry point (P4-01). Shows only the bootstrap surface; P4-02 injects
/// the host supervisor/client into AppServices, P4-04 builds the real shell
/// scenes (main window, settings window, menus).
@main
struct AeroricNativeApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @Environment(\.scenePhase) private var scenePhase
    @State private var services: AppServices

    init() {
        // App.init runs on the main actor, so resolving the injected preview
        // configuration here is the MainActor-safe construction point.
        _services = State(initialValue: AppServices.resolvingInjectedPreviewConfiguration())
    }

    var body: some Scene {
        WindowGroup {
            BootstrapView(services: services)
                // The supervisor owns its own run loop; startSupervisor is
                // idempotent, so re-activation after sleep/logout is a no-op
                // while it is already up.
                .task { await services.startSupervisor() }
                .onChange(of: scenePhase) { _, phase in
                    guard phase == .active else { return }
                    Task { await services.startSupervisor() }
                }
        }
    }
}
