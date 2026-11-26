@testable import AeroricMobileTransport
import Foundation
import Security
import XCTest

/// An address nothing answers on.
private final class DeadSocket: CompanionSocket, @unchecked Sendable {
    func send(text: String) async throws { throw URLError(.cannotConnectToHost) }
    func send(binary: Data) async throws { throw URLError(.cannotConnectToHost) }
    func receive() async throws -> CompanionSocketMessage { throw URLError(.cannotConnectToHost) }
    func close() async {}
}

/// D4 step 5 (2026-11-23): pairing from the Mac's link, the staged → committed
/// credentials, and the connector that keeps the phone connected.
final class CompanionPairingTests: XCTestCase {
    private let mac = ServerHandshakeResponder()
    private let lan = "ws://192.168.1.33:8765"
    private let relay = "wss://relay.example/connect/host-1"

    /// The link the way `control.rs` builds it.
    private func pairingLink(endpoints: [String]? = nil, version: Int = 2) -> String {
        let offer: [String: Any] = [
            "v": version, "endpoints": endpoints ?? [lan, relay], "invite": "invite-1",
            "hostName": "Studio Mac", "hostId": "host-1", "publicKey": mac.publicBase64Url,
            "pairingConfirmationVersion": 1,
        ]
        let data = try! JSONSerialization.data(withJSONObject: offer)
        return "aeroric://pair?code=\(Base64UrlCodec.encode(data))"
    }

    private func credentials(endpoints: [String]) -> CompanionCredentials {
        CompanionCredentials(hostName: "Studio Mac", hostId: "host-1", publicKey: mac.publicBase64Url,
                             endpoints: endpoints, deviceId: "dev-1", deviceToken: "token-1")
    }

    @MainActor
    private func eventually(_ what: String, _ condition: @MainActor () -> Bool) async {
        for _ in 0..<300 {
            if condition() { return }
            try? await Task.sleep(nanoseconds: 10_000_000)
        }
        XCTFail("timed out waiting for: \(what)")
    }

    // MARK: - The link

    func testAPairingLinkIsReadAsTheMacWroteIt() throws {
        let offer = try CompanionPairingOffer.parse(pairingLink())
        XCTAssertEqual(offer.endpoints, [lan, relay])
        XCTAssertEqual(offer.invite, "invite-1")
        XCTAssertEqual(offer.hostId, "host-1")
        let code = try XCTUnwrap(URLComponents(string: pairingLink())?.queryItems?.first?.value)
        XCTAssertEqual(try CompanionPairingOffer.parse("  \(code)\n"), offer, "the bare code works too")
        XCTAssertEqual(CompanionPairingOffer.mode(of: lan, hostId: "host-1"), .directLAN)
        XCTAssertEqual(CompanionPairingOffer.mode(of: relay, hostId: "host-1"), .selfHostedRelay)

        XCTAssertThrowsError(try CompanionPairingOffer.parse("https://example.com/pair?code=x")) {
            XCTAssertEqual($0 as? CompanionPairingOffer.ParseError, .unreadable)
        }
        XCTAssertThrowsError(try CompanionPairingOffer.parse("aeroric://other?code=x")) {
            XCTAssertEqual($0 as? CompanionPairingOffer.ParseError, .notAPairingLink)
        }
        XCTAssertThrowsError(try CompanionPairingOffer.parse(pairingLink(version: 1))) {
            XCTAssertEqual($0 as? CompanionPairingOffer.ParseError, .unsupportedVersion(1))
        }
        XCTAssertThrowsError(try CompanionPairingOffer.parse(pairingLink(endpoints: ["http://x"]))) {
            XCTAssertEqual($0 as? CompanionPairingOffer.ParseError, .noUsableEndpoint)
        }
    }

    // MARK: - Pairing on the link

