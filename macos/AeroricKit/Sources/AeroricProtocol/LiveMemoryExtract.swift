import Foundation

/// B5-2 (2026-10-31): the 整理设置.
public struct LiveMemorySettings: Equatable, Sendable {
    public var auto: Bool
    public var connectionId: String?
    public var model: String?
    public var dailyLimit: Int
    /// B6: 按意思找相近的记忆 (vectors from 随手记's embedding model).
    public var semantic: Bool

    public init(auto: Bool = false, connectionId: String? = nil, model: String? = nil, dailyLimit: Int = 30,
                semantic: Bool = false) {
        self.auto = auto
        self.connectionId = connectionId
        self.model = model
        self.dailyLimit = dailyLimit
        self.semantic = semantic
    }

    public static func decode(_ value: JSONValue?) -> LiveMemorySettings {
        let f = value?.fields ?? [:]
        return LiveMemorySettings(auto: f["auto"]?.boolValue ?? false, connectionId: f["connectionId"]?.stringValue,
                                  model: f["model"]?.stringValue,
                                  dailyLimit: f["dailyLimit"]?.numberText.flatMap { Int($0) } ?? 30,
                                  semantic: f["semantic"]?.boolValue ?? false)
    }

    public var json: JSONValue {
        var f: [String: JSONValue] = ["auto": .bool(auto), "dailyLimit": .number(String(dailyLimit)),
                                      "semantic": .bool(semantic)]
        if let connectionId { f["connectionId"] = .string(connectionId) }
        if let model { f["model"] = .string(model) }
        return .object(f)
    }

    public var hasModel: Bool { connectionId != nil && model != nil }
}

/// B6: how far the memory vectors are (`memory.semanticStatus`).
public struct LiveSemanticStatus: Equatable, Sendable {
    /// The model the vectors were made with; empty before the first.
    public var model: String
    public var embedded: Int
    public var missing: Int
    public var error: String?
    /// The model 随手记's embedding settings name now.
    public var configured: String

    public init(model: String = "", embedded: Int = 0, missing: Int = 0, error: String? = nil, configured: String = "") {
        self.model = model
        self.embedded = embedded
        self.missing = missing
        self.error = error
        self.configured = configured
    }

    public static func decode(_ value: JSONValue) -> LiveSemanticStatus {
        let f = value.objectValue?["status"]?.objectValue ?? [:]
        return LiveSemanticStatus(model: f["model"]?.stringValue ?? "",
                                  embedded: f["embedded"]?.numberText.flatMap { Int($0) } ?? 0,
                                  missing: f["missing"]?.numberText.flatMap { Int($0) } ?? 0,
                                  error: f["error"]?.stringValue,
                                  configured: value.objectValue?["model"]?.stringValue ?? "")
    }
}

/// A model that can be the analysis model.
public struct LiveMemoryModelChoice: Equatable, Sendable, Identifiable, Hashable {
    public var connectionId: String
    public var connection: String
    public var model: String
    public var id: String { "\(connectionId)|\(model)" }

    public init(connectionId: String, connection: String, model: String) {
        self.connectionId = connectionId
        self.connection = connection
        self.model = model
    }

    public static func decode(_ value: JSONValue) -> LiveMemoryModelChoice? {
        let f = value.fields
        guard let id = f["connectionId"]?.stringValue, let model = f["model"]?.stringValue else { return nil }
        return LiveMemoryModelChoice(connectionId: id, connection: f["connection"]?.stringValue ?? id, model: model)
    }
}

/// One row of 整理记录.
public struct LiveMemoryRun: Equatable, Sendable, Identifiable {
    public var id: String
    public var taskId: String
    public var projectId: String
    public var title: String
    public var atMs: UInt64
    /// `done` `partial` `failed` `skipped`; C3 (2026-11-20) `queued`
    /// `running` `canceled`.
    public var state: String
    public var reason: String?
    public var model: String?
    public var inputTokens: UInt64
    public var outputTokens: UInt64
    public var costUsd: Double?
    public var facts: Int
    public var candidates: Int
    /// B5-3a.
    public var experiences: Int
    /// B6 (2026-11-13): `extract` or `dream`.
    public var kind: String
    /// B6: items a dream merged on its own, groups it put to the user, and
    /// when it was undone.
    public var merged: Int
    public var proposed: Int
    public var undoneMs: UInt64?
    /// C3: writes the store failed (waiting to be made again), and where
    /// each thing the model proposed went.
    public var failedWrites: Int
    public var outcomes: [LiveRunOutcome]

