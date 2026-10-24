import XCTest
@testable import AeroricUI

/// Pure memory only. No TerminalView, process, host lease or real input.
@MainActor final class TaskPresentationInputGateTests: XCTestCase {
    func testPreviewReturnDoesNotGrantInputAgain() {
        let gate = TaskPresentationInputGate()
        gate.update(taskID: "a", visible: true)
        XCTAssertTrue(gate.permits("a"))
        gate.leave()
        XCTAssertFalse(gate.permits("a"))
        gate.update(taskID: "a", visible: true)
        XCTAssertFalse(gate.permits("a"), "showing a task is not acquiring control")
    }
    func testTaskAndWindowIdentityStayIndependent() {
        let first = TaskPresentationInputGate(), second = TaskPresentationInputGate()
        first.update(taskID: "a", visible: true)
        second.update(taskID: "b", visible: true)
        first.update(taskID: "c", visible: true)
        XCTAssertFalse(first.permits("a"))
        XCTAssertTrue(first.permits("c"))
        XCTAssertTrue(second.permits("b"))
        first.update(taskID: "a", visible: true)
        XCTAssertFalse(first.permits("a"))
        XCTAssertFalse(first.permits("c"))
        XCTAssertTrue(second.permits("b"))
    }
    func testRepeatedVisibilityAndAppearanceDoNotChangeInitialTaskPermission() {
        let gate = TaskPresentationInputGate()
        gate.update(taskID: "a", visible: true)
        for _ in 0..<10 { gate.update(taskID: "a", visible: true) }
        XCTAssertTrue(gate.permits("a"))
        gate.update(taskID: nil, visible: false)
        XCTAssertFalse(gate.permits("a"))
    }
}
