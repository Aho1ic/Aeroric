import Foundation
@preconcurrency import SwiftTerm

/// P5-01 Action 3/4/5 的桥：renderer（SwiftTerm）↔ 宿主字节环（经 client）。
///
/// 数据面（唯一方向性）：
/// * **renderer → host**：delegate `send(source:data:)` 是用户键入的字节，
///   经 `onInput` 闭包交给宿主 `terminal.send_input`——coordinator 自己不发
///   请求，只搬运字节。
/// * **host → renderer**：`ingest(planResult:from:)` 吃 `terminal.plan_replay`
///   的应答，按 store 的 TailDecision 把字节 `feed` 进 renderer 或整屏复位。
///   cursor 由 `TerminalStore` 记账（ADR-038 的唯一续看凭证）。
///
/// 生命周期：`attach`/`detach` 显式配对；同 (taskId, generation) 重复 attach
/// 复用同一份状态（切模式只 detach UI，进程与流都不动——P5-01 Action 4）。
/// 本类型**不 spawn 任何进程**（禁用会自行开进程的 SwiftTerm 视图变体），
/// 也不持有定时器：轮询节奏由持有方驱动 `pollTail()`。
@MainActor
public final class TerminalCoordinator: NSObject, TerminalViewDelegate {

    /// 宿主通道：用户键入的字节从这里出（注入便于单测断言）。
    public var onInput: ((_ taskId: String, _ generation: UInt64, _ data: [UInt8]) -> Void)?
    /// resize 转发（同上注入）。
    public var onResize: ((_ taskId: String, _ generation: UInt64, _ cols: Int, _ rows: Int) -> Void)?
    /// 链接点击（默认策略已校验 scheme；这里只上报给宿主层处理）。
    public var onLink: ((_ taskId: String, _ link: String) -> Void)?

    public let store: TerminalStore
    public private(set) var taskID: String
    public private(set) var generation: UInt64
    public private(set) var holdsLease: Bool
    public private(set) var isHidden: Bool

    /// renderer；`TerminalView(frame:font:)` 需要非零 frame 才能起排版。
    public let renderer: TerminalView

    /// 泄漏审计面（验收：100 次 attach/detach 后计数守恒）。
    public private(set) var feedCount = 0
    public private(set) var resetCount = 0

    public init(taskID: String, generation: UInt64, store: TerminalStore) {
        self.taskID = taskID
        self.generation = generation
        self.store = store
        self.holdsLease = false
        self.isHidden = false
        self.renderer = TerminalView(frame: NSRect(x: 0, y: 0, width: 640, height: 400), font: nil)
        super.init()
        renderer.terminalDelegate = self
        _ = store.attach(taskID: taskID, generation: generation)
    }

    // MARK: - 生命周期

    /// 取得输入/resize 租约（acquire_control 的协议结果由持有方回填）。
    public func attach(holdsLease: Bool) {
        self.holdsLease = holdsLease
        isHidden = false
        _ = store.attach(taskID: taskID, generation: generation)
    }

    /// 切走：只释放 UI 输入租约标记，流状态与 cursor 全保留（ADR-038）。
    public func detach() {
        holdsLease = false
        isHidden = true
        store.detach(taskID: taskID, generation: generation)
    }

    // MARK: - host → renderer

    public struct PlanTail {
        public var kind: String
        public var from: UInt64
        public var bytes: [UInt8]
        public var resumeAfter: UInt64
        public var reasonKind: String?
    }

    /// 吃 `terminal.plan_replay` 的应答。返回实际行为（测试断言用）。
    @discardableResult
    public func ingest(_ tail: PlanTail) -> TerminalStore.TailDecision {
        let decision = store.applyTail(
            taskID: taskID,
            generation: generation,
            kind: tail.kind,
            from: tail.from,
            resumeAfter: tail.resumeAfter,
            byteCount: tail.bytes.count,
            reasonKind: tail.reasonKind
        )
        switch decision {
        case .seamless:
            feed(tail.bytes)
        case let .reset(reason, _):
            // 整屏复位后从安全边界重放（Action 5：renderer reset + 从
            // host replay 重建）。
            renderer.getTerminal().resetToInitialState()
            resetCount += 1
            feed(tail.bytes)
        case .waitUntilStarted:
            break
        }
        return decision
    }

    private func feed(_ bytes: [UInt8]) {
        guard !bytes.isEmpty else { return }
        renderer.getTerminal().feed(byteArray: bytes)
        feedCount += 1
    }

    // MARK: - TerminalViewDelegate（真实 1.2.5 签名）

    // 协议本身未标 @MainActor（上游先于 Swift 6 严格隔离），但 renderer 的
    // 回调恒在主线程（NSView 事件路径）。用 nonisolated＋assumeIsolated 保住
    // 同步投递顺序——终端输入字节不能走异步跳线乱序。

    nonisolated public func send(source: TerminalView, data: ArraySlice<UInt8>) {
        MainActor.assumeIsolated {
            let bytes = [UInt8](data)
            guard !bytes.isEmpty else { return }
            guard TerminalInputPolicy.allowsInput(
                holdsLease: holdsLease,
                isHidden: isHidden,
                isComposing: false
            ) else {
                return
            }
            onInput?(taskID, generation, bytes)
        }
    }

    nonisolated public func sizeChanged(source: TerminalView, newCols: Int, newRows: Int) {
        MainActor.assumeIsolated {
            guard holdsLease, !isHidden else { return }
            onResize?(taskID, generation, newCols, newRows)
        }
    }

    nonisolated public func setTerminalTitle(source: TerminalView, title: String) {
        // 标题进 NSWindow title 由持有方订阅；coordinator 只存。
    }

    nonisolated public func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {
        // OSC 7；P5-03 的项目页可用来推断 cwd，这里不动作。
    }

    nonisolated public func scrolled(source: TerminalView, position: Double) {
        // 回放滚动条；渲染器自管。
    }

    nonisolated public func requestOpenLink(source: TerminalView, link: String, params: [String: String]) {
        MainActor.assumeIsolated {
            // Action 6：永不自动打开；用户点击也要过 scheme 白名单。
            guard TerminalInputPolicy.allowsOpeningLink(link) else { return }
            onLink?(taskID, link)
        }
    }

    nonisolated public func bell(source: TerminalView) {
        // 可留接到 NSBeep 的钩子；当前静默。
    }

    nonisolated public func clipboardCopy(source: TerminalView, content: Data) {
        // Action 6：OSC 52 永不自动写剪贴板（TerminalInputPolicy 的裁定）。
        _ = TerminalInputPolicy.osc52ClipboardRequest()
    }

    nonisolated public func iTermContent(source: TerminalView, content: ArraySlice<UInt8>) {
        // iTerm2 私有序列（img/notify）；P5-01 不实现，字节丢弃并记录在案。
    }

    nonisolated public func rangeChanged(source: TerminalView, startY: Int, endY: Int) {
        // 缓冲可视区变化；渲染器自管。
    }
}
