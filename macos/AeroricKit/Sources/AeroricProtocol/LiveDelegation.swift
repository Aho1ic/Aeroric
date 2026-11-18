import Foundation

/// B4-2 (2026-10-31): one delegation as the host reports it — the latest
/// `delegation` update of a `delegationId` replaces the card.
public struct LiveDelegationCard: Equatable, Sendable {
    public var delegationId: String
    public var parentTaskId: String
    public var childTaskId: String?
    /// `user` (from the app) or `session` (the parent's agent).
    public var origin: String
    public var roleName: String?
    public var harnessLabel: String
    public var agent: String
    public var model: String?
    public var contract: LiveDelegationContract
    /// `starting` `running` `checking` `reported` `failed` `stopped` `overBudget` `interrupted`.
    public var state: String
    public var createdMs: UInt64
    public var finishedMs: UInt64?
    public var result: String?
    public var files: Int
    public var additions: Int
    public var deletions: Int
    public var acceptance: [LiveDelegationAcceptance]
    public var evidence: Bool
    /// Handed to the delivery queue for the parent. A1: whether it arrived
    /// is `returnDelivery`, never this flag.
    public var sent: Bool
    public var reworks: Int
    public var error: String?
    public var tokens: UInt64?
    /// B4-3: failure probes that fired on it.
    public var probes: [LiveDelegationProbe]
    /// A1 (2026-11-16): the delivery carrying the result to the parent, as
    /// the queue last reported it.
    public var returnDelivery: LiveDelegationReturn?

    public init(delegationId: String, parentTaskId: String = "", childTaskId: String? = nil, origin: String = "user",
                roleName: String? = nil, harnessLabel: String = "", agent: String = "", model: String? = nil,
                contract: LiveDelegationContract = LiveDelegationContract(instruction: ""), state: String,
                createdMs: UInt64 = 0, finishedMs: UInt64? = nil, result: String? = nil, files: Int = 0,
                additions: Int = 0, deletions: Int = 0, acceptance: [LiveDelegationAcceptance] = [],
                evidence: Bool = false, sent: Bool = false, reworks: Int = 0, error: String? = nil,
                tokens: UInt64? = nil, probes: [LiveDelegationProbe] = [],
                returnDelivery: LiveDelegationReturn? = nil) {
        self.delegationId = delegationId
        self.parentTaskId = parentTaskId
        self.childTaskId = childTaskId
        self.origin = origin
        self.roleName = roleName
        self.harnessLabel = harnessLabel
        self.agent = agent
        self.model = model
        self.contract = contract
        self.state = state
        self.createdMs = createdMs
        self.finishedMs = finishedMs
        self.result = result
        self.files = files
        self.additions = additions
        self.deletions = deletions
        self.acceptance = acceptance
        self.evidence = evidence
        self.sent = sent
        self.reworks = reworks
        self.error = error
        self.tokens = tokens
        self.probes = probes
        self.returnDelivery = returnDelivery
    }

    public var isOpen: Bool { ["starting", "running", "checking"].contains(state) }
    /// "需要我处理": it failed, ran over budget, the host restarted under it,
    /// a probe fired that nobody has seen, or nobody knows whether its
    /// result reached the parent (A1).
    public var needsUser: Bool {
        ["failed", "overBudget", "interrupted"].contains(state) || probes.contains { !$0.acknowledged }
            || returnDelivery?.state == "unknown"
    }
    /// The user can send it to the parent now: never handed over, or its
    /// delivery failed, was paused, withdrawn or lost (A1: "再发一次").
    public var canSend: Bool {
        guard !isOpen else { return false }
        if let returnDelivery { return returnDelivery.canSendAgain || !sent }
        return !sent
    }
    /// Still something to do on its card: running, unsent, or needing the user.
    public var wantsUser: Bool { isOpen || canSend || needsUser }
    /// B3: a stale pass is not counted.
    public var passedCount: Int { acceptance.filter { $0.state == "passed" && !$0.stale }.count }
}

/// A1: the delivery a result card follows.
public struct LiveDelegationReturn: Equatable, Sendable {
    public var id: String
    /// `waiting` `sending` `accepted` `delivered` `failed` `unknown` `paused` `withdrawn`.
    public var state: String
    public var reason: String?
    /// `protocol` `echo` `turnStarted` `transcript`.
    public var evidence: String?

    public init(id: String, state: String, reason: String? = nil, evidence: String? = nil) {
        self.id = id
        self.state = state
        self.reason = reason
        self.evidence = evidence
    }

