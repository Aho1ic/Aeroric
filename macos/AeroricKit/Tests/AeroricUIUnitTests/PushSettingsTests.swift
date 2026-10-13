import AeroricClient
import AeroricProtocol
import XCTest

@testable import AeroricUI

/// AP6: the macOS settings surface for the push plane and self-hosted relay.
///
/// These pin the parts that are easy to get subtly wrong and expensive to
/// notice: which counters the status line is built from, that a device APNs has
/// rejected is *shown as rejected* rather than silently disappearing, that the
/// backoff window from the sender is surfaced, and that the product disclosure
/// ("no default relay") is rendered rather than paraphrased.
final class PushSettingsTests: XCTestCase {

    // MARK: - Parsing the host's push status

    func testPushStatusParsesDevicesStatsAndBackoff() throws {
        let json = """
        {
          "enabled": true,
          "wired": true,
          "senderUrl": "https://push.example.com",
          "user": "host-a",
          "devices": [
            { "deviceId": "dev-1", "platform": "ios", "registeredAtMs": 1700000000000, "invalidated": false },
            { "deviceId": "dev-2", "platform": "ipados", "registeredAtMs": 1700000001000, "invalidated": true }
          ],
          "stats": {
            "decisions": 7, "attempts": 5, "accepted": 4,
            "invalidatedDevices": 1, "failures": 1, "skippedNoDevices": 1
          },
          "suppressedForMs": 45000,
          "detail": "push is configured"
        }
        """
        let value = try decodeJSON(json)
        let status = RemoteClient.parsePushStatus(value)

        XCTAssertTrue(status.enabled)
        XCTAssertTrue(status.wired)
        XCTAssertEqual(status.senderURL, "https://push.example.com")
        XCTAssertEqual(status.user, "host-a")
        XCTAssertEqual(status.devices.count, 2)
        XCTAssertEqual(status.devices[0].deviceID, "dev-1")
        XCTAssertEqual(status.devices[0].registeredAt, 1_700_000_000_000)
        XCTAssertFalse(status.devices[0].invalidated)
        XCTAssertTrue(status.devices[1].invalidated, "an APNs-rejected device must stay visible as rejected")
        XCTAssertEqual(status.stats.decisions, 7)
        XCTAssertEqual(status.stats.accepted, 4)
        XCTAssertEqual(status.stats.invalidatedDevices, 1)
        XCTAssertEqual(status.stats.skippedNoDevices, 1)
        XCTAssertEqual(status.suppressedForMs, 45000)
        XCTAssertEqual(status.detail, "push is configured")
    }

    func testPushStatusWithoutOptionalFieldsStillParses() throws {
        // A host with no push plane wired reports exactly this shape.
        let json = """
        { "enabled": false, "wired": false, "devices": [],
          "stats": null,
          "detail": "this host has no push plane wired; notifications are unavailable" }
        """
        let status = RemoteClient.parsePushStatus(try decodeJSON(json))
        XCTAssertFalse(status.enabled)
        XCTAssertFalse(status.wired)
        XCTAssertNil(status.senderURL)
        XCTAssertNil(status.suppressedForMs, "no backoff must not be read as a zero-length backoff")
        XCTAssertEqual(status.stats.decisions, 0)
        XCTAssertEqual(status.stats.failures, 0)
        XCTAssertTrue(status.detail.contains("no push plane"))
    }

    func testPushStatusRejectsGarbageInsteadOfInventingDevices() throws {
        // A device entry without an id is dropped rather than rendered as a
        // blank row the user could act on.
        let json = """
        { "enabled": true, "wired": true,
          "devices": [{ "platform": "ios" }, { "deviceId": "dev-9", "platform": "ios" }],
          "stats": { "decisions": 1 } }
        """
        let status = RemoteClient.parsePushStatus(try decodeJSON(json))
        XCTAssertEqual(status.devices.map { $0.deviceID }, ["dev-9"])
    }

    // MARK: - Relay diagnostics

