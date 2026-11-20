import Foundation

/// C8 (2026-11-20): one tool of one launch of a session.
public struct LiveEffectiveTool: Equatable, Sendable {
    public var name: String
    /// `aeroric` `harness`.
    public var owner: String
    /// `base` `memory` `wiki` `references` `expert` `harness`.
    public var group: String
    /// `readOnly` `writes` `sendsOut`.
    public var scope: String
    public var ready: Bool
    public var basis: String

    public init(name: String, owner: String, group: String = "base", scope: String = "readOnly",
                ready: Bool = true, basis: String = "") {
        self.name = name
        self.owner = owner
        self.group = group
        self.scope = scope
        self.ready = ready
        self.basis = basis
    }

    public static func decode(_ value: JSONValue) -> LiveEffectiveTool? {
        let f = value.fields
        guard let name = f["name"]?.stringValue, let owner = f["owner"]?.stringValue else { return nil }
        return LiveEffectiveTool(name: name, owner: owner, group: f["group"]?.stringValue ?? "",
                                 scope: f["scope"]?.stringValue ?? "", ready: f["ready"]?.boolValue ?? false,
                                 basis: f["basis"]?.stringValue ?? "")
    }
}

/// C8: the tools one launch had (`session.capabilities {taskId}`'s `tools`).
public struct LiveToolLaunch: Equatable, Sendable {
    public var atMs: UInt64
    public var harness: String
    public var injected: Bool
    public var reason: String?
    public var tools: [LiveEffectiveTool]

    public init(atMs: UInt64 = 0, harness: String, injected: Bool, reason: String? = nil, tools: [LiveEffectiveTool]) {
        self.atMs = atMs
        self.harness = harness
        self.injected = injected
        self.reason = reason
        self.tools = tools
    }

    public static func decode(_ value: JSONValue?) -> LiveToolLaunch? {
        guard let f = value?.objectValue, let harness = f["harness"]?.stringValue else { return nil }
        return LiveToolLaunch(atMs: f["atMs"]?.numberText.flatMap { UInt64($0) } ?? 0, harness: harness,
                              injected: f["injected"]?.boolValue ?? false, reason: f["reason"]?.stringValue,
                              tools: (f["tools"]?.arrayValue ?? []).compactMap(LiveEffectiveTool.decode))
    }
}
