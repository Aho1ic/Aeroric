import AeroricClient
import AeroricProtocol
@testable import AeroricUI
import XCTest

/// A14b (2026-11-07): the 设置 → 通用 → Agent 进程 switch, and background
/// commands listed apart from subagents.
@MainActor
final class IdleReleaseTests: XCTestCase {
    private struct Refused: Error {}

    func testTheReplyNeedsBothFields() throws {
        let decoded = try SessionsClient.decodeIdleRelease(
            .object(["enabled": .bool(false), "idleMinutes": .number("60")]))
        XCTAssertEqual(decoded.enabled, false)
        XCTAssertEqual(decoded.idleMinutes, 60)
        XCTAssertThrowsError(try SessionsClient.decodeIdleRelease(.object(["enabled": .bool(true)])))
    }

    func testAHostWithoutTheMethodsLeavesTheCardHidden() async {
        let store = IdleReleaseStore()
        store.attach(read: { throw Refused() }, write: { _ in throw Refused() })
        await store.refresh()
        XCTAssertFalse(store.isAvailable)

        store.attach(read: { (enabled: false, idleMinutes: 60) }, write: { (enabled: $0, idleMinutes: 60) })
        await store.refresh()
        XCTAssertTrue(store.isAvailable)
        XCTAssertFalse(store.enabled)
    }

    func testAFailedSavePutsTheSwitchBackAndSaysWhy() async {
        let store = IdleReleaseStore()
        store.attach(read: { (enabled: true, idleMinutes: 60) }, write: { _ in throw Refused() })
        await store.refresh()
        await store.setEnabled(false)
        XCTAssertTrue(store.enabled)
        XCTAssertNotNil(store.lastError)

        store.attach(read: { (enabled: true, idleMinutes: 60) }, write: { (enabled: $0, idleMinutes: 60) })
        await store.setEnabled(false)
        XCTAssertFalse(store.enabled)
        XCTAssertNil(store.lastError)
    }

    func testABackgroundCommandIsNotASubagent() {
        let command = LiveSubagent(id: "b1", title: "后台命令：npm run dev", status: "running")
        let agent = LiveSubagent(id: "k1", title: "检索代码", status: "completed")
        XCTAssertTrue(command.isBackgroundCommand)
        XCTAssertEqual(command.shownTitle, "npm run dev")
        XCTAssertFalse(agent.isBackgroundCommand)
        XCTAssertEqual(agent.shownTitle, "检索代码")
    }
}
