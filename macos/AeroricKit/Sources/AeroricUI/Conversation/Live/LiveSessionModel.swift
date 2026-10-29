import Foundation
import AeroricProtocol

/// One row of the live (not yet in history) part of a session.
public enum LiveItem: Identifiable, Equatable, Sendable {
    /// The user's own message, echoed locally until history has it. `steer`:
    /// sent while a turn ran (插话), waiting until the agent answers past it.
    case userEcho(id: String, text: String, steer: LiveSteer)
    case message(id: String, text: String)
    case thought(id: String, text: String, summary: Bool)
    case tool(LiveTool)
    case notice(id: String, message: String, recoverable: Bool)
    /// An informational line from the agent (`notice`).
    case info(id: String, level: String, message: String)
    /// An approval answered from an "always allow in this session" grant.
    case autoApproved(id: String, kind: String, summary: String, subject: String)
    /// Protocol content this build does not render (INV-12: shown, not dropped).
    case unknown(id: String, kind: String)

    public var id: String {
        switch self {
        case let .userEcho(id, _, _): "user:\(id)"
        case let .message(id, _): "message:\(id)"
        case let .thought(id, _, _): "thought:\(id)"
        case let .tool(tool): "tool:\(tool.id)"
        case let .notice(id, _, _): "notice:\(id)"
        case let .info(id, _, _): "info:\(id)"
        case let .autoApproved(id, _, _, _): "auto:\(id)"
        case let .unknown(id, _): "unknown:\(id)"
        }
    }
}

/// Where a message sent during a turn stands.
public enum LiveSteer: Equatable, Sendable {
    case none
    /// Queued: the agent sees it once the running tool finishes.
    case waiting
    /// The agent has moved on past it.
    case delivered
}

/// A13: an agent repeating itself within one turn. Only a hint — the
/// candidate library says runaway detection reminds, it never stops the agent.
public enum LiveLoop: Equatable, Sendable {
    case none
    /// The same call `count` times in a row.
    case repeating(title: String, count: Int)
    /// Two calls alternating (A B A B …) for `cycles` rounds.
    case alternating(cycles: Int)

    public static let repeatThreshold = 3
    public static let alternationCycles = 3

    static func signature(kind: LiveToolKind, title: String, input: JSONValue) -> String {
        "\(kind)|\(title)|\(input)"
    }

    /// Looks only at the tail: what the agent is doing right now.
    public static func of(_ signatures: [String]) -> LiveLoop {
        guard let last = signatures.last else { return .none }
        let run = signatures.reversed().prefix { $0 == last }.count
        if run >= repeatThreshold {
            return .repeating(title: title(of: last), count: run)
        }
        let pairs = alternationCycles * 2
        if signatures.count >= pairs {
            let tail = Array(signatures.suffix(pairs))
            let a = tail[0], b = tail[1]
            if a != b, tail.enumerated().allSatisfy({ $0.element == ($0.offset % 2 == 0 ? a : b) }) {
                return .alternating(cycles: alternationCycles)
            }
        }
        return .none
    }

    private static func title(of signature: String) -> String {
        signature.split(separator: "|", maxSplits: 2).dropFirst().first.map(String.init) ?? ""
    }

    public var text: String? {
        switch self {
        case .none: nil
        case let .repeating(title, count): "Agent 连续 \(count) 次执行同一个操作（\(title)），可能在原地打转。"
        case let .alternating(cycles): "Agent 在两个操作之间来回切换（\(cycles) 轮），可能在原地打转。"
        }
    }
}

/// A14: a running turn that has gone quiet. Only a hint — nothing is stopped or
/// retried; a long command can legitimately print nothing for a while.
public enum LiveStall: Equatable, Sendable {
    case none
    /// No new output for 5 minutes.
    case suspicious(minutes: Int)
    /// No new output for 15 minutes.
    case severe(minutes: Int)

    public static let suspiciousAfterMs: UInt64 = 5 * 60 * 1000
    public static let severeAfterMs: UInt64 = 15 * 60 * 1000

