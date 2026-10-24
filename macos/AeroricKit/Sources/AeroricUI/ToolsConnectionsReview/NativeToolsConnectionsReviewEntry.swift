#if DEBUG
import AppKit
import Foundation
import SwiftUI

/// Fixed explanations only; request values, paths and diagnostics are never retained.
public enum ToolsConnectionsReviewRefusalReason: String, CaseIterable, Equatable, Identifiable,
    Sendable
{
    case invalidFlag
    case invalidEnvironment
    case invalidDirectory
    case conflictingReview

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .invalidFlag: "工具库与连接审阅开关无效"
        case .invalidEnvironment: "测试环境标签不符"
        case .invalidDirectory: "未提供有效的独立缓存子目录"
        case .conflictingReview: "同时请求了其他审阅样例"
        }
    }

    public var detail: String {
        switch self {
        case .invalidFlag:
            "显式工具库与连接审阅请求只接受精确值 1；关闭本请求请移除该变量。"
        case .invalidEnvironment:
            "工具库与连接样例只接受 ui-test 环境标签；标签不是 UI 运行许可，拒绝请求不会回落产品或其他审阅。"
        case .invalidDirectory:
            "需要显式指定 Caches 内的严格子目录；不接受缓存根、相对路径、点路径段，以及候选或缓存以下沿途的符号链接和非目录。校验只读取元数据，不创建目录。"
        case .conflictingReview:
            "工具库与连接审阅不能与成果、沟通、工作安排、技能、知识、进化、视觉或协作审阅同时请求；不按入口顺序挑选样例。"
        }
    }
}

public enum ToolsConnectionsReviewEntryDecision: Equatable, Sendable {
    case inactive
    case review
    case refused([ToolsConnectionsReviewRefusalReason])
}

/// Absence preserves the old entry chain. A present request owns review or
/// refusal before any old-review or product owner can be constructed.
public enum NativeToolsConnectionsReviewEntry {
    public static let flagKey = "AERORIC_NATIVE_TOOLS_CONNECTIONS_REVIEW"

    public static let otherReviewKeys = [
        "AERORIC_NATIVE_TASK_DELIVERABLES_REVIEW",
        "AERORIC_NATIVE_TASK_COMMUNICATION_REVIEW",
        "AERORIC_NATIVE_WORK_ARRANGEMENTS_REVIEW",
        "AERORIC_NATIVE_SKILL_REVIEW",
        "AERORIC_NATIVE_KNOWLEDGE_REVIEW",
        "AERORIC_NATIVE_EVOLUTION_REVIEW",
        "AERORIC_NATIVE_VISUAL_REVIEW",
        "AERORIC_NATIVE_COLLABORATION_REVIEW",
    ]

    public static func decide(
        environment: [String: String],
        home: String = NSHomeDirectory()
    ) -> ToolsConnectionsReviewEntryDecision {
        guard let requested = environment[flagKey] else { return .inactive }

        var reasons: [ToolsConnectionsReviewRefusalReason] = []
        if requested != "1" { reasons.append(.invalidFlag) }
        if environment["AERORIC_ENV"] != "ui-test" {
            reasons.append(.invalidEnvironment)
        }
        if otherReviewKeys.contains(where: { environment[$0] == "1" }) {
            reasons.append(.conflictingReview)
        }

        // This unchanged helper is the sole file-access path: read-only
        // metadata below Caches, not directory creation or sandbox certification.
        let directoryIsIsolated = environment["AERORIC_DATA_DIR"].map {
            NativeVisualReviewEntry.isIsolatedDataDirectory($0, home: home)
        } ?? false
        if !directoryIsIsolated { reasons.append(.invalidDirectory) }

        return reasons.isEmpty ? .review : .refused(reasons)
    }
}

/// Initialization stores policy only. Hosting roots are window-local and are
/// released on close, so reopening constructs a fresh root and its own Store.
@MainActor
public final class NativeToolsConnectionsReviewWindowLifecycle: NSObject, NSApplicationDelegate,
    NSWindowDelegate
{
    private let decision: ToolsConnectionsReviewEntryDecision
    private var controller: NSWindowController?

    public override init() {
        decision = NativeToolsConnectionsReviewEntry.decide(
            environment: ProcessInfo.processInfo.environment
        )
        super.init()
    }

    public init(decision: ToolsConnectionsReviewEntryDecision) {
        self.decision = decision
        super.init()
    }

    public var windowTitle: String {
        switch decision {
        case .review: "工具库与连接审阅样例"
        case .refused: "工具库与连接审阅未启动"
        case .inactive: ""
        }
    }

    var windowIdentifier: NSUserInterfaceItemIdentifier? {
        switch decision {
        case .review: NSUserInterfaceItemIdentifier("native-tools-connections-review")
        case .refused: NSUserInterfaceItemIdentifier("native-tools-connections-review-refusal")
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
            rootController = NSHostingController(rootView: NativeToolsConnectionsReviewView())
        case .refused(let reasons):
            rootController = NSHostingController(
                rootView: NativeToolsConnectionsReviewRefusalView(reasons: reasons)
            )
        case .inactive:
            return
        }

        // Match the standard review window's outer-frame dimensions.
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

/// Read-only refusal content; all leaves use fixed, readable explanations.
public struct NativeToolsConnectionsReviewRefusalView: View {
    private let reasons: [ToolsConnectionsReviewRefusalReason]

    public init(reasons: [ToolsConnectionsReviewRefusalReason]) {
        self.reasons = reasons
    }

    public var body: some View {
        GlassPanel(preference: .classic) {
            ScrollView {
                VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
                    Text("工具库与连接审阅未启动")
                        .font(.title2.bold())
                        .foregroundStyle(AeroricColor.warning)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("toolsConnectionsReview.refusal.title")
                    Text("只读拒绝页 · 未进入工具库与连接样例或产品工作区，未启动宿主。")
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("toolsConnectionsReview.refusal.explanation")
                    ForEach(reasons) { reason in
                        Text("\(reason.title)：\(reason.detail)")
                            .fixedSize(horizontal: false, vertical: true)
                            .accessibilityIdentifier(
                                "toolsConnectionsReview.refusal.reason.\(reason.rawValue)"
                            )
                    }
                    Text("本页不修改偏好或业务资料，不创建数据目录。目录元数据校验不证明系统沙箱、秘密保护或偏好完全隔离；测试标签不授予 App 或窗口运行许可。")
                        .foregroundStyle(AeroricColor.textSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("toolsConnectionsReview.refusal.isolation")
                }
                .textSelection(.enabled)
                .padding(NativePanelGeometry.panelInset * 2)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .padding(NativePanelGeometry.panelInset)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .foregroundStyle(AeroricColor.text)
        .background(NativePanelFoundation(preference: .classic))
    }
}
#endif
