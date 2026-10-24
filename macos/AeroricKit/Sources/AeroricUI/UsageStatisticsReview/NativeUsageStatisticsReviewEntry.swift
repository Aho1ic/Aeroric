#if DEBUG
import AppKit
import Foundation
import SwiftUI

/// Fixed explanations only; injected values are never retained or displayed.
public enum USReviewRefusalReason: String, CaseIterable, Equatable, Identifiable, Sendable {
    case invalidFlag
    case conflictingReview

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .invalidFlag: "审阅请求无效"
        case .conflictingReview: "审阅模式存在冲突"
        }
    }

    public var detail: String {
        switch self {
        case .invalidFlag:
            "使用统计审阅开关只接受精确值 1；停用本请求请移除该变量。拒绝不会启动样例或回落产品。"
        case .conflictingReview:
            "存在其他审阅模式键时，本次请求会被拒绝；不按入口顺序选择模式，也不会回落旧样例或产品。"
        }
    }
}

public enum USReviewEntryDecision: Equatable, Sendable {
    case inactive
    case review
    case refused([USReviewRefusalReason])
}

/// Pure dictionary policy, independent of the process environment and data roots.
public enum NativeUsageStatisticsReviewEntry {
    public static let flagKey = "AERORIC_NATIVE_USAGE_STATISTICS_REVIEW"

    public static let otherReviewKeys = [
        "AERORIC_NATIVE_TOOLS_CONNECTIONS_REVIEW",
        "AERORIC_NATIVE_TASK_DELIVERABLES_REVIEW",
        "AERORIC_NATIVE_TASK_COMMUNICATION_REVIEW",
        "AERORIC_NATIVE_WORK_ARRANGEMENTS_REVIEW",
        "AERORIC_NATIVE_SKILL_REVIEW",
        "AERORIC_NATIVE_KNOWLEDGE_REVIEW",
        "AERORIC_NATIVE_EVOLUTION_REVIEW",
        "AERORIC_NATIVE_VISUAL_REVIEW",
        "AERORIC_NATIVE_COLLABORATION_REVIEW",
    ]

    public static func decide(environment: [String: String]) -> USReviewEntryDecision {
        guard let requested = environment[flagKey] else { return .inactive }

        var reasons: [USReviewRefusalReason] = []
        if requested != "1" { reasons.append(.invalidFlag) }
        if otherReviewKeys.contains(where: { environment[$0] != nil }) {
            reasons.append(.conflictingReview)
        }
        return reasons.isEmpty ? .review : .refused(reasons)
    }

    /// The lifecycle uses this same branch; refusal never invokes the owner factory.
    public static func constructOwner<Owner>(
        decision: USReviewEntryDecision,
        makeOwner: () -> Owner
    ) -> Owner? {
        guard case .review = decision else { return nil }
        return makeOwner()
    }
}

/// Initialization stores policy only. Closing releases this window's root;
/// reopening constructs a fresh review root and its independent in-memory owner.
@MainActor
public final class NativeUsageStatisticsReviewWindowLifecycle: NSObject, NSApplicationDelegate,
    NSWindowDelegate
{
    private let decision: USReviewEntryDecision
    private var controller: NSWindowController?

    public init(decision: USReviewEntryDecision) {
        self.decision = decision
        self.controller = nil
        super.init()
    }

    public var windowTitle: String {
        switch decision {
        case .review: "使用统计审阅样例"
        case .refused: "使用统计审阅未启动"
        case .inactive: ""
        }
    }

    var windowIdentifier: NSUserInterfaceItemIdentifier? {
        switch decision {
        case .review: NSUserInterfaceItemIdentifier("native-usage-statistics-review")
        case .refused: NSUserInterfaceItemIdentifier("native-usage-statistics-review-refusal")
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
        if let reviewController = NativeUsageStatisticsReviewEntry.constructOwner(
            decision: decision,
            makeOwner: { NSHostingController(rootView: NativeUsageStatisticsReviewView()) }
        ) {
            rootController = reviewController
        } else if case .refused(let reasons) = decision {
            rootController = NSHostingController(
                rootView: NativeUsageStatisticsReviewRefusalView(reasons: reasons)
            )
        } else {
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

/// Independent read-only refusal content; no review Store or fixtures are constructed.
public struct NativeUsageStatisticsReviewRefusalView: View {
    private let reasons: [USReviewRefusalReason]

    public init(reasons: [USReviewRefusalReason]) {
        self.reasons = reasons
    }

    public var body: some View {
        GlassPanel(preference: .classic) {
            ScrollView {
                VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
                    Text("使用统计审阅未启动")
                        .font(.title2.bold())
                        .foregroundStyle(Color.dsWarning)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("usReview.refusal.title")
                    Text("独立只读拒绝内容 · 未创建统计样例 owner，未回落旧样例或产品。")
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("usReview.refusal.explanation")
                    ForEach(reasons) { reason in
                        Text("\(reason.title)：\(reason.detail)")
                            .fixedSize(horizontal: false, vertical: true)
                            .accessibilityIdentifier(
                                "usReview.refusal.reason.\(reason.rawValue)"
                            )
                    }
                    Text("不回显任意环境值，不读取 home、数据目录或业务偏好，不连接真实服务。本页不授予 App、窗口或 UI 运行许可。")
                        .foregroundStyle(Color.dsTextSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("usReview.refusal.isolation")
                }
                .font(.body)
                .textSelection(.enabled)
                .padding(NativePanelGeometry.panelInset * 2)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .padding(NativePanelGeometry.panelInset)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .foregroundStyle(Color.dsText)
        .background(NativePanelFoundation(preference: .classic))
    }
}
#endif
