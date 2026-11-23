import Foundation

/// D4 step 5 (2026-11-23): keeps this phone connected to its paired Mac —
/// pairing from a link, the credentials in a `CompanionCredentialStore`,
/// every address tried in order, backoff, and reconnecting when a connection
/// ends. All state the screens show is still in `CompanionStore`; this adds
/// only what the connection is doing. See `2026-11-23-d4-pairing-and-reconnect`.
@MainActor
@Observable
public final class CompanionConnector {
    public let store: CompanionStore
    public let link: CompanionLink
    /// What the connection is doing, for the pairing page.
    public private(set) var activity: String = "没有配对"
    /// Why the last attempt failed, if it did.
    public private(set) var lastError: String?
    public private(set) var isRunning = false
    public private(set) var credentials: CompanionCredentials?
    /// D4 step 7: a pairing link that came from a scan or from opening an
    /// `aeroric://pair` link, waiting for the user to look and tap 配对.
    public private(set) var proposedPairingCode: String?
    /// The Mac that link names, shown before pairing.
    public private(set) var proposedHostName: String?

    private let vault: CompanionCredentialStore
    private let makeSocket: @MainActor (URL) -> CompanionSocket
    private let pause: @MainActor (Double) async throws -> Void
    private var loop: Task<Void, Never>?
    private var dropped: CheckedContinuation<Void, Never>?

    public static let maxBackoffSeconds = 30.0

    public init(
        store: CompanionStore,
        credentials vault: CompanionCredentialStore,
        makeSocket: @escaping @MainActor (URL) -> CompanionSocket = { URLSessionCompanionSocket(url: $0) },
        pause: @escaping @MainActor (Double) async throws -> Void = {
            try await Task.sleep(nanoseconds: UInt64($0 * 1_000_000_000))
        }
    ) {
        self.store = store
        self.vault = vault
        self.makeSocket = makeSocket
        self.pause = pause
        link = CompanionLink(store: store)
        credentials = (try? vault.load()) ?? nil
        if let credentials {
            store.restorePairing(hostName: credentials.hostName, staticPubB64: credentials.publicKey,
                                 deviceID: credentials.deviceId)
            activity = "还没连接"
        }
        link.onClose = { [weak self] _ in self?.connectionEnded() }
        store.onCommandIssued = { [weak self] in self?.sendWaiting() }
        store.pushRegistrationUpload = { [weak self] token, deviceId, platform in
            guard let self else { throw CompanionLinkError.closed("没有连接") }
            return try await self.registerPush(token: token, deviceId: deviceId, platform: platform)
        }
    }

    // MARK: - Lifecycle

    /// The app came to the front: connect and stay connected.
    public func start() {
        guard !isRunning else { return }
        if credentials == nil, let pending = (try? vault.loadPending()) ?? nil {
            // The last pairing's confirmation went unanswered: try it.
            store.restorePairing(hostName: pending.hostName, staticPubB64: pending.publicKey,
                                 deviceID: pending.deviceId)
            run(pending, confirming: true)
        } else if let credentials {
            run(credentials, confirming: false)
        }
    }

    /// The app went to the background: disconnect and stop retrying.
    public func stop() async {
        isRunning = false
        loop?.cancel()
        loop = nil
        resumeDropped()
        await link.disconnect()
        if credentials != nil { activity = "已断开" }
    }

    /// Pair from the Mac's pairing link (or its code), then stay connected.
    public func pair(code: String, deviceName: String) async throws {
        let offer: CompanionPairingOffer
        do {
            offer = try CompanionPairingOffer.parse(code)
        } catch let error as CompanionPairingOffer.ParseError {
            lastError = CompanionPairingOffer.describe(error)
            throw error
        }
        await stop()
        var failures: [String] = []
        for endpoint in offer.endpoints {
            guard let url = URL(string: endpoint) else { continue }
            activity = "正在配对：\(endpoint)"
            do {
                let paired = try await link.pair(
                    socket: makeSocket(url), offer: offer, deviceName: deviceName, credentials: vault,
                    mode: CompanionPairingOffer.mode(of: endpoint, hostId: offer.hostId))
                credentials = paired
                lastError = nil
                proposedHostName = nil
                activity = "已连上 \(paired.hostName)"
                isRunning = true
                loop = Task { [weak self] in await self?.stayConnected(paired, after: endpoint) }
                return
            } catch let error as CompanionLinkError {
                failures.append("\(endpoint)：\(Self.describe(error))")
                // A refused invite will be refused on every address.
                if case .authRejected = error { break }
            } catch {
                failures.append("\(endpoint)：连不上（\(error.localizedDescription)）")
            }
        }
        lastError = failures.joined(separator: "\n")
        activity = "配对没成功"
        throw CompanionLinkError.closed(lastError ?? "配对没成功")
    }

