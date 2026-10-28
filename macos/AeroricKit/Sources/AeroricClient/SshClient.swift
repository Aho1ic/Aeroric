import AeroricProtocol
import Foundation

/// SSH hosts and remote projects (2026-10-27): host `connections.*` (saved
/// hosts, host keys, test) and `project.open_ssh`. Passwords go to the host's
/// keychain on save and never come back.
public struct SshClient: Sendable {
    let connection: HostConnection

    public init(connection: HostConnection) {
        self.connection = connection
    }

    public struct Host: Sendable, Equatable, Identifiable {
        public var id: String
        public var name: String
        public var group: String?
        public var host: String
        public var port: Int
        public var username: String
        public var identityFile: String?
        public var hasPassword: Bool

        public init(id: String, name: String, group: String? = nil, host: String, port: Int = 22, username: String,
                    identityFile: String? = nil, hasPassword: Bool = false) {
            self.id = id
            self.name = name
            self.group = group
            self.host = host
            self.port = port
            self.username = username
            self.identityFile = identityFile
            self.hasPassword = hasPassword
        }
    }

    public struct HostKey: Sendable, Equatable {
        public var keyType: String
        public var fingerprint: String
        public var knownHostsLine: String

        public init(keyType: String, fingerprint: String, knownHostsLine: String) {
            self.keyType = keyType
            self.fingerprint = fingerprint
            self.knownHostsLine = knownHostsLine
        }
    }

    public struct AgentVersion: Sendable, Equatable {
        public var agent: String
        public var version: String?
        public init(agent: String, version: String?) {
            self.agent = agent
            self.version = version
        }
    }

    /// A remote install / upgrade (R1/R3; offline O1–O6).
    public struct InstallJob: Sendable, Equatable {
        public var agent: String
        public var state: String
        public var command: String
        public var log: [String]
        public var message: String?
        /// "direct" or "offline".
        public var method: String
        /// download / upload / verify / check (offline installs).
        public var phase: String?
        public var done: UInt64
        public var total: UInt64
        public init(agent: String, state: String, command: String = "", log: [String] = [], message: String? = nil,
                    method: String = "direct", phase: String? = nil, done: UInt64 = 0, total: UInt64 = 0) {
            self.agent = agent
            self.state = state
            self.command = command
            self.log = log
            self.message = message
            self.method = method
            self.phase = phase
            self.done = done
            self.total = total
        }
        public var running: Bool { state == "running" }
        /// 0…1 for the current download / upload, nil when there is none.
        public var fraction: Double? {
            guard running, total > 0, phase == "download" || phase == "upload" else { return nil }
            return min(1, Double(done) / Double(total))
        }
    }

    /// Both ways to install an agent on a host (O1).
    public struct InstallPlan: Sendable, Equatable {
        public struct Offline: Sendable, Equatable {
            public var platform: String
            public var version: String
            public var size: UInt64
            public var cached: Bool
            public var destination: String
            public init(platform: String, version: String, size: UInt64, cached: Bool, destination: String) {
                self.platform = platform
                self.version = version
                self.size = size
                self.cached = cached
                self.destination = destination
            }
        }
        /// What a direct install runs remotely (nil: needs Node.js first).
        public var command: String?
        public var directHost: String
        /// Whether the host reached `directHost` at its last test (nil: unknown).
        public var directReachable: Bool?
        /// "direct" or "offline".
        public var recommended: String
        public var offline: Offline?
        public var offlineError: String?
        public init(command: String?, directHost: String = "", directReachable: Bool? = nil,
                    recommended: String = "direct", offline: Offline? = nil, offlineError: String? = nil) {
            self.command = command
            self.directHost = directHost
            self.directReachable = directReachable
            self.recommended = recommended
            self.offline = offline
            self.offlineError = offlineError
        }
    }

    /// What a host's last successful test found (R4).
    public struct Probe: Sendable, Equatable {
        public var agents: [AgentVersion]
        public var npm: String?
        public var node: String?
        public var checkedAtMs: UInt64?
        public init(agents: [AgentVersion], npm: String? = nil, node: String? = nil, checkedAtMs: UInt64? = nil) {
            self.agents = agents
            self.npm = npm
            self.node = node
            self.checkedAtMs = checkedAtMs
        }
    }