    @MainActor
    func testPairingStagesTheTokenBeforeConfirmingAndCommitsOnConfirmed() async throws {
        let store = CompanionStore()
        let vault = InMemoryCredentialStore()
        let socket = FakeMac(responder: mac)
        let link = CompanionLink(store: store)
        let paired = try await link.pair(socket: socket, offer: try CompanionPairingOffer.parse(pairingLink()),
                                         deviceName: "Test iPhone", credentials: vault, mode: .directLAN)

        XCTAssertEqual(paired.deviceToken, "token-1")
        XCTAssertEqual(paired.hostName, "Studio Mac")
        XCTAssertEqual(try vault.load(), paired)
        XCTAssertNil(try vault.loadPending())
        XCTAssertEqual(socket.confirmations, 1)
        XCTAssertEqual(store.connectionState, .macOnline(hostEpoch: "epoch-1", transportMode: .directLAN))
        XCTAssertEqual(store.pairedHostName, "Studio Mac")
        XCTAssertEqual(store.pairedDeviceID, "dev-1")
        XCTAssertEqual(store.projects.map(\.id), ["p1"])
    }

    @MainActor
    func testARefusedConfirmationDropsTheStagedToken() async throws {
        var config = FakeMac.Config()
        config.confirmation = .refuse
        let store = CompanionStore()
        let vault = InMemoryCredentialStore()
        let link = CompanionLink(store: store)
        do {
            try await link.pair(socket: FakeMac(responder: mac, config: config),
                                offer: try CompanionPairingOffer.parse(pairingLink()),
                                deviceName: "Test iPhone", credentials: vault, mode: .directLAN)
            XCTFail("paired although the Mac refused")
        } catch let error as CompanionLinkError {
            XCTAssertEqual(error, .authRejected("Pairing expired"))
        }
        XCTAssertNil(try vault.load())
        XCTAssertNil(try vault.loadPending())
        XCTAssertEqual(store.connectionState, .unpaired)
    }

    @MainActor
    func testAConfirmationThatGetsNoAnswerKeepsTheTokenStaged() async throws {
        var config = FakeMac.Config()
        config.confirmation = .silent
        let vault = InMemoryCredentialStore()
        let link = CompanionLink(store: CompanionStore())
        do {
            try await link.pair(socket: FakeMac(responder: mac, config: config),
                                offer: try CompanionPairingOffer.parse(pairingLink()),
                                deviceName: "Test iPhone", credentials: vault, mode: .directLAN)
            XCTFail("paired without the Mac's word")
        } catch {}
        XCTAssertNil(try vault.load())
        XCTAssertEqual(try vault.loadPending()?.deviceToken, "token-1", "the Mac may have kept the device")
    }

    @MainActor
    func testAnOlderMacWithoutConfirmationPairsAtOnce() async throws {
        var config = FakeMac.Config()
        config.confirmation = .legacy
        let socket = FakeMac(responder: mac, config: config)
        let vault = InMemoryCredentialStore()
        try await CompanionLink(store: CompanionStore()).pair(
            socket: socket, offer: try CompanionPairingOffer.parse(pairingLink()),
            deviceName: "Test iPhone", credentials: vault, mode: .directLAN)
        XCTAssertEqual(try vault.load()?.deviceToken, "token-1")
        XCTAssertEqual(socket.confirmations, 0)
    }

    // MARK: - The connector

    /// Review 1124b: an unsigned simulator build has no keychain access. The
    /// page must say so, once, instead of "CompanionCredentialError error 0"
    /// for every address.
    @MainActor
    func testAPhoneThatCannotStoreThePairingSaysWhyAndStops() async throws {
        let store = CompanionStore()
        let vault = RefusingVault()
        var dialled: [String] = []
        let connector = CompanionConnector(store: store, credentials: vault, makeSocket: { url in
            dialled.append(url.absoluteString)
            return FakeMac(responder: self.mac)
        }, pause: { _ in })

        do {
            try await connector.pair(code: pairingLink(), deviceName: "Test iPhone")
            XCTFail("pairing cannot succeed without storage")
        } catch {}
        XCTAssertEqual(dialled, [lan], "no other address can help")
        let shown = try XCTUnwrap(connector.lastError)
        XCTAssertTrue(shown.contains("存不下配对信息"), shown)
        XCTAssertTrue(shown.contains("-34018"), shown)
        XCTAssertFalse(shown.contains("error 0"), shown)
        XCTAssertNil(connector.credentials)
    }

