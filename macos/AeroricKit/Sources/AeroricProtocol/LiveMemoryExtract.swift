import Foundation

/// B5-2 (2026-10-31): the 整理设置.
public struct LiveMemorySettings: Equatable, Sendable {
    public var auto: Bool
    public var connectionId: String?
    public var model: String?
    public var dailyLimit: Int

    public init(auto: Bool = false, connectionId: String? = nil, model: String? = nil, dailyLimit: Int = 30) {
        self.auto = auto
        self.connectionId = connectionId
        self.model = model
        self.dailyLimit = dailyLimit
    }

    public static func decode(_ value: JSONValue?) -> LiveMemorySettings {
        let f = value?.fields ?? [:]
        return LiveMemorySettings(auto: f["auto"]?.boolValue ?? false, connectionId: f["connectionId"]?.stringValue,
                                  model: f["model"]?.stringValue,
                                  dailyLimit: f["dailyLimit"]?.numberText.flatMap { Int($0) } ?? 30)
    }

    public var json: JSONValue {
        var f: [String: JSONValue] = ["auto": .bool(auto), "dailyLimit": .number(String(dailyLimit))]
        if let connectionId { f["connectionId"] = .string(connectionId) }
        if let model { f["model"] = .string(model) }
        return .object(f)
    }

    public var hasModel: Bool { connectionId != nil && model != nil }
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
    /// `done` `failed` `skipped`.
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

    public init(id: String, taskId: String = "", projectId: String = "", title: String = "", atMs: UInt64 = 0,
                state: String, reason: String? = nil, model: String? = nil, inputTokens: UInt64 = 0,
                outputTokens: UInt64 = 0, costUsd: Double? = nil, facts: Int = 0, candidates: Int = 0,
                experiences: Int = 0) {
        self.experiences = experiences
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
                             facts: int("facts"), candidates: int("candidates"), experiences: int("experiences"))
    }
}

/// 整理记录 with today's count and this month's total.
public struct LiveMemoryRunsPage: Equatable, Sendable {
    public var runs: [LiveMemoryRun]
    public var usedToday: Int
    public var dailyLimit: Int
    public var monthCount: Int
    public var monthCostUsd: Double

    public init(runs: [LiveMemoryRun] = [], usedToday: Int = 0, dailyLimit: Int = 30, monthCount: Int = 0,
                monthCostUsd: Double = 0) {
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
                                  monthCostUsd: f["monthCostUsd"]?.numberText.flatMap { Double($0) } ?? 0)
    }
}
