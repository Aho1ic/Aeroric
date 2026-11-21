import Foundation

/// D5 (2026-11-21): a work plan as `delegation.plans` returns it — its units
/// and, for each, its state and the one sentence that explains it.
public struct LiveWorkUnit: Equatable, Sendable, Identifiable {
    public var id: String
    public var title: String
    public var dependsOn: [String]
    /// `notStarted`, `waitingDeps`, `running`, `awaitingAcceptance`, `passed`, `failed`, `cancelled`.
    public var state: String
    public var reason: String
    public var inputsStale: Bool
    public var delegationId: String?

    public init(id: String, title: String, dependsOn: [String] = [], state: String = "notStarted",
                reason: String = "", inputsStale: Bool = false, delegationId: String? = nil) {
        self.id = id; self.title = title; self.dependsOn = dependsOn; self.state = state
        self.reason = reason; self.inputsStale = inputsStale; self.delegationId = delegationId
    }
}

public struct LiveWorkPlan: Equatable, Sendable, Identifiable {
    public var id: String
    public var title: String
    public var confirmed: Bool
    public var units: [LiveWorkUnit]

    public init(id: String, title: String, confirmed: Bool = false, units: [LiveWorkUnit] = []) {
        self.id = id; self.title = title; self.confirmed = confirmed; self.units = units
    }

    /// One `{plan, statuses}` entry.
    public static func decode(_ value: JSONValue) -> LiveWorkPlan? {
        let plan = value["plan"]?.fields ?? [:]
        guard let id = plan["id"]?.stringValue else { return nil }
        var statuses: [String: [String: JSONValue]] = [:]
        for status in value["statuses"]?.arrayValue ?? [] {
            if let unit = status["id"]?.stringValue { statuses[unit] = status.fields }
        }
        let units: [LiveWorkUnit] = (plan["units"]?.arrayValue ?? []).compactMap { u in
            guard let unit = u["id"]?.stringValue else { return nil }
            let s = statuses[unit] ?? [:]
            return LiveWorkUnit(id: unit, title: u["title"]?.stringValue ?? unit,
                                dependsOn: (u["dependsOn"]?.arrayValue ?? []).compactMap(\.stringValue),
                                state: s["state"]?.stringValue ?? "notStarted", reason: s["reason"]?.stringValue ?? "",
                                inputsStale: s["inputsStale"]?.boolValue ?? false,
                                delegationId: u["delegationId"]?.stringValue)
        }
        return LiveWorkPlan(id: id, title: plan["title"]?.stringValue ?? "", confirmed: plan["confirmed"]?.boolValue ?? false,
                            units: units)
    }
}
