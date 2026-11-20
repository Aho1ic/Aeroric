import Foundation

/// H6 (2026-11-21): capability packs as `packs.list` returns them.

public struct LivePackServer: Equatable, Sendable {
    public var name: String
    public var description: String
    public var command: String
    public var args: [String]
    /// Key values come back masked (`••••••••`); saving the mask keeps the old value.
    public var env: [String: String]
    public var enabled: Bool

    public static let mask = "••••••••"

    public init(name: String, description: String = "", command: String, args: [String] = [],
                env: [String: String] = [:], enabled: Bool = true) {
        self.name = name
        self.description = description
        self.command = command
        self.args = args
        self.env = env
        self.enabled = enabled
    }

    public static func decode(_ value: JSONValue) -> LivePackServer? {
        let f = value.fields
        guard let name = f["name"]?.stringValue else { return nil }
        return LivePackServer(name: name, description: f["description"]?.stringValue ?? "",
                              command: f["command"]?.stringValue ?? "",
                              args: (f["args"]?.arrayValue ?? []).compactMap(\.stringValue),
                              env: (f["env"]?.objectValue ?? [:]).compactMapValues(\.stringValue),
                              enabled: f["enabled"]?.boolValue ?? true)
    }

    public var json: JSONValue {
        .object(["name": .string(name), "description": .string(description), "command": .string(command),
                 "args": .array(args.map { .string($0) }), "env": .object(env.mapValues { .string($0) }),
                 "enabled": .bool(enabled)])
    }
}

public struct LivePack: Equatable, Sendable, Identifiable {
    public var id: String
    public var name: String
    public var description: String
    /// `local`, `builtin`, `official-marketplace`…
    public var sourceKind: String
    /// `aeroric` for a server added in Aeroric, else the folder it was imported from.
    public var sourceOrigin: String
    public var revision: String?
    public var confirmed: Bool
    public var enabled: Bool
    public var skills: [String]
    public var servers: [LivePackServer]
    public var roles: [String]
    public var commands: [String]
    public var hooks: [String]

    public init(id: String, name: String, description: String = "", sourceKind: String = "local",
                sourceOrigin: String = "aeroric", revision: String? = nil, confirmed: Bool = true,
                enabled: Bool = true, skills: [String] = [], servers: [LivePackServer] = [],
                roles: [String] = [], commands: [String] = [], hooks: [String] = []) {
        self.id = id; self.name = name; self.description = description
        self.sourceKind = sourceKind; self.sourceOrigin = sourceOrigin; self.revision = revision
        self.confirmed = confirmed; self.enabled = enabled
        self.skills = skills; self.servers = servers; self.roles = roles; self.commands = commands; self.hooks = hooks
    }

    public static func decode(_ value: JSONValue) -> LivePack? {
        let f = value.fields
        guard let id = f["id"]?.stringValue else { return nil }
        let source = f["source"]?.fields ?? [:]
        let names = { (key: String, field: String) in
            (f[key]?.arrayValue ?? []).compactMap { $0[field]?.stringValue }
        }
        return LivePack(id: id, name: f["name"]?.stringValue ?? id, description: f["description"]?.stringValue ?? "",
                        sourceKind: source["kind"]?.stringValue ?? "", sourceOrigin: source["origin"]?.stringValue ?? "",
                        revision: source["revision"]?.stringValue, confirmed: f["confirmed"]?.boolValue ?? false,
                        enabled: f["enabled"]?.boolValue ?? true,
                        skills: (f["skills"]?.arrayValue ?? []).compactMap(\.stringValue),
                        servers: (f["mcpServers"]?.arrayValue ?? []).compactMap(LivePackServer.decode),
                        roles: names("roles", "id"), commands: names("commands", "name"), hooks: names("hooks", "event"))
    }

    /// A server the user added in Aeroric (editable on the page).
    public var isOwnServer: Bool { sourceKind == "local" && sourceOrigin == "aeroric" && servers.count == 1 }
}

/// One item at one place: given, or why not.
public struct LivePackItem: Equatable, Sendable {
    /// `skill` `mcpServer` `role` `command` `hook`.
    public var kind: String
    public var name: String
    public var ready: Bool
    public var basis: String

    public init(kind: String, name: String, ready: Bool, basis: String) {
        self.kind = kind; self.name = name; self.ready = ready; self.basis = basis
    }
}

public struct LivePackPlace: Equatable, Sendable {
    /// "Claude", "Codex（Agent 预设）"…
    public var place: String
    public var items: [LivePackItem]

    public init(place: String, items: [LivePackItem]) { self.place = place; self.items = items }
}

public struct LivePackView: Equatable, Sendable, Identifiable {
    public var pack: LivePack
    public var trusted: Bool
    public var places: [LivePackPlace]
    public var id: String { pack.id }

    public init(pack: LivePack, trusted: Bool, places: [LivePackPlace] = []) {
        self.pack = pack; self.trusted = trusted; self.places = places
    }
}

public struct LivePackOverview: Equatable, Sendable {
    public var packs: [LivePackView]
    /// Folders under `packs/` that could not be read, with why.
    public var broken: [(folder: String, reason: String)]

    public init(packs: [LivePackView] = [], broken: [(folder: String, reason: String)] = []) {
        self.packs = packs; self.broken = broken
    }

    public static func == (a: LivePackOverview, b: LivePackOverview) -> Bool {
        a.packs == b.packs && a.broken.map(\.folder) == b.broken.map(\.folder)
            && a.broken.map(\.reason) == b.broken.map(\.reason)
    }

    public static func decode(_ value: JSONValue) -> LivePackOverview {
        let packs: [LivePackView] = (value["packs"]?.arrayValue ?? []).compactMap { entry in
            guard let pack = entry["pack"].flatMap(LivePack.decode) else { return nil }
            let places = (entry["places"]?.arrayValue ?? []).map { place in
                LivePackPlace(place: place["place"]?.stringValue ?? "",
                              items: (place["items"]?.arrayValue ?? []).map { item in
                                  LivePackItem(kind: item["kind"]?.stringValue ?? "", name: item["name"]?.stringValue ?? "",
                                               ready: item["ready"]?.boolValue ?? false, basis: item["basis"]?.stringValue ?? "")
                              })
            }
            return LivePackView(pack: pack, trusted: entry["trusted"]?.boolValue ?? false, places: places)
        }
        let broken = (value["broken"]?.arrayValue ?? []).map {
            (folder: $0["folder"]?.stringValue ?? "", reason: $0["reason"]?.stringValue ?? "")
        }
        return LivePackOverview(packs: packs, broken: broken)
    }
}
