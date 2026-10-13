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

    // MARK: - AP6: push plane and self-hosted relay diagnostics

    /// One device's push registration as the host reports it.
    ///
    /// Note the absence of an APNs token: the token lives in the user's
    /// self-hosted sender, so the Mac (and this client) only ever sees *that* a
    /// device registered and whether APNs has since rejected it.
    public struct PushDevice: Sendable, Equatable {
        public var deviceID: String
        public var platform: String
        public var registeredAt: Int64
        public var invalidated: Bool

        public init(deviceID: String, platform: String, registeredAt: Int64, invalidated: Bool) {
            self.deviceID = deviceID
            self.platform = platform
            self.registeredAt = registeredAt
            self.invalidated = invalidated
        }
    }

    /// Counters the host keeps about the push plane. These are what answer
    /// "notifications are on but I get nothing".
    public struct PushStats: Sendable, Equatable {
        public var decisions: Int
        public var attempts: Int
        public var accepted: Int
        public var invalidatedDevices: Int
        public var failures: Int
        public var skippedNoDevices: Int

        public init(
            decisions: Int, attempts: Int, accepted: Int,
            invalidatedDevices: Int, failures: Int, skippedNoDevices: Int
        ) {
            self.decisions = decisions
            self.attempts = attempts
            self.accepted = accepted
            self.invalidatedDevices = invalidatedDevices
            self.failures = failures
            self.skippedNoDevices = skippedNoDevices
        }
    }

    public struct PushStatus: Sendable, Equatable {
        public var enabled: Bool
        /// Whether this host form has a push transport at all.
        public var wired: Bool
        public var senderURL: String?
        public var user: String?
        public var devices: [PushDevice]
        public var stats: PushStats
        /// Non-nil while the sender has asked us to back off (APNs 429/503).
        public var suppressedForMs: Int?
        public var detail: String

        public init(
            enabled: Bool, wired: Bool, senderURL: String?, user: String?,
            devices: [PushDevice], stats: PushStats, suppressedForMs: Int?, detail: String
        ) {
            self.enabled = enabled
            self.wired = wired
            self.senderURL = senderURL
            self.user = user
            self.devices = devices
            self.stats = stats
            self.suppressedForMs = suppressedForMs
            self.detail = detail
        }

        public static func unavailable() -> Self {
            Self(
                enabled: false, wired: false, senderURL: nil, user: nil,
                devices: [],
                stats: PushStats(
                    decisions: 0, attempts: 0, accepted: 0,
                    invalidatedDevices: 0, failures: 0, skippedNoDevices: 0
                ),
                suppressedForMs: nil,
                detail: "push status has not been read yet"
            )
        }
    }

    public struct RelayDiagnostics: Sendable, Equatable {
        public var url: String
        public var schemeOK: Bool
        public var reachable: Bool
        public var authenticated: Bool
        public var latencyMs: Int?
        public var detail: String

        public init(
            url: String, schemeOK: Bool, reachable: Bool,
            authenticated: Bool, latencyMs: Int?, detail: String
        ) {
            self.url = url
            self.schemeOK = schemeOK
            self.reachable = reachable
            self.authenticated = authenticated
            self.latencyMs = latencyMs
            self.detail = detail
        }
    }

    public func pushStatus() async throws -> PushStatus {
        let frame = try await connection.request("remote.pushStatus")
        return Self.parsePushStatus(frame.result)
    }

    /// Register or clear this device's push binding. An empty `token` unbinds.
    public func pushRegister(deviceID: String, platform: String, token: String) async throws {
        _ = try await connection.request(
            "remote.pushRegister",
            params: .object([
                "deviceId": .string(deviceID),
                "platform": .string(platform),
                "token": .string(token),
            ])
        )
    }

    /// Ask the host to send one notification now, so the user can verify the
    /// chain end to end instead of waiting for a task to finish.
    public func pushTest() async throws -> Int {
        let frame = try await connection.request("remote.pushTest")
        return frame.result["accepted"]?.numberText.flatMap(Int.init) ?? 0
    }

    public func relayDiagnostics() async throws -> RelayDiagnostics {
        let frame = try await connection.request("remote.relayDiagnostics")
        guard case let .object(object)? = frame.result["relay"],
              case let .string(url)? = object["url"] else {
            throw RemoteClientError.malformedReply
        }
        return RelayDiagnostics(
            url: url,
            schemeOK: object["schemeOk"]?.boolValue ?? false,
            reachable: object["reachable"]?.boolValue ?? false,
            authenticated: object["authenticated"]?.boolValue ?? false,
            latencyMs: object["latencyMs"]?.numberText.flatMap(Int.init),
            detail: {
                if case let .string(value)? = object["detail"] { return value }
                return ""
            }()
        )
    }

    /// The host's own note about relay responsibility; shown verbatim because
    /// it is the product disclosure, not UI copy.
    public func relayDisclosure() async throws -> (note: String, hasDefaultRelay: Bool) {
        let frame = try await connection.request("remote.relayDiagnostics")
        let note: String = {
            if case let .string(value)? = frame.result["note"] { return value }
            return ""
        }()
        let hasDefault = frame.result["defaultRelay"] != nil && frame.result["defaultRelay"] != .null
        return (note, hasDefault)
    }

    /// 解析 `remote.pushStatus` 的应答。内部可见性提到 `public` 是**给测试**用的:
    /// wire 形状的解析规则要在没有活宿主的情况下被钉住,而 `RemoteClient` 的其它
    /// 解析器(`parseStatus`)沿用内部可见性(它们只被本文件的请求方法消费)。
    public static func parsePushStatus(_ result: JSONValue) -> PushStatus {
        guard case let .object(object) = result else { return .unavailable() }
        func str(_ key: String) -> String? {
            if case let .string(value)? = object[key] { return value }
            return nil
        }
        func int(_ key: String) -> Int {
            object[key]?.numberText.flatMap(Int.init) ?? 0
        }
        let devices: [PushDevice] = (object["devices"]?.arrayValue ?? []).compactMap { item in
            guard case let .object(entry) = item,
                  case let .string(deviceID)? = entry["deviceId"] else { return nil }
            return PushDevice(
                deviceID: deviceID,
                platform: {
                    if case let .string(value)? = entry["platform"] { return value }
                    return ""
                }(),
                registeredAt: entry["registeredAtMs"]?.numberText.flatMap(Int64.init)
                    ?? entry["registeredAt"]?.numberText.flatMap(Int64.init) ?? 0,
                invalidated: entry["invalidated"]?.boolValue ?? false
            )
        }
        let stats = object["stats"].flatMap { value -> PushStats? in
            guard case let .object(entry) = value else { return nil }
            func count(_ key: String) -> Int {
                entry[key]?.numberText.flatMap(Int.init) ?? 0
            }
            return PushStats(
                decisions: count("decisions"),
                attempts: count("attempts"),
                accepted: count("accepted"),
                invalidatedDevices: count("invalidatedDevices"),
                failures: count("failures"),
                skippedNoDevices: count("skippedNoDevices")
            )
        } ?? PushStats(
            decisions: 0, attempts: 0, accepted: 0,
            invalidatedDevices: 0, failures: 0, skippedNoDevices: 0
        )
        return PushStatus(
            enabled: object["enabled"]?.boolValue ?? false,
            wired: object["wired"]?.boolValue ?? false,
            senderURL: str("senderUrl"),
            user: str("user"),
            devices: devices,
            stats: stats,
            suppressedForMs: object["suppressedForMs"]?.numberText.flatMap(Int.init),
            detail: str("detail") ?? ""
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


