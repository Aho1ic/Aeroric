import Foundation

/// A12 (2026-11-14): when a 定时任务 fires. Times of day are minutes after
/// local midnight; weekdays are 1 (Monday) … 7 (Sunday).
public enum LiveScheduleRepeat: Equatable, Sendable {
    case once(atMs: UInt64)
    case daily(minute: Int)
    case weekly(weekdays: [Int], minute: Int)
    case every(hours: Int, fromMs: UInt64)

    public static func decode(_ value: JSONValue?) -> LiveScheduleRepeat? {
        guard let f = value?.objectValue else { return nil }
        let number = { (key: String) in f[key]?.numberText.flatMap { UInt64($0) } ?? 0 }
        switch f["kind"]?.stringValue {
        case "once": return .once(atMs: number("atMs"))
        case "daily": return .daily(minute: Int(number("minute")))
        case "weekly":
            let days = (f["weekdays"]?.arrayValue ?? []).compactMap { $0.numberText.flatMap { Int($0) } }
            return .weekly(weekdays: days, minute: Int(number("minute")))
        case "every": return .every(hours: Int(number("hours")), fromMs: number("fromMs"))
        default: return nil
        }
    }

    public var json: JSONValue {
        switch self {
        case let .once(atMs): .object(["kind": .string("once"), "atMs": .number(String(atMs))])
        case let .daily(minute): .object(["kind": .string("daily"), "minute": .number(String(minute))])
        case let .weekly(weekdays, minute):
            .object(["kind": .string("weekly"), "weekdays": .array(weekdays.map { .number(String($0)) }),
                     "minute": .number(String(minute))])
        case let .every(hours, fromMs):
            .object(["kind": .string("every"), "hours": .number(String(hours)), "fromMs": .number(String(fromMs))])
        }
    }
}

/// 2026-11-14 活跃时段: minutes after local midnight; an end before the
/// start crosses midnight.
public struct LiveActiveHours: Equatable, Sendable {
    public var start: Int
    public var end: Int
    public init(start: Int, end: Int) {
        self.start = start
        self.end = end
    }
}

public struct LiveScheduleRule: Equatable, Sendable, Identifiable {
    public var id: String
    public var name: String
    public var projectId: String
    public var agent: String
    public var model: String?
    public var prompt: String
    public var repeatRule: LiveScheduleRepeat
    /// §3.8: a shell command run in the project first; exit 0 wakes the model.
    public var check: String?
    public var enabled: Bool
    public var nextMs: UInt64?
    /// 配额感知调度 (2026-11-14): the Codex quota was used up; caught up at
    /// this time. Read only: a save clears it.
    public var quotaRetryMs: UInt64?
    /// 第二批 (2026-11-14): `skip` or `after` (run once the busy session ends).
    public var whenBusy: String = "skip"
    public var activeHours: LiveActiveHours?
    /// Read only: put off by the hours to this time; a busy session holds one.
    public var hoursRetryMs: UInt64?
    public var busyPending = false
    /// E16 (2026-11-21): the JSON structure the session's last reply must
    /// give its result in; nil asks for none.
    public var resultSchema: JSONValue?

    public init(id: String = "", name: String = "", projectId: String = "", agent: String = "",
                model: String? = nil, prompt: String = "", repeatRule: LiveScheduleRepeat = .daily(minute: 9 * 60),
                check: String? = nil, enabled: Bool = true, nextMs: UInt64? = nil, quotaRetryMs: UInt64? = nil) {
        self.id = id
        self.name = name
        self.projectId = projectId
        self.agent = agent
        self.model = model
        self.prompt = prompt
        self.repeatRule = repeatRule
        self.check = check
        self.enabled = enabled
        self.nextMs = nextMs
        self.quotaRetryMs = quotaRetryMs
    }

