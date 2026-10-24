#if DEBUG
import AppKit
import SwiftUI

// Task-evolution review sample — DEBUG entry decision and read-only refusal
// page (Q26; execution package §4).
//
// The decision runs in `AeroricEntryPoint.main` BEFORE any product `App`,
// `AppDelegate` or `AppServices` is constructed, so a review request can never
// start the host or the product workspace. This file constructs no product
// service of any kind.
//
// The isolated-directory check is the existing, read-only
// `NativeVisualReviewEntry.isIsolatedDataDirectory` — the same symlink- and
// prefix-aware policy the stage-01 review window uses. This file deliberately
// does not add a looser prefix test of its own.
//
// Compiled out of ReleaseLocal: a release binary has no review entry and no
// refusal page, ignores the review variables and takes the original product
// path.

/// Why a review request was refused. Titles are readable and deliberately
/// contain no path, host name or other sensitive value — the refusal page must
/// be safe to screenshot.
public enum EvolutionReviewRefusalReason: String, CaseIterable, Identifiable, Sendable {
    case flagValueNotOne
    case environmentNotUITest
    case dataDirectoryMissing
    case dataDirectoryNotIsolated
    case legacyVisualReviewAlsoRequested

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .flagValueNotOne:
            "审阅开关存在但取值不是 1"
        case .environmentNotUITest:
            "运行环境不是 ui-test"
        case .dataDirectoryMissing:
            "未提供隔离数据目录"
        case .dataDirectoryNotIsolated:
            "数据目录不在安全 Caches 子目录内（含相对路径、..、根目录、前缀碰瓷或符号链接）"
        case .legacyVisualReviewAlsoRequested:
            "同时请求了旧的 visual-review 入口"
        }
    }
}

public enum EvolutionReviewEntryDecision: Equatable, Sendable {
    /// The new flag is absent: the original visual-review and product startup
    /// semantics stay exactly as they were.
    case inactive
    /// Explicit review request with valid conditions: enter the isolated
    /// review scene only.
    case review
    /// The flag is present but the request is invalid: show the read-only
    /// refusal page. Never fall back to the product, never start the host.
    case refused([EvolutionReviewRefusalReason])
}

/// Entry policy for the task-evolution review sample.
public enum NativeEvolutionReviewEntry {
    public static let flagKey = "AERORIC_NATIVE_EVOLUTION_REVIEW"
    public static let environmentKey = "AERORIC_ENV"
    public static let dataDirectoryKey = "AERORIC_DATA_DIR"
    public static let requiredEnvironment = "ui-test"
    public static let requiredFlagValue = "1"

    /// Pure decision function so the whole matrix is unit-testable without
    /// launching anything.
    public static func decide(environment: [String: String]) -> EvolutionReviewEntryDecision {
        // Absent flag: untouched original behaviour.
        guard let value = environment[flagKey] else { return .inactive }

        var reasons: [EvolutionReviewRefusalReason] = []
        if value != requiredFlagValue {
            reasons.append(.flagValueNotOne)
        }
        if environment[environmentKey] != requiredEnvironment {
            reasons.append(.environmentNotUITest)
        }
        if let directory = environment[dataDirectoryKey], !directory.isEmpty {
            if !NativeVisualReviewEntry.isIsolatedDataDirectory(directory) {
                reasons.append(.dataDirectoryNotIsolated)
            }
        } else {
            reasons.append(.dataDirectoryMissing)
        }
        // Both entries requested at once is refused explicitly instead of
        // silently picking one by code order.
        if environment[NativeVisualReviewEntry.flagKey] == NativeVisualReviewEntry.requiredFlagValue {
            reasons.append(.legacyVisualReviewAlsoRequested)
        }
        return reasons.isEmpty ? .review : .refused(reasons)
    }
}

/// Owns only the explicitly requested DEBUG review/refusal window. Creating
/// the window after launch does not depend on SwiftUI receiving a separate
/// scene-open event. The product delegate/services and other review entries
/// are never constructed here.
@MainActor
public final class NativeEvolutionReviewWindowLifecycle: NSObject, NSApplicationDelegate, NSWindowDelegate {
    private let decision: EvolutionReviewEntryDecision
    private var controller: NSWindowController?

    public override init() {
        decision = NativeEvolutionReviewEntry.decide(environment: ProcessInfo.processInfo.environment)
        super.init()
    }

    /// Policy-only unit-test seam. Initialization never creates an App or window.
    init(decision: EvolutionReviewEntryDecision) {
        self.decision = decision
        super.init()
    }

    public var windowTitle: String {
        switch decision {
        case .review: "任务内进化审阅样例"
        case .refused: "审阅未启动"
        case .inactive: ""
        }
    }

    var windowIdentifier: NSUserInterfaceItemIdentifier? {
        switch decision {
        case .review: NSUserInterfaceItemIdentifier("native-evolution-review")
        case .refused: NSUserInterfaceItemIdentifier("native-evolution-review-refusal")
        case .inactive: nil
        }
    }