    public enum TestResult: Sendable, Equatable {
        case ok([AgentVersion])
        case untrusted([HostKey])
        case unreachable(String)
        case failed(String)
    }

    public func load() async throws -> [Host] {
        let value = try await connection.request("connections.load_ssh_connections", params: .object([:])).result
        return (value.arrayValue ?? []).compactMap(Self.decodeHost)
    }

    /// Saves the whole list; `password` sets one host's stored password.
    public func save(_ hosts: [Host], password: (id: String, value: String)? = nil) async throws {
        let list: [JSONValue] = hosts.map { host in
            var fields: [String: JSONValue] = [
                "id": .string(host.id), "name": .string(host.name), "host": .string(host.host),
                "port": .number(String(host.port)), "username": .string(host.username),
                "hasPassword": .bool(host.hasPassword),
            ]
            if let group = host.group, !group.isEmpty { fields["group"] = .string(group) }
            if let identity = host.identityFile, !identity.isEmpty { fields["identityFile"] = .string(identity) }
            if let password, password.id == host.id, !password.value.isEmpty {
                fields["password"] = .string(password.value)
            }
            return .object(fields)
        }
        _ = try await connection.request("connections.save_ssh_connections",
                                         params: .object(["connections": .array(list)]))
    }

    /// Groups the user created (a group may have no host yet).
    public func loadGroups() async throws -> [String] {
        let value = try await connection.request("connections.load_ssh_groups", params: .object([:])).result
        return (value["groups"]?.arrayValue ?? []).compactMap(\.stringValue)
    }

    public func saveGroups(_ groups: [String]) async throws {
        _ = try await saveGroupsReturning(groups)
    }

    /// Saves and returns the host's normalized list.
    public func saveGroupsReturning(_ groups: [String]) async throws -> [String] {
        let value = try await connection.request("connections.save_ssh_groups",
                                                 params: .object(["groups": .array(groups.map { .string($0) })])).result
        return (value["groups"]?.arrayValue ?? []).compactMap(\.stringValue)
    }

    public func delete(id: String) async throws {
        _ = try await connection.request("connections.delete_ssh_connection",
                                         params: .object(["connectionId": .string(id)]))
    }

    public func test(id: String) async throws -> TestResult {
        Self.decodeTest(try await connection.request("connections.test_ssh_connection",
                                                     params: .object(["id": .string(id)])).result)
    }

    /// The last successful test of every host.
    public func probes() async throws -> [String: Probe] {
        let value = try await connection.request("connections.ssh_probes", params: .object([:])).result
        var out: [String: Probe] = [:]
        for (id, item) in value.fields { if let probe = Self.decodeProbe(item) { out[id] = probe } }
        return out
    }

    /// Both ways installing or upgrading `agent` there could go.
    public func installPlan(id: String, agent: String) async throws -> InstallPlan {
        Self.decodePlan(try await connection.request("connections.remote_install_plan",
                                                     params: .object(["id": .string(id), "agent": .string(agent)])).result)
    }

    public static func decodePlan(_ value: JSONValue) -> InstallPlan {
        let offline = value["offline"].flatMap { item -> InstallPlan.Offline? in
            guard let platform = item["platform"]?.stringValue, let version = item["version"]?.stringValue else {
                return nil
            }
            return InstallPlan.Offline(platform: platform, version: version,
                                       size: item["size"]?.numberText.flatMap { UInt64($0) } ?? 0,
                                       cached: item["cached"]?.boolValue ?? false,
                                       destination: item["destination"]?.stringValue ?? "")
        }
        return InstallPlan(command: value["command"]?.stringValue, directHost: value["directHost"]?.stringValue ?? "",
                           directReachable: value["directReachable"]?.boolValue,
                           recommended: value["recommended"]?.stringValue ?? "direct", offline: offline,
                           offlineError: value["offlineError"]?.stringValue)
    }

    public func install(id: String, agent: String, method: String) async throws {
        _ = try await connection.request("connections.remote_install", params: .object([
            "id": .string(id), "agent": .string(agent), "method": .string(method),
        ]))
    }

