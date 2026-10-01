import SwiftUI

/// SwiftUI entry point (P4-01). Shows only the bootstrap surface; P4-02 injects
/// the host supervisor/client into AppServices, P4-04 builds the real shell
/// scenes (main window, settings window, menus).
@main
struct AeroricNativeApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @State private var services: AppServices

    init() {
        // App.init runs on the main actor, so resolving the injected preview
        // configuration here is the MainActor-safe construction point.
        _services = State(initialValue: AppServices.resolvingInjectedPreviewConfiguration())
    }

    var body: some Scene {
        WindowGroup {
            BootstrapView(services: services)
        }
    }
}
