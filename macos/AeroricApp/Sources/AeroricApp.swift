import AeroricUI
import SwiftUI

/// Bootstrap inside the existing app target, not a second executable.
/// The DEBUG review decision happens BEFORE constructing the product App
/// or its delegate. ReleaseLocal only calls the unchanged product App.
@main
enum AeroricEntryPoint {
    @MainActor
    static func main() {
        #if DEBUG
        // M2 D2: offline session sample for visual review; no host, services
        // or product App are constructed.
        if SessionSampleReviewEntry.isActive(environment: ProcessInfo.processInfo.environment) {
            AeroricSessionSampleReviewApp.main()
            return
        }
        // Q103: decide once from an explicit dictionary before ANY older
        // gate (some inspect directories), App, delegate or product owner.
        let usageDecision = NativeUsageStatisticsReviewEntry.decide(environment: ProcessInfo.processInfo.environment)
        switch usageDecision {
        case .review, .refused:
            AeroricUsageStatisticsReviewApp.main(decision: usageDecision)
            return
        case .inactive:
            break
        }
        // Q93: a present tools/connections request owns review OR refusal
        // before every older review/product App, delegate and service owner.
        // Absence leaves the complete pre-existing chain unchanged.
        switch NativeToolsConnectionsReviewEntry.decide(environment: ProcessInfo.processInfo.environment) {
        case .review, .refused:
            AeroricToolsConnectionsReviewApp.main()
            return
        case .inactive:
            break
        }
        // Q82: task-deliverables review owns a present request or its
        // read-only refusal before any older review/product App, delegate or
        // services are constructed. Absence preserves the old chain.
        switch NativeTaskDeliverablesReviewEntry.decide(environment: ProcessInfo.processInfo.environment) {
        case .review, .refused:
            AeroricTaskDeliverablesReviewApp.main()
            return
        case .inactive:
            break
        }
        // Q75: an explicit communication request owns review OR refusal
        // before any old-review/product App, delegate or services exist.
        // Absence preserves the complete WorkArrangements → ... → product chain.
        switch NativeTaskCommunicationReviewEntry.decide(environment: ProcessInfo.processInfo.environment) {
        case .review, .refused:
            AeroricTaskCommunicationReviewApp.main()
            return
        case .inactive:
            break
        }
        // Q68: a present offline work-arrangements request owns review OR
        // refusal before any existing review/product owner is constructed.
        // Absence leaves the original Skill → Knowledge → Evolution → Visual chain.
        switch NativeWorkArrangementsReviewEntry.decide(environment: ProcessInfo.processInfo.environment) {
        case .review, .refused:
            AeroricWorkArrangementsReviewApp.main()
            return
        case .inactive:
            break
        }
        // Q50: a present skill request owns review OR refusal before any
        // existing review/product App or delegate is constructed. Absence
        // preserves the Knowledge → Evolution → Visual → product chain.
        switch NativeSkillReviewEntry.decide(environment: ProcessInfo.processInfo.environment) {
        case .review, .refused:
            AeroricSkillReviewApp.main()
            return
        case .inactive:
            break
        }
        // Q45: only an explicitly present knowledge request owns this
        // decision. Both outcomes return before ANY product/old-review App
        // or delegate is constructed; absence preserves the original chain.
        switch NativeKnowledgeReviewEntry.decide(environment: ProcessInfo.processInfo.environment) {
        case .review, .refused:
            AeroricKnowledgeReviewApp.main()
            return
        case .inactive:
            break
        }
        // Task-evolution review sample (Q26). Decided BEFORE the visual-review
        // branch and before any product App/delegate/services exists, so a
        // review request can never construct the product shell or the host.
        // An invalid or conflicting request shows the read-only refusal page;
        // it never falls back to the product.
        switch NativeEvolutionReviewEntry.decide(environment: ProcessInfo.processInfo.environment) {
        case .review, .refused:
            AeroricEvolutionReviewApp.main()
            return
        case .inactive:
            break
        }
        if NativeVisualReviewEntry.isActive(environment: ProcessInfo.processInfo.environment) {
            AeroricVisualReviewApp.main()
            return
        }
        #endif
        AeroricNativeApp.main()
    }
}

