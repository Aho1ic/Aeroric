@testable import AeroricProtocol
import XCTest

final class PackageSmokeTests: XCTestCase {
    func testRoundTripPreservesUnknownMembers() {
        let text = #"{"v":1,"id":"req-1","method":"task.start","futureField":{"a":1}}"#
        guard let decoded = RPCEnvelope.decode(text) else { return XCTFail("decode returned nil") }
        XCTAssertEqual(decoded.version, 1)
        XCTAssertEqual(decoded.id, "req-1")
        XCTAssertEqual(decoded.method, "task.start")
        let reencoded = decoded.encode() ?? ""
        XCTAssertTrue(reencoded.contains("futureField"), "unknown member must be re-emitted, not dropped")
        guard let again = RPCEnvelope.decode(reencoded) else { return XCTFail("re-decode returned nil") }
        XCTAssertEqual(decoded, again, "unknown members must survive one round trip")
    }

    func testUnknownScalarMemberSurvivesRoundTrip() {
        let decoded = RPCEnvelope.decode(#"{"v":1,"id":"x","flag":true}"#)
        XCTAssertNotNil(decoded)
        XCTAssertEqual(decoded?.unknownMembers.keys.sorted(), ["flag"])
        XCTAssertEqual(RPCEnvelope.decode(decoded?.encode() ?? "")?.unknownMembers["flag"], decoded?.unknownMembers["flag"])
    }

    func testRejectsFramesWithoutVersion() {
        XCTAssertNil(RPCEnvelope.decode(#"{"id":"x"}"#))
        XCTAssertNil(RPCEnvelope.decode("not json at all"))
        XCTAssertNil(RPCEnvelope.decode(""))
    }

    func testVersionMarker() {
        XCTAssertEqual(ProtocolMarker.version, 1)
    }
}