    /// Sending the card again does something: the same delivery goes again
    /// (failed, paused) or a new one does (unknown, withdrawn).
    public var canSendAgain: Bool { ["failed", "paused", "unknown", "withdrawn"].contains(state) }

    static func decode(_ value: JSONValue?) -> LiveDelegationReturn? {
        guard let f = value?.objectValue, let id = f["id"]?.stringValue, let state = f["state"]?.stringValue else {
            return nil
        }
        return LiveDelegationReturn(id: id, state: state, reason: f["reason"]?.stringValue,
                                    evidence: f["evidence"]?.stringValue)
    }
}

public struct LiveDelegationAcceptance: Equatable, Sendable {
    public var id: String
    public var text: String
    /// `open` `passed` `failed` `unknown`.
    public var state: String
    /// B3: what settled it (an auto check, the user); empty when not recorded.
    public var evidence: [LiveEvidenceRef]
    /// B3: the work tree the deciding check saw.
    public var checkedRevision: String?
    /// B3: passed, but files it rested on changed since.
    public var stale: Bool

    public init(id: String, text: String, state: String, evidence: [LiveEvidenceRef] = [],
                checkedRevision: String? = nil, stale: Bool = false) {
        self.id = id
        self.text = text
        self.state = state
        self.evidence = evidence
        self.checkedRevision = checkedRevision
        self.stale = stale
    }
}

public struct LiveAcceptanceItem: Equatable, Sendable {
    public var text: String
    public var check: String?

    public init(text: String, check: String? = nil) {
        self.text = text
        self.check = check
    }

    var json: JSONValue {
        var f: [String: JSONValue] = ["text": .string(text)]
        if let check, !check.isEmpty { f["check"] = .string(check) }
        return .object(f)
    }

    static func decode(_ value: JSONValue) -> LiveAcceptanceItem? {
        guard let text = value.fields["text"]?.stringValue else { return nil }
        return LiveAcceptanceItem(text: text, check: value.fields["check"]?.stringValue)
    }
}

/// Which harness runs a child.
public enum LiveDelegateAgent: Equatable, Sendable, Hashable {
    case same
    case other
    /// B2-6: `agentConfigId` is the host's binding to an agent configuration;
    /// a role saved without it would lose that binding.
    case named(agent: String, agentConfigId: String? = nil, model: String? = nil)

    public var json: JSONValue {
        switch self {
        case .same: return .string("same")
        case .other: return .string("other")
        case let .named(agent, configID, model):
            var fields: [String: JSONValue] = ["agent": .string(agent)]
            if let configID { fields["agentConfigId"] = .string(configID) }
            if let model { fields["model"] = .string(model) }
            return .object(fields)
        }
    }

    public static func decode(_ value: JSONValue?) -> LiveDelegateAgent {
        if value?.stringValue == "other" { return .other }
        if let agent = value?.fields["agent"]?.stringValue {
            return .named(agent: agent, agentConfigId: value?.fields["agentConfigId"]?.stringValue,
                          model: value?.fields["model"]?.stringValue)
        }
        return .same
    }
}

/// What a delegation asks for. `nil` fields take the role's defaults.
public struct LiveDelegationContract: Equatable, Sendable {
    public var instruction: String
    public var role: String?
    public var agent: LiveDelegateAgent?
    /// `fork` `brief` `isolated`.
    public var contextMode: String?
    public var contextRefs: [String]
    /// `plan` `ask` `auto_edit` `full_access`.
    public var permission: String?
    /// `readOnly` `shared`.
    public var workspace: String?
    public var acceptance: [LiveAcceptanceItem]
    public var budgetMinutes: Int?
    public var budgetTokens: UInt64?

    public init(instruction: String, role: String? = nil, agent: LiveDelegateAgent? = nil,
                contextMode: String? = nil, contextRefs: [String] = [], permission: String? = nil,
                workspace: String? = nil, acceptance: [LiveAcceptanceItem] = [], budgetMinutes: Int? = nil,
                budgetTokens: UInt64? = nil) {
        self.instruction = instruction
        self.role = role
        self.agent = agent
        self.contextMode = contextMode
        self.contextRefs = contextRefs
        self.permission = permission
        self.workspace = workspace
        self.acceptance = acceptance
        self.budgetMinutes = budgetMinutes
        self.budgetTokens = budgetTokens
    }

