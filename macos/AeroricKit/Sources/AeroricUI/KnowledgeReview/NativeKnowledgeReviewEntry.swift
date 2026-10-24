#if DEBUG
import AppKit
import Foundation
import SwiftUI

/// Reasons describe the request, never echo a data path or environment value.
/// This is review plumbing, not a product permission or protocol enum.
public enum KnowledgeReviewRefusalReason: String, CaseIterable, Equatable, Identifiable {
    case invalidFlag
    case invalidEnvironment
    case invalidDirectory
    case conflictingReview

    public var id: String { rawValue }

    public var title: String {
        switch self {
        case .invalidFlag: "知识审阅开关无效"
        case .invalidEnvironment: "不是获准的测试环境"
        case .invalidDirectory: "未提供安全的独立缓存子目录"
        case .conflictingReview: "同时请求了其他审阅样例"
        }
    }

    public var detail: String {
        switch self {
        case .invalidFlag:
            "显式知识审阅请求只接受值 1；关闭本请求请移除该变量。"
        case .invalidEnvironment:
            "知识样例只接受 ui-test 环境；本页不会回落到产品工作区。"
        case .invalidDirectory:
            "需要显式指定 Caches 内的独立子目录；不接受根目录、路径穿越或符号链接。未创建目录。"
        case .conflictingReview:
            "知识、进化、视觉和协作审阅不能同时请求；不按入口顺序挑选一个。"
        }
    }
}

public enum KnowledgeReviewEntryDecision: Equatable {
    case inactive
    case review
    case refused([KnowledgeReviewRefusalReason])
}

/// Checked before constructing ANY product App, delegate or services.
/// Only an explicitly present knowledge flag gives this entry ownership
/// of the request. The directory guard performs read-only boundary checks.
public enum NativeKnowledgeReviewEntry {
    public static let flagKey = "AERORIC_NATIVE_KNOWLEDGE_REVIEW"

    public static func decide(environment: [String: String]) -> KnowledgeReviewEntryDecision {
        decide(environment: environment, home: NSHomeDirectory())
    }

    // A test seam for isolated temporary homes, not an environment override.
    // Runtime always uses the real account home through the public entry.
    static func decide(environment: [String: String], home: String) -> KnowledgeReviewEntryDecision {
        guard let requested = environment[flagKey] else { return .inactive }

        var reasons: [KnowledgeReviewRefusalReason] = []
        if requested != "1" { reasons.append(.invalidFlag) }
        if environment["AERORIC_ENV"] != "ui-test" { reasons.append(.invalidEnvironment) }
        if let directory = environment["AERORIC_DATA_DIR"],
           NativeVisualReviewEntry.isIsolatedDataDirectory(directory, home: home) {
            // This guard neither creates a directory nor isolates preferences.
        } else {
            reasons.append(.invalidDirectory)
        }
        let conflictingKeys = [
            "AERORIC_NATIVE_EVOLUTION_REVIEW",
            "AERORIC_NATIVE_VISUAL_REVIEW",
            "AERORIC_NATIVE_COLLABORATION_REVIEW",
        ]
        if conflictingKeys.contains(where: { environment[$0] == "1" }) {
            reasons.append(.conflictingReview)
        }
        return reasons.isEmpty ? .review : .refused(reasons)
    }
}

/// Owns only this DEBUG entry's standard window. A directly launched SwiftUI
/// process can finish launching without receiving the LaunchServices open event;
/// activation alone then exposes a menu but never instantiates its Window scene.
/// Explicit ownership also makes close/reopen allocate a fresh hosting root.
/// No product delegate/services, defaults, external processes or global windows.
@MainActor
public final class NativeKnowledgeReviewWindowLifecycle: NSObject, NSApplicationDelegate, NSWindowDelegate {
    private var controller: NSWindowController?

    public var windowTitle: String {
        switch NativeKnowledgeReviewEntry.decide(environment: ProcessInfo.processInfo.environment) {
        case .review: "项目知识审阅样例"
        case .refused: "知识审阅未启动"
        case .inactive: ""
        }
    }