    public init(id: String, taskId: String = "", projectId: String = "", title: String = "", atMs: UInt64 = 0,
                state: String, reason: String? = nil, model: String? = nil, inputTokens: UInt64 = 0,
                outputTokens: UInt64 = 0, costUsd: Double? = nil, facts: Int = 0, candidates: Int = 0,
                experiences: Int = 0, kind: String = "extract", merged: Int = 0, proposed: Int = 0,
                undoneMs: UInt64? = nil, failedWrites: Int = 0, outcomes: [LiveRunOutcome] = []) {
        self.failedWrites = failedWrites
        self.outcomes = outcomes
        self.experiences = experiences
        self.kind = kind
        self.merged = merged
        self.proposed = proposed
        self.undoneMs = undoneMs
        self.id = id
        self.taskId = taskId
        self.projectId = projectId
        self.title = title
        self.atMs = atMs
        self.state = state
        self.reason = reason
        self.model = model
        self.inputTokens = inputTokens
        self.outputTokens = outputTokens
        self.costUsd = costUsd
        self.facts = facts
        self.candidates = candidates
    }

    public static func decode(_ value: JSONValue?) -> LiveMemoryRun? {
        guard let f = value?.objectValue, let id = f["id"]?.stringValue else { return nil }
        let int = { (key: String) in f[key]?.numberText.flatMap { Int($0) } ?? 0 }
        let u64 = { (key: String) in f[key]?.numberText.flatMap { UInt64($0) } ?? 0 }
        return LiveMemoryRun(id: id, taskId: f["taskId"]?.stringValue ?? "", projectId: f["projectId"]?.stringValue ?? "",
                             title: f["title"]?.stringValue ?? "", atMs: u64("atMs"), state: f["state"]?.stringValue ?? "done",
                             reason: f["reason"]?.stringValue, model: f["model"]?.stringValue,
                             inputTokens: u64("inputTokens"), outputTokens: u64("outputTokens"),
                             costUsd: f["costUsd"]?.numberText.flatMap { Double($0) },
                             facts: int("facts"), candidates: int("candidates"), experiences: int("experiences"),
                             kind: f["kind"]?.stringValue ?? "extract", merged: f["retired"]?.arrayValue?.count ?? 0,
                             proposed: int("proposed"), undoneMs: f["undoneMs"]?.numberText.flatMap { UInt64($0) },
                             failedWrites: int("failedWrites"),
                             outcomes: (f["outcomes"]?.arrayValue ?? []).compactMap(LiveRunOutcome.decode))
    }

    public var isDream: Bool { kind == "dream" }
    /// C3: 重建索引.
    public var isReindex: Bool { kind == "reindex" }
    /// C3: still waiting or going — 取消 applies, and the page keeps reading.
    public var isOpen: Bool { state == "queued" || state == "running" }
}

/// B6 (2026-11-13): a group a dream put to the user — the same thing said
/// twice (keep one) or two things that cannot both hold (pick one).
/// B6 second batch: one item of a profile rewrite.
public struct LiveDreamRewrite: Equatable, Sendable {
    public var text: String
    public var category: String?

    public init(text: String, category: String? = nil) {
        self.text = text
        self.category = category
    }
}

public struct LiveDreamProposal: Equatable, Sendable, Identifiable {
    public var id: String
    public var runId: String
    public var projectId: String?
    /// The items' kind (`profile` `fact` `experience`).
    public var kind: String
    /// `duplicate` `conflict`; second batch: `stale` `compress`.
    public var relation: String
    public var ids: [String]
    /// The one the model would keep (`stale`: the item itself).
    public var keep: String
    public var reason: String
    /// `pending` `accepted` `rejected` `withdrawn` `reverted`.
    public var state: String
    public var createdMs: UInt64
    public var decidedMs: UInt64?
    /// `stale`: the paths and symbols no longer found.
    public var missing: [String]
    /// `compress`: the shorter profile.
    public var rewrite: [LiveDreamRewrite]

    public init(id: String, runId: String = "", projectId: String? = nil, kind: String = "fact",
                relation: String = "duplicate", ids: [String] = [], keep: String = "", reason: String = "",
                state: String = "pending", createdMs: UInt64 = 0, decidedMs: UInt64? = nil,
                missing: [String] = [], rewrite: [LiveDreamRewrite] = []) {
        self.id = id
        self.runId = runId
        self.projectId = projectId
        self.kind = kind
        self.relation = relation
        self.ids = ids
        self.keep = keep
        self.reason = reason
        self.state = state
        self.createdMs = createdMs
        self.decidedMs = decidedMs
        self.missing = missing
        self.rewrite = rewrite
    }

