@testable import AeroricUI
import XCTest

final class PackageSmokeTests: XCTestCase {
    func testPresentationOfRequestFrame() {
        XCTAssertEqual(present(#"{"v":1,"id":"r9","method":"task.start"}"#),
                       ConnectionLine(text: "task.start #r9"))
    }

    func testPresentationOfResponseFrame() {
        XCTAssertEqual(present(#"{"v":1,"id":"r9","ok":true}"#),
                       ConnectionLine(text: "response ok=true"))
    }

    func testGarbageFrameYieldsNilInsteadOfCrashing() {
        XCTAssertNil(present(""))
        XCTAssertNil(present("{oops"))
    }

    func testDependencyDirectionIsTransitive() {
        XCTAssertEqual(UIMarker.seenProtocolVersion, 1)
    }
}
