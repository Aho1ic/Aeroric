import XCTest
@testable import AeroricUI

/// P5-04 Action 3：DSH composer 能力门控/斜杠解析/轨迹合并。
@MainActor
final class DshComposerTests: XCTestCase {

    func testCapabilityGatesModelAndEffort() {
        let catalog = DshComposerModel.Catalog(
            currentProvider: "p1", currentModel: "m1", currentEffort: "low",
            choices: [("p1", "m1"), ("p1", "m2")], efforts: ["low", "high"]
        )
        XCTAssertTrue(DshComposerModel.isAllowed(provider: "p1", model: "m1", effort: "low", in: catalog))
        XCTAssertFalse(DshComposerModel.isAllowed(provider: "p2", model: "m1", effort: nil, in: catalog), "未知 provider 拒绝")
        XCTAssertFalse(DshComposerModel.isAllowed(provider: "p1", model: "m1", effort: "ultra", in: catalog), "capability 外 effort 拒绝")
        XCTAssertTrue(DshComposerModel.isAllowed(provider: "p1", model: "m2", effort: nil, in: catalog))
    }

    func testEmptyEffortCatalogMeansNoEffortOverride() {
        let catalog = DshComposerModel.Catalog(
            currentProvider: "p", currentModel: "m", currentEffort: nil,
            choices: [("p", "m")], efforts: []
        )
        XCTAssertTrue(DshComposerModel.isAllowed(provider: "p", model: "m", effort: nil, in: catalog))
        XCTAssertFalse(
            DshComposerModel.isAllowed(provider: "p", model: "m", effort: "low", in: catalog),
            "provider 接受模型但无 effort 覆盖"
        )
    }

    func testParseSlashCommand() {
        XCTAssertEqual(DshComposerModel.parseSlashCommand("/run x y")?.command, "/run")
        XCTAssertEqual(DshComposerModel.parseSlashCommand("/run x y")?.rest, "x y")
        XCTAssertEqual(DshComposerModel.parseSlashCommand("/help")?.command, "/help")
        XCTAssertNil(DshComposerModel.parseSlashCommand("hello /world"))
        XCTAssertNil(DshComposerModel.parseSlashCommand("   "))
    }

    func testComposerValidationRejectsEmptyPrompt() {
        XCTAssertEqual(DshComposerModel.validateComposer(text: "  ", attachmentPaths: []), "消息为空")
        XCTAssertNil(DshComposerModel.validateComposer(text: "hi", attachmentPaths: []))
    }

    func testTrajectoryMergeDeduplicatesBySeq() {
        let current = [
            DshTrajectoryEntry(seq: 1, type: "tool/call"),
            DshTrajectoryEntry(seq: 2, type: "tool/result"),
        ]
        let incoming = [
            DshTrajectoryEntry(seq: 2, type: "tool/result"),
            DshTrajectoryEntry(seq: 3, type: "assistant/message"),
        ]
        let merged = DshTrajectoryLogic.merge(current: current, incoming: incoming)
        XCTAssertEqual(merged.map(\.seq), [1, 2, 3], "live 与 history 按 seq 去重")
    }

    func testTrajectoryMergeFastPathIsAppendOnly() {
        let current = [DshTrajectoryEntry(seq: 5, type: "a")]
        let incoming = [DshTrajectoryEntry(seq: 9, type: "b"), DshTrajectoryEntry(seq: 7, type: "c")]
        let merged = DshTrajectoryLogic.merge(current: current, incoming: incoming)
        XCTAssertEqual(merged.map(\.seq), [5, 9, 7], "升序快路径 append-only（React 同款）")
    }
}
