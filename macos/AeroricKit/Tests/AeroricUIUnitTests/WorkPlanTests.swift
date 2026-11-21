import AeroricProtocol
@testable import AeroricUI
import XCTest

private let listed: JSONValue = .object([
    "plan": .object([
        "id": .string("plan-1"), "title": .string("拆开做"), "confirmed": .bool(false),
        "units": .array([
            .object(["id": .string("a"), "title": .string("调研"), "dependsOn": .array([])]),
            .object(["id": .string("b"), "title": .string("实现"), "dependsOn": .array([.string("a")]),
                     "delegationId": .string("del-b")]),
        ]),
    ]),
    "statuses": .array([
        .object(["id": .string("a"), "state": .string("notStarted"), "reason": .string("计划还没确认")]),
        .object(["id": .string("b"), "state": .string("running"), "reason": .string("在运行"),
                 "inputsStale": .bool(true)]),
    ]),
])

/// D5 step 3 (2026-11-21): a work plan in the progress page's 委派 section.
final class WorkPlanTests: XCTestCase {
    func testAPlanReadsAsTheSectionShowsIt() throws {
        let plan = try XCTUnwrap(LiveWorkPlan.decode(listed))
        XCTAssertEqual(WorkPlanText.title(plan), "计划 · 拆开做（待你确认）")
        XCTAssertEqual(plan.units.map(\.state), ["notStarted", "running"])
        XCTAssertEqual(WorkPlanText.unit(plan.units[0], in: plan), "调研 · 未开始 · 计划还没确认")
        XCTAssertEqual(WorkPlanText.unit(plan.units[1], in: plan), "实现 · 进行中 · 在运行（依赖 调研）")
        XCTAssertEqual(plan.units[1].delegationId, "del-b")
        XCTAssertFalse(WorkPlanText.needsUser(plan.units[0]))
        XCTAssertTrue(WorkPlanText.needsUser(plan.units[1]), "its input went stale")
        XCTAssertEqual(WorkPlanText.symbol(plan.units[0]), "circle")
        XCTAssertEqual(WorkPlanText.state("waitingDeps"), "等待依赖")

        var confirmed = plan
        confirmed.confirmed = true
        XCTAssertEqual(WorkPlanText.title(confirmed), "计划 · 拆开做")
        XCTAssertNil(LiveWorkPlan.decode(.object(["plan": .object([:])])))
    }
}
