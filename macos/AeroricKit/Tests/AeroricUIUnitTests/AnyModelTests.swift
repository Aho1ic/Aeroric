@testable import AeroricUI
import XCTest

/// Any third-party model on any harness (2026-10-29).
@MainActor
final class AnyModelTests: XCTestCase {
    private func configuration(_ id: String, models: [String], connections: [String: String] = [:],
                               notes: [String: String] = [:]) -> SessionConfiguration {
        var configuration = SessionConfiguration(
            id: id, version: "1", title: id, agent: "codex", model: nil, effort: nil,
            options: .init(agents: ["codex"], models: models, efforts: [], permissionModes: []))
        configuration.modelConnections = connections
        configuration.modelNotes = notes
        return configuration
    }

    func testAllThirdPartyModelsGroupByConnectionWithOneLineNotes() {
        let all = configuration("agent-config:via-codex-all", models: ["deepseek", "gpt", "glm"],
                                connections: ["deepseek": "cline", "gpt": "CPA", "glm": "CPA"],
                                notes: ["glm": "不支持图片"])
        let sections = ComposerPickerOptions.groupedModels(all) { $0 == "gpt" }
        XCTAssertEqual(sections.map(\.title), ["cline", "CPA"])
        XCTAssertEqual(sections[1].options.map(\.title), ["gpt", "glm"])
        XCTAssertEqual(sections[1].options[1].trailing, "不支持图片", "the note rides on the same line")
        XCTAssertNil(sections[1].options[1].detail, "no second line")
        XCTAssertTrue(sections[1].options[0].selected)
    }

    func testOtherSourcesAreOfferedAndTaggedForAMove() {
        var facts = LiveSessionFacts(agent: "codex", models: ["deepseek"])
        facts.onLogin = false
        facts.modelSources = [
            LiveModelSource(configID: "via-codex-all", title: "全部第三方模型",
                            configuration: configuration("agent-config:via-codex-all", models: ["deepseek", "gpt"],
                                                         connections: ["deepseek": "cline", "gpt": "CPA"])),
            LiveModelSource(configID: nil, title: "Codex", configuration: configuration("codex", models: ["gpt-5.5"])),
        ]
        let menu = LivePickerOptions.allModels(facts, current: "deepseek")
        let ids = menu.sections.flatMap { $0.options.map(\.id) }
        XCTAssertEqual(ids.first, "deepseek", "the current source comes first, as plain ids")
        XCTAssertFalse(ids.dropFirst().contains { $0.hasSuffix("deepseek") }, "a model already here is not repeated")
        let gpt = ids.first { $0.hasSuffix("gpt") }!
        XCTAssertEqual(LivePickerOptions.moveTarget(gpt)?.source, "via-codex-all")
        let login = ids.first { $0.hasSuffix("gpt-5.5") }!
        XCTAssertEqual(LivePickerOptions.moveTarget(login)?.source, "builtin")
        XCTAssertEqual(menu.sections.last?.title, "本机登录 · 换过去时原样搬会话")
        XCTAssertNil(LivePickerOptions.moveTarget("deepseek"))
    }
}
