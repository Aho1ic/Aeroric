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
