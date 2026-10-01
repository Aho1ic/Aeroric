import XCTest
@testable import AeroricUI

/// Shell policy tests (P4-04): the pure `DetailState.resolve` mapping and
/// the pure `filterCommands` query filter. No AppKit session is needed —
/// both are plain value-level decisions.
final class ShellPolicyTests: XCTestCase {
    // MARK: DetailState.resolve — the four shell states are deliberately
    // separate (P4-04 Action 5): loading never looks like "no projects".

    func testResolveIdleAndConnectingAreLoading() {
        XCTAssertEqual(DetailState.resolve(.idle), .loading)
        XCTAssertEqual(DetailState.resolve(.connecting), .loading)
    }

    func testResolveReadyIsEmpty() {
        XCTAssertEqual(DetailState.resolve(.ready), .empty)
    }

    func testResolveDegradedCarriesReason() {
        XCTAssertEqual(
            DetailState.resolve(.degraded(reason: "spawn failed")),
            .disconnected(reason: "spawn failed")
        )
    }

    func testResolveReconnectingIsDisconnectedWithoutReason() {
        XCTAssertEqual(DetailState.resolve(.reconnecting), .disconnected(reason: nil))
    }

    func testResolveStoppedIsDisconnected() {
        XCTAssertEqual(DetailState.resolve(.stopped), .disconnected(reason: nil))
    }

    func testResolveIncompatibleIsDistinctError() {
        XCTAssertEqual(
            DetailState.resolve(.incompatible(reason: "wire v9")),
            .incompatible(reason: "wire v9")
        )
    }

    // MARK: filterCommands

    func testFilterCommandsEmptyQueryReturnsAll() {
        let commands = [
            PaletteCommand(id: "a", title: "打开项目"),
            PaletteCommand(id: "b", title: "设置…"),
        ]
        XCTAssertEqual(filterCommands(commands, query: ""), commands)
        XCTAssertEqual(filterCommands(commands, query: "   "), commands)
    }

    func testFilterCommandsCaseInsensitiveSubstring() {
        let commands = [
            PaletteCommand(id: "a", title: "Open Project…"),
            PaletteCommand(id: "b", title: "Settings…"),
        ]
        XCTAssertEqual(
            filterCommands(commands, query: "open proj"),
            [PaletteCommand(id: "a", title: "Open Project…")]
        )
    }

    func testFilterCommandsNoMatchIsEmpty() {
        let commands = [PaletteCommand(id: "a", title: "设置…")]
        XCTAssertTrue(filterCommands(commands, query: "xyz").isEmpty)
    }

    func testFilterCommandsCJKSubstringMatches() {
        let commands = [
            PaletteCommand(id: "a", title: "打开项目…"),
            PaletteCommand(id: "b", title: "设置…"),
        ]
        XCTAssertEqual(
            filterCommands(commands, query: "项目"),
            [PaletteCommand(id: "a", title: "打开项目…")]
        )
    }
}
