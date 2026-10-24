#if DEBUG
import AppKit
import Foundation
import SwiftUI

public enum TaskDeliverablesReviewRefusalReason: String, CaseIterable, Equatable, Identifiable,
    Sendable
{
    case invalidFlag
    case invalidEnvironment
    case invalidDirectory
    case conflictingReview

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .invalidFlag: "成果审阅开关无效"
        case .invalidEnvironment: "不是获准的测试环境"
        case .invalidDirectory: "未提供安全的独立缓存子目录"
        case .conflictingReview: "同时请求了其他审阅样例"
        }
    }

    public var detail: String {
        switch self {
        case .invalidFlag:
            "显式成果审阅请求只接受值 1；关闭本请求请移除该变量。"
        case .invalidEnvironment:
            "成果样例只接受 ui-test 环境；环境标签不是 UI 运行许可。"
        case .invalidDirectory:
            "需要显式指定 Caches 内的独立子目录；不接受根目录、相对路径、路径穿越、符号链接或非目录。未创建目录。"
        case .conflictingReview:
            "成果、沟通、工作安排、技能、知识、进化、视觉和协作审阅不能同时请求；不按入口顺序挑选一个。"
        }
    }
}

public enum TaskDeliverablesReviewEntryDecision: Equatable, Sendable {
    case inactive
    case review
    case refused([TaskDeliverablesReviewRefusalReason])
}

/// The new key owns a present request before any existing review/product owner
/// is constructed. Missing means inactive; invalid means refusal, never fallback.
public enum NativeTaskDeliverablesReviewEntry {
    public static let flagKey = "AERORIC_NATIVE_TASK_DELIVERABLES_REVIEW"

    public static func decide(
        environment: [String: String],
        home: String = NSHomeDirectory()
    ) -> TaskDeliverablesReviewEntryDecision {
        guard let requested = environment[flagKey] else { return .inactive }

        var reasons: [TaskDeliverablesReviewRefusalReason] = []
        if requested != "1" { reasons.append(.invalidFlag) }
        if environment["AERORIC_ENV"] != "ui-test" {
            reasons.append(.invalidEnvironment)
        }

        let otherReviewKeys = [
            "AERORIC_NATIVE_TASK_COMMUNICATION_REVIEW",
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

        if let directory = environment["AERORIC_DATA_DIR"],
            NativeVisualReviewEntry.isIsolatedDataDirectory(directory, home: home)
        {
            // The directory is validated as metadata only. The entry point
            // never creates it and never falls back to a product data root.
        } else {
            reasons.append(.invalidDirectory)
        }

        guard reasons.isEmpty else { return .refused(reasons) }
        return .review
    }
}

/// Owns only the sample/refusal window. Tests can inspect policy metadata
/// through the injected initializer without constructing a window.
@MainActor
public final class NativeTaskDeliverablesReviewWindowLifecycle: NSObject, NSApplicationDelegate,
    NSWindowDelegate
{
    private let decision: TaskDeliverablesReviewEntryDecision
    private var controller: NSWindowController?

    public override init() {
        decision = NativeTaskDeliverablesReviewEntry.decide(
            environment: ProcessInfo.processInfo.environment
        )
        super.init()
    }

    init(decision: TaskDeliverablesReviewEntryDecision) {
        self.decision = decision
        super.init()
    }

    public var windowTitle: String {
        switch decision {
        case .review: "任务成果与交付审阅样例"
        case .refused: "任务成果审阅未启动"
        case .inactive: ""
        }
    }

    var windowIdentifier: NSUserInterfaceItemIdentifier? {
        switch decision {
        case .review: NSUserInterfaceItemIdentifier("native-task-deliverables-review")
        case .refused: NSUserInterfaceItemIdentifier("native-task-deliverables-review-refusal")
        case .inactive: nil
        }
    }

    var isWindowCreated: Bool { controller?.window != nil }

    public func applicationDidFinishLaunching(_ notification: Notification) {
        showReviewWindow()
    }

    public func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    public func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool)
        -> Bool
    {
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
            rootController = NSHostingController(rootView: NativeTaskDeliverablesReviewView())
        case .refused(let reasons):
            rootController = NSHostingController(
                rootView: NativeTaskDeliverablesReviewRefusalView(reasons: reasons)
            )
        case .inactive:
            return
        }

        let frame = CGRect(origin: .zero, size: CGSize(width: 1260, height: 860))
        let style: NSWindow.StyleMask = [.titled, .closable, .miniaturizable, .resizable]
        let window = NSWindow(
            contentRect: NSWindow.contentRect(forFrameRect: frame, styleMask: style),
            styleMask: style,
            backing: .buffered,
            defer: false
        )
        window.title = windowTitle
        window.identifier = identifier
        window.isRestorable = false
        window.isReleasedWhenClosed = false
        window.minSize = CGSize(width: 400, height: 600)
        window.delegate = self
        window.contentViewController = rootController
        window.setFrame(frame, display: false)
        window.center()
        controller = NSWindowController(window: window)
        window.makeKeyAndOrderFront(nil)
    }

    public func windowWillClose(_ notification: Notification) {
        guard let closing = notification.object as? NSWindow,
            controller?.window === closing
        else { return }
        closing.contentViewController = nil
        closing.delegate = nil
        controller = nil
    }
}

public struct NativeTaskDeliverablesReviewRefusalView: View {
    private let reasons: [TaskDeliverablesReviewRefusalReason]

    public init(reasons: [TaskDeliverablesReviewRefusalReason]) {
        self.reasons = reasons
    }

    public var body: some View {
        GlassPanel(preference: .classic) {
            ScrollView {
                VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
                    Text("任务成果审阅未启动")
                        .font(.title2.bold())
                        .foregroundStyle(AeroricColor.warning)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("taskDeliverablesReview.refusal.title")
                    Text("只读拒绝页 · 未进入成果样例或产品工作区，未启动宿主。")
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("taskDeliverablesReview.refusal.explanation")
                    ForEach(reasons) { reason in
                        Text("\(reason.title)：\(reason.detail)")
                            .fixedSize(horizontal: false, vertical: true)
                            .accessibilityIdentifier(
                                "taskDeliverablesReview.refusal.reason.\(reason.rawValue)"
                            )
                    }
                    Text("本页不修改偏好、业务资料或真实成果，也不会创建数据目录。")
                        .foregroundStyle(AeroricColor.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("taskDeliverablesReview.refusal.isolation")
                }
                .textSelection(.enabled)
                .padding(NativePanelGeometry.panelInset * 2)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .frame(minWidth: 400, minHeight: 300)
    }
}
#endif