    public func applicationDidFinishLaunching(_ notification: Notification) {
        showReviewWindow()
    }

    public func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        false
    }

    public func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows: Bool) -> Bool {
        showReviewWindow()
        return false // Handled here; do not ask SwiftUI for another default window.
    }

    public func showReviewWindow() {
        if let window = controller?.window {
            if window.isMiniaturized { window.deminiaturize(nil) }
            window.makeKeyAndOrderFront(nil)
            NSApplication.shared.activate(ignoringOtherApps: true)
            return
        }
        let content: AnyView
        let title: String
        let size: CGSize
        switch NativeKnowledgeReviewEntry.decide(environment: ProcessInfo.processInfo.environment) {
        case .review:
            content = AnyView(NativeKnowledgeReviewView())
            title = "项目知识审阅样例"
            size = CGSize(width: 1260, height: 860)
        case let .refused(reasons):
            content = AnyView(NativeKnowledgeReviewRefusalView(reasons: reasons))
            title = "知识审阅未启动"
            size = CGSize(width: 560, height: 400)
        case .inactive:
            return // Never fall through to or construct the product.
        }
        let window = KnowledgeReviewInputWindow(
            contentRect: CGRect(origin: .zero, size: size),
            styleMask: [.titled, .closable, .miniaturizable, .resizable],
            backing: .buffered, defer: false
        )
        window.title = title
        window.identifier = NSUserInterfaceItemIdentifier("native-knowledge-review-owned")
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
        controller = nil // Reopen creates a new View and window-local Store.
    }
}

/// Only a known, visible leading gutter is eligible; unknown geometry is not.
enum KnowledgeReviewLeadingGutter {
    static func contains(_ point: NSPoint, row: NSRect, visible: NSRect, cells: [NSRect]) -> Bool {
        func valid(_ rect: NSRect) -> Bool {
            [rect.origin.x, rect.origin.y, rect.size.width, rect.size.height,
             rect.maxX, rect.maxY].allSatisfy(\.isFinite)
                && rect.size.width > 0 && rect.size.height > 0
        }
        guard point.x.isFinite, point.y.isFinite, valid(row), valid(visible), !cells.isEmpty,
              cells.allSatisfy({ valid($0) && $0.intersects(row) }),
              row.intersection(visible).contains(point),
              let firstCellX = cells.map(\.minX).min() else { return false }
        return point.x >= max(row.minX, visible.minX)
            && point.x < min(firstCellX, row.maxX, visible.maxX)
    }
}

/// The standard window dispatches every event exactly once. A real list-gutter
/// click hands focus to its native outline, never to a guessed SwiftUI subtree.
/// Selection and every arrow event remain owned by the native List.
@MainActor
final class KnowledgeReviewInputWindow: NSWindow {
    var observeNativeInput: ((String, NSEvent) -> Void)?
    private(set) var gutterFocusTransfer: [String: Any] = [:]

    override func sendEvent(_ event: NSEvent) {
        let observed = event.type == .leftMouseDown || event.type == .scrollWheel
            || (event.type == .keyDown && (123...126).contains(event.keyCode))
        if observed { observeNativeInput?("before", event) }
        let candidate = event.type == .leftMouseDown ? leadingGutterTarget(for: event) : nil
        super.sendEvent(event)
        if let candidate {
            // Revalidate after native mouse handling: layout/window ownership
            // may have changed. Never clear a responder or replay a refused hit.
            if let current = leadingGutterTarget(for: event),
               current.outline === candidate.outline, current.row == candidate.row {
                gutterFocusTransfer["makeFirstResponderReturned"] = makeFirstResponder(current.outline)
            } else {
                gutterFocusTransfer["reason"] = "target-changed-after-native-dispatch"
            }
        }
        if observed {
            observeNativeInput?("after", event)
            DispatchQueue.main.async { [weak self] in
                self?.observeNativeInput?("settled", event)
            }
        }
    }