    var isWindowCreated: Bool { controller?.window != nil }

    public func applicationDidFinishLaunching(_ notification: Notification) {
        showReviewWindow()
    }

    public func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true // This ephemeral, single-window review has no background work.
    }

    public func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        showReviewWindow()
        return false // Do not ask SwiftUI to open another default scene.
    }

    public func showReviewWindow() {
        // Even a directly constructed inactive lifecycle must stay inert.
        // Return before creating a window or accessing NSApplication.shared.
        guard let identifier = windowIdentifier else { return }
        if let window = controller?.window {
            if window.isMiniaturized { window.deminiaturize(nil) }
            window.makeKeyAndOrderFront(nil)
            NSApplication.shared.activate(ignoringOtherApps: true)
            return
        }

        let content: AnyView
        let size: CGSize
        switch decision {
        case .review:
            content = AnyView(NativeEvolutionReviewView())
            size = CGSize(width: 1260, height: 860)
        case let .refused(reasons):
            content = AnyView(NativeEvolutionReviewRefusalView(reasons: reasons))
            size = CGSize(width: 560, height: 400)
        case .inactive:
            return
        }
        let window = NSWindow(
            contentRect: CGRect(origin: .zero, size: size),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered, defer: false
        )
        window.title = windowTitle
        window.identifier = identifier
        window.isRestorable = false
        window.isReleasedWhenClosed = false
        window.delegate = self
        window.contentViewController = NSHostingController(rootView: content)
        window.center()
        controller = NSWindowController(window: window)
        window.makeKeyAndOrderFront(nil)
        NSApplication.shared.activate(ignoringOtherApps: true)
    }

    public func windowWillClose(_ notification: Notification) {
        guard let closing = notification.object as? NSWindow,
              controller?.window === closing else { return }
        controller = nil
    }
}

/// DEBUG-only, read-only refusal page. It states that the sample did not start
/// and lists the reasons without any sensitive path.
public struct NativeEvolutionReviewRefusalView: View {
    public let reasons: [EvolutionReviewRefusalReason]

    public init(reasons: [EvolutionReviewRefusalReason]) {
        self.reasons = reasons
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: AeroricSpace.l) {
            Text("审阅未启动")
                .font(.system(size: AeroricType.title, weight: .semibold))
                .accessibilityLabel("审阅未启动")
                .accessibilityValue(Text("审阅未启动"))
                .accessibilityIdentifier("evolutionReview.refusal.title")
            Text("本次请求未满足离线审阅的启动条件。未显示样例，未回落到产品工作区，未启动宿主。")
                .font(.system(size: AeroricType.body))
                .foregroundStyle(AeroricColor.textSecondary)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityLabel("拒绝说明")
                .accessibilityValue(Text("未显示样例，未回落到产品工作区，未启动宿主"))
                .accessibilityIdentifier("evolutionReview.refusal.explanation")
            VStack(alignment: .leading, spacing: AeroricSpace.m) {
                Text("原因")
                    .font(.system(size: AeroricType.caption, weight: .semibold))
                    .foregroundStyle(AeroricColor.textTertiary)
                ForEach(reasons) { reason in
                    HStack(alignment: .firstTextBaseline, spacing: AeroricSpace.m) {
                        Image(systemName: "xmark.circle")
                            .foregroundStyle(AeroricColor.danger)
                        Text(reason.title)
                            .font(.system(size: AeroricType.secondary))
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    .accessibilityElement(children: .combine)
                    .accessibilityLabel(Text(reason.title))
                    .accessibilityIdentifier("evolutionReview.refusal.reason.\(reason.rawValue)")
                }
            }
            .padding(AeroricSpace.m)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(AeroricColor.layer1,
                        in: RoundedRectangle(cornerRadius: AeroricRadius.s, style: .continuous))
            Text("DEBUG 限定 · 只读页面 · 不启动宿主、不写真实配置")
                .font(.system(size: AeroricType.caption))
                .foregroundStyle(AeroricColor.textTertiary)
                .accessibilityLabel("隔离说明")
                .accessibilityValue(Text("只读 · 不启动宿主、不写真实配置"))
                .accessibilityIdentifier("evolutionReview.refusal.isolation")
            Text("审阅进程：\(String(ProcessInfo.processInfo.processIdentifier))")
                .font(.system(size: AeroricType.caption))
                .monospacedDigit()
                .foregroundStyle(AeroricColor.textTertiary)
                .accessibilityLabel("审阅进程")
                .accessibilityValue(Text(String(ProcessInfo.processInfo.processIdentifier)))
                .accessibilityIdentifier("evolutionReview.process.pid")
        }
        .padding(AeroricSpace.xl)
        .frame(minWidth: 420, minHeight: 300, alignment: .topLeading)
        .background(NativePanelFoundation(preference: .classic).ignoresSafeArea())
    }
}
#endif