    @MainActor
    func testTheConnectorPairsTriesTheNextAddressAndReconnectsAfterADrop() async throws {
        let store = CompanionStore()
        let vault = InMemoryCredentialStore()
        var macs: [FakeMac] = []
        var dialled: [String] = []
        let connector = CompanionConnector(store: store, credentials: vault, makeSocket: { url in
            dialled.append(url.absoluteString)
            if url.absoluteString == self.lan { return DeadSocket() }
            let fake = FakeMac(responder: self.mac)
            macs.append(fake)
            return fake
        }, pause: { _ in })

        try await connector.pair(code: pairingLink(), deviceName: "Test iPhone")
        XCTAssertEqual(dialled, [lan, relay], "the LAN address first, then the relay")
        XCTAssertEqual(store.connectionState, .macOnline(hostEpoch: "epoch-1", transportMode: .selfHostedRelay))
        XCTAssertEqual(connector.credentials?.deviceToken, "token-1")

        macs[0].closeFromMac(reason: nil)
        await eventually("reconnected") { macs.count == 2 && store.connectionState.isMacOnline }
        XCTAssertEqual(dialled, [lan, relay, lan, relay])
        await connector.stop()
        XCTAssertFalse(store.connectionState.isMacOnline)
    }

    @MainActor
    func testWhatThePhoneIssuesIsSentWithoutAskingTheConnector() async throws {
        let store = CompanionStore()
        let vault = InMemoryCredentialStore(current: credentials(endpoints: [lan]))
        var config = FakeMac.Config()
        config.tasks = ["p1": [["id": "t1", "projectId": "p1", "name": "x", "agent": "codex", "status": "running",
                                "approval": ["requestId": "r1", "kind": "command"]]]]
        let fake = FakeMac(responder: mac, config: config)
        let connector = CompanionConnector(store: store, credentials: vault, makeSocket: { _ in fake }, pause: { _ in })
        connector.start()
        await eventually("online") { store.connectionState.isMacOnline }

        try store.submitApproval(requestId: "r1", approve: true)
        await eventually("the card leaves") { store.pendingApprovals.isEmpty }
        XCTAssertEqual(fake.receivedCommands.map(\.method), ["task.respond"])
        await connector.stop()
    }

    /// 1124e: the pairing stored a VM bridge's address. The Mac's `hello`
    /// names the addresses it has now; the phone keeps them, so when that
    /// address goes away the next reconnect tries the others.
    @MainActor
    func testThePhoneKeepsTheAddressesTheMacGivesOnEachConnection() async throws {
        let bridge = "ws://192.168.64.1:6790"
        let en0 = "ws://192.168.0.159:6790"
        let wireguard = "ws://10.0.0.2:6790"
        let store = CompanionStore()
        let vault = InMemoryCredentialStore(current: credentials(endpoints: [bridge]))
        var config = FakeMac.Config()
        config.helloIdentity = ["hostId": "host-1", "endpoints": [en0, wireguard, "http://not-a-socket"]]
        var macs: [FakeMac] = []
        var dialled: [String] = []
        let connector = CompanionConnector(store: store, credentials: vault, makeSocket: { url in
            dialled.append(url.absoluteString)
            // The bridge answered at pairing time and is gone after the drop.
            if url.absoluteString == bridge, !macs.isEmpty { return DeadSocket() }
            let fake = FakeMac(responder: self.mac, config: config)
            macs.append(fake)
            return fake
        }, pause: { _ in })
        connector.start()
        await eventually("online") { store.connectionState.isMacOnline }
        XCTAssertEqual(connector.credentials?.endpoints, [en0, wireguard], "only ws:// and wss:// are kept")
        XCTAssertEqual(try vault.load()?.endpoints, [en0, wireguard])
        XCTAssertEqual(try vault.load()?.deviceToken, "token-1")

        macs[0].closeFromMac(reason: nil)
        await eventually("reconnected") { macs.count == 2 && store.connectionState.isMacOnline }
        XCTAssertEqual(dialled, [bridge, en0], "the reconnect does not go back to the bridge")
        await connector.stop()
    }

