import Foundation
import AeroricProtocol

/// A13 / R3 (D 批 2026-11-08): a session that has run long, spent a lot, or
/// keeps turning without doing anything. Thresholds and the idle-turn rule
/// are gstack's (`lib/runaway.ts`); like `LiveLoop`, it only reminds — no
/// stop, no rotation.
public struct LiveRunawayAlert: Identifiable, Equatable, Sendable {
    /// What it is about (`tokens`, `active`, `idle`, `delegation:<id>`): the
    /// key a dismissal is kept under.
    public var id: String
    public var text: String
}

public enum LiveRunaway {
    public struct Limits: Equatable, Sendable {
        public var sessionTokens: UInt64 = 400_000_000
        public var delegationTokens: UInt64 = 100_000_000
        public var activeMs: UInt64 = 3 * 24 * 3_600_000
        /// The last this many turns are looked at …
        public var window = 40
        /// … once at least this many of them can be judged …
        public var minTurns = 20
        /// … and idle ones above this share are reminded of.
        public var idleShare = 0.25

        public init() {}

        /// The defaults, or test values the review build reads from the
        /// app's defaults (`RunawayTestLimits`, a dictionary of the fields).
        public static func current(_ defaults: UserDefaults = .standard) -> Limits {
            var limits = Limits()
            guard let test = defaults.dictionary(forKey: "RunawayTestLimits") else { return limits }
            if let v = test["sessionTokens"] as? Int { limits.sessionTokens = UInt64(max(v, 1)) }
            if let v = test["delegationTokens"] as? Int { limits.delegationTokens = UInt64(max(v, 1)) }
            if let v = test["activeMs"] as? Int { limits.activeMs = UInt64(max(v, 1)) }
            if let v = test["window"] as? Int { limits.window = max(v, 1) }
            if let v = test["minTurns"] as? Int { limits.minTurns = max(v, 1) }
            if let v = test["idleShare"] as? Double { limits.idleShare = v }
            return limits
        }
    }

    /// How one turn of the history reads.
    public enum Turn: Equatable, Sendable {
        case busy
        /// Only waited, polled or read status, and said next to nothing.
        case idle
        /// Nothing to judge it by (no reply, no tool call): neither idle nor
        /// fine, and left out of the share (gstack: a scan without a result
        /// is a failed scan, never a clean one).
        case unknown
    }

    /// Calls that wait or read status without moving the work on.
    static let waiting: Set<String> = [
        "wait", "wait_agent", "list_agents", "sleep", "write_stdin", "bashoutput", "taskoutput",
        "get_goal", "todoread",
    ]
    /// Under this many characters a turn's own text says nothing new.
    static let saysNothing = 200

    /// Most telling first.
    public static func alerts(stats: SessionTranscriptStats?, history: [SessionMessage],
                              delegations: [LiveDelegationCard], nowMs: UInt64,
                              limits: Limits = Limits()) -> [LiveRunawayAlert] {
        var out: [LiveRunawayAlert] = []
        let turns = Self.turns(history).suffix(limits.window)
        let judged = turns.filter { $0 != .unknown }
        let idle = judged.filter { $0 == .idle }.count
        if judged.count >= limits.minTurns, Double(idle) / Double(judged.count) > limits.idleShare {
            let share = Int((Double(idle) / Double(judged.count) * 100).rounded())
            out.append(LiveRunawayAlert(
                id: "idle",
                text: "最近 \(turns.count) 个回合里有 \(idle) 个空转（\(share)%，提醒线 \(Int(limits.idleShare * 100))%）"))
        }
        let total = (stats?.inputTokens ?? 0) + (stats?.outputTokens ?? 0)
        if total > limits.sessionTokens {
            out.append(LiveRunawayAlert(
                id: "tokens",
                text: "这个会话累计用了 \(amount(total)) token（提醒线 \(amount(limits.sessionTokens))）"))
        }
        for card in delegations {
            if let tokens = card.tokens, tokens > limits.delegationTokens {
                out.append(LiveRunawayAlert(
                    id: "delegation:\(card.delegationId)",
                    text: "\(DelegationText.title(card)) 已用 \(amount(tokens)) token（提醒线 \(amount(limits.delegationTokens))）"))
            }
        }
        if let first = stats?.turns.first?.startedAt, nowMs > first, nowMs - first > limits.activeMs {
            out.append(LiveRunawayAlert(
                id: "active",
                text: "这个会话已连续活跃 \(duration(nowMs - first))（提醒线 \(duration(limits.activeMs))）"))
        }
        return out
    }

    /// The history cut at each message of the person's own, oldest first.
    public static func turns(_ history: [SessionMessage]) -> [Turn] {
        var turns: [Turn] = []
        var current: [SessionContent]?
        func close() {
            if let content = current { turns.append(judge(content)) }
        }
        for message in history {
            let typed = message.role == "user" && message.content.contains {
                if case .text = $0 { return true } else { return false }
            }
            if typed {
                close()
                current = []
            } else if message.role == "assistant" {
                current?.append(contentsOf: message.content)
            }
        }
        close()
        return turns
    }

    static func judge(_ content: [SessionContent]) -> Turn {
        var calls: [(name: String, input: String)] = []
        var said = 0
        for block in content {
            switch block {
            case let .toolUse(_, name, input): calls.append((name.lowercased(), input))
            case let .text(text): said += text.trimmingCharacters(in: .whitespacesAndNewlines).count
            default: break
            }
        }
        if calls.isEmpty { return said == 0 ? .unknown : .busy }
        let onlyWaiting = calls.allSatisfy { call in
            if waiting.contains(call.name) { return true }
            // Codex's multi-agent calls are `Agent` cards whose tool says what they do.
            guard call.name == "agent", let data = call.input.data(using: .utf8),
                  let fields = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let tool = fields["tool"] as? String else { return false }
            return waiting.contains(tool)
        }
        return onlyWaiting && said < saysNothing ? .idle : .busy
    }

    /// 4.1 亿, 21.8 万, 900.
    static func amount(_ tokens: UInt64) -> String {
        if tokens >= 100_000_000 { return trimmed(Double(tokens) / 100_000_000) + " 亿" }
        if tokens >= 10_000 { return trimmed(Double(tokens) / 10_000) + " 万" }
        return "\(tokens)"
    }

    static func trimmed(_ value: Double) -> String {
        let text = String(format: "%.1f", value)
        return text.hasSuffix(".0") ? String(text.dropLast(2)) : text
    }

    /// 3 天 2 小时, 5 小时, 12 分钟.
    static func duration(_ ms: UInt64) -> String {
        let minutes = ms / 60_000
        let days = minutes / (24 * 60), hours = minutes / 60 % 24
        if days > 0 { return hours > 0 ? "\(days) 天 \(hours) 小时" : "\(days) 天" }
        if minutes >= 60 { return "\(minutes / 60) 小时" }
        return "\(max(minutes, 1)) 分钟"
    }

    /// What 请它先总结 sends: a stop to look back, not an order to stop.
    public static let summaryRequest =
        "请先停一下，总结目前的进度：做完了什么、还剩什么、卡在哪里。然后再判断要不要继续、怎么继续。"
}