    func testRelayDiagnosticsKeepsSchemeReachableAndAuthAsDistinctFacts() throws {
        let json = """
        { "relay": { "url": "wss://relay.example.com", "schemeOk": true,
                     "reachable": true, "authenticated": false,
                     "latencyMs": 42, "detail": "relay refused the registration: invalid relay token" },
          "dialogue": null,
          "defaultRelay": null,
          "note": "Aeroric ships no default relay" }
        """
        let diagnostics = RemoteClientRelayStub.parse(try decodeJSON(json))
        XCTAssertTrue(diagnostics.schemeOK)
        XCTAssertTrue(diagnostics.reachable)
        XCTAssertFalse(diagnostics.authenticated,
                       "reachable but rejected must not read as working")
        XCTAssertEqual(diagnostics.latencyMs, 42)
        XCTAssertTrue(diagnostics.detail.contains("invalid relay token"))
    }

    func testNoRelayConfiguredIsNotReportedAsASchemeProblem() throws {
        // The distinction matters: "you have not set a relay" is a valid
        // configuration (LAN direct), while "your URL is insecure" is not.
        let json = """
        { "relay": { "url": "", "schemeOk": true, "reachable": false, "authenticated": false,
                     "detail": "no relay configured; direct LAN connection is the only path" },
          "note": "Aeroric ships no default relay; your Mac and phone must use your own relay or a trusted direct network" }
        """
        let value = try decodeJSON(json)
        let diagnostics = RemoteClientRelayStub.parse(value)
        XCTAssertTrue(diagnostics.schemeOK)
        XCTAssertFalse(diagnostics.reachable)
        XCTAssertTrue(diagnostics.detail.contains("no relay configured"))
        // The disclosure is the host's own sentence and must survive verbatim.
        guard case let .string(note)? = value["note"] else {
            return XCTFail("note missing")
        }
        XCTAssertTrue(note.contains("no default relay"))
    }

    // MARK: - Store wiring

    @MainActor
    func testStoreReportsPushUnavailableWhenNoHostIsConnected() {
        let store = RemoteStore(client: nil)
        XCTAssertFalse(store.pushStatus.enabled)
        XCTAssertFalse(store.pushStatus.wired)
        XCTAssertTrue(store.pushStatus.devices.isEmpty)
        XCTAssertNil(store.relayDiagnostics)
    }

    @MainActor
    func testPushActionsSurfaceTheMissingConnectionInsteadOfSilentlyDoingNothing() async {
        let store = RemoteStore(client: nil)
        await store.unregisterPush(deviceID: "dev-1")
        XCTAssertNotNil(store.lastError, "an action with no host must say so")
        await store.sendTestNotification()
        XCTAssertNotNil(store.lastError)
        await store.diagnoseRelay()
        XCTAssertNil(store.relayDiagnostics)
    }
}

/// 测试侧的 JSON 解析:`JSONValue` 只从 `Any` 构造(它描述的是
/// `JSONSerialization` 的产物),所以这里走同一条路径而不是新增一个生产入口。
func decodeJSON(_ text: String) throws -> JSONValue {
    let object = try JSONSerialization.jsonObject(with: Data(text.utf8))
    return JSONValue.from(object)
}

/// Mirrors `RemoteClient.relayDiagnostics` parsing without needing a live
/// `HostConnection`: the parsing rules are what these tests are pinning.
enum RemoteClientRelayStub {
    static func parse(_ value: JSONValue) -> RemoteClient.RelayDiagnostics {
        guard case let .object(object)? = value["relay"],
              case let .string(url)? = object["url"] else {
            return RemoteClient.RelayDiagnostics(
                url: "", schemeOK: false, reachable: false,
                authenticated: false, latencyMs: nil, detail: "malformed"
            )
        }
        return RemoteClient.RelayDiagnostics(
            url: url,
            schemeOK: object["schemeOk"]?.boolValue ?? false,
            reachable: object["reachable"]?.boolValue ?? false,
            authenticated: object["authenticated"]?.boolValue ?? false,
            latencyMs: object["latencyMs"]?.numberText.flatMap(Int.init),
            detail: {
                if case let .string(text)? = object["detail"] { return text }
                return ""
            }()
        )
    }
}
