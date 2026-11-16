import AeroricClient
import AeroricProtocol
@testable import AeroricUI
import XCTest

/// 2026-11-13 Smart Friend: 设置 → Agents → 求助专家.
@MainActor
final class ExpertStoreTests: XCTestCase {
    private struct Refused: Error, CustomStringConvertible {
        var description: String { "这个配置没有模型 opus-9" }
    }

    /// The host's expert, kept by the fake calls.
    private actor Host {
        var expert: LiveExpert?
        func set(_ expert: LiveExpert?) { self.expert = expert }
    }

    private let choices = [
        LiveExpertChoice(configurationId: "agent-config:a", title: "Claude 官方", agent: "claude",
                         models: ["sonnet", "opus"]),
        LiveExpertChoice(configurationId: "codex", title: "Codex", agent: "codex"),
    ]

    func testTheExpertAndItsChoicesDecode() {
        let expert = LiveExpert.decode(.object([
            "configurationId": .string("agent-config:a"), "agent": .string("claude"),
            "model": .string("opus"), "label": .string("Claude 官方 · opus"),
        ]))
        XCTAssertEqual(expert?.label, "Claude 官方 · opus")
        XCTAssertNil(LiveExpert.decode(.null))
        let choice = LiveExpertChoice.decode(.object([
            "configurationId": .string("codex"), "title": .string("Codex"), "agent": .string("codex"),
            "models": .array([]),
        ]))
        XCTAssertEqual(choice?.models, [])
        XCTAssertEqual(choice.map(ExpertText.models), [nil])
    }

    func testSettingAModelMakesItTheExpertAndClearingTakesItAway() async {
        let host = Host()
        let choices = self.choices
        let store = ExpertStore()
        XCTAssertFalse(store.isAvailable)
        store.attach(ExpertStore.Calls(
            load: { (await host.expert, choices) },
            set: { id, model in
                if model == "opus-9" { throw Refused() }
                let title = choices.first { $0.configurationId == id }?.title ?? id
                await host.set(LiveExpert(configurationId: id, agent: "claude", model: model,
                                          label: model.map { "\(title) · \($0)" } ?? title))
                return (await host.expert, choices)
            },
            clear: {
                await host.set(nil)
                return (nil, choices)
            }))
        await store.refresh()
        XCTAssertTrue(store.isAvailable)
        XCTAssertEqual(ExpertText.current(store.expert), "未设置")
        XCTAssertEqual(store.choices.count, 2)

        await store.set("agent-config:a", model: "opus")
        XCTAssertEqual(ExpertText.current(store.expert), "Claude 官方 · opus")
        XCTAssertTrue(ExpertText.isCurrent(store.expert, choices[0], model: "opus"))
        XCTAssertFalse(ExpertText.isCurrent(store.expert, choices[0], model: "sonnet"))

        await store.set("agent-config:a", model: "opus-9")
        XCTAssertEqual(store.lastError, "这个配置没有模型 opus-9")
        XCTAssertEqual(store.expert?.model, "opus", "a refused change keeps the expert")

        await store.clear()
        XCTAssertNil(store.expert)
        XCTAssertNil(store.lastError)
    }

    func testAHostWithoutTheMethodHidesTheCard() async {
        struct Missing: Error {}
        let store = ExpertStore()
        store.attach(ExpertStore.Calls(load: { throw Missing() }, set: { _, _ in throw Missing() },
                                       clear: { throw Missing() }))
        await store.refresh()
        XCTAssertFalse(store.isAvailable)
        store.attach(nil)
        await store.refresh()
        XCTAssertFalse(store.isAvailable)
    }
}
