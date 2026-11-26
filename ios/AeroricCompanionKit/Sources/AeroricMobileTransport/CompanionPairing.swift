import Foundation
import Security

/// D4 step 5 (2026-11-23): what the Mac's pairing link carries —
/// `aeroric://pair?code=<base64url(JSON)>`, built by
/// `core/.../remote/control.rs` (`create_invite_for_addresses`).
public struct CompanionPairingOffer: Equatable, Sendable {
    public static let protocolVersion = 2
    public static let confirmationVersion = 1

    public var endpoints: [String]
    public var invite: String
    public var hostName: String
    public var hostId: String
    public var publicKey: String

    public enum ParseError: Error, Equatable, Sendable {
        case notAPairingLink
        case unreadable
        case unsupportedVersion(Int)
        case missing(String)
        case badPublicKey
        case noUsableEndpoint
    }

    /// The whole link, or just its code.
    public static func parse(_ text: String) throws -> CompanionPairingOffer {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        let code: String
        if trimmed.lowercased().hasPrefix("aeroric://") {
            guard let components = URLComponents(string: trimmed), components.host == "pair",
                  let value = components.queryItems?.first(where: { $0.name == "code" })?.value,
                  !value.isEmpty else {
                throw ParseError.notAPairingLink
            }
            code = value
        } else {
            code = trimmed
        }
        guard let data = Base64UrlCodec.decode(code),
              let object = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else {
            throw ParseError.unreadable
        }
        let version = object["v"] as? Int ?? 0
        guard version == protocolVersion else { throw ParseError.unsupportedVersion(version) }
        func string(_ key: String) throws -> String {
            guard let value = object[key] as? String, !value.isEmpty else { throw ParseError.missing(key) }
            return value
        }
        let publicKey = try string("publicKey")
        guard Base64UrlCodec.decode(publicKey)?.count == 32 else { throw ParseError.badPublicKey }
        let endpoints = (object["endpoints"] as? [String] ?? []).filter {
            let lower = $0.lowercased()
            return lower.hasPrefix("ws://") || lower.hasPrefix("wss://")
        }
        guard !endpoints.isEmpty else { throw ParseError.noUsableEndpoint }
        return CompanionPairingOffer(endpoints: endpoints, invite: try string("invite"),
                                     hostName: (object["hostName"] as? String) ?? "Mac",
                                     hostId: try string("hostId"), publicKey: publicKey)
    }

    /// A relay's client URL is `<base>/connect/<hostId>`
    /// (`remote-protocol::client_connect_url`); anything else is direct.
    public static func mode(of endpoint: String, hostId: String) -> CompanionConnectionState.TransportMode {
        URL(string: endpoint)?.path.hasSuffix("/connect/\(hostId)") == true ? .selfHostedRelay : .directLAN
    }

    /// Plain words for the pairing page.
    public static func describe(_ error: ParseError) -> String {
        switch error {
        case .notAPairingLink: "这不是 Aeroric 的配对链接（应以 aeroric://pair?code= 开头）"
        case .unreadable: "配对码读不出来，请从 Mac 上重新复制"
        case let .unsupportedVersion(version): "配对码的版本是 \(version)，这个 App 只认 \(protocolVersion)"
        case let .missing(key): "配对码里缺少 \(key)"
        case .badPublicKey: "配对码里 Mac 的公钥不对"
        case .noUsableEndpoint: "配对码里没有能连的地址"
        }
    }
}

/// D4 step 5: what this phone keeps to reach its Mac again.
public struct CompanionCredentials: Codable, Equatable, Sendable {
    public var hostName: String
    public var hostId: String
    public var publicKey: String
    public var endpoints: [String]
    public var deviceId: String
    public var deviceToken: String

    public init(hostName: String, hostId: String, publicKey: String, endpoints: [String],
                deviceId: String, deviceToken: String) {
        self.hostName = hostName
        self.hostId = hostId
        self.publicKey = publicKey
        self.endpoints = endpoints
        self.deviceId = deviceId
        self.deviceToken = deviceToken
    }
}

/// D4 step 5: where the credentials live. A pairing is staged before the
/// phone confirms it to the Mac and committed once the Mac says so.
@MainActor
public protocol CompanionCredentialStore: AnyObject {
    func load() throws -> CompanionCredentials?
    func loadPending() throws -> CompanionCredentials?
    func stagePending(_ credentials: CompanionCredentials) throws
    /// The staged pairing becomes the one in use.
    func commitPending() throws
    func discardPending() throws
    /// Forget everything (unpaired, or the Mac no longer knows this phone).
    func clear() throws
    /// 1124e: the pairing in use, with new details (the Mac's addresses).
    func replace(_ credentials: CompanionCredentials) throws
}

