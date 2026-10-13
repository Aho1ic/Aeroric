import Foundation
import XCTest

@testable import AeroricMobileTransport

/// AP6: push-plane acceptance for the native companion.
///
/// What these tests exist to prove (they fail on the AP5 behaviour, so they are
/// regressions rather than restatements):
///
/// 1. A notification can never become an approval or any other authorized
///    action — the old `ingestPushEvent` fabricated
///    `CompanionApproval(requestId: "req-\(seq)")` straight from the push
///    sequence number, which is exactly what Q7 forbids.
/// 2. A payload carrying business content is refused rather than partially
///    rendered.
/// 3. Registration refuses rather than queueing while the Mac is offline, and
///    refuses a token it cannot validate instead of binding garbage.
/// 4. Revocation clears hint state and stops accepting hints from the old epoch.
final class PushIntegrationTests: XCTestCase {

    // MARK: - 1. Privacy: only minimal, well-formed payloads are accepted

    func testOnlyTheSendersMinimalPayloadShapeIsAccepted() {
        // The shape our sender actually produces.
        let good: [String: Any] = [
            "aps": ["alert": ["loc-key": "push.task_done"], "sound": "default", "badge": 1, "thread-id": "ab12:done"],
            "kind": "task_done",
            "eventSeq": 7,
        ]
        let parsed = PushAlert.parse(userInfo: good)
        XCTAssertEqual(parsed?.kind, .taskDone)
        XCTAssertEqual(parsed?.eventSeq, 7)

        // A literal alert string is only allowed if it is a bundle key.
        var keyed = good
        keyed["aps"] = ["alert": "push.approval_pending"]
        XCTAssertNotNil(PushAlert.parse(userInfo: keyed))
        keyed["aps"] = ["alert": "Build failed in /Users/someone/secret"]
        XCTAssertNil(PushAlert.parse(userInfo: keyed), "content must not be rendered from a push")

        // Unknown top-level keys: refused outright (this is how a project name
        // would otherwise sneak in through a third-party or compromised sender).
        for extra in [["title": "Refactor billing"], ["body": "at /Users/x"], ["projectName": "Aeroric"]] {
            var payload = good
            payload.merge(extra) { _, new in new }
            XCTAssertNil(PushAlert.parse(userInfo: payload), "extra key must be refused: \(extra.keys)")
        }

        // Unknown aps keys likewise.
        var extraAps = good
        extraAps["aps"] = ["alert": ["loc-key": "push.task_done"], "custom": "secret"]
        XCTAssertNil(PushAlert.parse(userInfo: extraAps))

        // Missing/garbled fields.
        XCTAssertNil(PushAlert.parse(userInfo: ["aps": ["alert": ["loc-key": "push.task_done"]]]))
        XCTAssertNil(PushAlert.parse(userInfo: ["aps": [:], "kind": "nope", "eventSeq": 1]))
        XCTAssertNil(PushAlert.parse(userInfo: ["aps": [:], "kind": "task_done", "eventSeq": "abc"]))
        XCTAssertNil(PushAlert.parse(userInfo: [:]))
    }

    // MARK: - 2. A notification is never an authorization

    @MainActor
    func testPushHintNeverCreatesAnApprovalAndOnlyRequestsARecheck() throws {
        let store = CompanionStore()
        let server = ServerHandshakeResponder()
        try store.pairWithMac(
            hostName: "Studio-Mac",
            staticPubB64: server.publicBase64Url,
            initialEpoch: "epoch-1",
            mode: .directLAN
        )
        XCTAssertFalse(store.resyncNeeded)

        try store.ingestPushHint(PushAlert(kind: .approvalPending, eventSeq: 3))
        XCTAssertTrue(store.pendingApprovals.isEmpty, "a push must never create an approval")
        XCTAssertEqual(store.sentFramesLog.count, 0, "a push must never send anything to the Mac")
        XCTAssertTrue(store.resyncNeeded)
        XCTAssertEqual(store.lastPushKind, .approvalPending)

        // Answering anything is still impossible: there is no approval to answer.
        XCTAssertThrowsError(try store.submitApproval(requestId: "req-3", approve: true))

        // The authoritative snapshot is what produces an approval.
        store.handleReconnect(
            newEpoch: "epoch-1",
            latestHostSeq: 3,
            mode: .directLAN,
            snapshotTasks: [CompanionTask(id: "t1", projectId: "p1", title: "x", agent: "omp", status: "input_required")],
            snapshotApprovals: [CompanionApproval(requestId: "req-real", taskId: "t1", summary: "approve?", hostEpoch: "epoch-1")]
        )
        XCTAssertEqual(store.pendingApprovals.map(\.requestId), ["req-real"])
        store.markResynced()
        XCTAssertFalse(store.resyncNeeded)
        XCTAssertThrowsError(try store.submitApproval(requestId: "req-3", approve: true),
                             "the fabricated requestId must not be answerable")
        try store.submitApproval(requestId: "req-real", approve: true)
    }

    @MainActor
    func testHintsAreRefusedWhileTheMacIsOffline() throws {
        let store = CompanionStore()
        let server = ServerHandshakeResponder()
        try store.pairWithMac(
            hostName: "Mac",
            staticPubB64: server.publicBase64Url,
            initialEpoch: "e1",
            mode: .selfHostedRelay,
            relayURL: "wss://relay.example"
        )
        store.markMacOffline(relayStillConnected: true)
        // The refusal comes from the one epoch gate, so the wording is the
        // shared "offline, nothing is queued" one rather than a push-specific
        // variant: there must not be two different opinions about what offline
        // means.
        XCTAssertThrowsError(try store.ingestPushHint(PushAlert(kind: .taskDone, eventSeq: 1))) { error in
            guard case .macOfflineNoQueueing = error as? CompanionActionError else {
                XCTFail("expected macOfflineNoQueueing, got \(error)")
                return
            }
        }
        XCTAssertFalse(store.resyncNeeded)
    }