    /// Quiet time counts from the newest update, or from when the page first
    /// saw the turn running (`since`). Waiting on the user is not a stall.
    public static func of(turnActive: Bool, waitingOnUser: Bool, lastActivityAtMs: UInt64?,
                          since: UInt64, nowMs: UInt64) -> LiveStall {
        guard turnActive, !waitingOnUser else { return .none }
        let last = max(lastActivityAtMs ?? since, since)
        guard nowMs > last else { return .none }
        let quiet = nowMs - last
        let minutes = Int(quiet / 60_000)
        if quiet >= severeAfterMs { return .severe(minutes: minutes) }
        if quiet >= suspiciousAfterMs { return .suspicious(minutes: minutes) }
        return .none
    }

    public var isSevere: Bool {
        if case .severe = self { true } else { false }
    }

    public var text: String? {
        switch self {
        case .none: nil
        case let .suspicious(minutes): "已 \(minutes) 分钟没有新输出：可能在跑长命令，也可能卡住了。"
        case let .severe(minutes): "已 \(minutes) 分钟没有新输出，可能卡住了。可以中断这一轮后重试。"
        }
    }
}

/// A05: one turn's working-tree changes.
public struct LiveTurnChanges: Equatable, Sendable {
    public var turnId: String
    public var files: [LiveFileChange]
    public var additions: UInt64
    public var deletions: UInt64
    public var truncated: Bool

    /// `+12 −3 · 2 个文件` (`+` 前缀的文件数在截断时为“至少”)。
    public var summary: String {
        let count = truncated ? "\(files.count)+" : "\(files.count)"
        return "+\(additions) −\(deletions) · \(count) 个文件"
    }
}

/// A05: the project's checks after a turn that changed files.
public struct LiveAutoCheck: Equatable, Sendable {
    public var turnId: String
    /// `running` | `passed` | `failed` | `cancelled` | `skipped`.
    public var state: String
    public var items: [LiveAutoCheckItem]

    public var failing: [LiveAutoCheckItem] { items.filter { $0.state != "passed" && $0.state != "cancelled" } }

    /// A later turn changed files again, so this verdict no longer describes the tree.
    public func isStale(given changes: LiveTurnChanges?) -> Bool {
        guard let changes else { return false }
        return changes.turnId != turnId
    }

    public var title: String {
        switch state {
        case "running": "正在运行项目检查…"
        case "passed": "项目检查通过"
        case "failed": "项目检查失败 · \(failing.count) 项"
        case "cancelled": "项目检查已取消（新一轮开始）"
        case "skipped": "已开启自动检查，但没找到可运行的测试或诊断"
        default: "项目检查：\(state)"
        }
    }

    /// What “把失败发给 Agent” sends; nil when nothing failed.
    public var failureReport: String? {
        guard state == "failed", !failing.isEmpty else { return nil }
        var lines = ["上一轮改动后自动运行的项目检查没有通过，请修复："]
        for item in failing {
            lines.append("")
            lines.append("【\(item.label)】\(item.summary)")
            if item.failures.isEmpty {
                let tail = item.outputTail.split(separator: "\n", omittingEmptySubsequences: false).suffix(30)
                if !tail.isEmpty { lines.append(tail.joined(separator: "\n")) }
            } else {
                lines.append(contentsOf: item.failures.map { "- \($0)" })
            }
        }
        return lines.joined(separator: "\n")
    }
}

public struct LiveTool: Equatable, Sendable {
    public var id: String
    public var kind: LiveToolKind
    public var title: String
    public var input: JSONValue
    public var output: String
    public var status: LiveToolStatus
    public var result: JSONValue?
}

public struct LiveSubagent: Equatable, Sendable {
    public var id: String
    public var title: String
    public var status: String
    /// The latest activity, one line.
    public var detail: String? = nil
}

public struct LiveConfig: Equatable, Sendable {
    public var model: String?
    public var permission: String?
    public var effort: String?
}

