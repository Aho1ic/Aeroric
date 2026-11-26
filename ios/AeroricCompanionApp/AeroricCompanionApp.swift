import AeroricMobileTransport
import AeroricMobileUI
import SwiftUI
import UserNotifications

/// AP6: APNs wiring for the companion app.
///
/// The delegate exists for one reason: turn a delivered notification into a
/// **hint** on `CompanionStore`, never into an action. The store's
/// `ingestPushHint` has no path to `pendingApprovals`, so a notification —
/// including one delivered while the app was suspended and replayed later —
/// cannot authorize anything (Q7, MIGRATION_PLAN §D AP6).
///
/// Registration itself happens only while the Mac is online: the device token is
/// uploaded over the authenticated channel, and a token bound while this device
/// is unauthorized would push notifications for work it may not see.
@MainActor
final class PushCoordinator: NSObject, ObservableObject, UNUserNotificationCenterDelegate {
    /// What the user should see when the OS refuses notifications. Kept as
    /// state rather than logged, because "notifications are off" is a thing the
    /// user can fix and therefore needs to be told about.
    @Published private(set) var authorization: PushAuthorization = .notDetermined
    @Published private(set) var registrationError: String?

    /// Ask for permission, then register with APNs. Called when the app becomes
    /// active and is already paired; asking earlier would prompt the user before
    /// there is anything to notify them about.
    func requestAuthorizationAndRegister() async {
        let center = UNUserNotificationCenter.current()
        do {
            let granted = try await center.requestAuthorization(options: [.alert, .sound, .badge])
            authorization = granted ? .authorized : .denied
            guard granted else { return }
            center.delegate = self
            // `registerForRemoteNotifications` must run on the main actor; the
            // callback lands in the app delegate below.
            UIApplication.shared.registerForRemoteNotifications()
        } catch {
            authorization = .denied
            registrationError = String(describing: error)
        }
    }

    /// Called by the app delegate with the APNs token. Upload happens only if
    /// the caller wired an E2EE upload; otherwise the error is surfaced rather
    /// than swallowed (a silently unregistered device looks identical to a
    /// broken sender).
    func handleDeviceToken(_ token: Data) async {
        guard let store else {
            registrationError = "no store attached; cannot address the pairing"
            return
        }
        do {
            // Registration goes through the store so the "Mac must be online"
            // gate and the device binding cannot diverge between callers.
            _ = try await store.registerPushToken(token)
            registrationError = nil
        } catch {
            registrationError = String(describing: error)
        }
    }

    func handleRegistrationFailure(_ error: Error) {
        registrationError = String(describing: error)
    }

    /// Stop pushing to this device (the user turned notifications off, or the
    /// Mac revoked it).
    ///
    /// Goes through the store for the same reason registration does: the device
    /// id and the online gate have one owner, and a second path could unbind a
    /// different device than the one the UI named.
    func unregister() async {
        guard let store else { return }
        _ = try? await store.unregisterPushToken()
    }

    // MARK: - UNUserNotificationCenterDelegate

    /// Foreground presentation: show the banner so the user notices, but the tap
    /// still has to go through `didReceive` below.
    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        completionHandler([.banner, .sound])
    }

    /// A tap routes the payload through the store's hint path. Anything that is
    /// not our minimal shape is ignored: a payload we cannot validate did not
    /// come from our sender.
    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        // `userInfo` is `[AnyHashable: Any]`; the parser works on the concrete
        // key type, so narrow it here instead of loosening the parser.
        let plain: [String: Any] = response.notification.request.content.userInfo
            .reduce(into: [:]) { partial, pair in
                guard let key = pair.key as? String else { return }
                partial[key] = pair.value
            }
        Task { @MainActor [weak self] in
            defer { completionHandler() }
            guard let self, let alert = PushAlert.parse(userInfo: plain),
                  let store = self.store else { return }
            _ = try? store.ingestPushHint(alert)
            // The hint asked for a re-check; the app reacts by refreshing over
            // E2EE. Nothing here executes or approves anything.
        }
    }

    /// Set by the app entry point; the delegate needs it to record hints.
    weak var store: CompanionStore?
}

@main
struct AeroricCompanionApp: App {
    /// B4-4: without the adaptor UIKit never calls the delegate, and the APNs
    /// token callback (`didRegisterForRemoteNotificationsWithDeviceToken`) is lost.
    @UIApplicationDelegateAdaptor(CompanionAppDelegate.self) private var appDelegate
    @State private var store = CompanionStore()
    /// D4 step 5: pairing, the keychain, and staying connected.
    @State private var connector: CompanionConnector?
    /// D4 step 7: a pairing link that launched the app before the connector existed.
    @State private var launchLink: String?
    @StateObject private var push = PushCoordinator()
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @Environment(\.scenePhase) private var scenePhase

    var body: some Scene {
        WindowGroup {
            CompanionRootView(
                store: store,
                connector: connector,
                isIPadLayout: horizontalSizeClass == .regular,
                reduceTransparency: reduceTransparency
            )
            .environmentObject(push)
            .onAppear {
                appDelegate.push = push
                push.store = store
                // D4 step 5: the connector wires the store's push upload to
                // `push.register` on the authenticated E2EE channel, and sends
                // what the store issues.
                if connector == nil {
                    let made = CompanionConnector(store: store, credentials: KeychainCredentialStore())
                    connector = made
                    made.start()
                    if let link = launchLink {
                        made.offerPairingLink(link)
                        launchLink = nil
                    }
                }
            }
            // D4 step 7: `aeroric://pair?code=…` only fills the pairing page.
            .onOpenURL { url in
                guard url.scheme?.lowercased() == "aeroric" else { return }
                if let connector {
                    connector.offerPairingLink(url.absoluteString)
                } else {
                    launchLink = url.absoluteString
                }
            }
        }
        .onChange(of: scenePhase) { _, phase in
            switch phase {
            case .active:
                connector?.start()
            case .background:
                Task { await connector?.stop(.background) }
            default:
                break
            }
        }
        .onChange(of: store.connectionState.isMacOnline) { _, online in
            // Register while the Mac is reachable. Registration is refused
            // (not queued) when it is offline — see `push_register`.
            guard online, scenePhase == .active else { return }
            Task { await push.requestAuthorizationAndRegister() }
        }
    }
}

/// App-delegate bridge for the two APNs callbacks, which are delivered outside
/// SwiftUI's lifecycle.
final class CompanionAppDelegate: NSObject, UIApplicationDelegate {
    /// Wired by the App after launch.
    weak var push: PushCoordinator?

    func application(
        _ application: UIApplication,
        didRegisterForRemoteNotificationsWithDeviceToken deviceToken: Data
    ) {
        Task { @MainActor [weak push] in
            guard let push else { return }
            // The device id is the host's own id for this pairing; ask the store
            // so both sides agree on which device is being bound.
            guard push.store?.pairedDeviceID != nil else { return }
            await push.handleDeviceToken(deviceToken)
        }
    }

    func application(
        _ application: UIApplication,
        didFailToRegisterForRemoteNotificationsWithError error: Error
    ) {
        Task { @MainActor [weak push] in
            push?.handleRegistrationFailure(error)
        }
    }
}