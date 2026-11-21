import Foundation

/// E6 (2026-11-21): the unattended-work budget as `budget.status` returns it.
public struct LiveUsageCap: Equatable, Sendable, Identifiable {
    public var id = UUID()
    /// `day` or `month`.
    public var period: String
    public var tokens: Int?
    public var costUsd: Double?

    public init(period: String = "day", tokens: Int? = nil, costUsd: Double? = nil) {
        self.period = period; self.tokens = tokens; self.costUsd = costUsd
    }

    public static func == (a: LiveUsageCap, b: LiveUsageCap) -> Bool {
        a.period == b.period && a.tokens == b.tokens && a.costUsd == b.costUsd
    }

    public var json: JSONValue {
        var f: [String: JSONValue] = ["period": .string(period)]
        if let tokens { f["tokens"] = .number(String(tokens)) }
        if let costUsd { f["costUsd"] = .number(String(costUsd)) }
        return .object(f)
    }
}

public struct LiveBudget: Equatable, Sendable {
    public var caps: [LiveUsageCap]
    public var paused: Bool
    public var todayTokens: Int
    public var todayCost: Double
    public var monthTokens: Int
    public var monthCost: Double
    /// `go`, `near`, `stop`, `paused`.
    public var gate: String
    public var reason: String?

    public init(caps: [LiveUsageCap] = [], paused: Bool = false, todayTokens: Int = 0, todayCost: Double = 0,
                monthTokens: Int = 0, monthCost: Double = 0, gate: String = "go", reason: String? = nil) {
        self.caps = caps; self.paused = paused; self.todayTokens = todayTokens; self.todayCost = todayCost
        self.monthTokens = monthTokens; self.monthCost = monthCost; self.gate = gate; self.reason = reason
    }

    public static func decode(_ value: JSONValue) -> LiveBudget {
        let settings = value["settings"]?.fields ?? [:]
        let int = { (v: JSONValue?) in v?.numberText.flatMap { Int(Double($0) ?? 0) } ?? 0 }
        let double = { (v: JSONValue?) in v?.numberText.flatMap(Double.init) ?? 0 }
        return LiveBudget(
            caps: (settings["caps"]?.arrayValue ?? []).map { c in
                LiveUsageCap(period: c["period"]?.stringValue ?? "day",
                             tokens: c["tokens"]?.numberText.flatMap { Int(Double($0) ?? 0) },
                             costUsd: c["costUsd"]?.numberText.flatMap(Double.init))
            },
            paused: settings["paused"]?.boolValue ?? false,
            todayTokens: int(value["today"]?["tokens"]), todayCost: double(value["today"]?["costUsd"]),
            monthTokens: int(value["month"]?["tokens"]), monthCost: double(value["month"]?["costUsd"]),
            gate: value["gate"]?["kind"]?.stringValue ?? "go", reason: value["gate"]?["reason"]?.stringValue)
    }

    public var settingsJSON: JSONValue {
        .object(["caps": .array(caps.map(\.json)), "paused": .bool(paused)])
    }
}
