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
    /// H6 (review 2026-11-22): each capability pack item at this launch —
    /// given to the harness, or why not.
    public var packs: [PackItem] = []

    public struct PackItem: Equatable, Sendable {
        public var pack: String
        /// `skill` `mcpServer` `role` `command` `hook`.
        public var kind: String
        public var name: String
        public var ready: Bool
        public var basis: String

        public init(pack: String, kind: String, name: String, ready: Bool, basis: String) {
            self.pack = pack
            self.kind = kind
            self.name = name
            self.ready = ready
            self.basis = basis
        }
    }

    public init(atMs: UInt64 = 0, harness: String, injected: Bool, reason: String? = nil, tools: [LiveEffectiveTool]) {
        self.atMs = atMs
        self.harness = harness
        self.injected = injected
        self.reason = reason
        self.tools = tools
    }

    public static func decode(_ value: JSONValue?) -> LiveToolLaunch? {
        guard let f = value?.objectValue, let harness = f["harness"]?.stringValue else { return nil }
        var launch = LiveToolLaunch(atMs: f["atMs"]?.numberText.flatMap { UInt64($0) } ?? 0, harness: harness,
                                    injected: f["injected"]?.boolValue ?? false, reason: f["reason"]?.stringValue,
                                    tools: (f["tools"]?.arrayValue ?? []).compactMap(LiveEffectiveTool.decode))
        launch.packs = (f["packs"]?.arrayValue ?? []).compactMap { value in
            let p = value.fields
            guard let name = p["name"]?.stringValue else { return nil }
            return PackItem(pack: p["pack"]?.stringValue ?? "", kind: p["kind"]?.stringValue ?? "",
                            name: name, ready: p["ready"]?.boolValue ?? false, basis: p["basis"]?.stringValue ?? "")
        }
        return launch
    }
}