/// What applying a batch asks of the caller.
public enum LiveApplyOutcome: Equatable, Sendable {
    /// Applied in order. `turnEnded` = a turn finished in this batch (reload history).
    case applied(turnEnded: Bool)
    /// A gap (host-reported, or a skipped seq): re-read history and `session.state`,
    /// then `resync` — nothing after the hole was applied.
    case needsResync
}

/// Pure reducer from `session.updates` batches to what the session page shows.
/// Updates at or below the cursor are duplicates and dropped; the next seq is
/// applied; anything past a hole stops the batch and asks for a resync.
public struct LiveSessionModel: Equatable, Sendable {
    public private(set) var cursor: UInt64
    public private(set) var items: [LiveItem] = []
    public private(set) var pending: [LivePendingPermission] = []
    /// Question cards waiting for the user (omp dialogs).
    public private(set) var questions: [LivePendingQuestion] = []
    /// The permission is fixed for this session (omp).
    public private(set) var permissionLocked = false
    public private(set) var plan: [LivePlanEntry] = []
    public private(set) var subagents: [LiveSubagent] = []
    public private(set) var usage: UsageSummary
    /// 2026-10-28 统计栏: timings measured from the updates this page saw.
    public private(set) var stats = SessionStats()
    public private(set) var config: LiveConfig
    public private(set) var turnActive: Bool
    /// A05: the last finished turn's working-tree changes. Kept apart from
    /// `items`, which history replaces after a turn.
    public private(set) var lastTurnChanges: LiveTurnChanges?
    /// A05: the latest project check (kept apart from `items` like the changes).
    public private(set) var lastAutoCheck: LiveAutoCheck?
    /// A14: host time (ms) of the newest update applied; nil until one arrives.
    public private(set) var lastActivityAtMs: UInt64?
    /// A13: this turn's tool calls as signatures, for the runaway-loop hint.
    public private(set) var turnToolSignatures: [String] = []
    private var echoCount = 0

    public init(state: LiveSessionState) {
        cursor = state.latestSeq
        pending = state.pending
        questions = state.questions
        permissionLocked = state.permissionLocked
        plan = state.plan
        usage = UsageSummary(contextUsed: state.contextUsed, contextLimit: state.contextLimit,
                             inputTokens: state.inputTokens, outputTokens: state.outputTokens,
                             cachedInputTokens: state.cachedInputTokens)
        stats = SessionStats(outputBaseline: state.outputTokens ?? 0)
        config = LiveConfig(model: state.model, permission: state.permission, effort: state.effort)
        turnActive = state.turnActive
    }

    /// After a gap: start over from the host's snapshot (history is re-read by the caller).
    public mutating func resync(_ state: LiveSessionState) {
        self = LiveSessionModel(state: state)
    }

    /// History now holds this turn: drop the live rows (D3), keep the rest.
    public mutating func clearItems() {
        items.removeAll()
    }

    /// The message the user just sent, until history has it.
    public mutating func echo(_ text: String) {
        echoCount += 1
        items.append(.userEcho(id: "local-\(echoCount)", text: text, steer: turnActive ? .waiting : .none))
    }

    /// The agent produced something after a 插话: it has seen it.
    private mutating func deliverSteers() {
        for index in items.indices {
            if case let .userEcho(id, text, .waiting) = items[index] {
                items[index] = .userEcho(id: id, text: text, steer: .delivered)
            }
        }
    }

    /// Optimistic config shown until the host's `configChanged` arrives.
    public mutating func setConfig(_ change: LiveConfig) {
        config = LiveConfig(model: change.model ?? config.model, permission: change.permission ?? config.permission,
                            effort: change.effort ?? config.effort)
    }