    public static func decode(_ value: JSONValue) -> LiveDreamProposal? {
        guard let f = value.objectValue, let id = f["id"]?.stringValue else { return nil }
        let rewrite = (f["rewrite"]?.arrayValue ?? []).compactMap { value -> LiveDreamRewrite? in
            guard let r = value.objectValue, let text = r["text"]?.stringValue else { return nil }
            return LiveDreamRewrite(text: text, category: r["category"]?.stringValue)
        }
        return LiveDreamProposal(id: id, runId: f["runId"]?.stringValue ?? "", projectId: f["projectId"]?.stringValue,
                                 kind: f["kind"]?.stringValue ?? "fact",
                                 relation: f["relation"]?.stringValue ?? "duplicate",
                                 ids: (f["ids"]?.arrayValue ?? []).compactMap(\.stringValue),
                                 keep: f["keep"]?.stringValue ?? "", reason: f["reason"]?.stringValue ?? "",
                                 state: f["state"]?.stringValue ?? "pending",
                                 createdMs: f["createdMs"]?.numberText.flatMap { UInt64($0) } ?? 0,
                                 decidedMs: f["decidedMs"]?.numberText.flatMap { UInt64($0) },
                                 missing: (f["missing"]?.arrayValue ?? []).compactMap(\.stringValue),
                                 rewrite: rewrite)
    }

    public var isPending: Bool { state == "pending" }
}

/// Q4 (2026-11-02): one imported session 整理历史会话 could send.
public struct LiveMemoryHistoryCandidate: Equatable, Sendable, Identifiable {
    public var id: String
    public var title: String
    public var agent: String
    public var lastAtMs: UInt64
    /// Already extracted: skipped.
    public var done: Bool

    public init(id: String, title: String = "", agent: String = "", lastAtMs: UInt64 = 0, done: Bool = false) {
        self.id = id
        self.title = title
        self.agent = agent
        self.lastAtMs = lastAtMs
        self.done = done
    }
}

/// Q4: what 整理历史会话 would do, estimated before anything is sent.
public struct LiveMemoryHistoryPlan: Equatable, Sendable {
    public var sessions: [LiveMemoryHistoryCandidate]
    public var pending: Int
    public var inputTokens: UInt64
    public var outputTokens: UInt64
    public var costUsd: Double?
    public var remainingToday: Int
    public var model: String?
    public var enabled: Bool

    public init(sessions: [LiveMemoryHistoryCandidate] = [], pending: Int = 0, inputTokens: UInt64 = 0,
                outputTokens: UInt64 = 0, costUsd: Double? = nil, remainingToday: Int = 0, model: String? = nil,
                enabled: Bool = false) {
        self.sessions = sessions
        self.pending = pending
        self.inputTokens = inputTokens
        self.outputTokens = outputTokens
        self.costUsd = costUsd
        self.remainingToday = remainingToday
        self.model = model
        self.enabled = enabled
    }

    public static func decode(_ value: JSONValue?) -> LiveMemoryHistoryPlan {
        let f = value?.fields ?? [:]
        let int = { (key: String) in f[key]?.numberText.flatMap { Int($0) } ?? 0 }
        let u64 = { (key: String) in f[key]?.numberText.flatMap { UInt64($0) } ?? 0 }
        let sessions: [LiveMemoryHistoryCandidate] = (f["sessions"]?.arrayValue ?? []).compactMap { value in
            guard let s = value.objectValue, let id = s["id"]?.stringValue else { return nil }
            return LiveMemoryHistoryCandidate(id: id, title: s["title"]?.stringValue ?? "",
                                              agent: s["agent"]?.stringValue ?? "",
                                              lastAtMs: s["lastAtMs"]?.numberText.flatMap { UInt64($0) } ?? 0,
                                              done: s["done"]?.boolValue ?? false)
        }
        return LiveMemoryHistoryPlan(sessions: sessions, pending: int("pending"), inputTokens: u64("inputTokens"),
                                     outputTokens: u64("outputTokens"),
                                     costUsd: f["costUsd"]?.numberText.flatMap { Double($0) },
                                     remainingToday: int("remainingToday"), model: f["model"]?.stringValue,
                                     enabled: f["enabled"]?.boolValue ?? false)
    }
}

