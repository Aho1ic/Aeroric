#if DEBUG
import AppKit
import Foundation
import SwiftUI

/// Fixed sample-entry explanations, never request values, paths or diagnostics.
/// These reasons do not represent product permissions or UI-run authorization.
public enum TaskCommunicationReviewRefusalReason: String, CaseIterable, Equatable, Identifiable, Sendable {
    case invalidFlag
    case invalidEnvironment
    case invalidDirectory
    case conflictingReview

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .invalidFlag: "执行中沟通审阅开关无效"
        case .invalidEnvironment: "测试环境标签不符"
        case .invalidDirectory: "未提供有效的独立缓存子目录"
        case .conflictingReview: "同时请求了其他审阅样例"
        }
    }

    public var detail: String {
        switch self {
        case .invalidFlag:
            "显式执行中沟通审阅请求只接受值 1；关闭本请求请移除该变量。"
        case .invalidEnvironment:
            "沟通样例只接受指定的隔离测试标签；标签不是解除运行限制的许可，本页不会回落到产品工作区。"
        case .invalidDirectory:
            "需要显式指定缓存内的独立子目录；不接受缓存根、相对路径、点路径段，以及候选或缓存以下沿途的符号链接和非目录。目录校验只读取元数据，不创建目录。"
        case .conflictingReview:
            "执行中沟通、工作安排、技能、知识、进化、视觉和协作审阅不能同时请求；不按入口顺序挑选其他样例。"
        }
    }
}

public enum TaskCommunicationReviewEntryDecision: Equatable, Sendable {
    case inactive
    case review
    case refused([TaskCommunicationReviewRefusalReason])
}

/// Read-only decision before any product or old-review owner is constructed.
/// Absence leaves the old chain untouched. A present request owns review or
/// refusal; invalid flags, labels and conflicts never reach directory metadata.
public enum NativeTaskCommunicationReviewEntry {
    public static let flagKey = "AERORIC_NATIVE_TASK_COMMUNICATION_REVIEW"

    public static func decide(
        environment: [String: String], home: String = NSHomeDirectory()
    ) -> TaskCommunicationReviewEntryDecision {
        guard let requested = environment[flagKey] else { return .inactive }

        var reasons: [TaskCommunicationReviewRefusalReason] = []
        if requested != "1" { reasons.append(.invalidFlag) }
        if environment["AERORIC_ENV"] != "ui-test" { reasons.append(.invalidEnvironment) }
        let otherReviewKeys = [
            "AERORIC_NATIVE_WORK_ARRANGEMENTS_REVIEW",
            "AERORIC_NATIVE_SKILL_REVIEW",
            "AERORIC_NATIVE_KNOWLEDGE_REVIEW",
            "AERORIC_NATIVE_EVOLUTION_REVIEW",
            "AERORIC_NATIVE_VISUAL_REVIEW",
            "AERORIC_NATIVE_COLLABORATION_REVIEW",
        ]
        if otherReviewKeys.contains(where: { environment[$0] == "1" }) {
            reasons.append(.conflictingReview)
        }
        guard reasons.isEmpty else { return .refused(reasons) }

        // Reuse the helper unchanged, and only after the string-only gates.
        // It checks the candidate and ancestors below Caches, not Caches itself
        // or higher levels. This is not a sandbox or preference-domain boundary.
        guard let directory = environment["AERORIC_DATA_DIR"],
              NativeVisualReviewEntry.isIsolatedDataDirectory(directory, home: home) else {
            return .refused([.invalidDirectory])
        }
        return .review
    }
}

/// Owns only this sample's review/refusal window, with no business lifecycle.
/// Initialization never builds a root or window. Closing drops the hosting
/// root; a later show constructs a fresh root and its window-local Store.
@MainActor
public final class NativeTaskCommunicationReviewWindowLifecycle: NSObject, NSApplicationDelegate, NSWindowDelegate {
    private let decision: TaskCommunicationReviewEntryDecision
    private var controller: NSWindowController?

    public override init() {
        decision = NativeTaskCommunicationReviewEntry.decide(environment: ProcessInfo.processInfo.environment)
        super.init()
    }

    /// Policy-only test seam: no environment reads, root or window construction.
    init(decision: TaskCommunicationReviewEntryDecision) {
        self.decision = decision
        super.init()
    }

    public var windowTitle: String {
        switch decision {
        case .review: "执行中沟通审阅样例"
        case .refused: "执行中沟通审阅未启动"
        case .inactive: ""
        }
    }

    var windowIdentifier: NSUserInterfaceItemIdentifier? {
        switch decision {
        case .review: NSUserInterfaceItemIdentifier("native-task-communication-review")
        case .refused: NSUserInterfaceItemIdentifier("native-task-communication-review-refusal")
        case .inactive: nil
        }
    }

    var isWindowCreated: Bool { controller?.window != nil }

    public func applicationDidFinishLaunching(_ notification: Notification) {
        showReviewWindow()
    }

    public func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false // Reopening must allocate a fresh window-local root and Store.
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
            rootController = NSHostingController(rootView: NativeTaskCommunicationReviewView())
        case let .refused(reasons):
            rootController = NSHostingController(rootView: NativeTaskCommunicationReviewRefusalView(reasons: reasons))
        case .inactive:
            return
        }

        // Dimensions describe the standard window's outer frame.
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

/// Read-only refusal content: fixed explanations and leaf accessibility IDs.
/// No environment values, paths, preferences or external actions are exposed.
struct NativeTaskCommunicationReviewRefusalView: View {
    private let reasons: [TaskCommunicationReviewRefusalReason]

    init(reasons: [TaskCommunicationReviewRefusalReason]) {
        self.reasons = reasons
    }

    var body: some View {
        GlassPanel(preference: .glass) {
            ScrollView {
                VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
                    Text("执行中沟通审阅未启动")
                        .font(.title2.bold())
                        .foregroundStyle(Color.dsWarning)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("taskCommunicationReview.refusal.title")
                    Text("离线沟通示例 · 全部为虚构数据")
                        .foregroundStyle(Color.dsTextSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("taskCommunicationReview.refusal.fixture")
                    Text("只读拒绝页 · 未进入沟通样例或产品工作区，未启动宿主。")
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("taskCommunicationReview.refusal.explanation")
                    ForEach(reasons) { reason in
                        Text("\(reason.title)：\(reason.detail)")
                            .fixedSize(horizontal: false, vertical: true)
                            .accessibilityIdentifier("taskCommunicationReview.refusal.reason.\(reason.rawValue)")
                    }
                    Text("入口标签不授予运行权限；请遵守当前测试政策。本页不修改偏好或业务资料，也不会创建数据目录。目录校验不证明系统沙箱或偏好域完全隔离。")
                        .foregroundStyle(Color.dsTextSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("taskCommunicationReview.refusal.isolation")
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
        .background(NativePanelFoundation(preference: .glass))
    }
}
#endif
