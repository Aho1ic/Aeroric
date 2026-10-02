import XCTest
@testable import AeroricUI

/// P5-01 验收：coordinator 的 cursor 记账、输入策略、重置语义与
/// 100 次 attach/detach 无泄漏（Action 2：不 spawn 子进程——本套件在
/// AeroricUIUnitTests 里跑，不启动 App，也没有任何 PTY 子进程）。
@MainActor
final class TerminalCoordinatorTests: XCTestCase {

    private func makeCoordinator(
        store: TerminalStore = TerminalStore()
    ) -> TerminalCoordinator {
        TerminalCoordinator(taskID: "task-1", generation: 1, store: store)
    }

    private func tail(
        kind: String,
        from: UInt64,
        bytes: [UInt8],
        resumeAfter: UInt64,
        reason: String? = nil
    ) -> TerminalCoordinator.PlanTail {
        TerminalCoordinator.PlanTail(
            kind: kind, from: from, bytes: bytes, resumeAfter: resumeAfter, reasonKind: reason
        )
    }

    // MARK: - host → renderer

    func testSeamlessTailFeedsRendererAndAdvancesCursor() {
        let coordinator = makeCoordinator()
        coordinator.attach(holdsLease: true)
        let decision = coordinator.ingest(tail(kind: "seamless", from: 0, bytes: [104, 105], resumeAfter: 2))
        XCTAssertEqual(decision, .seamless)
        XCTAssertEqual(coordinator.feedCount, 1)
        let state = coordinator.store.state(taskID: "task-1", generation: 1)
        XCTAssertEqual(state?.cursor, 2)
        XCTAssertEqual(state?.hasReceivedBytes, true)
    }

    func testEvictedTailResetsRendererAndRebuildsFromSafeBoundary() {
        let coordinator = makeCoordinator()
        coordinator.attach(holdsLease: true)
        coordinator.ingest(tail(kind: "seamless", from: 0, bytes: [1, 2, 3], resumeAfter: 3))
        let before = coordinator.feedCount
        // 流被退场：整屏复位 + 从安全边界重建（Action 5）。
        let decision = coordinator.ingest(
            tail(kind: "reset", from: 0, bytes: [9], resumeAfter: 1, reason: "evicted")
        )
        XCTAssertEqual(decision, .reset(reason: "evicted", safeBoundary: 0))
        XCTAssertEqual(coordinator.resetCount, 1)
        XCTAssertEqual(coordinator.feedCount, before + 1)
        let state = coordinator.store.state(taskID: "task-1", generation: 1)
        XCTAssertEqual(state?.cursor, 1)
        XCTAssertEqual(state?.lastResetReason, "evicted")
    }

    func testNotStartedTailWaitsWithoutFeeding() {
        let coordinator = makeCoordinator()
        coordinator.attach(holdsLease: true)
        let decision = coordinator.ingest(
            tail(kind: "reset", from: 0, bytes: [], resumeAfter: 0, reason: "notStarted")
        )
        XCTAssertEqual(decision, .waitUntilStarted)
        XCTAssertEqual(coordinator.feedCount, 0)
        XCTAssertEqual(coordinator.resetCount, 0)
    }

    // MARK: - renderer → host

    func testDelegateSendReachesHostWhenLeaseHeld() {
        let coordinator = makeCoordinator()
        var received: [UInt8]?
        coordinator.onInput = { _, _, data in received = data }
        coordinator.attach(holdsLease: true)
        coordinator.send(source: coordinator.renderer, data: [104, 105][...])
        XCTAssertEqual(received, [104, 105])
    }

    func testDelegateSendIsRefusedWithoutLease() {
        let coordinator = makeCoordinator()
        var called = false
        coordinator.onInput = { _, _, _ in called = true }
        coordinator.attach(holdsLease: false)
        coordinator.send(source: coordinator.renderer, data: [104][...])
        XCTAssertFalse(called, "无租约不得写 PTY")
    }

