import Foundation
import AeroricProtocol

/// `agentConnections.*` (G, Q23–Q34): connections, model catalog, pools and
/// agent configurations. The document mirrors the core's
/// `agent_connections::model` field for field so a save never drops data;
/// credentials only ever travel towards the host and come back as
/// "stored / missing".
public struct AgentConnectionsClient: Sendable {
    public enum MalformedReply: Error, Equatable, LocalizedError {
        case malformedReply(String)
        public var errorDescription: String? {
            switch self { case let .malformedReply(text): "宿主回复不完整：\(text)" }
        }
    }

    // MARK: Document

    public struct ReasoningTier: Codable, Equatable, Sendable, Hashable {
        public var label: String
        public var value: String
        public init(label: String, value: String) { self.label = label; self.value = value }
    }

    public struct ModelEntry: Codable, Equatable, Sendable, Hashable, Identifiable {
        public var upstreamId: String
        public var mappedName: String = ""
        public var displayName: String = ""
        /// `fetched` | `manual`.
        public var source: String = "manual"
        public var contextWindow: UInt64?
        public var maxOutput: UInt64?
        public var inputs: [String] = []
        public var reasoningTiers: [ReasoningTier] = []
        /// Empty means "the connection's protocols" (R10).
        public var protocols: [String] = []
        public var enabled: Bool = true

        public var id: String { upstreamId }
        /// What routing groups by: the mapped name, else the upstream id.
        public var routingName: String {
            let mapped = mappedName.trimmingCharacters(in: .whitespaces)
            return mapped.isEmpty ? upstreamId.trimmingCharacters(in: .whitespaces) : mapped
        }

        public init(upstreamId: String) { self.upstreamId = upstreamId }

