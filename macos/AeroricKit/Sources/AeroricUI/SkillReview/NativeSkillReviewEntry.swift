#if DEBUG
import AppKit
import Foundation
import SwiftUI

/// Fixed, screenshot-safe explanations: no request value or path is retained.
/// These are sample-entry reasons, not product permissions or protocol values.
public enum SkillReviewRefusalReason: String, CaseIterable, Equatable, Identifiable, Sendable {
    case invalidFlag
    case invalidEnvironment
    case invalidDirectory
    case conflictingReview

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .invalidFlag: "技能审阅开关无效"
        case .invalidEnvironment: "不是获准的测试环境"
        case .invalidDirectory: "未提供安全的独立缓存子目录"
        case .conflictingReview: "同时请求了其他审阅样例"
        }
    }

    public var detail: String {
        switch self {
        case .invalidFlag:
            "显式技能审阅请求只接受值 1；关闭本请求请移除该变量。"
        case .invalidEnvironment:
            "技能样例只接受 ui-test 环境；本页不会回落到产品工作区。"
        case .invalidDirectory:
            "需要显式指定 Caches 内的独立子目录；不接受根目录、相对路径、路径穿越、符号链接或非目录。未创建目录。"
        case .conflictingReview:
            "技能、知识、进化、视觉和协作审阅不能同时请求；不按入口顺序挑选一个。"
        }
    }
}

public enum SkillReviewEntryDecision: Equatable, Sendable {
    case inactive
    case review
    case refused([SkillReviewRefusalReason])
}

/// Runs before constructing a product App, delegate or services. A present
/// skill flag owns the request, including refusal; absence leaves the old
/// review/product chain untouched. Directory checks are read-only metadata
/// checks, not a sandbox, a preference-domain boundary or a permission grant.
public enum NativeSkillReviewEntry {
    public static let flagKey = "AERORIC_NATIVE_SKILL_REVIEW"
    /// Read-only XCTest correlation, NOT an entry/permission flag. Only a
    /// validated UUID may be exposed, never arbitrary injected text/paths.
    static func testLaunchID(environment: [String: String]) -> String {
        guard let value = environment["AERORIC_SKILL_REVIEW_TEST_LAUNCH_ID"],
              let uuid = UUID(uuidString: value) else { return "无测试启动标识" }
        return uuid.uuidString
    }

    public static func decide(
        environment: [String: String], home: String = NSHomeDirectory()
    ) -> SkillReviewEntryDecision {
        guard let requested = environment[flagKey] else { return .inactive }

        var reasons: [SkillReviewRefusalReason] = []
        if requested != "1" { reasons.append(.invalidFlag) }
        if environment["AERORIC_ENV"] != "ui-test" { reasons.append(.invalidEnvironment) }
        if let directory = environment["AERORIC_DATA_DIR"],
           NativeVisualReviewEntry.isIsolatedDataDirectory(directory, home: home) {
            // Reuse the existing strict policy; never create a data directory.
        } else {
            reasons.append(.invalidDirectory)
        }
        let otherReviewKeys = [
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

/// Owns only this sample's standard window. No product delegate, business
/// menus, global window traversal, input patches, activation or defaults.
/// Closing releases the hosting root; reopening allocates a fresh View/Store.
@MainActor
public final class NativeSkillReviewWindowLifecycle: NSObject, NSApplicationDelegate, NSWindowDelegate {
    private let decision: SkillReviewEntryDecision
    private var controller: NSWindowController?

    public override init() {
        decision = NativeSkillReviewEntry.decide(environment: ProcessInfo.processInfo.environment)
        super.init()
    }

    /// Policy-only test seam: initialization does not create an App or window.
    init(decision: SkillReviewEntryDecision) {
        self.decision = decision
        super.init()
    }

    public var windowTitle: String {
        switch decision {
        case .review: "技能审阅样例"
        case .refused: "技能审阅未启动"
        case .inactive: ""
        }
    }

    var windowIdentifier: NSUserInterfaceItemIdentifier? {
        switch decision {
        case .review: NSUserInterfaceItemIdentifier("native-skill-review")
        case .refused: NSUserInterfaceItemIdentifier("native-skill-review-refusal")
        case .inactive: nil
        }
    }

    var isWindowCreated: Bool { controller?.window != nil }

    public func applicationDidFinishLaunching(_ notification: Notification) {
        showReviewWindow()
    }

    public func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false // Reopen in this process must start with a fresh window-local Store.
    }

    public func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        showReviewWindow()
        return false // Handled here, not by a product/SwiftUI default window route.
    }

    public func showReviewWindow() {
        // An inactive lifecycle is inert, including direct calls by tests.
        // This file never accesses NSApplication.shared, even on active paths.
        guard let identifier = windowIdentifier else { return }
        if let window = controller?.window {
            if window.isMiniaturized { window.deminiaturize(nil) }
            window.makeKeyAndOrderFront(nil)
            return
        }

        let rootController: NSViewController
        switch decision {
        case .review:
            rootController = NSHostingController(rootView: NativeSkillReviewView())
        case let .refused(reasons):
            rootController = NSHostingController(rootView: NativeSkillReviewRefusalView(reasons: reasons))
        case .inactive:
            return
        }

        // The review dimensions describe the OUTER frame, not contentRect.
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

/// Read-only refusal content with leaf AX identifiers and no request-value
/// echo. Opaque text and vertical scrolling remain usable in a 400pt window.
public struct NativeSkillReviewRefusalView: View {
    private let reasons: [SkillReviewRefusalReason]

    public init(reasons: [SkillReviewRefusalReason]) {
        self.reasons = reasons
    }

    public var body: some View {
        GlassPanel(preference: .classic) {
            ScrollView {
                VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
                    Text("技能审阅未启动")
                        .font(.title2.bold())
                        .foregroundStyle(Color.dsWarning)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("skillReview.refusal.title")
                    Text("只读拒绝页 · 未进入技能样例或产品工作区，未启动宿主。")
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("skillReview.refusal.explanation")
                    ForEach(reasons) { reason in
                        Text("\(reason.title)：\(reason.detail)")
                            .fixedSize(horizontal: false, vertical: true)
                            .accessibilityIdentifier("skillReview.refusal.reason.\(reason.rawValue)")
                    }
                    Text("请在获准测试环境中重新提供单一、有效的审阅请求。本页不修改偏好或业务资料，也不会创建数据目录。")
                        .foregroundStyle(Color.dsTextSecondary)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("skillReview.refusal.isolation")
                    Text("审阅进程：\(String(ProcessInfo.processInfo.processIdentifier))")
                        .font(.caption)
                        .monospacedDigit()
                        .foregroundStyle(Color.dsTextSecondary)
                        .accessibilityLabel("审阅进程")
                        .accessibilityValue(Text(String(ProcessInfo.processInfo.processIdentifier)))
                        .accessibilityIdentifier("skillReview.process.pid")
                    Text(NativeSkillReviewEntry.testLaunchID(environment: ProcessInfo.processInfo.environment))
                        .font(.caption)
                        .accessibilityLabel("本次测试启动标识（非授权）")
                        .accessibilityValue(Text(NativeSkillReviewEntry.testLaunchID(environment: ProcessInfo.processInfo.environment)))
                        .accessibilityIdentifier("skillReview.process.launchID")
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
