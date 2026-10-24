import XCTest

@testable import AeroricProtocol

/// Reads the committed `contracts/native/v1/fixtures/legacy-cleanup.json` that the
/// Rust DTOs generate, so both sides agree on the same bytes.
final class LegacyCleanupContractTests: XCTestCase {
    private static let repoRoot: URL = {
        var url = URL(fileURLWithPath: #filePath)
        for _ in 0..<5 { url.deleteLastPathComponent() }
        return url
    }()

    private func fixture() throws -> JSONValue {
        let url = Self.repoRoot.appendingPathComponent("contracts/native/v1/fixtures/legacy-cleanup.json")
        let data = try Data(contentsOf: url)
        return JSONValue.from(try JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed]))
    }

    private func json(_ text: String) throws -> JSONValue {
        JSONValue.from(try JSONSerialization.jsonObject(with: Data(text.utf8), options: [.fragmentsAllowed]))
    }

    func testCommittedFixtureDecodesEveryState() throws {
        let fixture = try fixture()
        let plan = try LegacyCleanupReport.decode(fixture["planResult"] ?? .null)
        XCTAssertNil(plan.quarantineBatch)
        XCTAssertEqual(plan.candidates.map(\.source), [
            "custom:fixture-ready", "custom:fixture-rotated", "custom:fixture-partial", "custom:fixture-done",
        ])
        XCTAssertEqual(plan.candidates[0].state, .eligible)
        guard case let .blocked(reason) = plan.candidates[1].state else { return XCTFail("blocked") }
        XCTAssertFalse(reason.isEmpty)
        XCTAssertEqual(plan.candidates[2].state, .partial(
            remaining: [.quarantineWrapper, .deleteOldSecrets],
            reason: "QuarantineWrapper: legacy launcher is no longer an Aeroric-generated wrapper; left in place"
        ))
        XCTAssertEqual(plan.candidates[3].state, .done)
        XCTAssertEqual(plan.candidates.map(\.state.isActionable), [true, false, true, false])
        XCTAssertEqual(plan.candidates[0].agentID, "fixture-ready")

        let applied = try LegacyCleanupReport.decode(fixture["applyResult"] ?? .null)
        XCTAssertEqual(applied.candidates[0].state, .done)
        XCTAssertEqual(applied.quarantineBatch, "20261024T120000.000Z-0")
    }

    func testApplyParamsMatchTheCommittedRequest() throws {
        let expected = try fixture()["applyRequest"]
        XCTAssertEqual(legacyCleanupApplyParams(["custom:fixture-ready"]), expected)
    }

    func testMalformedRepliesAreRejected() throws {
        let valid = #"{"schemaVersion":1,"candidates":[{"source":"custom:a","state":{"state":"eligible"}}],"quarantineBatch":null}"#
        XCTAssertNoThrow(try LegacyCleanupReport.decode(try json(valid)))
        let malformed = [
            #"{"schemaVersion":2,"candidates":[],"quarantineBatch":null}"#,
            #"{"schemaVersion":1,"candidates":[],"quarantineBatch":null,"extra":1}"#,
            #"{"schemaVersion":1,"candidates":[]}"#,
            #"{"schemaVersion":1,"candidates":[{"source":"custom:a","state":{"state":"exploded"}}],"quarantineBatch":null}"#,
            #"{"schemaVersion":1,"candidates":[{"source":"custom:a","state":{"state":"eligible","extra":1}}],"quarantineBatch":null}"#,
            #"{"schemaVersion":1,"candidates":[{"source":"builtin:claude","state":{"state":"done"}}],"quarantineBatch":null}"#,
            #"{"schemaVersion":1,"candidates":[{"source":"custom:","state":{"state":"done"}}],"quarantineBatch":null}"#,
            #"{"schemaVersion":1,"candidates":[{"source":"custom:a","state":{"state":"done"}},{"source":"custom:a","state":{"state":"done"}}],"quarantineBatch":null}"#,
            #"{"schemaVersion":1,"candidates":[{"source":"custom:a","state":{"state":"blocked"}}],"quarantineBatch":null}"#,
            #"{"schemaVersion":1,"candidates":[{"source":"custom:a","state":{"state":"partial","remaining":["teleport"],"reason":null}}],"quarantineBatch":null}"#,
            #"{"schemaVersion":1,"candidates":[],"quarantineBatch":"../escape"}"#,
            #"{"schemaVersion":1,"candidates":[],"quarantineBatch":""}"#,
        ]
        for text in malformed {
            XCTAssertThrowsError(try LegacyCleanupReport.decode(try json(text)), text)
        }
    }
}
