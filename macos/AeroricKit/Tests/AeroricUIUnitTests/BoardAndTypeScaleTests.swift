import XCTest
@testable import AeroricUI

final class BoardAndTypeScaleTests: XCTestCase {
    private func task(_ id: String, _ status: String, updated: UInt64? = nil, created: UInt64? = nil,
                      starred: Bool = false, archived: Bool = false) -> TaskStore.TaskEntry {
        TaskStore.TaskEntry(id: id, projectID: "p", title: id, status: status, agent: "claude",
                            createdAtMs: created, updatedAtMs: updated, archived: archived, starred: starred)
    }

    // MARK: - Board

    func testStatusesLandInTheirColumns() {
        let mapping: [String: TaskBoardColumn] = [
            "todo": .active, "pending": .active, "running": .active, "detached": .active,
            "input_required": .waiting, "done": .done,
            "failed": .stopped, "cancelled": .stopped, "interrupted": .stopped,
            "something_new": .active,
        ]
        for (status, column) in mapping { XCTAssertEqual(TaskBoardColumn(status: status), column, status) }
        XCTAssertTrue(TaskBoardColumn.waiting.allowsCancel)
        XCTAssertFalse(TaskBoardColumn.done.allowsCancel)
    }

    func testEveryColumnIsPresentArchivedIsHiddenAndOrderIsStarredThenRecent() {
        let columns = TaskBoard.columns([
            task("old", "running", updated: 10),
            task("new", "running", updated: 30),
            task("star", "running", updated: 5, starred: true),
            task("createdOnly", "running", created: 20),
            task("gone", "running", updated: 99, archived: true),
            task("wait", "input_required", updated: 1),
        ])
        XCTAssertEqual(columns.map(\.id), TaskBoardColumn.allCases)
        XCTAssertEqual(columns[0].tasks.map(\.id), ["star", "new", "createdOnly", "old"])
        XCTAssertEqual(columns[1].tasks.map(\.id), ["wait"])
        XCTAssertTrue(columns[2].tasks.isEmpty, "empty columns stay visible")
    }

    func testProjectTintIsStableAndInRange() {
        let first = ProjectTint.index(for: "project-a")
        XCTAssertEqual(first, ProjectTint.index(for: "project-a"))
        for id in ["", "a", "b", "很长的项目名", UUID().uuidString] {
            XCTAssertTrue((0..<ProjectTint.count).contains(ProjectTint.index(for: id)))
        }
    }

    func testFileSymbolsAndChangeBadges() {
        XCTAssertEqual(FileSymbol.symbol(forFile: "main.swift"), "chevron.left.forwardslash.chevron.right")
        XCTAssertEqual(FileSymbol.symbol(forFile: "Cargo.toml"), "curlybraces")
        XCTAssertEqual(FileSymbol.symbol(forFile: "Makefile"), "doc")
        XCTAssertEqual(ChangeBadge(status: "M"), .modified)
        XCTAssertEqual(ChangeBadge(status: "?"), .untracked)
        XCTAssertEqual(ChangeBadge(status: "?").letter, "U")
        XCTAssertEqual(ChangeBadge(status: "T").letter, "T")
    }

    // MARK: - Type scale

    private func scratchDefaults() -> UserDefaults {
        let name = "type-scale-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: name)!
        defaults.removePersistentDomain(forName: name)
        return defaults
    }

    func testStepsMapToTheAgreedSizes() {
        XCTAssertEqual(AeroricTypeScale.Step.allCases.map { AeroricTypeScale.Kind.text.size($0) }, [12, 13, 14, 15])
        XCTAssertEqual(AeroricTypeScale.Step.allCases.map { AeroricTypeScale.Kind.code.size($0) }, [11, 12, 13, 14])
        XCTAssertEqual(AeroricTypeScale.Step.allCases.map { AeroricTypeScale.Kind.terminal.size($0) }, [12, 13, 15, 17])
        // 2026-10-28: the session's own size starts larger than the interface's.
        XCTAssertEqual(AeroricTypeScale.Step.allCases.map { AeroricTypeScale.Kind.conversation.size($0) },
                       [13, 15, 17, 19])
        XCTAssertEqual(AeroricTypeScale(defaults: scratchDefaults()).conversationSize, 15)
    }

    func testDefaultsToMediumAndUnknownValuesFallBack() {
        let defaults = scratchDefaults()
        defaults.set("gigantic", forKey: AeroricTypeScale.Kind.code.defaultsKey)
        let scale = AeroricTypeScale(defaults: defaults)
        XCTAssertEqual(scale.text, .medium)
        XCTAssertEqual(scale.code, .medium, "an unknown stored value falls back to medium")
        XCTAssertEqual(scale.textSize, 13)
        XCTAssertEqual(scale.terminalSize, 13)
    }

    func testSettingAStepPersistsAndReloads() {
        let defaults = scratchDefaults()
        let scale = AeroricTypeScale(defaults: defaults)
        scale.set(.large, for: .terminal)
        scale.set(.small, for: .text)
        XCTAssertEqual(scale.terminalSize, 15)
        let reloaded = AeroricTypeScale(defaults: defaults)
        XCTAssertEqual(reloaded.terminal, .large)
        XCTAssertEqual(reloaded.textSize, 12)
    }

    func testDerivedSizesAtTheDefaultStepMatchThePreviousConstants() throws {
        guard AeroricTypeScale.shared.text == .medium, AeroricTypeScale.shared.code == .medium else {
            throw XCTSkip("the shared scale has a non-default stored step on this machine")
        }
        XCTAssertEqual(AeroricType.body, 13)
        XCTAssertEqual(AeroricType.secondary, 12)
        XCTAssertEqual(AeroricType.caption, 11)
        XCTAssertEqual(AeroricType.code, 12)
        XCTAssertEqual(AeroricType.headline, 15, "headings do not scale")
    }
}