    public mutating func apply(_ batch: LiveUpdatesBatch) -> LiveApplyOutcome {
        if batch.gapOldestAvailable != nil { return .needsResync }
        var turnEnded = false
        for update in batch.updates {
            if update.seq <= cursor { continue }
            guard update.seq == cursor + 1 else { return .needsResync }
            cursor = update.seq
            lastActivityAtMs = max(lastActivityAtMs ?? 0, update.atMs)
            if reduce(update) { turnEnded = true }
        }
        return .applied(turnEnded: turnEnded)
    }

    /// Returns true when the update ended a turn.
    private mutating func reduce(_ update: LiveSessionUpdate) -> Bool {
        stats.observe(update.kind, atMs: update.atMs)
        switch update.kind {
        case .turnStarted:
            turnActive = true
            turnToolSignatures = []
        case let .turnEnded(_, reason, error):
            turnActive = false
            pending.removeAll()
            questions.removeAll()
            deliverSteers()
            if reason == .failed {
                items.append(.notice(id: "seq\(update.seq)", message: error ?? "这一轮失败了", recoverable: false))
            } else if reason == .interrupted {
                items.append(.notice(id: "seq\(update.seq)", message: "已中断", recoverable: true))
            }
            return true
        case let .messageChunk(id, role, text):
            if role == "user" { break }
            if !items.contains(where: { $0.id == "message:\(id)" }) { deliverSteers() }
            if let index = items.firstIndex(where: { $0.id == "message:\(id)" }), case let .message(_, existing) = items[index] {
                items[index] = .message(id: id, text: existing + text)
            } else {
                items.append(.message(id: id, text: text))
            }
        case let .messageCompleted(id, text):
            guard let text else { break }
            if let index = items.firstIndex(where: { $0.id == "message:\(id)" }) {
                items[index] = .message(id: id, text: text)
            } else {
                items.append(.message(id: id, text: text))
            }
        case let .thoughtChunk(id, text, summary):
            if let index = items.firstIndex(where: { $0.id == "thought:\(id)" }),
               case let .thought(_, existing, isSummary) = items[index] {
                items[index] = .thought(id: id, text: existing + text, summary: isSummary || summary)
            } else {
                items.append(.thought(id: id, text: text, summary: summary))
            }
        case let .toolCall(id, kind, title, input):
            // Replays are already dropped by seq, so every call here is new.
            turnToolSignatures.append(LiveLoop.signature(kind: kind, title: title, input: input))
            if !items.contains(where: { $0.id == "tool:\(id)" }) {
                items.append(.tool(LiveTool(id: id, kind: kind, title: title, input: input, output: "",
                                            status: .running, result: nil)))
            }
        case let .toolCallUpdate(id, status, outputDelta, result):
            if let index = items.firstIndex(where: { $0.id == "tool:\(id)" }), case var .tool(tool) = items[index] {
                tool.status = status
                if let outputDelta { tool.output += outputDelta }
                if let result { tool.result = result }
                items[index] = .tool(tool)
            } else {
                items.append(.tool(LiveTool(id: id, kind: .other, title: "工具", input: .null,
                                            output: outputDelta ?? "", status: status, result: result)))
            }
        case let .plan(entries):
            plan = entries
        case let .permissionRequest(id, toolCallId, kind, summary, options):
            if !pending.contains(where: { $0.id == id }) {
                pending.append(LivePendingPermission(id: id, kind: kind, summary: summary, toolCallId: toolCallId,
                                                     options: options.isEmpty ? LivePendingPermission.allOptions : options))
            }
        case let .permissionResolved(id, _):
            pending.removeAll { $0.id == id }
        case let .subagent(id, title, status, detail):
            let agent = LiveSubagent(id: id, title: title, status: status, detail: detail)
            if let index = subagents.firstIndex(where: { $0.id == id }) {
                subagents[index] = agent
            } else {
                subagents.append(agent)
            }
        case let .autoApproved(id, _, kind, summary, subject):
            items.append(.autoApproved(id: id, kind: kind, summary: summary, subject: subject))
        case let .question(question):
            if !questions.contains(where: { $0.id == question.id }) {
                var question = question
                if question.askedAtMs == 0 { question.askedAtMs = update.atMs }
                questions.append(question)
            }
        case let .questionResolved(id, _):
            questions.removeAll { $0.id == id }
        case let .notice(level, message):
            items.append(.info(id: "seq\(update.seq)", level: level, message: message))
        case let .usage(used, limit, input, output, cached):
            usage = UsageSummary(contextUsed: used ?? usage.contextUsed, contextLimit: limit ?? usage.contextLimit,
                                 inputTokens: input ?? usage.inputTokens, outputTokens: output ?? usage.outputTokens,
                                 cachedInputTokens: cached ?? usage.cachedInputTokens)
            stats.outputTokens = usage.outputTokens ?? stats.outputTokens
        case let .configChanged(model, permission, effort):
            setConfig(LiveConfig(model: model, permission: permission, effort: effort))
        case let .error(message, recoverable):
            items.append(.notice(id: "seq\(update.seq)", message: message, recoverable: recoverable))
        case let .turnChanges(turnId, files, additions, deletions, truncated):
            lastTurnChanges = LiveTurnChanges(turnId: turnId, files: files, additions: additions,
                                              deletions: deletions, truncated: truncated)
        case let .autoCheck(turnId, state, items):
            lastAutoCheck = LiveAutoCheck(turnId: turnId, state: state, items: items)
        case let .unknown(kind, _):
            items.append(.unknown(id: "seq\(update.seq)", kind: kind))
        }
        return false
    }
}