    @MainActor
    func testDuplicateAndOutOfOrderHintsAreDroppedWithoutInflatingResyncCount() throws {
        let store = CompanionStore()
        let server = ServerHandshakeResponder()
        try store.pairWithMac(hostName: "Mac", staticPubB64: server.publicBase64Url, initialEpoch: "e1", mode: .directLAN)

        try store.ingestPushHint(PushAlert(kind: .taskDone, eventSeq: 5))
        XCTAssertThrowsError(try store.ingestPushHint(PushAlert(kind: .taskDone, eventSeq: 5)))
        XCTAssertThrowsError(try store.ingestPushHint(PushAlert(kind: .taskDone, eventSeq: 4)))
        XCTAssertEqual(store.lastEventSeq, 5)
        XCTAssertEqual(store.snapshotResyncCount, 0, "pure duplicates are not a resync trigger")

        // A real gap does ask for a re-check (APNs drops notifications), but it
        // is not treated as an error and does not reject the later hint.
        try store.ingestPushHint(PushAlert(kind: .taskFailed, eventSeq: 9))
        XCTAssertTrue(store.resyncNeeded)
        XCTAssertEqual(store.lastEventSeq, 9)
    }

    // MARK: - 3. Revocation

    @MainActor
    func testRevocationClearsHintStateAndBlocksFurtherHints() throws {
        let store = CompanionStore()
        let server = ServerHandshakeResponder()
        try store.pairWithMac(hostName: "Mac", staticPubB64: server.publicBase64Url, initialEpoch: "e1", mode: .directLAN)
        try store.ingestPushHint(PushAlert(kind: .taskDone, eventSeq: 2))
        XCTAssertTrue(store.resyncNeeded)

        store.handleDeviceRevoked()
        XCTAssertFalse(store.resyncNeeded)
        XCTAssertNil(store.lastPushKind)
        XCTAssertEqual(store.lastEventSeq, 0)
        XCTAssertThrowsError(try store.ingestPushHint(PushAlert(kind: .taskDone, eventSeq: 3))) { error in
            XCTAssertEqual(error as? CompanionActionError, .deviceRevoked)
        }
    }

    // MARK: - 4. Token registration

    func testRegistrationUploadsOnlyAPNsHexTokenAndRefusesGarbage() async throws {
        // Capture what the bridge uploads.
        final class Sink: @unchecked Sendable {
            var calls: [(String, String, String)] = []
        }
        let sink = Sink()
        let client = PushRegistrationClient { token, deviceId, platform in
            sink.calls.append((token, deviceId, platform))
            return "{\"bound\":true}"
        }

        let outcome = try await client.register(
            deviceToken: Data([0xaa, 0x11, 0xbb, 0x22]),
            deviceId: "dev-1",
            platform: "ios"
        )
        XCTAssertEqual(outcome.deviceToken, "aa11bb22")
        XCTAssertEqual(outcome.state, .registered(deviceId: "dev-1"))
        XCTAssertEqual(sink.calls.count, 1)
        XCTAssertEqual(sink.calls[0].0, "aa11bb22")
        // Nothing but the token/device/platform may leave the device here.
        XCTAssertEqual(sink.calls[0].1, "dev-1")
        XCTAssertEqual(sink.calls[0].2, "ios")

        // An empty token is not a registration: it would be bound blindly and
        // then rejected by APNs forever.
        do {
            _ = try await client.register(deviceToken: Data(), deviceId: "dev-1", platform: "ios")
            XCTFail("an empty token must be refused before upload")
        } catch {
            XCTAssertEqual(error as? PushBridgeError, .invalidDeviceToken)
        }
        XCTAssertEqual(sink.calls.count, 1, "an invalid token must not reach the host")
    }

    func testUnregisterSendsAnEmptyToken() async throws {
        final class Sink: @unchecked Sendable {
            var tokens: [String] = []
        }
        let sink = Sink()
        let client = PushRegistrationClient { token, _, _ in
            sink.tokens.append(token)
            return "{\"bound\":false}"
        }
        _ = try await client.unregister(deviceId: "dev-1", platform: "ios")
        XCTAssertEqual(sink.tokens, [""], "unbind is expressed as an empty token")
    }

    func testRegistrationSurfacesHostRefusalInsteadOfClaimingSuccess() async throws {
        struct Boom: Error {}
        let client = PushRegistrationClient { _, _, _ in throw Boom() }
        do {
            _ = try await client.register(deviceToken: Data([1, 2]), deviceId: "d", platform: "ios")
            XCTFail("a refused registration must not report success")
        } catch {
            guard case .registrationRefused = error as? PushBridgeError else {
                XCTFail("expected registrationRefused, got \(error)")
                return
            }
        }
    }

    func testHexTokenEncodingIsLowercaseAndLossless() {
        XCTAssertEqual(PushRegistrationClient.hexToken(Data([0x00, 0x0f, 0xff])), "000fff")
        XCTAssertEqual(PushRegistrationClient.hexToken(Data()), "")
    }
}