        public init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            upstreamId = try c.decode(String.self, forKey: .upstreamId)
            mappedName = try c.decodeIfPresent(String.self, forKey: .mappedName) ?? ""
            displayName = try c.decodeIfPresent(String.self, forKey: .displayName) ?? ""
            source = try c.decodeIfPresent(String.self, forKey: .source) ?? "manual"
            contextWindow = try c.decodeIfPresent(UInt64.self, forKey: .contextWindow)
            maxOutput = try c.decodeIfPresent(UInt64.self, forKey: .maxOutput)
            inputs = try c.decodeIfPresent([String].self, forKey: .inputs) ?? []
            reasoningTiers = try c.decodeIfPresent([ReasoningTier].self, forKey: .reasoningTiers) ?? []
            protocols = try c.decodeIfPresent([String].self, forKey: .protocols) ?? []
            enabled = try c.decodeIfPresent(Bool.self, forKey: .enabled) ?? true
        }
    }

    public struct Connection: Codable, Equatable, Sendable, Hashable, Identifiable {
        public var id: String
        public var label: String
        /// `api_key` | `official_o_auth` | `linked_local_login`.
        public var kind: String = "api_key"
        /// `anthropic` | `open_ai` | `other`.
        public var vendor: String = "other"
        public var baseUrl: String = ""
        /// `responses` | `messages` | `chat_completions`.
        public var protocols: [String] = []
        public var useGlobalProxy: Bool = false
        public var models: [ModelEntry] = []
        public var catalogRevision: UInt64 = 0
        public var enabled: Bool = true
        public var restOnLimit: Bool = true

        public init(id: String, label: String) { self.id = id; self.label = label }

        public init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            id = try c.decode(String.self, forKey: .id)
            label = try c.decode(String.self, forKey: .label)
            kind = try c.decodeIfPresent(String.self, forKey: .kind) ?? "api_key"
            vendor = try c.decodeIfPresent(String.self, forKey: .vendor) ?? "other"
            baseUrl = try c.decodeIfPresent(String.self, forKey: .baseUrl) ?? ""
            protocols = try c.decodeIfPresent([String].self, forKey: .protocols) ?? []
            useGlobalProxy = try c.decodeIfPresent(Bool.self, forKey: .useGlobalProxy) ?? false
            models = try c.decodeIfPresent([ModelEntry].self, forKey: .models) ?? []
            catalogRevision = try c.decodeIfPresent(UInt64.self, forKey: .catalogRevision) ?? 0
            enabled = try c.decodeIfPresent(Bool.self, forKey: .enabled) ?? true
            restOnLimit = try c.decodeIfPresent(Bool.self, forKey: .restOnLimit) ?? true
        }
    }

    public struct PoolMember: Codable, Equatable, Sendable, Hashable {
        public var connectionId: String
        public var priority: Int = 0
        public var weight: UInt32 = 1
        public var free: Bool = false
        public init(connectionId: String) { self.connectionId = connectionId }
    }

    public struct Pool: Codable, Equatable, Sendable, Hashable, Identifiable {
        public var id: String
        public var label: String
        /// `priority` | `round_robin` | `weighted_round_robin` | `fill_first` |
        /// `least_used` | `free_first` | `pace`.
        public var strategy: String = "priority"
        public var members: [PoolMember] = []
        public var stickySessions: Bool = true
        public var stickyTtlSeconds: UInt64 = 3600
        public init(id: String, label: String) { self.id = id; self.label = label }
    }

    /// `{"kind": "connection" | "pool" | "route", …}` flattened.
    public struct ConfigTarget: Codable, Equatable, Sendable, Hashable {
        public var kind: String
        public var connectionId: String?
        public var poolId: String?
        public init(kind: String, connectionId: String? = nil, poolId: String? = nil) {
            self.kind = kind; self.connectionId = connectionId; self.poolId = poolId
        }
    }

    public struct AgentConfig: Codable, Equatable, Sendable, Hashable, Identifiable {
        public var id: String
        public var label: String
        /// `claude` | `codex` | `omp` | `dsh`.
        public var harness: String
        public var target: ConfigTarget
        public var defaultModel: String = ""
        public var defaultReasoning: String = ""
        /// R8: absent on the wire when empty, so optional here.
        public var fallbackModels: [String]?
        public init(id: String, label: String, harness: String, target: ConfigTarget) {
            self.id = id; self.label = label; self.harness = harness; self.target = target
        }

        /// Mapped names tried in order when the default model has no ready channel.
        public var fallbacks: [String] {
            get { fallbackModels ?? [] }
            set { fallbackModels = newValue.isEmpty ? nil : newValue }
        }
    }

    public struct Document: Codable, Equatable, Sendable {
        public var version: UInt32 = 1
        public var connections: [Connection] = []
        public var pools: [Pool] = []
        public var configs: [AgentConfig] = []
        public init() {}
    }

    /// The document plus, per connection, `stored` | `missing` | `unreadable`.
    public struct Snapshot: Equatable, Sendable {
        public var document: Document
        public var credentials: [String: String]
        public init(document: Document, credentials: [String: String]) {
            self.document = document; self.credentials = credentials
        }
    }

    // MARK: Status

    public struct Cooldown: Codable, Equatable, Sendable, Hashable {
        public var connectionId: String
        public var model: String?
        public var untilEpochS: UInt64
        /// `RateLimited` | `AuthFailed` | `PaymentRequired` | `NotFound` | …
        public var reason: String
        public init(connectionId: String, model: String?, untilEpochS: UInt64, reason: String) {
            self.connectionId = connectionId; self.model = model; self.untilEpochS = untilEpochS; self.reason = reason
        }
    }

    /// R8: a task whose last answer came from a fallback model.
    public struct Downgrade: Codable, Equatable, Sendable, Hashable {
        public var taskId: String
        public var requested: String
        public var served: String
        /// Why the requested model last failed (`RateLimited` | …), if known.
        public var reason: String?
        public var sinceEpochS: UInt64
        public init(taskId: String, requested: String, served: String, reason: String?, sinceEpochS: UInt64) {
            self.taskId = taskId; self.requested = requested; self.served = served
            self.reason = reason; self.sinceEpochS = sinceEpochS
        }
    }

    public struct OfficialLogin: Codable, Equatable, Sendable {
        public var installed: Bool
        public var signedIn: Bool
        public var mode: String
        public var account: String?
        public var sources: [String]
        public init(installed: Bool, signedIn: Bool, mode: String, account: String?, sources: [String]) {
            self.installed = installed; self.signedIn = signedIn; self.mode = mode
            self.account = account; self.sources = sources
        }
    }

    public struct OfficialStatus: Codable, Equatable, Sendable {
        public var codex: OfficialLogin
        public var claude: OfficialLogin
        public init(codex: OfficialLogin, claude: OfficialLogin) { self.codex = codex; self.claude = claude }
    }

    public struct Probe: Equatable, Sendable {
        public var `protocol`: String
        public var elapsedMs: UInt64
        public var status: UInt16
        public var excerpt: String
        public init(protocol: String, elapsedMs: UInt64, status: UInt16, excerpt: String) {
            self.protocol = `protocol`; self.elapsedMs = elapsedMs; self.status = status; self.excerpt = excerpt
        }
    }

    public struct FetchResult: Equatable, Sendable {
        public var snapshot: Snapshot
        public var fetched: Int
        public var added: Int
        public var kept: Int
        public init(snapshot: Snapshot, fetched: Int, added: Int, kept: Int) {
            self.snapshot = snapshot; self.fetched = fetched; self.added = added; self.kept = kept
        }
    }

    public struct Channel: Equatable, Sendable, Hashable {
        public var connectionId: String
        public var upstreamModel: String
        /// "直连" or "经网关 A → B".
        public var reach: String
    }

    public struct Candidates: Equatable, Sendable {
        public var models: [String]
        public var model: String
        public var channels: [Channel]
        /// (connection or pool id, reason code).
        public var excluded: [(id: String, reason: String)]

        public init(models: [String], model: String, channels: [Channel], excluded: [(id: String, reason: String)]) {
            self.models = models; self.model = model; self.channels = channels; self.excluded = excluded
        }

        public static func == (a: Candidates, b: Candidates) -> Bool {
            a.models == b.models && a.model == b.model && a.channels == b.channels
                && a.excluded.map(\.id) == b.excluded.map(\.id) && a.excluded.map(\.reason) == b.excluded.map(\.reason)
        }
    }

    // MARK: Calls

    private let connection: HostConnection
    public init(connection: HostConnection) { self.connection = connection }

    public func load() async throws -> Snapshot {
        try Self.decodeSnapshot(try await connection.request("agentConnections.load", params: .object([:])).result)
    }

    /// `credential` nil leaves the stored key alone; "" removes it.
    public func putConnection(_ value: Connection, credential: String?) async throws -> Snapshot {
        var params: [String: JSONValue] = ["connection": try Self.encode(value)]
        if let credential { params["credential"] = .string(credential) }
        return try Self.decodeSnapshot(try await connection.request("agentConnections.putConnection", params: .object(params)).result)
    }

    public func setCredential(connectionId: String, credential: String) async throws -> Snapshot {
        try Self.decodeSnapshot(try await connection.request("agentConnections.setCredential", params: .object([
            "connectionId": .string(connectionId), "credential": .string(credential),
        ])).result)
    }

    public func deleteConnection(id: String) async throws -> Snapshot {
        try await mutate("agentConnections.deleteConnection", ["id": .string(id)])
    }

    public func putPool(_ value: Pool) async throws -> Snapshot {
        try await mutate("agentConnections.putPool", ["pool": try Self.encode(value)])
    }

    public func deletePool(id: String) async throws -> Snapshot {
        try await mutate("agentConnections.deletePool", ["id": .string(id)])
    }

    public func putConfig(_ value: AgentConfig) async throws -> Snapshot {
        try await mutate("agentConnections.putConfig", ["config": try Self.encode(value)])
    }

    public func deleteConfig(id: String) async throws -> Snapshot {
        try await mutate("agentConnections.deleteConfig", ["id": .string(id)])
    }

    public func fetchModels(connectionId: String) async throws -> FetchResult {
        let reply = try await connection.request("agentConnections.fetchModels",
                                                 params: .object(["connectionId": .string(connectionId)])).result
        guard let snapshot = reply["snapshot"] else { throw MalformedReply.malformedReply("fetchModels needs snapshot") }
        func count(_ key: String) -> Int { reply[key]?.numberText.flatMap { Int($0) } ?? 0 }
        return FetchResult(snapshot: try Self.decodeSnapshot(snapshot), fetched: count("fetched"),
                           added: count("added"), kept: count("kept"))
    }

    public func candidates(configId: String, model: String?) async throws -> Candidates {
        var params: [String: JSONValue] = ["configId": .string(configId)]
        if let model { params["model"] = .string(model) }
        return try Self.decodeCandidates(try await connection.request("agentConnections.candidates", params: .object(params)).result)
    }

    public func status() async throws -> [Cooldown] {
        let reply = try await connection.request("agentConnections.status", params: .object([:])).result
        return try Self.decode([Cooldown].self, reply["cooldowns"] ?? .array([]), snake: false)
    }

    /// R7: connections that refused replayed thinking; the gateway strips it for them.
    public func reasoningStripped() async throws -> [String] {
        let reply = try await connection.request("agentConnections.status", params: .object([:])).result
        return (reply["reasoningStripped"]?.arrayValue ?? []).compactMap(\.stringValue)
    }

    /// R8: tasks currently answered by a fallback model.
    public func downgrades() async throws -> [Downgrade] {
        let reply = try await connection.request("agentConnections.status", params: .object([:])).result
        return try Self.decode([Downgrade].self, reply["downgrades"] ?? .array([]), snake: false)
    }

    public func officialStatus() async throws -> OfficialStatus {
        try Self.decode(OfficialStatus.self,
                        try await connection.request("agentConnections.officialStatus", params: .object([:])).result,
                        snake: false)
    }

    public func testConnection(connectionId: String, model: String) async throws -> Probe {
        let reply = try await connection.request("agentConnections.testConnection", params: .object([
            "connectionId": .string(connectionId), "model": .string(model),
        ])).result
        let probe = reply["probe"]
        guard let proto = reply["protocol"]?.stringValue,
              let elapsed = probe?["elapsedMs"]?.numberText.flatMap({ UInt64($0) }),
              let status = probe?["status"]?.numberText.flatMap({ UInt16($0) }) else {
            throw MalformedReply.malformedReply("testConnection reply is incomplete")
        }
        return Probe(protocol: proto, elapsedMs: elapsed, status: status, excerpt: probe?["excerpt"]?.stringValue ?? "")
    }

    private func mutate(_ method: String, _ params: [String: JSONValue]) async throws -> Snapshot {
        try Self.decodeSnapshot(try await connection.request(method, params: .object(params)).result)
    }

    // MARK: Coding (pure, tested)

    public static func decodeSnapshot(_ value: JSONValue) throws -> Snapshot {
        guard let document = value["document"] else { throw MalformedReply.malformedReply("snapshot needs document") }
        var credentials: [String: String] = [:]
        for (key, state) in value["credentials"]?.fields ?? [:] {
            credentials[key] = state.stringValue ?? "unreadable"
        }
        return Snapshot(document: try decode(Document.self, document, snake: true), credentials: credentials)
    }

    public static func decodeCandidates(_ value: JSONValue) throws -> Candidates {
        let channels = (value["candidates"]?.arrayValue ?? []).compactMap { item -> Channel? in
            guard let id = item["connection_id"]?.stringValue,
                  let model = item["upstream_model"]?.stringValue else { return nil }
            return Channel(connectionId: id, upstreamModel: model, reach: reachText(item["reach"] ?? .null))
        }
        let excluded = (value["excluded"]?.arrayValue ?? []).compactMap { item -> (id: String, reason: String)? in
            guard let id = item["id"]?.stringValue else { return nil }
            return (id, item["reason"]?["code"]?.stringValue ?? "unknown")
        }
        return Candidates(models: (value["models"]?.arrayValue ?? []).compactMap(\.stringValue),
                          model: value["model"]?.stringValue ?? "", channels: channels, excluded: excluded)
    }

    /// `"direct"` or `{"via_gateway": {"from": …, "to": …}}`.
    public static func reachText(_ reach: JSONValue) -> String {
        if reach.stringValue == "direct" { return "直连" }
        if let via = reach["via_gateway"], let from = via["from"]?.stringValue, let to = via["to"]?.stringValue {
            return "经网关 \(protocolTitle(from)) → \(protocolTitle(to))"
        }
        return "未知"
    }

    public static func protocolTitle(_ raw: String) -> String {
        switch raw {
        case "responses": "Responses"
        case "messages": "Messages"
        case "chat_completions": "Chat Completions"
        default: raw
        }
    }

    static func decode<T: Decodable>(_ type: T.Type, _ value: JSONValue, snake: Bool) throws -> T {
        let decoder = JSONDecoder()
        if snake { decoder.keyDecodingStrategy = .convertFromSnakeCase }
        let data = try JSONSerialization.data(withJSONObject: value.foundationObject, options: [.fragmentsAllowed])
        return try decoder.decode(T.self, from: data)
    }

    public static func encode<T: Encodable>(_ value: T) throws -> JSONValue {
        let encoder = JSONEncoder()
        encoder.keyEncodingStrategy = .convertToSnakeCase
        let data = try encoder.encode(value)
        return JSONValue.from(try JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed]))
    }
}
