import AeroricProtocol
@testable import AeroricUI
import XCTest

/// A7 (2026-11-17): files already as the undo would leave them are their own
/// list, not a conflict; a half-done undo says what failed and that trying
/// again finishes it.
final class TurnUndoPlanTests: XCTestCase {
    func testInPlaceAndFailedAreDecoded() {
        let plan = LiveTurnPlan.decode(.object([
            "files": .array([.object(["path": .string("src/b.rs"), "delete": .bool(true)])]),
            "inPlace": .array([.string("src/a.rs")]),
            "failed": .array([.object(["path": .string("src/c.rs"), "reason": .string("Permission denied")])]),
        ]))
        XCTAssertEqual(plan.files, [.init(path: "src/b.rs", delete: true)])
        XCTAssertEqual(plan.inPlace, ["src/a.rs"])
        XCTAssertEqual(plan.failed, [.init(path: "src/c.rs", reason: "Permission denied")])
        XCTAssertTrue(LiveTurnPlan.decode(.object([:])).inPlace.isEmpty, "older hosts send neither")
    }

    func testFilesAllInPlaceCanStillBeConfirmed() {
        let plan = LiveTurnPlan(inPlace: ["src/a.rs"])
        XCTAssertTrue(plan.actionable)
        XCTAssertFalse(LiveTurnPlan(skipped: [.init(path: "a", reason: "这一轮之后又改过")]).actionable)
        XCTAssertEqual(TurnLedgerText.nothingToWrite(plan, action: "undo"),
                       "这些文件已经是撤销后的样子，确认后只把这一轮记为已撤销。")
        XCTAssertEqual(TurnLedgerText.nothingToWrite(LiveTurnPlan(), action: "undo"), "没有可以恢复的文件。")
        XCTAssertNil(TurnLedgerText.nothingToWrite(LiveTurnPlan(files: [.init(path: "a")]), action: "undo"))
        XCTAssertEqual(TurnLedgerText.inPlaceTitle("undo"), "已是撤销后的样子")
        XCTAssertEqual(TurnLedgerText.inPlaceTitle("restore"), "已是这一轮的结果")
    }

    func testAHalfDoneUndoSaysWhatFailedAndThatTryingAgainFinishesIt() {
        let plan = LiveTurnPlan(files: [.init(path: "keep.txt")],
                                failed: [.init(path: "src/a.rs", reason: "Permission denied")])
        XCTAssertEqual(TurnLedgerText.halfDone(plan, action: "undo"),
                       "撤销没做完：1 个文件写不进去（src/a.rs：Permission denied）。已改好的 1 个文件保持现在的样子，再撤销一次会接着做完。")
        XCTAssertNil(TurnLedgerText.halfDone(LiveTurnPlan(files: [.init(path: "a")]), action: "undo"))
        let nothingDone = LiveTurnPlan(failed: [.init(path: "a", reason: "x")])
        XCTAssertEqual(TurnLedgerText.halfDone(nothingDone, action: "restore"),
                       "撤回撤销没做完：1 个文件写不进去（a：x）。再撤回撤销一次会接着做完。")
    }
}
