import AeroricProtocol
@testable import AeroricUI
import XCTest

private let status: JSONValue = .object([
    "settings": .object([
        "caps": .array([
            .object(["period": .string("day"), "tokens": .number("200000")]),
            .object(["period": .string("month"), "costUsd": .number("20.0")]),
        ]),
        "paused": .bool(false),
    ]),
    "today": .object(["tokens": .number("170000"), "costUsd": .number("1.2")]),
    "month": .object(["tokens": .number("2100000"), "costUsd": .number("18")]),
    "gate": .object(["kind": .string("near"), "reason": .string("今天无人值守的工作用了 170000 / 200000 token（85%）")]),
])

/// E6 step 3 (2026-11-21): the caps on 用量与成本, 暂停所有 Agent in the sidebar.
@MainActor
final class BudgetTests: XCTestCase {
    func testTheStatusReadsAsThePageShowsIt() {
        let b = LiveBudget.decode(status)
        XCTAssertEqual(b.caps.map(BudgetText.cap), ["每天 200K tok", "每月 $20.00"])
        XCTAssertEqual(b.gate, "near")
        XCTAssertEqual(BudgetText.gate(b), "今天无人值守的工作用了 170000 / 200000 token（85%），快到上限了")
        XCTAssertTrue(BudgetText.spent(b).hasPrefix("今天 170K tok"), BudgetText.spent(b))
        XCTAssertEqual(BudgetText.gate(LiveBudget()), "没有设上限")
        XCTAssertTrue(BudgetText.gate(LiveBudget(gate: "paused")).hasPrefix("已暂停所有 Agent"))
        let wire = b.settingsJSON
        XCTAssertEqual(wire["caps"]?.arrayValue?.count, 2)
        XCTAssertEqual(wire["caps"]?.arrayValue?[1]["costUsd"]?.numberText, "20.0")
        XCTAssertEqual(wire["paused"]?.boolValue, false)
    }

    /// Pausing and changing caps go to the host with the rest kept; a refusal is said.
    func testTheBookPausesAndSavesCaps() async {
        let book = BudgetBook()
        let sent = Sent()
        book.attach(BudgetBook.Calls(
            status: { LiveBudget.decode(status) },
            set: { next in
                if next.caps.contains(where: { $0.tokens == 0 }) { throw Failure() }
                await sent.add(next)
                var back = next
                back.gate = next.paused ? "paused" : "go"
                return back
            }))
        await book.refresh()
        XCTAssertTrue(book.loaded)
        await book.setPaused(true)
        XCTAssertEqual(book.budget.gate, "paused")
        let paused = await sent.last
        XCTAssertEqual(paused?.paused, true)
        XCTAssertEqual(paused?.caps.count, 2, "the caps are kept")
        let saved = await book.saveCaps([LiveUsageCap(period: "day", tokens: 1000)])
        XCTAssertTrue(saved)
        let capped = await sent.last
        XCTAssertEqual(capped?.caps, [LiveUsageCap(period: "day", tokens: 1000)])
        XCTAssertEqual(capped?.paused, true, "still paused")
        let refused = await book.saveCaps([LiveUsageCap(period: "day", tokens: 0)])
        XCTAssertFalse(refused)
        XCTAssertTrue(book.errorText?.hasPrefix("保存失败") ?? false)
    }

    private struct Failure: Error {}

    private actor Sent {
        var last: LiveBudget?
        func add(_ b: LiveBudget) { last = b }
    }
}
