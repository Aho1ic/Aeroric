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
            config("agent-config:via-codex-0123456789abcdef", harness: "codex"),
            config("agent-config:ds", harness: "codex"),
            config("claude", harness: "claude"),
        ]
        let menu = ComposerPickerOptions.configurations(catalogue, draft: NewSessionDraft(), harness: "codex")
        XCTAssertEqual(menu.sections.map(\.title), ["本机登录", "我的连接", "Agent 预设"],
                       "2026-10-27: saved connections are offered directly")
        XCTAssertEqual(menu.sections[1].options.first?.detail, "经本机网关")
        XCTAssertEqual(menu.sections[2].options.first?.detail, "经本机网关 · 自定义配置")
        let remote = ComposerPickerOptions.configurations(catalogue, draft: NewSessionDraft(), harness: "codex",
                                                          remote: true)
        XCTAssertEqual(remote.sections.map(\.title), ["经本机网关 · 我的连接", "经本机网关 · Agent 预设", "远程自带登录"],
                       "G4: presets first, the remote machine's own login last")
        XCTAssertEqual(remote.sections[2].options.first?.detail, "使用远程机器自己的账号")
        XCTAssertEqual(remote.sections[1].options.first?.detail, "经 SSH 转发到本机网关 · 自定义配置")
        XCTAssertEqual(LiveSessionFacts(agent: "codex", presetTitle: "DeepSeek",
                                        workingDirectory: "ssh://c1/srv/api").remoteGatewayBadge,
                       "经本机网关 · DeepSeek")
        XCTAssertNil(LiveSessionFacts(agent: "codex", presetTitle: "DeepSeek",
                                      workingDirectory: "/Users/me/api").remoteGatewayBadge)
        XCTAssertNil(LiveSessionFacts(agent: "codex", configurationTitle: "Codex",
                                      workingDirectory: "ssh://c1/srv/api").remoteGatewayBadge,
                     "the remote CLI's own login shows no gateway badge")
        let empty = ComposerPickerOptions.configurations(catalogue, draft: NewSessionDraft(), harness: "omp")
        XCTAssertTrue(empty.isEmpty)
        XCTAssertNotNil(empty.note)
    }

    func testBranchesSwitchOnlyOverACleanTree() {
        XCTAssertEqual(BranchSwitch.decision(changedFiles: 0), .allowed)
        guard case let .blocked(reason) = BranchSwitch.decision(changedFiles: 3) else { return XCTFail() }
        XCTAssertTrue(reason.contains("3"))
    }

    // MARK: - Sliding capsule picker (2026-10-27)

    func testTheSlidingPickerShowsTitlesAndSkipsDisabledNeighbours() {
        typealias Picker = SlidingCapsulePicker<String>
        let segments: [Picker.Segment] = [
            .init(value: "codex", title: "Codex", asset: "HarnessCodex"),
            .init(value: "claude", title: "Claude Code", asset: "HarnessClaude"),
            .init(value: "dsh", title: "DSH", asset: "HarnessDSH", isEnabled: false),
            .init(value: "omp", title: "oh-my-pi", asset: "HarnessOMP"),
            .init(value: "plain", title: "纯文字"),
        ]
        XCTAssertTrue(Picker.showsTitle(segments[0], selection: "codex", mode: .selectedOnly))
        XCTAssertFalse(Picker.showsTitle(segments[1], selection: "codex", mode: .selectedOnly))
        XCTAssertTrue(Picker.showsTitle(segments[4], selection: "codex", mode: .selectedOnly), "no icon: keep the title")
        XCTAssertTrue(Picker.showsTitle(segments[1], selection: "codex", mode: .always))
        XCTAssertEqual(Picker.neighbour(of: "claude", step: 1, in: segments), "omp", "a disabled choice is skipped")
        XCTAssertEqual(Picker.neighbour(of: "codex", step: -1, in: segments), nil)
    }
}