#if DEBUG
import AppKit

/// M2 D2 sample window: the real `SessionRouteView` over fixed data.
private struct AeroricSessionSampleReviewApp: App {
    var body: some Scene {
        WindowGroup("会话样例") { SessionSampleReview() }
    }
}

/// A minimal AppKit lifecycle bridge for the independent SwiftUI specimen.
/// Passing the original decision avoids a second environment read or an eager
/// default delegate/Store. No product App/delegate/commands are constructed.
private enum AeroricUsageStatisticsReviewApp {
    @MainActor
    static func main(decision: USReviewEntryDecision) {
        let lifecycle = NativeUsageStatisticsReviewWindowLifecycle(decision: decision)
        let application = NSApplication.shared
        application.setActivationPolicy(.regular)
        application.delegate = lifecycle
        // Standard native close/quit only; no product commands or settings.
        let menu = NSMenu()
        let appMenu = NSMenu()
        appMenu.addItem(withTitle: "退出使用统计样例", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        let appItem = NSMenuItem()
        appItem.submenu = appMenu
        menu.addItem(appItem)
        let windowMenu = NSMenu(title: "窗口")
        windowMenu.addItem(withTitle: "关闭窗口", action: #selector(NSWindow.performClose(_:)), keyEquivalent: "w")
        let windowItem = NSMenuItem()
        windowItem.submenu = windowMenu
        menu.addItem(windowItem)
        application.mainMenu = menu
        application.windowsMenu = windowMenu
        withExtendedLifetime(lifecycle) { application.run() }
    }
}

/// Offline, DEBUG-only command scene. Its dedicated lifecycle owns only the
/// review/refusal window and never constructs product services or settings.
private struct AeroricToolsConnectionsReviewApp: App {
    @NSApplicationDelegateAdaptor(NativeToolsConnectionsReviewWindowLifecycle.self) private var lifecycle

    var body: some Scene {
        Settings { EmptyView() }
            .commands {
                CommandGroup(replacing: .appSettings) {}
                CommandGroup(replacing: .newItem) {}
                CommandGroup(before: .windowArrangement) {
                    Button(lifecycle.windowTitle) { lifecycle.showReviewWindow() }
                }
            }
    }
}

/// The deliverables specimen owns only its standard window and fictional
/// in-memory Store. It never constructs product services or business menus.
private struct AeroricTaskDeliverablesReviewApp: App {
    @NSApplicationDelegateAdaptor(NativeTaskDeliverablesReviewWindowLifecycle.self) private var lifecycle

    var body: some Scene {
        Settings { EmptyView() }
            .commands {
                CommandGroup(replacing: .appSettings) {}
                CommandGroup(replacing: .newItem) {}
                CommandGroup(before: .windowArrangement) {
                    Button(lifecycle.windowTitle) { lifecycle.showReviewWindow() }
                }
            }
    }
}

/// The communication specimen owns only its window and local memory.
/// No product settings, business commands or old-review lifecycle is installed.
private struct AeroricTaskCommunicationReviewApp: App {
    @NSApplicationDelegateAdaptor(NativeTaskCommunicationReviewWindowLifecycle.self) private var lifecycle

    var body: some Scene {
        Settings { EmptyView() }
            .commands {
                CommandGroup(replacing: .appSettings) {}
                CommandGroup(replacing: .newItem) {}
                CommandGroup(before: .windowArrangement) {
                    Button(lifecycle.windowTitle) { lifecycle.showReviewWindow() }
                }
            }
    }
}

/// DEBUG-only scene without product settings, services or business menus.
/// Its own lifecycle releases the root/Store when the sample window closes.
private struct AeroricWorkArrangementsReviewApp: App {
    @NSApplicationDelegateAdaptor(NativeWorkArrangementsReviewWindowLifecycle.self) private var lifecycle

    var body: some Scene {
        Settings { EmptyView() }
            .commands {
                CommandGroup(replacing: .appSettings) {}
                CommandGroup(replacing: .newItem) {}
                CommandGroup(before: .windowArrangement) {
                    Button(lifecycle.windowTitle) { lifecycle.showReviewWindow() }
                }
            }
    }
}

/// The skill-only delegate owns its review/refusal window and fresh memory.
/// This command scene never constructs product services or settings.
private struct AeroricSkillReviewApp: App {
    @NSApplicationDelegateAdaptor(NativeSkillReviewWindowLifecycle.self) private var lifecycle

    var body: some Scene {
        Settings {
            EmptyView()
        }
        .commands {
            CommandGroup(replacing: .appSettings) {}
            CommandGroup(replacing: .newItem) {}
            CommandGroup(before: .windowArrangement) {
                Button(lifecycle.windowTitle) { lifecycle.showReviewWindow() }
            }
        }
    }
}

/// Only the knowledge entry's lifecycle owns its native hosting window.
/// Settings supplies a SwiftUI command scene, never a product settings route.
private struct AeroricKnowledgeReviewApp: App {
    @NSApplicationDelegateAdaptor(NativeKnowledgeReviewWindowLifecycle.self) private var lifecycle

    var body: some Scene {
        Settings {
            EmptyView()
        }
        .commands {
            CommandGroup(replacing: .appSettings) {}
            CommandGroup(replacing: .newItem) {}
            CommandGroup(before: .windowArrangement) {
                Button(lifecycle.windowTitle) { lifecycle.showReviewWindow() }
            }
        }
    }
}

private struct AeroricVisualReviewApp: App {
    @NSApplicationDelegateAdaptor(NativeVisualReviewWindowLifecycle.self) private var lifecycle

    var body: some Scene {
        Settings {
            EmptyView()
        }
        .commands {
            CommandGroup(replacing: .appSettings) {}
            CommandGroup(replacing: .newItem) {}
            CommandGroup(before: .windowArrangement) {
                Button(lifecycle.windowTitle) { lifecycle.showReviewWindow() }
            }
        }
    }
}

/// Isolated task-evolution review entry. Its DEBUG-only delegate owns only
/// the review/refusal window, never the product AppDelegate or services.
private struct AeroricEvolutionReviewApp: App {
    @NSApplicationDelegateAdaptor(NativeEvolutionReviewWindowLifecycle.self) private var lifecycle

    var body: some Scene {
        Settings {
            EmptyView()
        }
        .commands {
            CommandGroup(replacing: .appSettings) {}
            CommandGroup(replacing: .newItem) {}
            CommandGroup(before: .windowArrangement) {
                Button(lifecycle.windowTitle) { lifecycle.showReviewWindow() }
            }
        }
    }
}
#endif
/// SwiftUI entry point (P4-01 bootstrap shell, P4-04 real workspace shell).
/// The main window renders the shell from `WorkspaceRootView`; scene state
/// (selection, palette) is per-window `@State` inside that view, and the
/// menu commands (`AeroricCommands`) route through the focused window's
/// focused values, so with two windows open an operation lands on the one
/// the user is actually looking at.
struct AeroricNativeApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var appDelegate
    @Environment(\.scenePhase) private var scenePhase
    @State private var services: AppServices
    @AppStorage(OnboardingFlow.completedKey) private var onboardingCompleted = false

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
            // M4: first-launch onboarding once the host is ready.
            .sheet(isPresented: Binding(
                get: { OnboardingFlow.shouldPresent(completed: onboardingCompleted, phase: services.phaseSnapshot) },
                set: { if !$0 { onboardingCompleted = true } }
            )) {
                OnboardingView(
                    book: services.importBook,
                    projects: services.taskStore.projects,
                    dataDirectory: services.dataDirectory.path,
                    onOpenProject: { WorkspaceActions.openProjectPanel(services: services) },
                    onFinish: { onboardingCompleted = true }
                )
            }
            // The supervisor owns its own run loop; startSupervisor is
            // idempotent, so re-activation after sleep/logout is a no-op
            // while it is already up.
            .task {
                await services.startSupervisor()
                await services.openInjectedTestProjectsIfRequested()
            }
            // A16: `aeroric://task/<id>`, `aeroric://attention`.
            .onOpenURL { url in
                switch AeroricDeepLink.target(url) {
                case let .task(id)?: services.requestedSelection = SessionNavigation.task(id)
                case .attention?: services.requestedSelection = WorkspaceSection.attention.sidebarID
                case nil: break
                }
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