    func testResizeForwardsOnlyWhenLeaseHeld() {
        let coordinator = makeCoordinator()
        var sizes: [(Int, Int)] = []
        coordinator.onResize = { _, _, cols, rows in sizes.append((cols, rows)) }
        coordinator.attach(holdsLease: false)
        coordinator.sizeChanged(source: coordinator.renderer, newCols: 80, newRows: 24)
        XCTAssertTrue(sizes.isEmpty, "无租约不得 resize")
        coordinator.attach(holdsLease: true)
        coordinator.sizeChanged(source: coordinator.renderer, newCols: 100, newRows: 30)
        // Swift 6 元组不直接满足 XCTAssertEqual 的泛型约束——拆字段断言。
        XCTAssertEqual(sizes.count, 1)
        XCTAssertEqual(sizes.first?.0, 100)
        XCTAssertEqual(sizes.first?.1, 30)
    }

    // MARK: - 生命周期（100 次 attach/detach 无泄漏）

    func testHundredAttachDetachCyclesKeepSingleStateAndCounters() {
        let store = TerminalStore()
        let coordinator = makeCoordinator(store: store)
        for _ in 0..<100 {
            coordinator.attach(holdsLease: true)
            coordinator.detach()
        }
        XCTAssertEqual(store.liveStreamCount, 1, "同 taskId+generation 复用同一状态")
        // 101 = 构造器 1 次 + 循环 100 次；detach 只在循环里发生。
        XCTAssertEqual(store.attachCount, 101)
        XCTAssertEqual(store.detachCount, 100)
        // 循环后再重显一次，cursor 补流仍可用。
        coordinator.attach(holdsLease: true)
        coordinator.ingest(tail(kind: "seamless", from: 0, bytes: [7], resumeAfter: 1))
        XCTAssertEqual(coordinator.store.state(taskID: "task-1", generation: 1)?.cursor, 1)
    }

    // MARK: - 输入策略（Action 6）

    func testInputPolicyGatesOnLeaseVisibilityAndComposition() {
        XCTAssertTrue(TerminalInputPolicy.allowsInput(holdsLease: true, isHidden: false, isComposing: false))
        XCTAssertFalse(TerminalInputPolicy.allowsInput(holdsLease: false, isHidden: false, isComposing: false))
        XCTAssertFalse(TerminalInputPolicy.allowsInput(holdsLease: true, isHidden: true, isComposing: false))
        // IME 组合中 Enter 不提交：isComposing 直接拒绝。
        XCTAssertFalse(TerminalInputPolicy.allowsInput(holdsLease: true, isHidden: false, isComposing: true))
    }

    func testPastePolicyRequiresLeaseAndContent() {
        XCTAssertTrue(TerminalInputPolicy.allowsPaste(holdsLease: true, isHidden: false, content: "x"))
        XCTAssertFalse(TerminalInputPolicy.allowsPaste(holdsLease: false, isHidden: false, content: "x"))
        XCTAssertFalse(TerminalInputPolicy.allowsPaste(holdsLease: true, isHidden: false, content: ""))
    }

    func testOSC52ClipboardIsNeverAutomatic() {
        let decision = TerminalInputPolicy.osc52ClipboardRequest()
        XCTAssertFalse(decision.writesClipboard, "OSC 52 永不自动写剪贴板")
        XCTAssertFalse(decision.userApproved, "确认对话框落地前用户通道也关闭")
    }

    func testLinkPolicyWhitelistsSchemes() {
        XCTAssertTrue(TerminalInputPolicy.allowsOpeningLink("https://example.com"))
        XCTAssertTrue(TerminalInputPolicy.allowsOpeningLink("mailto:a@b.c"))
        XCTAssertFalse(TerminalInputPolicy.allowsOpeningLink("file:///etc/passwd"))
        XCTAssertFalse(TerminalInputPolicy.allowsOpeningLink("javascript:alert(1)"))
        XCTAssertFalse(TerminalInputPolicy.allowsOpeningLink("not a url"))
    }

    func testRequestOpenLinkDropsNonWhitelistedSchemes() {
        let coordinator = makeCoordinator()
        var links: [String] = []
        coordinator.onLink = { _, link in links.append(link) }
        coordinator.attach(holdsLease: true)
        coordinator.requestOpenLink(source: coordinator.renderer, link: "file:///etc/passwd", params: [:])
        XCTAssertTrue(links.isEmpty)
        coordinator.requestOpenLink(source: coordinator.renderer, link: "https://example.com", params: [:])
        XCTAssertEqual(links, ["https://example.com"])
    }
}