    /// 1124e: a `hello` from another Mac, or one without a usable address,
    /// leaves the stored addresses alone.
    @MainActor
    func testAnotherMacsOrAnEmptyAddressListChangesNothing() async throws {
        for identity: [String: Any] in [["hostId": "host-2", "endpoints": ["ws://10.9.9.9:6790"]],
                                         ["hostId": "host-1", "endpoints": ["http://x"]],
                                         ["hostId": "host-1"]] {
            let store = CompanionStore()
            let vault = InMemoryCredentialStore(current: credentials(endpoints: [lan]))
            var config = FakeMac.Config()
            config.helloIdentity = identity
            let connector = CompanionConnector(store: store, credentials: vault,
                                               makeSocket: { _ in FakeMac(responder: self.mac, config: config) },
                                               pause: { _ in })
            connector.start()
            await eventually("online") { store.connectionState.isMacOnline }
            XCTAssertEqual(connector.credentials?.endpoints, [lan], "\(identity)")
            XCTAssertEqual(try vault.load()?.endpoints, [lan], "\(identity)")
            await connector.stop()
        }
    }

    /// 1124e: pairing again with the same Mac keeps the new token; the old
    /// pairing's details are not written back when its `hello` arrives.
    @MainActor
    func testPairingAgainKeepsTheNewTokenWhenHelloNamesTheMac() async throws {
        let store = CompanionStore()
        var old = credentials(endpoints: [lan])
        old.deviceToken = "old-token"
        let vault = InMemoryCredentialStore(current: old)
        var config = FakeMac.Config()
        config.issuedToken = "new-token"
        config.knownToken = "new-token"
        config.helloIdentity = ["hostId": "host-1", "endpoints": ["ws://192.168.0.159:6790"]]
        let connector = CompanionConnector(store: store, credentials: vault,
                                           makeSocket: { _ in FakeMac(responder: self.mac, config: config) },
                                           pause: { _ in })
        try await connector.pair(code: pairingLink(endpoints: [lan]), deviceName: "Test iPhone")
        XCTAssertEqual(try vault.load()?.deviceToken, "new-token")
        XCTAssertEqual(connector.credentials?.deviceToken, "new-token")
        await connector.stop()
    }

    func testNetworkErrorsAreSaidInWords() {
        XCTAssertEqual(CompanionLink.networkReason(URLError(.timedOut)), "连接超时，Mac 的地址可能变了")
        let bridged = NSError(domain: NSURLErrorDomain, code: NSURLErrorTimedOut,
                              userInfo: [NSURLErrorFailingURLStringErrorKey: "http://192.168.64.1:6790/"])
        XCTAssertEqual(CompanionLink.networkReason(bridged), "连接超时，Mac 的地址可能变了")
        XCTAssertFalse(CompanionLink.networkReason(URLError(.cannotConnectToHost)).contains("Domain"))
    }

    @MainActor
    func testNoAnswerBacksOffUpToThirtySeconds() async throws {
        let store = CompanionStore()
        let vault = InMemoryCredentialStore(current: credentials(endpoints: [lan]))
        var pauses: [Double] = []
        var connector: CompanionConnector!
        connector = CompanionConnector(store: store, credentials: vault, makeSocket: { _ in DeadSocket() },
                                       pause: { seconds in
            pauses.append(seconds)
            if pauses.count == 7 { Task { @MainActor in await connector.stop() } }
            await Task.yield()
        })
        connector.start()
        await eventually("seven rounds") { pauses.count >= 7 }
        await eventually("stopped") { !connector.isRunning }
        XCTAssertEqual(Array(pauses.prefix(7)), [1, 2, 4, 8, 16, 30, 30])
        XCTAssertNotNil(connector.lastError)
    }

    @MainActor
    func testATokenTheMacNoLongerKnowsEndsThePairing() async throws {
        let store = CompanionStore()
        let vault = InMemoryCredentialStore(current: credentials(endpoints: [lan]))
        var config = FakeMac.Config()
        config.knownToken = "another"
        let connector = CompanionConnector(store: store, credentials: vault,
                                           makeSocket: { _ in FakeMac(responder: self.mac, config: config) },
                                           pause: { _ in })
        connector.start()
        await eventually("stopped") { !connector.isRunning }
        XCTAssertNil(try vault.load())
        XCTAssertNil(connector.credentials)
        guard case .revoked = store.connectionState else { return XCTFail("\(store.connectionState)") }
    }

