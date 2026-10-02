import Foundation

/// 一条终端流的续看状态（P5-01 Action 4/5）。每个 taskId+generation 一份；
/// cursor 是「已交付字节之后的 offset」——ADR-038 的唯一无损续看凭证。
/// 隐藏/重显/重连都不清空它；只有 `resetFor(reason:)` 在 host 明确回答
/// 「无缝不可得」时归零并记录原因。
public struct TerminalStreamState: Equatable, Sendable {
    public let taskID: String
    public let generation: UInt64
    /// 已交付字节之后的 offset；nil = 尚未做过第一次 plan_replay。
    public private(set) var cursor: UInt64?
    /// 上一次 reset 的具名理由（ADR-038：notStarted/evicted/compacted/…）。
    public private(set) var lastResetReason: String?
    /// 是否已经拿过第一片字节（notStarted 的等待期置 false）。
    public private(set) var hasReceivedBytes: Bool

    public init(taskID: String, generation: UInt64) {
        self.taskID = taskID
        self.generation = generation
        self.cursor = nil
        self.lastResetReason = nil
        self.hasReceivedBytes = false
    }

    /// 无缝续看：cursor 前进到 `resumeAfter`。
    public mutating func advanceSeamless(to resumeAfter: UInt64, byteCount: Int) {
        cursor = resumeAfter
        if byteCount > 0 { hasReceivedBytes = true }
    }

    /// 重置视图：cursor 回到安全边界 `from`（host 从那里整段重放），
    /// renderer 屏幕已被清空，接收计数归零。
    public mutating func resetFor(reason: String, safeBoundary from: UInt64) {
        cursor = from
        lastResetReason = reason
        hasReceivedBytes = false
    }
}

/// 会话里全部终端流状态的簿记。P5-01 验收的「100 次 attach/detach 无泄漏」
/// 就是对这份簿记的断言：attach 复用同 key 的状态，detach 只标记不重建，
/// 同一 (taskId, generation) 永远拿到同一个实例。
@MainActor
public final class TerminalStore {
    private var streams: [String: TerminalStreamState] = [:]
    private(set) var attachCount = 0
    private(set) var detachCount = 0

    public init() {}

    public static func key(taskID: String, generation: UInt64) -> String {
        "\(taskID)#\(generation)"
    }

    @discardableResult
    public func attach(taskID: String, generation: UInt64) -> TerminalStreamState {
        attachCount += 1
        return ensureState(taskID: taskID, generation: generation)
    }

    /// 取状态而不计入 attach 计数（活流轮询到达时用——那是数据事件，
    /// 不是 UI attach）。
    private func ensureState(taskID: String, generation: UInt64) -> TerminalStreamState {
        let key = Self.key(taskID: taskID, generation: generation)
        if let existing = streams[key] { return existing }
        let state = TerminalStreamState(taskID: taskID, generation: generation)
        streams[key] = state
        return state
    }

    public func detach(taskID: String, generation: UInt64) {
        detachCount += 1
        // 状态保留：切回来时 cursor 补流（ADR-038 的核心承诺）。
    }

    /// 活流轮询应答到达：按 kind 推进簿记，返回「该喂给 renderer 的字节」
    /// 之外的决策信息（是否需要整屏复位）。
    public enum TailDecision: Equatable, Sendable {
        case seamless
        case reset(reason: String, safeBoundary: UInt64)
        case waitUntilStarted
    }

    public func applyTail(
        taskID: String,
        generation: UInt64,
        kind: String,
        from: UInt64,
        resumeAfter: UInt64,
        byteCount: Int,
        reasonKind: String?
    ) -> TailDecision {
        var state = ensureState(taskID: taskID, generation: generation)
        switch kind {
        case "seamless":
            state.advanceSeamless(to: resumeAfter, byteCount: byteCount)
            streams[Self.key(taskID: taskID, generation: generation)] = state
            return .seamless
        case "reset":
            let reason = reasonKind ?? "unknown"
            state.resetFor(reason: reason, safeBoundary: from)
            // 重置分支的字节（若有）会立刻被喂进 renderer，所以记账同样
            // 推进到 resumeAfter——与 core 的 resume_after() 同一算法。
            state.advanceSeamless(to: resumeAfter, byteCount: byteCount)
            streams[Self.key(taskID: taskID, generation: generation)] = state
            if reason == "notStarted" { return .waitUntilStarted }
            return .reset(reason: reason, safeBoundary: from)
        default:
            return .waitUntilStarted
        }
    }

    public func state(taskID: String, generation: UInt64) -> TerminalStreamState? {
        streams[Self.key(taskID: taskID, generation: generation)]
    }

    public var liveStreamCount: Int { streams.count }
}