    public var json: JSONValue {
        var f: [String: JSONValue] = ["instruction": .string(instruction)]
        if let role { f["role"] = .string(role) }
        if let agent { f["agent"] = agent.json }
        if let contextMode { f["contextMode"] = .string(contextMode) }
        if !contextRefs.isEmpty { f["contextRefs"] = .array(contextRefs.map(JSONValue.string)) }
        if let permission { f["permission"] = .string(permission) }
        if let workspace { f["workspace"] = .string(workspace) }
        if !acceptance.isEmpty { f["acceptance"] = .array(acceptance.map(\.json)) }
        if budgetMinutes != nil || budgetTokens != nil {
            var b: [String: JSONValue] = [:]
            if let budgetMinutes { b["minutes"] = .number(String(budgetMinutes)) }
            if let budgetTokens { b["tokens"] = .number(String(budgetTokens)) }
            f["budget"] = .object(b)
        }
        return .object(f)
    }

    /// A resolved contract as the card carries it.
    static func decode(_ value: JSONValue?) -> LiveDelegationContract {
        let f = value?.fields ?? [:]
        let budget = f["budget"]?.fields ?? [:]
        return LiveDelegationContract(
            instruction: f["instruction"]?.stringValue ?? "", role: f["role"]?.stringValue,
            agent: LiveDelegateAgent.decode(f["agent"]), contextMode: f["contextMode"]?.stringValue,
            contextRefs: (f["contextRefs"]?.arrayValue ?? []).compactMap(\.stringValue),
            permission: f["permission"]?.stringValue, workspace: f["workspace"]?.stringValue,
            acceptance: (f["acceptance"]?.arrayValue ?? []).compactMap(LiveAcceptanceItem.decode),
            budgetMinutes: budget["minutes"]?.numberText.flatMap { Int($0) },
            budgetTokens: budget["tokens"]?.numberText.flatMap { UInt64($0) })
    }
}

extension LiveDelegationCard {
    public static func decode(_ value: JSONValue) -> LiveDelegationCard? {
        let f = value.fields
        guard let id = f["delegationId"]?.stringValue, let state = f["state"]?.stringValue else { return nil }
        let int = { (key: String) in f[key]?.numberText.flatMap { Int($0) } ?? 0 }
        let acceptance = (f["acceptance"]?.arrayValue ?? []).compactMap { item -> LiveDelegationAcceptance? in
            let g = item.fields
            guard let id = g["id"]?.stringValue, let text = g["text"]?.stringValue else { return nil }
            return LiveDelegationAcceptance(
                id: id, text: text, state: g["state"]?.stringValue ?? "open",
                evidence: (g["evidence"]?.arrayValue ?? []).compactMap(LiveEvidenceRef.decode),
                checkedRevision: g["checkedRevision"]?.stringValue, stale: g["stale"]?.boolValue ?? false)
        }
        return LiveDelegationCard(
            delegationId: id, parentTaskId: f["parentTaskId"]?.stringValue ?? "",
            childTaskId: f["childTaskId"]?.stringValue, origin: f["origin"]?.stringValue ?? "user",
            roleName: f["roleName"]?.stringValue, harnessLabel: f["harnessLabel"]?.stringValue ?? "",
            agent: f["agent"]?.stringValue ?? "", model: f["model"]?.stringValue,
            contract: LiveDelegationContract.decode(f["contract"]), state: state,
            createdMs: f["createdMs"]?.numberText.flatMap { UInt64($0) } ?? 0,
            finishedMs: f["finishedMs"]?.numberText.flatMap { UInt64($0) }, result: f["result"]?.stringValue,
            files: int("files"), additions: int("additions"), deletions: int("deletions"),
            acceptance: acceptance, evidence: f["evidence"]?.boolValue ?? false,
            sent: f["sent"]?.boolValue ?? false, reworks: int("reworks"), error: f["error"]?.stringValue,
            tokens: f["tokens"]?.numberText.flatMap { UInt64($0) },
            probes: (f["probes"]?.arrayValue ?? []).compactMap(LiveDelegationProbe.decode),
            returnDelivery: LiveDelegationReturn.decode(f["returnDelivery"]))
    }
}

/// One entry of the role library.
public struct LiveDelegationRole: Equatable, Sendable, Identifiable {
    public var id: String
    public var name: String
    public var description: String
    public var instruction: String
    public var agent: LiveDelegateAgent
    public var contextMode: String
    public var permission: String
    public var workspace: String
    public var acceptance: [LiveAcceptanceItem]
    public var budgetMinutes: Int?
    public var builtin: Bool
    public var customized: Bool