    @MainActor
    func testAStagedPairingIsCommittedOnceTheMacTakesItsToken() async throws {
        let store = CompanionStore()
        let vault = InMemoryCredentialStore(pending: credentials(endpoints: [lan]))
        let connector = CompanionConnector(store: store, credentials: vault,
                                           makeSocket: { _ in FakeMac(responder: self.mac) }, pause: { _ in })
        connector.start()
        await eventually("online") { store.connectionState.isMacOnline }
        XCTAssertEqual(try vault.load()?.deviceToken, "token-1")
        XCTAssertNil(try vault.loadPending())
        await connector.stop()
    }

    @MainActor
    func testAStagedPairingTheMacNeverKeptIsDropped() async throws {
        let store = CompanionStore()
        let vault = InMemoryCredentialStore(pending: credentials(endpoints: [lan]))
        var config = FakeMac.Config()
        config.knownToken = "another"
        let connector = CompanionConnector(store: store, credentials: vault,
                                           makeSocket: { _ in FakeMac(responder: self.mac, config: config) },
                                           pause: { _ in })
        connector.start()
        await eventually("stopped") { !connector.isRunning }
        XCTAssertNil(try vault.loadPending())
        XCTAssertNil(try vault.load())
    }

    @MainActor
    func testForgettingTheMacUnpairsAndClearsTheKeychain() async throws {
        let store = CompanionStore()
        let vault = InMemoryCredentialStore(current: credentials(endpoints: [lan]))
        let connector = CompanionConnector(store: store, credentials: vault,
                                           makeSocket: { _ in FakeMac(responder: self.mac) }, pause: { _ in })
        connector.start()
        await eventually("online") { store.connectionState.isMacOnline }
        await connector.forget()
        XCTAssertEqual(store.connectionState, .unpaired)
        XCTAssertNil(try vault.load())
        XCTAssertNil(store.pairedStaticPubB64)
        XCTAssertTrue(store.projects.isEmpty)
    }

    // MARK: - D4 step 7: scanned or opened links

    /// A link from the camera or another app only fills the pairing page:
    /// nothing is dialled until the user taps 配对.
    @MainActor
    func testAnOfferedLinkIsOnlyProposedNeverPairedOnItsOwn() async throws {
        let store = CompanionStore()
        var dialled = 0
        let connector = CompanionConnector(store: store, credentials: InMemoryCredentialStore(),
                                           makeSocket: { _ in dialled += 1; return DeadSocket() }, pause: { _ in })
        XCTAssertTrue(connector.offerPairingLink("  \(pairingLink())\n"))
        XCTAssertEqual(connector.proposedPairingCode, pairingLink())
        XCTAssertEqual(connector.proposedHostName, "Studio Mac")
        XCTAssertEqual(dialled, 0, "offering a link connects nothing")
        XCTAssertEqual(store.connectionState, .unpaired)

        XCTAssertEqual(connector.takeProposedPairingCode(), pairingLink())
        XCTAssertNil(connector.proposedPairingCode, "taken once")

        XCTAssertFalse(connector.offerPairingLink("aeroric://pair?code=not-json"))
        XCTAssertNil(connector.proposedPairingCode)
        XCTAssertEqual(connector.lastError, "配对码读不出来，请从 Mac 上重新复制")
    }

    /// Pairing from the offered link clears what was proposed.
    @MainActor
    func testPairingFromAnOfferedLinkClearsTheProposal() async throws {
        let store = CompanionStore()
        let connector = CompanionConnector(store: store, credentials: InMemoryCredentialStore(),
                                           makeSocket: { _ in FakeMac(responder: self.mac) }, pause: { _ in })
        connector.offerPairingLink(pairingLink())
        let code = try XCTUnwrap(connector.takeProposedPairingCode())
        try await connector.pair(code: code, deviceName: "Test iPhone")
        XCTAssertNil(connector.proposedHostName)
        XCTAssertEqual(connector.credentials?.hostName, "Studio Mac")
        await connector.stop()
    }
}

@MainActor
private final class RefusingVault: CompanionCredentialStore {
    func load() throws -> CompanionCredentials? { nil }
    func loadPending() throws -> CompanionCredentials? { nil }
    func stagePending(_ credentials: CompanionCredentials) throws {
        throw CompanionCredentialError.keychain(errSecMissingEntitlement)
    }
    func commitPending() throws { throw CompanionCredentialError.nothingStaged }
    func discardPending() throws {}
    func clear() throws {}
}
