import XCTest
@testable import AeroricUI

final class BuiltinSkillsTests: XCTestCase {
    func testEachHarnessCallsASkillItsOwnWay() {
        XCTAssertEqual(BuiltinSkillBook.invocation(skill: "systematic-debugging", harness: "codex", direct: true, shadowed: false),
                       "$systematic-debugging")
        XCTAssertEqual(BuiltinSkillBook.invocation(skill: "systematic-debugging", harness: "claude", direct: true, shadowed: false),
                       "/aeroric:systematic-debugging")
        XCTAssertEqual(BuiltinSkillBook.invocation(skill: "systematic-debugging", harness: "claude", direct: false, shadowed: false),
                       "/systematic-debugging", "a managed home has the skill unprefixed")
        XCTAssertEqual(BuiltinSkillBook.invocation(skill: "brainstorming", harness: "claude", direct: true, shadowed: true),
                       "/brainstorming", "the user's own skill of that name answers")
        XCTAssertEqual(BuiltinSkillBook.invocation(skill: "peer-review", harness: "omp", direct: true, shadowed: false),
                       "/peer-review")
    }

    @MainActor
    func testAnOffOrUnknownSkillFallsBackToThePlainTemplate() {
        let book = BuiltinSkillBook()
        XCTAssertNil(book.invocation(skill: "brainstorming", harness: "codex", configurationID: nil))
    }

    func testTheScienceGroupHasThirteenCardsAndDevelopCardsCallTheirSkills() {
        XCTAssertEqual(Scenario.science.recipes.count, 13)
        XCTAssertEqual(Scenario.science.title, "科研")
        XCTAssertTrue(Scenario.science.recipes.allSatisfy { $0.skill != nil })
        XCTAssertEqual(ScenarioCatalog.recipes.first { $0.id == "debug" }?.skill, "systematic-debugging")
        XCTAssertNil(ScenarioCatalog.recipes.first { $0.id == "ci" }?.skill, "cards without a skill stay plain")
    }

    func testACardWithASkillTypesTheCallBeforeItsTemplate() {
        let card = ScenarioCatalog.recipes.first { $0.id == "debug" }!
        let text = ScenarioCatalog.merged("", with: card, as: .replace, invocation: "$systematic-debugging")
        XCTAssertTrue(text.hasPrefix("$systematic-debugging "))
        XCTAssertTrue(text.hasSuffix(card.template))
        XCTAssertEqual(ScenarioCatalog.merged("", with: card, as: .replace), card.template)
    }

    @MainActor
    func testAPanelSkillWithoutACardGetsAPlainOne() {
        let item = BuiltinSkillBook.Item(id: "writing-skills", group: "develop", title: "编写技能", detail: "d", symbol: "s")
        let recipe = ScenarioCatalog.recipe(forSkill: item)
        XCTAssertEqual(recipe.skill, "writing-skills")
        XCTAssertNotNil(recipe.placeholderRange)
        let card = BuiltinSkillBook.Item(id: "experimental-design", group: "science", title: "实验设计", detail: "d", symbol: "s")
        XCTAssertEqual(ScenarioCatalog.recipe(forSkill: card).id, "experiment")
    }

    func testStatusLines() {
        var item = BuiltinSkillBook.Item(id: "x", group: "science", title: "t", detail: "d", symbol: "s", needs: ["python"])
        XCTAssertNil(BuiltinSkillBook.status(item))
        item.missing = ["python"]
        XCTAssertEqual(BuiltinSkillBook.status(item), "需要 Python")
        item.shadowedBy = ["codex"]
        XCTAssertEqual(BuiltinSkillBook.status(item), "Codex 里已被你的同名技能取代")
    }
}