    public init(id: String = "", name: String, description: String = "", instruction: String = "",
                agent: LiveDelegateAgent = .same, contextMode: String = "brief", permission: String = "plan",
                workspace: String = "readOnly", acceptance: [LiveAcceptanceItem] = [], budgetMinutes: Int? = nil,
                builtin: Bool = false, customized: Bool = false) {
        self.id = id
        self.name = name
        self.description = description
        self.instruction = instruction
        self.agent = agent
        self.contextMode = contextMode
        self.permission = permission
        self.workspace = workspace
        self.acceptance = acceptance
        self.budgetMinutes = budgetMinutes
        self.builtin = builtin
        self.customized = customized
    }

    public var json: JSONValue {
        var f: [String: JSONValue] = [
            "id": .string(id), "name": .string(name), "description": .string(description),
            "instruction": .string(instruction), "agent": agent.json, "contextMode": .string(contextMode),
            "permission": .string(permission), "workspace": .string(workspace),
            "acceptance": .array(acceptance.map(\.json)),
        ]
        f["budget"] = .object(budgetMinutes.map { ["minutes": .number(String($0))] } ?? [:])
        return .object(f)
    }

    public static func decode(_ value: JSONValue) -> LiveDelegationRole? {
        let f = value.fields
        guard let id = f["id"]?.stringValue, let name = f["name"]?.stringValue else { return nil }
        return LiveDelegationRole(
            id: id, name: name, description: f["description"]?.stringValue ?? "",
            instruction: f["instruction"]?.stringValue ?? "", agent: LiveDelegateAgent.decode(f["agent"]),
            contextMode: f["contextMode"]?.stringValue ?? "brief", permission: f["permission"]?.stringValue ?? "plan",
            workspace: f["workspace"]?.stringValue ?? "readOnly",
            acceptance: (f["acceptance"]?.arrayValue ?? []).compactMap(LiveAcceptanceItem.decode),
            budgetMinutes: f["budget"]?.fields["minutes"]?.numberText.flatMap { Int($0) },
            builtin: f["builtin"]?.boolValue ?? false, customized: f["customized"]?.boolValue ?? false)
    }
}

/// One line of the audit log.
public struct LiveDelegationAudit: Equatable, Sendable {
    public var atMs: UInt64
    public var actor: String
    /// `start` `refuse` `report` `fail` `stop` `redo` `send` `overBudget`.
    public var action: String
    public var delegationId: String
    public var childTaskId: String?
    public var agent: String
    public var permission: String
    public var detail: String

    public init(atMs: UInt64, actor: String, action: String, delegationId: String = "", childTaskId: String? = nil,
                agent: String = "", permission: String = "", detail: String = "") {
        self.atMs = atMs
        self.actor = actor
        self.action = action
        self.delegationId = delegationId
        self.childTaskId = childTaskId
        self.agent = agent
        self.permission = permission
        self.detail = detail
    }

    public static func decode(_ value: JSONValue) -> LiveDelegationAudit? {
        let f = value.fields
        guard let action = f["action"]?.stringValue else { return nil }
        return LiveDelegationAudit(
            atMs: f["atMs"]?.numberText.flatMap { UInt64($0) } ?? 0, actor: f["actor"]?.stringValue ?? "",
            action: action, delegationId: f["delegationId"]?.stringValue ?? "",
            childTaskId: f["childTaskId"]?.stringValue, agent: f["agent"]?.stringValue ?? "",
            permission: f["permission"]?.stringValue ?? "", detail: f["detail"]?.stringValue ?? "")
    }
}

/// Track record of one harness × model × role.
public struct LiveReputationRow: Equatable, Sendable {
    public var agent: String
    public var model: String?
    public var role: String?
    public var total: Int
    public var reported: Int
    public var failed: Int
    public var accepted: Int
    public var withAcceptance: Int
    public var reworks: Int

    public init(agent: String, model: String? = nil, role: String? = nil, total: Int = 0, reported: Int = 0,
                failed: Int = 0, accepted: Int = 0, withAcceptance: Int = 0, reworks: Int = 0) {
        self.agent = agent
        self.model = model
        self.role = role
        self.total = total
        self.reported = reported
        self.failed = failed
        self.accepted = accepted
        self.withAcceptance = withAcceptance
        self.reworks = reworks
    }

    public static func decode(_ value: JSONValue) -> LiveReputationRow? {
        let f = value.fields
        guard let agent = f["agent"]?.stringValue else { return nil }
        let int = { (key: String) in f[key]?.numberText.flatMap { Int($0) } ?? 0 }
        return LiveReputationRow(agent: agent, model: f["model"]?.stringValue, role: f["role"]?.stringValue,
                                 total: int("total"), reported: int("reported"), failed: int("failed"),
                                 accepted: int("accepted"), withAcceptance: int("withAcceptance"),
                                 reworks: int("reworks"))
    }
}
