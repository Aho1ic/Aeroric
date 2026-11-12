import Foundation

/// R5-2 (2026-11-11): one turn run again with other configurations, read
/// back from `rerun.list`. Mirrors the host's `session_live_services/rerun.rs`.
public struct LiveRerunGroup: Equatable, Sendable, Identifiable {
    public var id: String
    public var turnId: String
    public var message: String
    public var withBrief: Bool
    public var createdMs: UInt64
    /// The original turn, the first column.
    public var original: LiveRerunColumn
    public var runs: [LiveRerunRun]

    public init(id: String, turnId: String, message: String = "", withBrief: Bool = true, createdMs: UInt64 = 0,
                original: LiveRerunColumn = LiveRerunColumn(), runs: [LiveRerunRun] = []) {
        self.id = id
        self.turnId = turnId
        self.message = message
        self.withBrief = withBrief
        self.createdMs = createdMs
        self.original = original
        self.runs = runs
    }

    public static func decodeList(_ value: JSONValue?) -> [LiveRerunGroup] {
        (value?.arrayValue ?? []).compactMap(decode)
    }

    public static func decode(_ value: JSONValue) -> LiveRerunGroup? {
        let f = value.fields
        let group = f["group"]?.fields ?? [:]
        guard let id = group["id"]?.stringValue, let turnId = group["turnId"]?.stringValue else { return nil }
        let runs = (f["runs"]?.arrayValue ?? []).compactMap(LiveRerunRun.decode)
        return LiveRerunGroup(id: id, turnId: turnId, message: group["message"]?.stringValue ?? "",
                              withBrief: group["withBrief"]?.boolValue ?? true,
                              createdMs: group["createdMs"]?.numberText.flatMap { UInt64($0) } ?? 0,
                              original: LiveRerunColumn.decode(f["original"]), runs: runs)
    }
}

public struct LiveRerunRun: Equatable, Sendable, Identifiable {
    public var id: String { taskId }
    public var taskId: String
    /// e.g. `Codex · gpt-6.1-sol`.
    public var label: String
    /// The session was removed.
    public var gone: Bool
    /// Its isolated copy is still there to bring back or drop.
    public var copy: Bool
    public var result: LiveRerunColumn

    public init(taskId: String, label: String, gone: Bool = false, copy: Bool = true,
                result: LiveRerunColumn = LiveRerunColumn()) {
        self.taskId = taskId
        self.label = label
        self.gone = gone
        self.copy = copy
        self.result = result
    }

    static func decode(_ value: JSONValue) -> LiveRerunRun? {
        let f = value.fields
        let run = f["run"]?.fields ?? [:]
        guard let taskId = run["taskId"]?.stringValue else { return nil }
        return LiveRerunRun(taskId: taskId, label: run["label"]?.stringValue ?? taskId,
                            gone: f["gone"]?.boolValue ?? false, copy: f["copy"]?.boolValue ?? true,
                            result: LiveRerunColumn.decode(f["result"]))
    }
}

/// One column of the comparison.
public struct LiveRerunColumn: Equatable, Sendable {
    /// running / pending / completed / notRun / interrupted / productFailure
    /// / replaced (the original turn, undone after the rerun started).
    public var outcome: String
    public var notRun: SessionTranscriptStats.NotRun?
    public var productFailure: SessionTranscriptStats.ProductFailure?
    public var inputTokens: UInt64?
    public var outputTokens: UInt64?
    public var costUsd: Double?
    public var durationMs: UInt64?
    public var tools: Int?
    public var files: Int
    public var additions: UInt64
    public var deletions: UInt64
    /// nil when no check ran on it.
    public var checkPassed: Bool?
    public var checkSummary: String?

    public init(outcome: String = "pending", notRun: SessionTranscriptStats.NotRun? = nil,
                productFailure: SessionTranscriptStats.ProductFailure? = nil, inputTokens: UInt64? = nil,
                outputTokens: UInt64? = nil, costUsd: Double? = nil, durationMs: UInt64? = nil, tools: Int? = nil,
                files: Int = 0, additions: UInt64 = 0, deletions: UInt64 = 0, checkPassed: Bool? = nil,
                checkSummary: String? = nil) {
        self.outcome = outcome
        self.notRun = notRun
        self.productFailure = productFailure
        self.inputTokens = inputTokens
        self.outputTokens = outputTokens
        self.costUsd = costUsd
        self.durationMs = durationMs
        self.tools = tools
        self.files = files
        self.additions = additions
        self.deletions = deletions
        self.checkPassed = checkPassed
        self.checkSummary = checkSummary
    }

    static func decode(_ value: JSONValue?) -> LiveRerunColumn {
        let f = value?.fields ?? [:]
        func n(_ key: String) -> UInt64? { f[key]?.numberText.flatMap { UInt64($0) } }
        var column = LiveRerunColumn(outcome: f["outcome"]?.stringValue ?? "pending")
        if let notRun = f["notRun"]?.objectValue, let reason = notRun["reason"]?.stringValue {
            column.notRun = .init(reason: reason, status: notRun["status"]?.numberText.flatMap { Int($0) },
                                  message: notRun["message"]?.stringValue ?? "")
        }
        if let failure = f["productFailure"]?.objectValue {
            column.productFailure = .init(undone: failure["undone"]?.boolValue ?? false,
                                          check: failure["check"]?.stringValue)
        }
        column.inputTokens = n("inputTokens")
        column.outputTokens = n("outputTokens")
        column.costUsd = f["costUsd"]?.numberText.flatMap { Double($0) }
        column.durationMs = n("durationMs")
        column.tools = n("tools").map { Int($0) }
        column.files = Int(n("files") ?? 0)
        column.additions = n("additions") ?? 0
        column.deletions = n("deletions") ?? 0
        if let check = f["check"]?.objectValue {
            column.checkPassed = check["passed"]?.boolValue
            column.checkSummary = check["summary"]?.stringValue
        }
        return column
    }
}

/// One configuration chosen to run the turn again.
public struct LiveRerunChoice: Hashable, Sendable {
    public var configurationId: String
    public var model: String?
    public init(configurationId: String, model: String?) {
        self.configurationId = configurationId
        self.model = model
    }
}

/// What `rerun.start` answered: the runs it opened and those refused.
public struct LiveRerunStarted: Equatable, Sendable {
    public var runs: Int
    public var refused: [String]
    public init(runs: Int, refused: [String] = []) {
        self.runs = runs
        self.refused = refused
    }

    public static func decode(_ value: JSONValue) -> LiveRerunStarted {
        let f = value.fields
        return LiveRerunStarted(runs: f["group"]?.fields["runs"]?.arrayValue?.count ?? 0,
                                refused: (f["refused"]?.arrayValue ?? []).compactMap(\.stringValue))
    }
}