/// 整理记录 with today's count and this month's total.
public struct LiveMemoryRunsPage: Equatable, Sendable {
    public var runs: [LiveMemoryRun]
    public var usedToday: Int
    public var dailyLimit: Int
    public var monthCount: Int
    public var monthCostUsd: Double
    /// C3: whether the full-text index is whole (an older host: assumed).
    public var indexHealthy: Bool

    public init(runs: [LiveMemoryRun] = [], usedToday: Int = 0, dailyLimit: Int = 30, monthCount: Int = 0,
                monthCostUsd: Double = 0, indexHealthy: Bool = true) {
        self.indexHealthy = indexHealthy
        self.runs = runs
        self.usedToday = usedToday
        self.dailyLimit = dailyLimit
        self.monthCount = monthCount
        self.monthCostUsd = monthCostUsd
    }

    public static func decode(_ value: JSONValue?) -> LiveMemoryRunsPage {
        let f = value?.fields ?? [:]
        let int = { (key: String) in f[key]?.numberText.flatMap { Int($0) } ?? 0 }
        return LiveMemoryRunsPage(runs: (f["runs"]?.arrayValue ?? []).compactMap { LiveMemoryRun.decode($0) },
                                  usedToday: int("usedToday"), dailyLimit: f["dailyLimit"]?.numberText.flatMap { Int($0) } ?? 30,
                                  monthCount: int("monthCount"),
                                  monthCostUsd: f["monthCostUsd"]?.numberText.flatMap { Double($0) } ?? 0,
                                  indexHealthy: f["indexHealthy"]?.boolValue ?? true)
    }
}

/// C3 (2026-11-20): one thing an extraction's model proposed and where it
/// went. No text: `itemId` is the item it became or joined.
public struct LiveRunOutcome: Equatable, Sendable {
    /// `fact` `experience` `candidate`.
    public var subject: String
    /// `written` `merged` `refused` `failed` `withdrawn` `canceled`.
    public var outcome: String
    public var sig: String
    public var itemId: String?
    public var reason: String?

    public init(subject: String, outcome: String, sig: String = "", itemId: String? = nil, reason: String? = nil) {
        self.subject = subject
        self.outcome = outcome
        self.sig = sig
        self.itemId = itemId
        self.reason = reason
    }

    public static func decode(_ value: JSONValue) -> LiveRunOutcome? {
        let f = value.fields
        guard let subject = f["subject"]?.stringValue, let outcome = f["outcome"]?.stringValue else { return nil }
        return LiveRunOutcome(subject: subject, outcome: outcome, sig: f["sig"]?.stringValue ?? "",
                              itemId: f["itemId"]?.stringValue, reason: f["reason"]?.stringValue)
    }
}

/// B6 (2026-11-13): 代码版本链 — one block of one turn's change: the hunks
/// of one file under one definition, from the session a memory came from.
public struct LiveCodeChange: Equatable, Sendable, Identifiable {
    public var path: String
    public var symbol: String?
    public var branch: String?
    public var taskId: String
    public var turnId: String
    public var atMs: UInt64
    public var additions: Int
    public var deletions: Int
    /// `pending` `accepted` `undone`.
    public var state: String
    public var diff: String
    public var id: String { "\(taskId)|\(turnId)|\(path)|\(symbol ?? "")" }

    public init(path: String, symbol: String? = nil, branch: String? = nil, taskId: String = "",
                turnId: String = "", atMs: UInt64 = 0, additions: Int = 0, deletions: Int = 0,
                state: String = "pending", diff: String = "") {
        self.path = path
        self.symbol = symbol
        self.branch = branch
        self.taskId = taskId
        self.turnId = turnId
        self.atMs = atMs
        self.additions = additions
        self.deletions = deletions
        self.state = state
        self.diff = diff
    }

    public static func decode(_ value: JSONValue) -> LiveCodeChange? {
        guard let f = value.objectValue, let path = f["path"]?.stringValue else { return nil }
        let int = { (key: String) in f[key]?.numberText.flatMap { Int($0) } ?? 0 }
        return LiveCodeChange(path: path, symbol: f["symbol"]?.stringValue, branch: f["branch"]?.stringValue,
                              taskId: f["taskId"]?.stringValue ?? "", turnId: f["turnId"]?.stringValue ?? "",
                              atMs: f["atMs"]?.numberText.flatMap { UInt64($0) } ?? 0,
                              additions: int("additions"), deletions: int("deletions"),
                              state: f["state"]?.stringValue ?? "pending", diff: f["diff"]?.stringValue ?? "")
    }
}
