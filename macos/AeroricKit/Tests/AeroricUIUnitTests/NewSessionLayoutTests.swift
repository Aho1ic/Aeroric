@testable import AeroricUI
import XCTest

@MainActor
final class NewSessionLayoutTests: XCTestCase {
    private func config(_ id: String, harness: String, blocked: [String] = []) -> SessionConfiguration {
        SessionConfiguration(id: id, version: "1", title: id, agent: harness, model: nil, effort: nil,
                             options: .init(agents: [harness], models: ["m"], efforts: [], permissionModes: ["ask"]),
                             harness: harness, blockedReasons: blocked)
    }

    func testAvailabilityAndTheDefaultHarness() {
        let catalogue = [
            config("codex", harness: "codex", blocked: ["未登录"]),
            config("agent-config:ds", harness: "codex"),
            config("claude", harness: "claude", blocked: ["未安装"]),
        ]
        let available = NewSessionLayout.availability(catalogue)
        XCTAssertEqual(available["codex"], true)
        XCTAssertEqual(available["claude"], false)
        XCTAssertEqual(available["omp"], false)
        XCTAssertEqual(NewSessionLayout.defaultHarness(preferred: "claude", availability: available), "codex")
        XCTAssertEqual(NewSessionLayout.defaultHarness(preferred: "codex", availability: available), "codex")
        XCTAssertEqual(NewSessionLayout.defaultHarness(preferred: "dsh", availability: [:]), "dsh",
                       "nothing usable keeps the preference")
    }

    func testSwitchingHarnessPicksTheBuiltInLoginFirst() {
        let catalogue = [
            config("agent-config:ds", harness: "codex"),
            config("codex", harness: "codex"),
            config("agent-config:bad", harness: "claude", blocked: ["没有渠道"]),
        ]
        XCTAssertEqual(NewSessionLayout.firstConfiguration(for: "codex", in: catalogue)?.id, "codex")
        XCTAssertNil(NewSessionLayout.firstConfiguration(for: "claude", in: catalogue))
    }

    func testThePickerSplitsLoginsFromGatewayConfigs() {
        let catalogue = [
            config("codex", harness: "codex"),
            config("agent-config:ds", harness: "codex"),
            config("claude", harness: "claude"),
        ]
        let menu = ComposerPickerOptions.configurations(catalogue, draft: NewSessionDraft(), harness: "codex")
        XCTAssertEqual(menu.sections.map(\.title), ["本机登录", "Agent 配置"])
        XCTAssertEqual(menu.sections[1].options.first?.detail, "经本机网关")
        let empty = ComposerPickerOptions.configurations(catalogue, draft: NewSessionDraft(), harness: "omp")
        XCTAssertTrue(empty.isEmpty)
        XCTAssertNotNil(empty.note)
    }

    func testBranchesSwitchOnlyOverACleanTree() {
        XCTAssertEqual(BranchSwitch.decision(changedFiles: 0), .allowed)
        guard case let .blocked(reason) = BranchSwitch.decision(changedFiles: 3) else { return XCTFail() }
        XCTAssertTrue(reason.contains("3"))
    }
}