    /// D4 step 7: a link from the camera or from another app. It is only
    /// offered — never paired on its own: a link from anywhere must not
    /// connect this phone to a Mac the user did not choose. A link that does
    /// not read says why and offers nothing.
    @discardableResult
    public func offerPairingLink(_ text: String) -> Bool {
        do {
            let offer = try CompanionPairingOffer.parse(text)
            proposedPairingCode = text.trimmingCharacters(in: .whitespacesAndNewlines)
            proposedHostName = offer.hostName
            lastError = nil
            return true
        } catch let error as CompanionPairingOffer.ParseError {
            lastError = CompanionPairingOffer.describe(error)
        } catch {
            lastError = "\(error)"
        }
        return false
    }

    /// The pairing page took the offered link into its field.
    public func takeProposedPairingCode() -> String? {
        defer { proposedPairingCode = nil }
        return proposedPairingCode
    }

    /// Forget this Mac on the phone. (The Mac still lists the device until
    /// it is removed there.)
    public func forget() async {
        await stop()
        try? vault.clear()
        credentials = nil
        store.unpair()
        activity = "没有配对"
        lastError = nil
    }

    // MARK: - The loop

    private func run(_ credentials: CompanionCredentials, confirming: Bool) {
        isRunning = true
        loop = Task { [weak self] in await self?.keepConnected(credentials, confirming: confirming) }
    }

    /// Try each address in order; back off when none answers; when a
    /// connection ends, start again.
    private func keepConnected(_ credentials: CompanionCredentials, confirming: Bool) async {
        var confirming = confirming
        var failedRounds = 0
        while isRunning, !Task.isCancelled {
            var connected = false
            var failures: [String] = []
            for endpoint in credentials.endpoints {
                guard isRunning, let url = URL(string: endpoint) else { break }
                activity = "正在连接 \(credentials.hostName)：\(endpoint)"
                do {
                    try await link.connect(socket: makeSocket(url), deviceToken: credentials.deviceToken,
                                           mode: CompanionPairingOffer.mode(of: endpoint, hostId: credentials.hostId))
                } catch let error as CompanionLinkError {
                    if case .authRejected = error {
                        // The Mac does not know this token: it was revoked, or
                        // the pairing being confirmed never reached the Mac.
                        if confirming { try? vault.discardPending() } else { try? vault.clear() }
                        self.credentials = nil
                        isRunning = false
                        lastError = "Mac 不认这台设备了，需要重新配对"
                        activity = "需要重新配对"
                        return
                    }
                    failures.append("\(endpoint)：\(Self.describe(error))")
                    continue
                } catch {
                    failures.append("\(endpoint)：连不上（\(error.localizedDescription)）")
                    continue
                }
                if confirming {
                    // The Mac took the staged pairing after all.
                    try? vault.commitPending()
                    self.credentials = credentials
                    confirming = false
                }
                connected = true
                failedRounds = 0
                lastError = nil
                activity = "已连上 \(credentials.hostName)"
                await waitUntilDropped()
                break
            }
            guard isRunning, !Task.isCancelled else { return }
            if connected { continue }
            failedRounds += 1
            let delay = min(Self.maxBackoffSeconds, pow(2, Double(failedRounds - 1)))
            lastError = failures.joined(separator: "\n")
            activity = "连不上 \(credentials.hostName)，\(Int(delay)) 秒后再试（第 \(failedRounds) 次）"
            do { try await pause(delay) } catch { return }
        }
    }

    private func stayConnected(_ credentials: CompanionCredentials, after _: String) async {
        await waitUntilDropped()
        guard isRunning, !Task.isCancelled else { return }
        await keepConnected(credentials, confirming: false)
    }

    private func waitUntilDropped() async {
        guard link.isConnected else { return }
        await withCheckedContinuation { continuation in dropped = continuation }
    }

    private func connectionEnded() {
        if isRunning, let credentials { activity = "和 \(credentials.hostName) 的连接断了，正在重连" }
        resumeDropped()
    }

    private func resumeDropped() {
        let waiting = dropped
        dropped = nil
        waiting?.resume()
    }

    // MARK: - On the connection

    private func sendWaiting() {
        guard link.isConnected else { return }
        Task { [weak self] in try? await self?.link.flush() }
    }

    private func registerPush(token: String, deviceId: String, platform: String) async throws -> String {
        try await link.call("push.register", ["deviceId": deviceId, "token": token, "platform": platform])
    }

    private static func describe(_ error: CompanionLinkError) -> String {
        switch error {
        case let .handshake(reason): reason
        case let .authRejected(reason): "Mac 拒绝了：\(reason)"
        case let .closed(reason): reason
        case let .unexpected(reason): reason
        case let .remote(method, message): "读取 \(method) 失败：\(message)"
        case .notPaired: "还没配对"
        }
    }
}
