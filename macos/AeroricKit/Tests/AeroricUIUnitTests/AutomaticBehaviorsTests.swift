import AeroricProtocol
@testable import AeroricUI
import XCTest

private let rolloutListed: JSONValue = .object([
    "features": .array([
        .object([
            "id": .string("recall"), "tier": .string("described"),
            "history": .array([.object(["tier": .string("candidate"), "atMs": .number("10")])]),
            "checked": .number("10"), "would": .number("3"),
        ]),
        .object(["id": .string("skill:ssh-timeout"), "tier": .string("described"), "history": .array([])]),
        .object(["tier": .string("off")]),
    ]),
])

/// H8 step 3 (2026-11-21): Agent 配置 → 自动行为.
@MainActor
final class AutomaticBehaviorsTests: XCTestCase {
    func testTheListReadsAsThePageShowsIt() {
        let features = LiveRolloutFeature.decodeList(rolloutListed)
        XCTAssertEqual(features.count, 2, "one without an id is dropped")
        let recall = features[0]
        XCTAssertEqual(recall.history, ["candidate"])
        XCTAssertEqual([recall.checked, recall.would], [10, 3])
        XCTAssertEqual(RolloutText.title(recall), "按消息召回记忆和 Wiki")
        XCTAssertEqual(RolloutText.rate(recall), "本来会触发 3/10 次（30%）")
        XCTAssertEqual(RolloutText.choices(recall), ["off", "candidate", "described", "default"])
        XCTAssertTrue(RolloutText.meaning(recall).hasPrefix("找到时只附出处"))
        XCTAssertEqual(RolloutText.revertLabel(recall), "退回到“候选”", "the tier before the last move")

        let skill = features[1]
        XCTAssertEqual(skill.skillName, "ssh-timeout")
        XCTAssertEqual(RolloutText.title(skill), "学到的技能 ssh-timeout")
        XCTAssertEqual(RolloutText.rate(skill), "还没有判断过")
        XCTAssertEqual(RolloutText.choices(skill), ["candidate", "described", "default"], "never off here")
        XCTAssertEqual(RolloutText.revertLabel(skill), "退回到“候选”", "one tier down without history")
        let lowest = LiveRolloutFeature(id: "skill:x", tier: "candidate")
        XCTAssertNil(RolloutText.revertLabel(lowest))
        XCTAssertNil(RolloutText.revertLabel(LiveRolloutFeature(id: "recall", tier: "off")))
    }

    /// A move and a 退回 each go to the host and read the list again; the
    /// same tier is not sent; a refusal says what failed.
    func testTheBookMovesAndRevertsThenReadsAgain() async {
        let book = RolloutBook()
        let sent = Sent()
        book.attach(RolloutBook.Calls(
            list: { await sent.add("list"); return LiveRolloutFeature.decodeList(rolloutListed) },
            set: { id, tier in
                if tier == "off" { throw Failure() }
                await sent.add("set \(id) \(tier)")
            },
            revert: { id in await sent.add("revert \(id)") }))
        await book.refresh()
        XCTAssertTrue(book.loaded)
        await book.set("recall", tier: "described")
        await book.set("recall", tier: "default")
        await book.revert("skill:ssh-timeout")
        let calls = await sent.lines
        XCTAssertEqual(calls, ["list", "set recall default", "list", "revert skill:ssh-timeout", "list"])
        await book.set("recall", tier: "off")
        XCTAssertTrue(book.errorText?.hasPrefix("换档失败") ?? false)
        XCTAssertEqual(book.features.count, 2, "the page stays")
    }

    private struct Failure: Error {}

    private actor Sent {
        var lines: [String] = []
        func add(_ line: String) { lines.append(line) }
    }
}
