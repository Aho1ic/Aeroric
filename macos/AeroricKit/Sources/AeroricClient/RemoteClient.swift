import AeroricProtocol
import Foundation

/// P8-03 刀五：远程 plane 的类型化客户端（宿主 `remote.status/start/stop/
/// updateConfig/selectLanIp/createInvite/listDevices/revokeDevice`）。
/// wire 解析只在 AeroricClient；视图经 RemoteStore 消费。
/// 远程面解析失败（形状与 core 不符）。
public enum RemoteClientError: Error, Sendable {
    case malformedReply
}

public struct RemoteClient: Sendable {
    let connection: HostConnection

    public init(connection: HostConnection) {
        self.connection = connection
    }

    // MARK: - Shapes

    public struct RemoteStatus: Sendable, Equatable {
        public var enabled: Bool
        public var running: Bool
        public var networkExposed: Bool
        public var port: Int
        public var lanIP: String?
        public var lanAddresses: [NetworkAddress]
        public var onlineCount: Int
        public var relayURL: String?
        public var relayToken: String?
        public var publicEndpoints: [String]
        /// off | connecting | online | error:<msg>
        public var relayState: String

        public init(
            enabled: Bool, running: Bool, networkExposed: Bool, port: Int,
            lanIP: String?, lanAddresses: [NetworkAddress], onlineCount: Int,
            relayURL: String?, relayToken: String?, publicEndpoints: [String],
            relayState: String
        ) {
            self.enabled = enabled
            self.running = running
            self.networkExposed = networkExposed
            self.port = port
            self.lanIP = lanIP
            self.lanAddresses = lanAddresses
            self.onlineCount = onlineCount
            self.relayURL = relayURL
            self.relayToken = relayToken
            self.publicEndpoints = publicEndpoints
            self.relayState = relayState
        }

        public static func offline() -> Self {
            Self(
                enabled: false, running: false, networkExposed: false, port: 6790,
                lanIP: nil, lanAddresses: [], onlineCount: 0,
                relayURL: nil, relayToken: nil, publicEndpoints: [], relayState: "off"
            )
        }
    }

    public struct NetworkAddress: Sendable, Equatable {
        public var interfaceName: String
        public var ip: String

        public init(interfaceName: String, ip: String) {
            self.interfaceName = interfaceName
            self.ip = ip
        }
    }

    public struct DeviceView: Sendable, Equatable {
        public var id: String
        public var name: String
        public var createdAt: Int64
        public var lastSeenAt: Int64
        public var online: Bool

        public init(id: String, name: String, createdAt: Int64, lastSeenAt: Int64, online: Bool) {
            self.id = id
            self.name = name
            self.createdAt = createdAt
            self.lastSeenAt = lastSeenAt
            self.online = online
        }
    }

    // MARK: - Methods

    public func status() async throws -> RemoteStatus {
        let frame = try await connection.request("remote.status")
        return Self.parseStatus(frame.result)
    }

    public func start(port: Int?) async throws -> RemoteStatus {
        var fields: [String: JSONValue] = [:]
        if let port {
            fields["port"] = .number(String(port))
        }
        let frame = try await connection.request("remote.start", params: .object(fields))
        return Self.parseStatus(frame.result)
    }

    public func stop() async throws -> RemoteStatus {
        let frame = try await connection.request("remote.stop")
        return Self.parseStatus(frame.result)
    }

    public func updateConfig(
        relayURL: String?, relayToken: String?, publicEndpoints: [String]?
    ) async throws -> RemoteStatus {
        var fields: [String: JSONValue] = [:]
        if let relayURL { fields["relayUrl"] = .string(relayURL) }
        if let relayToken { fields["relayToken"] = .string(relayToken) }
        if let publicEndpoints {
            fields["publicEndpoints"] = .array(publicEndpoints.map { .string($0) })
        }
        let frame = try await connection.request("remote.updateConfig", params: .object(fields))
        return Self.parseStatus(frame.result)
    }

    public func selectLanIP(_ ip: String) async throws -> RemoteStatus {
        let frame = try await connection.request(
            "remote.selectLanIp",
            params: .object(["lanIp": .string(ip)])
        )
        return Self.parseStatus(frame.result)
    }

    /// 配对深链：`pairingUrl`（前端渲染 QR）＋首选 endpoint＋过期秒数。
    public struct PairingInvite: Sendable, Equatable {
        public var pairingURL: String
        public var endpoint: String
        public var expiresInSeconds: Int

        public init(pairingURL: String, endpoint: String, expiresInSeconds: Int) {
            self.pairingURL = pairingURL
            self.endpoint = endpoint
            self.expiresInSeconds = expiresInSeconds
        }
    }

    public func createInvite() async throws -> PairingInvite {
        let frame = try await connection.request("remote.createInvite")
        guard
            case let .string(url)? = frame.result["pairingUrl"],
            case let .string(endpoint)? = frame.result["endpoint"]
        else { throw RemoteClientError.malformedReply }
        let expires = frame.result["expiresInSeconds"]?.numberText.flatMap(Int.init) ?? 600
        return PairingInvite(pairingURL: url, endpoint: endpoint, expiresInSeconds: expires)
    }

    public func listDevices() async throws -> [DeviceView] {
        let frame = try await connection.request("remote.listDevices")
        guard case let .array(items) = frame.result else { return [] }
        return items.compactMap { item in
            guard case let .object(object) = item,
                  case let .string(id)? = object["id"],
                  case let .string(name)? = object["name"],
                  let createdAt = object["createdAt"]?.numberText.flatMap(Int64.init),
                  let lastSeenAt = object["lastSeenAt"]?.numberText.flatMap(Int64.init)
            else { return nil }
            let online = object["online"]?.boolValue ?? false
            return DeviceView(
                id: id, name: name,
                createdAt: Int64(createdAt), lastSeenAt: Int64(lastSeenAt),
                online: online
            )
        }
    }

    public func revokeDevice(id: String) async throws {
        _ = try await connection.request(
            "remote.revokeDevice",
            params: .object(["deviceId": .string(id)])
        )
    }

    // MARK: - Parsing

    static func parseStatus(_ result: JSONValue) -> RemoteStatus {
        guard case let .object(object) = result else { return .offline() }
        func str(_ key: String) -> String? {
            if case let .string(value)? = object[key] { return value }
            return nil
        }
        func bool(_ key: String) -> Bool {
            object[key]?.boolValue ?? false
        }
        func int(_ key: String) -> Int {
            object[key]?.numberText.flatMap(Int.init) ?? 0
        }
        let addresses: [NetworkAddress] = (object["lanAddresses"]?.arrayValue ?? []).compactMap {
            guard case let .object(entry) = $0,
                  case let .string(name) = entry["interfaceName"],
                  case let .string(ip) = entry["ip"]
            else { return nil }
            return NetworkAddress(interfaceName: name, ip: ip)
        }
        let endpoints: [String] = (object["publicEndpoints"]?.arrayValue ?? []).compactMap {
            if case let .string(value) = $0 { return value }
            return nil
        }
        return RemoteStatus(
            enabled: bool("enabled"),
            running: bool("running"),
            networkExposed: bool("networkExposed"),
            port: int("port"),
            lanIP: str("lanIp"),
            lanAddresses: addresses,
            onlineCount: int("onlineCount"),
            relayURL: str("relayUrl"),
            relayToken: str("relayToken"),
            publicEndpoints: endpoints,
            relayState: str("relayState") ?? "off"
        )
    }
}