    private func leadingGutterTarget(for event: NSEvent) -> (outline: NSOutlineView, row: Int)? {
        gutterFocusTransfer = ["eventTimestamp": event.timestamp, "eligible": false,
                               "reason": "not-an-owned-visible-native-gutter"]
        guard event.type == .leftMouseDown, event.window === self,
              identifier?.rawValue == "native-knowledge-review-owned",
              isKeyWindow, isVisible, attachedSheet == nil, let content = contentView else { return nil }
        let hitPoint = content.superview?.convert(event.locationInWindow, from: nil) ?? event.locationInWindow
        guard let hit = content.hitTest(hitPoint),
              let outline = (hit as? NSOutlineView)
                ?? (hit.enclosingScrollView?.documentView as? NSOutlineView),
              outline.window === self, outline.isEnabled, outline.acceptsFirstResponder,
              !outline.isHiddenOrHasHiddenAncestor,
              outline.userInterfaceLayoutDirection == .leftToRight,
              !outline.tableColumns.isEmpty else { return nil }
        let point = outline.convert(event.locationInWindow, from: nil)
        let row = outline.row(at: point)
        guard row >= 0, row < outline.numberOfRows else { return nil }
        var cells: [NSRect] = []
        for column in outline.tableColumns.indices {
            guard let cell = outline.view(atColumn: column, row: row, makeIfNecessary: false),
                  cell.window === self, !cell.isHiddenOrHasHiddenAncestor else {
                gutterFocusTransfer["reason"] = "unknown-public-cell-geometry"
                return nil
            }
            cells.append(cell.convert(cell.bounds, to: outline))
        }
        let rowRect = outline.rect(ofRow: row)
        gutterFocusTransfer["pointInOutline"] = NSStringFromPoint(point)
        gutterFocusTransfer["row"] = row
        gutterFocusTransfer["rowRect"] = NSStringFromRect(rowRect)
        gutterFocusTransfer["visibleRect"] = NSStringFromRect(outline.visibleRect)
        gutterFocusTransfer["publicCellRects"] = cells.map(NSStringFromRect)
        guard KnowledgeReviewLeadingGutter.contains(point, row: rowRect, visible: outline.visibleRect, cells: cells)
        else { return nil }
        gutterFocusTransfer["eligible"] = true
        gutterFocusTransfer["reason"] = "actual-leading-gutter"
        return (outline, row)
    }
}

/// A read-only refusal page, deliberately not an offline sample "success".
/// No delegate, defaults, production store, process or external link is used.
public struct NativeKnowledgeReviewRefusalView: View {
    private let reasons: [KnowledgeReviewRefusalReason]

    public init(reasons: [KnowledgeReviewRefusalReason]) {
        self.reasons = reasons
    }

    public var body: some View {
        GlassPanel(preference: .classic) {
            ScrollView {
                VStack(alignment: .leading, spacing: NativePanelGeometry.panelGap) {
                    Label("知识审阅未启动", systemImage: "exclamationmark.shield")
                        .font(.title2.bold())
                        .foregroundStyle(Color.dsWarning)
                        .accessibilityIdentifier("knowledgeReview.refusal.title")
                    Text("只读拒绝页 · 未进入知识样例或产品工作区")
                        .accessibilityIdentifier("knowledgeReview.refusal.notice")
                    ForEach(reasons) { reason in
                        Text("\(reason.title)：\(reason.detail)")
                            .fixedSize(horizontal: false, vertical: true)
                            .accessibilityIdentifier("knowledgeReview.refusal.reason.\(reason.rawValue)")
                    }
                    Text("请在获准测试环境中重新提供单一、有效的审阅请求。本页不修改偏好或业务资料。")
                        .foregroundStyle(Color.dsTextSecondary)
                }
                .padding(NativePanelGeometry.panelInset * 2)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .padding(NativePanelGeometry.panelInset)
        .frame(minWidth: 320, minHeight: 240)
        .foregroundStyle(Color.dsText)
        .background(NativePanelFoundation(preference: .classic))
    }
}
#endif