/// N4 D3: history and live updates do not share ids, so after a turn ends the
/// live rows give way to history only once history has grown past what it held
/// when those rows began.
public enum LiveHistoryMerge {
    public static func assistantCount(_ history: [SessionMessage]) -> Int {
        history.filter { $0.role == "assistant" }.count
    }

    public static func historyCaughtUp(baseline: Int, history: [SessionMessage]) -> Bool {
        assistantCount(history) > baseline
    }
}

/// Bottom usage line (Q28).
public struct UsageSummary: Equatable, Sendable {
    public var contextUsed: UInt64?
    public var contextLimit: UInt64?
    public var inputTokens: UInt64?
    public var outputTokens: UInt64?
    /// The prompt-cache share of `inputTokens` (Codex `cachedInputTokens`,
    /// Claude `cache_read_input_tokens`); nil when the agent does not say.
    public var cachedInputTokens: UInt64?

    public static let highThreshold = 0.8

    public init(transcript: SessionTranscriptStats) {
        self.init(contextUsed: transcript.contextUsed, contextLimit: transcript.contextLimit,
                  inputTokens: transcript.inputTokens, outputTokens: transcript.outputTokens,
                  cachedInputTokens: transcript.cachedInputTokens)
    }

    public init(contextUsed: UInt64?, contextLimit: UInt64?, inputTokens: UInt64?, outputTokens: UInt64?,
                cachedInputTokens: UInt64? = nil) {
        self.contextUsed = contextUsed
        self.contextLimit = contextLimit
        self.inputTokens = inputTokens
        self.outputTokens = outputTokens
        self.cachedInputTokens = cachedInputTokens
    }

    /// Input plus output, the 统计栏's headline count.
    public var totalTokens: UInt64? {
        guard inputTokens != nil || outputTokens != nil else { return nil }
        return (inputTokens ?? 0) + (outputTokens ?? 0)
    }

    /// 0…1 share of input served from the cache.
    public var cacheHitRate: Double? {
        guard let cached = cachedInputTokens, let input = inputTokens, input > 0 else { return nil }
        return min(1, Double(cached) / Double(input))
    }

    public var uncachedInputTokens: UInt64? {
        guard let input = inputTokens else { return nil }
        return input - min(input, cachedInputTokens ?? 0)
    }

    /// 0…1, nil when either side is unknown.
    public var fraction: Double? {
        guard let used = contextUsed, let limit = contextLimit, limit > 0 else { return nil }
        return min(1, Double(used) / Double(limit))
    }

