#if DEBUG
import AppKit
import Foundation
import SwiftUI

/// Fixed, non-sensitive sample-entry explanations. No request values, paths
/// or runtime diagnostics are retained; these are not product permissions.
public enum WorkArrangementsReviewRefusalReason: String, CaseIterable, Equatable, Identifiable, Sendable {
    case invalidFlag
    case invalidEnvironment
    case invalidDirectory
    case conflictingReview

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .invalidFlag: "工作安排审阅开关无效"
        case .invalidEnvironment: "不是获准的测试环境"
        case .invalidDirectory: "未提供安全的独立缓存子目录"
        case .conflictingReview: "同时请求了其他审阅样例"
        }
    }

    public var detail: String {
        switch self {
        case .invalidFlag:
            "显式工作安排审阅请求只接受值 1；关闭本请求请移除该变量。"
        case .invalidEnvironment:
            "工作安排样例只接受隔离测试上下文；本页不会回落到产品工作区。"
        case .invalidDirectory:
            "需要显式指定缓存内的独立子目录；不接受缓存根、相对路径、点路径段，以及缓存子目录内候选或沿途的符号链接和非目录。仅检查元数据，未创建目录。"
        case .conflictingReview:
            "工作安排、技能、知识、进化、视觉和协作审阅不能同时请求；不按入口顺序挑选一个。"
        }
    }
}

public enum WorkArrangementsReviewEntryDecision: Equatable, Sendable {
    case inactive
    case review
    case refused([WorkArrangementsReviewRefusalReason])
}

/// Decides before any product App, delegate or services are constructed.
/// Absence leaves the old entry chain untouched; a present flag owns review
/// or refusal. The reused directory policy only reads filesystem metadata:
/// it is not a sandbox, preference-domain boundary or permission grant.
public enum NativeWorkArrangementsReviewEntry {
    public static let flagKey = "AERORIC_NATIVE_WORK_ARRANGEMENTS_REVIEW"

    public static func decide(
        environment: [String: String], home: String = NSHomeDirectory()
    ) -> WorkArrangementsReviewEntryDecision {
        guard let requested = environment[flagKey] else { return .inactive }

        var reasons: [WorkArrangementsReviewRefusalReason] = []
        if requested != "1" { reasons.append(.invalidFlag) }
        if environment["AERORIC_ENV"] != "ui-test" { reasons.append(.invalidEnvironment) }
        if let directory = environment["AERORIC_DATA_DIR"],
           NativeVisualReviewEntry.isIsolatedDataDirectory(directory, home: home) {
            // Keep the existing helper unchanged. It checks the candidate
            // and ancestors below Caches, not Caches itself or higher levels.
        } else {
            reasons.append(.invalidDirectory)
        }
        let otherReviewKeys = [
            "AERORIC_NATIVE_SKILL_REVIEW",
            "AERORIC_NATIVE_KNOWLEDGE_REVIEW",
            "AERORIC_NATIVE_EVOLUTION_REVIEW",
            "AERORIC_NATIVE_VISUAL_REVIEW",
            "AERORIC_NATIVE_COLLABORATION_REVIEW",
        ]
        if otherReviewKeys.contains(where: { environment[$0] == "1" }) {
            reasons.append(.conflictingReview)
        }
        return reasons.isEmpty ? .review : .refused(reasons)
    }
}

/// Owns only this sample's standard window, including its refusal route.
/// No product owner, shared instance, global traversal, activation or defaults.
/// Closing releases the hosting root; reopening creates a fresh View/Store.
@MainActor
public final class NativeWorkArrangementsReviewWindowLifecycle: NSObject, NSApplicationDelegate, NSWindowDelegate {
    private let decision: WorkArrangementsReviewEntryDecision
    private var controller: NSWindowController?

    public override init() {
        decision = NativeWorkArrangementsReviewEntry.decide(environment: ProcessInfo.processInfo.environment)
        super.init()
    }

    /// Policy-only test seam. Neither initialization path builds a root/window.
    init(decision: WorkArrangementsReviewEntryDecision) {
        self.decision = decision
        super.init()
    }

    public var windowTitle: String {
        switch decision {
        case .review: "工作安排审阅样例"
        case .refused: "工作安排审阅未启动"
        case .inactive: ""
        }
    }