    /// Bytes of standalone builds kept on this Mac for offline installs (O4).
    public func packageCacheBytes() async throws -> UInt64 {
        try await connection.request("connections.remote_package_cache", params: .object([:]))
            .result["bytes"]?.numberText.flatMap { UInt64($0) } ?? 0
    }

    public func clearPackageCache() async throws {
        _ = try await connection.request("connections.clear_remote_package_cache", params: .object([:]))
    }

    public func installStatus(id: String) async throws -> [InstallJob] {
        let value = try await connection.request("connections.remote_install_status",
                                                 params: .object(["id": .string(id)])).result
        return (value["jobs"]?.arrayValue ?? []).compactMap { item in
            guard let agent = item["agent"]?.stringValue, let state = item["state"]?.stringValue else { return nil }
            return InstallJob(agent: agent, state: state, command: item["command"]?.stringValue ?? "",
                              log: (item["log"]?.arrayValue ?? []).compactMap(\.stringValue),
                              message: item["message"]?.stringValue,
                              method: item["method"]?.stringValue ?? "direct", phase: item["phase"]?.stringValue,
                              done: item["done"]?.numberText.flatMap { UInt64($0) } ?? 0,
                              total: item["total"]?.numberText.flatMap { UInt64($0) } ?? 0)
        }
    }

    public static func decodeProbe(_ value: JSONValue) -> Probe? {
        guard value["state"]?.stringValue == "ok" else { return nil }
        return Probe(agents: (value["agents"]?.arrayValue ?? []).compactMap { item in
            item["agent"]?.stringValue.map { AgentVersion(agent: $0, version: item["version"]?.stringValue) }
        }, npm: value["npm"]?.stringValue, node: value["node"]?.stringValue,
        checkedAtMs: value["checkedAt"]?.numberText.flatMap { UInt64($0) })
    }

    public func trust(_ keys: [HostKey]) async throws {
        _ = try await connection.request("connections.trust_ssh_host_key", params: .object([
            "knownHostsLines": .array(keys.map { .string($0.knownHostsLine) }),
        ]))
    }

    /// A project on `connectionId` at `remotePath`; returns (project id, path).
    public func openRemoteProject(connectionId: String, remotePath: String, name: String?) async throws
        -> (id: String, path: String) {
        var params: [String: JSONValue] = [
            "connectionId": .string(connectionId), "remotePath": .string(remotePath),
            "newId": .string(UUID().uuidString),
        ]
        if let name, !name.trimmingCharacters(in: .whitespaces).isEmpty { params["name"] = .string(name) }
        let result = try await connection.request("project.open_ssh", params: .object(params)).result
        guard let id = result["project"]?["id"]?.stringValue,
              let path = result["project"]?["path"]?.stringValue else {
            throw URLError(.cannotParseResponse)
        }
        return (id, path)
    }

    public static func decodeHost(_ value: JSONValue) -> Host? {
        let f = value.fields
        guard let id = f["id"]?.stringValue, let host = f["host"]?.stringValue else { return nil }
        return Host(id: id, name: f["name"]?.stringValue ?? host, group: f["group"]?.stringValue,
                    host: host, port: f["port"]?.numberText.flatMap { Int($0) } ?? 22,
                    username: f["username"]?.stringValue ?? "", identityFile: f["identityFile"]?.stringValue,
                    hasPassword: f["hasPassword"]?.boolValue ?? false)
    }

    public static func decodeTest(_ value: JSONValue) -> TestResult {
        let f = value.fields
        switch f["state"]?.stringValue {
        case "ok":
            return .ok((f["agents"]?.arrayValue ?? []).compactMap { item in
                item["agent"]?.stringValue.map { AgentVersion(agent: $0, version: item["version"]?.stringValue) }
            })
        case "untrusted":
            return .untrusted((f["keys"]?.arrayValue ?? []).compactMap { item in
                guard let type = item["keyType"]?.stringValue, let fingerprint = item["fingerprint"]?.stringValue,
                      let line = item["knownHostsLine"]?.stringValue else { return nil }
                return HostKey(keyType: type, fingerprint: fingerprint, knownHostsLine: line)
            })
        case "unreachable":
            return .unreachable(f["message"]?.stringValue ?? "连不上这台主机")
        default:
            return .failed(f["message"]?.stringValue ?? "连接失败")
        }
    }
}