    public var isHigh: Bool { (fraction ?? 0) > Self.highThreshold }
    public var isEmpty: Bool { fraction == nil && inputTokens == nil && outputTokens == nil }

    /// `上下文 42% · 输入 12.3k · 输出 1.1k`; unknown parts are omitted.
    public var text: String {
        var parts: [String] = []
        if let fraction { parts.append("上下文 \(Int((fraction * 100).rounded()))%") }
        if let inputTokens { parts.append("输入 \(Self.compact(inputTokens))") }
        if let outputTokens { parts.append("输出 \(Self.compact(outputTokens))") }
        return parts.joined(separator: " · ")
    }

    public static func compact(_ count: UInt64) -> String {
        switch count {
        case ..<1000: return "\(count)"
        case ..<1_000_000: return String(format: "%.1fk", Double(count) / 1000).replacingOccurrences(of: ".0k", with: "k")
        default: return String(format: "%.1fM", Double(count) / 1_000_000).replacingOccurrences(of: ".0M", with: "M")
        }
    }
}

/// The 会话统计 card (2026-10-28): how the model and the tools spent this
/// session's time, measured from the live updates' host timestamps. Turns and
/// steps for the whole session come from history (`SessionStatsBar`); these
/// timings start when the page first saw the session.
public struct SessionStats: Equatable, Sendable {
    public var turns = 0
    public var steps = 0
    public var modelMs: UInt64 = 0
    public var toolMs: UInt64 = 0
    public var firstTokenMsTotal: UInt64 = 0
    public var firstTokenSamples = 0
    /// Cumulative output tokens (the usage line), and where they stood on open.
    public var outputTokens: UInt64?
    public var outputBaseline: UInt64 = 0

    private var turnStartMs: UInt64?
    private var turnToolMs: UInt64 = 0
    private var awaitingFirstToken = false
    private var toolStarts: [String: UInt64] = [:]

    /// True when the figures cover the whole session (read from its
    /// transcript), not just since the page opened.
    public var wholeSession = false

    public init(outputBaseline: UInt64 = 0) {
        self.outputBaseline = outputBaseline
    }

    public init(transcript: SessionTranscriptStats) {
        modelMs = transcript.modelMs
        toolMs = transcript.toolMs
        firstTokenMsTotal = transcript.firstTokenMsTotal
        firstTokenSamples = transcript.firstTokenSamples
        outputTokens = transcript.outputTokens
        wholeSession = true
    }

    public var averageFirstTokenMs: UInt64? {
        firstTokenSamples > 0 ? firstTokenMsTotal / UInt64(firstTokenSamples) : nil
    }

    /// Output tokens per second of model time since the page opened.
    public var tokensPerSecond: Double? {
        guard modelMs >= 500, let output = outputTokens, output > outputBaseline else { return nil }
        return Double(output - outputBaseline) / (Double(modelMs) / 1000)
    }

    mutating func observe(_ kind: LiveUpdateKind, atMs: UInt64) {
        switch kind {
        case .turnStarted:
            turns += 1
            turnStartMs = atMs
            turnToolMs = 0
            awaitingFirstToken = true
        case .turnEnded:
            if let start = turnStartMs, atMs >= start {
                let spent = atMs - start
                modelMs += spent - min(spent, turnToolMs)
            }
            turnStartMs = nil
            awaitingFirstToken = false
            toolStarts.removeAll()
        case .messageChunk, .thoughtChunk:
            if awaitingFirstToken, let start = turnStartMs, atMs >= start {
                firstTokenMsTotal += atMs - start
                firstTokenSamples += 1
                awaitingFirstToken = false
            }
        case let .toolCall(id, _, _, _):
            steps += 1
            toolStarts[id] = atMs
        case let .toolCallUpdate(id, status, _, _):
            guard status != .running, let start = toolStarts.removeValue(forKey: id),
                  atMs >= start else { break }
            toolMs += atMs - start
            turnToolMs += atMs - start
        default:
            break
        }
    }
}