    public static func decode(_ value: JSONValue) -> LiveScheduleRule? {
        guard let f = value.objectValue, let id = f["id"]?.stringValue,
              let repeatRule = LiveScheduleRepeat.decode(f["repeat"]) else { return nil }
        return LiveScheduleRule(id: id, name: f["name"]?.stringValue ?? "", projectId: f["projectId"]?.stringValue ?? "",
                                agent: f["agent"]?.stringValue ?? "", model: f["model"]?.stringValue,
                                prompt: f["prompt"]?.stringValue ?? "", repeatRule: repeatRule,
                                check: f["check"]?.stringValue, enabled: f["enabled"]?.boolValue ?? false,
                                nextMs: f["nextMs"]?.numberText.flatMap { UInt64($0) },
                                quotaRetryMs: f["quotaRetryMs"]?.numberText.flatMap { UInt64($0) })
            .with { rule in
                rule.whenBusy = f["whenBusy"]?.stringValue ?? "skip"
                if let h = f["activeHours"]?.objectValue,
                   let start = h["startMinute"]?.numberText.flatMap({ Int($0) }),
                   let end = h["endMinute"]?.numberText.flatMap({ Int($0) }) {
                    rule.activeHours = LiveActiveHours(start: start, end: end)
                }
                rule.hoursRetryMs = f["hoursRetryMs"]?.numberText.flatMap { UInt64($0) }
                rule.busyPending = f["busyPending"]?.boolValue ?? false
                rule.resultSchema = f["resultSchema"].flatMap { $0.isNull ? nil : $0 }
            }
    }

    public var json: JSONValue {
        var f: [String: JSONValue] = [
            "id": .string(id), "name": .string(name), "projectId": .string(projectId), "agent": .string(agent),
            "prompt": .string(prompt), "repeat": repeatRule.json, "enabled": .bool(enabled),
        ]
        if let model { f["model"] = .string(model) }
        if let check, !check.trimmingCharacters(in: .whitespaces).isEmpty { f["check"] = .string(check) }
        if whenBusy == "after" { f["whenBusy"] = .string("after") }
        if let resultSchema { f["resultSchema"] = resultSchema }
        if let activeHours {
            f["activeHours"] = .object(["startMinute": .number(String(activeHours.start)),
                                        "endMinute": .number(String(activeHours.end))])
        }
        return .object(f)
    }

    func with(_ change: (inout LiveScheduleRule) -> Void) -> LiveScheduleRule {
        var copy = self
        change(&copy)
        return copy
    }
}

public struct LiveScheduleRun: Equatable, Sendable, Identifiable {
    public var id: String
    public var ruleId: String
    public var dueMs: UInt64
    public var atMs: UInt64
    /// `started` `checkFailed` `busy` `queued` `outsideHours` `limited` `quotaOut` `failed`.
    public var outcome: String
    public var taskId: String?
    public var detail: String?
    public var catchUp: Bool
    public var manual: Bool
    /// E16: asked for a structured result; `result` when it fit,
    /// `resultError` (the run failed) when it did not or never came.
    public var resultExpected = false
    public var result: JSONValue?
    public var resultError: String?

    public init(id: String, ruleId: String = "", dueMs: UInt64 = 0, atMs: UInt64 = 0, outcome: String = "started",
                taskId: String? = nil, detail: String? = nil, catchUp: Bool = false, manual: Bool = false) {
        self.id = id
        self.ruleId = ruleId
        self.dueMs = dueMs
        self.atMs = atMs
        self.outcome = outcome
        self.taskId = taskId
        self.detail = detail
        self.catchUp = catchUp
        self.manual = manual
    }

    public static func decode(_ value: JSONValue) -> LiveScheduleRun? {
        guard let f = value.objectValue, let id = f["id"]?.stringValue else { return nil }
        let number = { (key: String) in f[key]?.numberText.flatMap { UInt64($0) } ?? 0 }
        var run = LiveScheduleRun(id: id, ruleId: f["ruleId"]?.stringValue ?? "", dueMs: number("dueMs"),
                                  atMs: number("atMs"), outcome: f["outcome"]?.stringValue ?? "failed",
                                  taskId: f["taskId"]?.stringValue, detail: f["detail"]?.stringValue,
                                  catchUp: f["catchUp"]?.boolValue ?? false, manual: f["manual"]?.boolValue ?? false)
        run.resultExpected = f["resultExpected"]?.boolValue ?? false
        run.result = f["result"].flatMap { $0.isNull ? nil : $0 }
        run.resultError = f["resultError"]?.stringValue
        return run
    }
}