public extension CompanionCredentialStore {
    /// Staged and committed at once: nothing is staged while a pairing is
    /// in use.
    func replace(_ credentials: CompanionCredentials) throws {
        try stagePending(credentials)
        try commitPending()
    }
}

public enum CompanionCredentialError: Error, Equatable, Sendable, LocalizedError {
    case nothingStaged
    case keychain(OSStatus)

    /// Shown on the pairing page. Swift numbers payload cases first, so the
    /// bare error read "CompanionCredentialError error 0" for a keychain
    /// refusal (review 1124b).
    public var errorDescription: String? {
        switch self {
        case .nothingStaged:
            "这台手机上没有待确认的配对信息"
        case let .keychain(status):
            Self.keychainText(status)
        }
    }

    static func keychainText(_ status: OSStatus) -> String {
        if status == errSecMissingEntitlement {
            return "这台手机存不下配对信息：App 缺少钥匙串权限（\(status)）。模拟器里要用带签名的构建"
        }
        return "这台手机存不下配对信息：钥匙串出错（\(status)）"
    }
}

/// For tests and previews.
@MainActor
public final class InMemoryCredentialStore: CompanionCredentialStore {
    public private(set) var current: CompanionCredentials?
    public private(set) var pending: CompanionCredentials?

    public init(current: CompanionCredentials? = nil, pending: CompanionCredentials? = nil) {
        self.current = current
        self.pending = pending
    }

    public func load() throws -> CompanionCredentials? { current }
    public func loadPending() throws -> CompanionCredentials? { pending }
    public func stagePending(_ credentials: CompanionCredentials) throws { pending = credentials }
    public func commitPending() throws {
        guard let pending else { throw CompanionCredentialError.nothingStaged }
        current = pending
        self.pending = nil
    }
    public func discardPending() throws { pending = nil }
    public func clear() throws {
        current = nil
        pending = nil
    }
}

/// D4 step 5: the app's store — generic-password items readable after the
/// first unlock, on this device only (never synced). No automated test runs
/// it; the in-memory store stands in.
@MainActor
public final class KeychainCredentialStore: CompanionCredentialStore {
    private let service: String
    private static let currentAccount = "pairing"
    private static let pendingAccount = "pairing.pending"

    public init(service: String = "com.aeroric.companion") {
        self.service = service
    }

    public func load() throws -> CompanionCredentials? { try read(Self.currentAccount) }
    public func loadPending() throws -> CompanionCredentials? { try read(Self.pendingAccount) }
    public func stagePending(_ credentials: CompanionCredentials) throws { try write(credentials, Self.pendingAccount) }

    public func commitPending() throws {
        guard let pending = try read(Self.pendingAccount) else { throw CompanionCredentialError.nothingStaged }
        try write(pending, Self.currentAccount)
        try delete(Self.pendingAccount)
    }

    public func discardPending() throws { try delete(Self.pendingAccount) }

    public func clear() throws {
        try delete(Self.pendingAccount)
        try delete(Self.currentAccount)
    }

    private func query(_ account: String) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword,
         kSecAttrService as String: service,
         kSecAttrAccount as String: account]
    }

    private func read(_ account: String) throws -> CompanionCredentials? {
        var request = query(account)
        request[kSecReturnData as String] = true
        request[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(request as CFDictionary, &result)
        if status == errSecItemNotFound { return nil }
        guard status == errSecSuccess, let data = result as? Data else {
            throw CompanionCredentialError.keychain(status)
        }
        return try JSONDecoder().decode(CompanionCredentials.self, from: data)
    }

    private func write(_ credentials: CompanionCredentials, _ account: String) throws {
        let data = try JSONEncoder().encode(credentials)
        try delete(account)
        var item = query(account)
        item[kSecValueData as String] = data
        item[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly
        let status = SecItemAdd(item as CFDictionary, nil)
        guard status == errSecSuccess else { throw CompanionCredentialError.keychain(status) }
    }

    private func delete(_ account: String) throws {
        let status = SecItemDelete(query(account) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw CompanionCredentialError.keychain(status)
        }
    }
}
