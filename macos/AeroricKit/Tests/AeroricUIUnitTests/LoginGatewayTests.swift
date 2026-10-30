import AeroricClient
@testable import AeroricUI
import XCTest

/// 2026-10-30: the local login through the gateway, as the App shows it.
@MainActor
final class LoginGatewayTests: XCTestCase {
    func testTokenLineShowsTheExpiryOrTheProblem() {
        typealias Status = AgentConnectionsClient.LoginGatewayStatus
        XCTAssertEqual(AgentConfigText.loginTokenLine(Status(active: true)), "过期前自动续期")
        let line = AgentConfigText.loginTokenLine(Status(active: true, expiresAt: Date(timeIntervalSince1970: 0)))
        XCTAssertTrue(line.hasPrefix("令牌有效到 ") && line.hasSuffix(" · 过期前自动续期"), line)
        XCTAssertEqual(AgentConfigText.loginTokenLine(Status(active: true, problem: "本机登录已失效")), "本机登录已失效")
    }

    func testConsentNamesTheRightVendorAndTerms() {
        let codex = AgentConfigText.loginGatewayConsent("Codex")
        XCTAssertTrue(codex.contains("ChatGPT") && codex.contains("OpenAI"))
        let claude = AgentConfigText.loginGatewayConsent("Claude Code")
        XCTAssertTrue(claude.contains("Claude 登录") && claude.contains("Anthropic"))
        XCTAssertEqual(codex.split(separator: "\n").count, 3)
    }

    func testTheLoginSectionDropsTheMoveWarningOnTheGateway() {
        let configuration = SessionConfiguration(
            id: "codex", version: "1", title: "Codex", agent: "codex", model: nil, effort: nil,
            options: .init(agents: ["codex"], models: ["gpt-5.5-codex"], efforts: [], permissionModes: ["ask"]))
        var facts = LiveSessionFacts(agent: "codex", models: ["deepseek-v4.1-flash"])
        facts.modelSources = [LiveModelSource(configID: nil, title: "Codex", configuration: configuration)]
        let titles = { LivePickerOptions.allModels(facts, current: nil).sections.compactMap(\.title) }
        XCTAssertTrue(titles().contains("本机登录 · 换过去时原样搬会话"))
        facts.loginOnGateway = true
        XCTAssertTrue(titles().contains("本机登录"))
    }

    func testLoginTroubleIsRecognisedOnlyForQuotaAndSignIn() {
        XCTAssertEqual(LiveNoticeRow.loginTrouble(in: "You've hit your usage limit. Try again later."), "本机登录额度已用完或被限流")
        XCTAssertEqual(LiveNoticeRow.loginTrouble(in: "unexpected status 401 Unauthorized"), "本机登录需要重新登录")
        XCTAssertNil(LiveNoticeRow.loginTrouble(in: "stream disconnected before completion"))
    }
}
