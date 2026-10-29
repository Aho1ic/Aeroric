@testable import AeroricUI
import AeroricClient
import XCTest

/// Project and session tags (2026-10-29).
final class TagBookTests: XCTestCase {
    private let important = TagRecord(id: "t1", name: "重要", color: "red")
    private let research = TagRecord(id: "t2", name: "调研", color: "purple", projectId: "p1")

    func testAProjectOnlyTagIsOfferedToThatProjectsSessionsOnly() {
        let tags = [important, research]
        XCTAssertEqual(TagBook.available(tags, isProject: false, projectID: "p1").map(\.id), ["t1", "t2"])
        XCTAssertEqual(TagBook.available(tags, isProject: false, projectID: "p2").map(\.id), ["t1"])
        XCTAssertEqual(TagBook.available(tags, isProject: false, projectID: nil).map(\.id), ["t1"],
                       "an unfiled import gets 所有会话 tags only")
        XCTAssertEqual(TagBook.available(tags, isProject: true, projectID: "p1").map(\.id), ["t1", "t2"],
                       "a project carries its own 仅本项目 tags (Q1-a)")
        XCTAssertEqual(TagBook.available(tags, isProject: true, projectID: "p2").map(\.id), ["t1"])
    }

    func testFilteringByATaggedProjectShowsAllItsSessions() {
        let library = TagLibrary(tags: [important, research],
                                 assignments: ["p1": ["t1"], "task.b": ["t2"], "imported.x": ["t1"]])
        let byProject = TagBook.filter(project: "p1", sessions: ["task.a", "task.b"], by: "t1", in: library)
        XCTAssertTrue(byProject.showsProject)
        XCTAssertEqual(byProject.sessions, ["task.a", "task.b"])
        let bySession = TagBook.filter(project: "p1", sessions: ["task.a", "task.b"], by: "t2", in: library)
        XCTAssertEqual(bySession.sessions, ["task.b"])
        let none = TagBook.filter(project: "p2", sessions: ["task.c"], by: "t2", in: library)
        XCTAssertFalse(none.showsProject)
    }

    func testARowsTagsFollowLibraryOrder() {
        let library = TagLibrary(tags: [important, research], assignments: ["task.a": ["t2", "t1"]])
        XCTAssertEqual(TagBook.tags(on: "task.a", in: library).map(\.name), ["重要", "调研"])
        XCTAssertTrue(TagBook.tags(on: "task.none", in: library).isEmpty)
    }

    /// Narrowing to a project counts the rows that would lose the tag (Q2);
    /// widening loses nothing.
    func testNarrowingAScopeCountsTheRowsElsewhere() {
        let library = TagLibrary(tags: [important],
                                 assignments: ["task.a": ["t1"], "task.b": ["t1"], "p1": ["t1"], "imported.x": ["t1"]],
                                 rowProjects: ["task.a": "p1", "task.b": "p2", "p1": "p1"])
        XCTAssertEqual(TagBook.impact(of: "t1", scope: "p1", in: library), 2, "task.b and the unfiled import")
        XCTAssertEqual(TagBook.impact(of: "t1", scope: nil, in: library), 0)
    }
}