    var windowIdentifier: NSUserInterfaceItemIdentifier? {
        switch decision {
        case .review: NSUserInterfaceItemIdentifier("native-work-arrangements-review")
        case .refused: NSUserInterfaceItemIdentifier("native-work-arrangements-review-refusal")
        case .inactive: nil
        }
    }

    var isWindowCreated: Bool { controller?.window != nil }

    public func applicationDidFinishLaunching(_ notification: Notification) {
        showReviewWindow()
    }

    public func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false // A reopen must allocate a fresh window-local root and Store.
    }

    public func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        showReviewWindow()
        return false
    }

    public func showReviewWindow() {
        guard let identifier = windowIdentifier else { return }
        if let window = controller?.window {
            if window.isMiniaturized { window.deminiaturize(nil) }
            window.makeKeyAndOrderFront(nil)
            return
        }

        let rootController: NSViewController
        switch decision {
        case .review:
            rootController = NSHostingController(rootView: NativeWorkArrangementsReviewView())
        case let .refused(reasons):
            rootController = NSHostingController(rootView: NativeWorkArrangementsReviewRefusalView(reasons: reasons))
        case .inactive:
            return
        }

        // These dimensions describe the outer frame, not the content rect.
        let initialFrame = CGRect(origin: .zero, size: CGSize(width: 1260, height: 860))
        let style: NSWindow.StyleMask = [.titled, .closable, .miniaturizable, .resizable]
        let window = NSWindow(
            contentRect: NSWindow.contentRect(forFrameRect: initialFrame, styleMask: style),
            styleMask: style, backing: .buffered, defer: false
        )
        window.title = windowTitle
        window.identifier = identifier
        window.isRestorable = false
        window.isReleasedWhenClosed = false
        window.minSize = CGSize(width: 400, height: 600)
        window.delegate = self
        window.contentViewController = rootController
        window.setFrame(initialFrame, display: false)
        window.center()
        controller = NSWindowController(window: window)
        window.makeKeyAndOrderFront(nil)
    }

    public func windowWillClose(_ notification: Notification) {
        guard let closing = notification.object as? NSWindow,
              controller?.window === closing else { return }
        closing.contentViewController = nil
        closing.delegate = nil
        controller = nil
    }
}

/// Read-only refusal content. Only fixed explanations and leaf identifiers
/// are displayed; no path, environment value or process/launch identifier.
public struct NativeWorkArrangementsReviewRefusalView: View {
    private let reasons: [WorkArrangementsReviewRefusalReason]

    public init(reasons: [WorkArrangementsReviewRefusalReason]) {
        self.reasons = reasons
    }

    public var body: some View {
        GlassPanel(preference: .classic) {
            ScrollView {
                VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
                    Text("工作安排审阅未启动")
                        .font(.title2.bold())
                        .foregroundStyle(Color.dsWarning)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("workArrangementsReview.refusal.title")
                    Text("离线工作安排示例 · 全部为虚构数据")
                        .foregroundStyle(Color.dsTextSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("workArrangementsReview.refusal.fixture")
                    Text("只读拒绝页 · 未进入工作安排样例或产品工作区，未启动宿主。")
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("workArrangementsReview.refusal.explanation")
                    ForEach(reasons) { reason in
                        Text("\(reason.title)：\(reason.detail)")
                            .fixedSize(horizontal: false, vertical: true)
                            .accessibilityIdentifier("workArrangementsReview.refusal.reason.\(reason.rawValue)")
                    }
                    Text("请在获准测试上下文中重新提供单一、有效的审阅请求。本页不修改偏好或业务资料，也不会创建数据目录。")
                        .foregroundStyle(Color.dsTextSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("workArrangementsReview.refusal.isolation")
                }
                .textSelection(.enabled)
                .padding(NativePanelGeometry.panelInset * 2)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .background(
                Color.dsTextBackground,
                in: RoundedRectangle(cornerRadius: NativePanelGeometry.panelRadius, style: .continuous)
            )
            .padding(NativePanelGeometry.panelInset)
        }
        .padding(NativePanelGeometry.panelInset)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .foregroundStyle(Color.dsText)
        .background(NativePanelFoundation(preference: .classic))
    }
}
#endif
