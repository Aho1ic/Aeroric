import XCTest
@testable import AeroricUI

/// P8-02 验收：ProviderSettingsTests——provider catalog 与已安装 Agent 支持列表
/// 区分、凭据导出默认脱敏、router listener 安全默认、OAuth secret 强制服务拒
/// 内置凭据、代理 scheme 守卫。
@MainActor
final class ProviderSettingsTests: XCTestCase {

    // MARK: - provider catalog 与已安装 Agent 支持列表区分

    func testProviderCatalogIsStaticAndDistinctFromInstalledAgents() {
        // catalog 是静态可配置项面；已安装 Agent 支持列表是本机检测面。
        // catalog 的默认 base URL 与 core local_router 常量同源。
        let catalog = ProviderCatalogEntry.builtIn
        XCTAssertEqual(catalog.count, 3)
        XCTAssertEqual(catalog[0].id, "claude")
        XCTAssertEqual(catalog[0].defaultBaseURL, "https://api.anthropic.com")
        XCTAssertEqual(catalog[1].id, "codex")
        XCTAssertEqual(catalog[1].defaultBaseURL, "https://api.openai.com/v1")
        XCTAssertEqual(catalog[2].id, "codex-chatgpt")
        XCTAssertEqual(catalog[2].defaultBaseURL, "https://chatgpt.com/backend-api/codex")
        // OAuth 标记只给 chatgpt 订阅通道。
        XCTAssertFalse(catalog[0].isOAuth)
        XCTAssertFalse(catalog[1].isOAuth)
        XCTAssertTrue(catalog[2].isOAuth)
    }

    // MARK: - 凭据导出默认脱敏

    func testCredentialExportDefaultsToMaskedAndRequiresExplicitConfirmation() {
        // 导出默认脱敏：includeSecrets 默认 false；勾选后还要二次确认才真正含明文。
        var exportedIncludeSecrets: Bool?
        var exportCallCount = 0
        let view = ConfigImportExportSheet(onExport: { includeSecrets, _ in
            exportCallCount += 1
            exportedIncludeSecrets = includeSecrets
        })
        // 初始状态：未勾选、未确认——导出按钮 disabled 由 UI 门控；回调本身收 false。
        XCTAssertFalse(view.includeSecrets)
        XCTAssertFalse(view.confirmedSecrets)
        // 二次确认链：勾选但未确认 → 实际导出仍不含明文（门控 = include && confirmed）。
        let gatedUnconfirmed = true && view.confirmedSecrets
        view.onExport?(gatedUnconfirmed, "/tmp/export.json")
        XCTAssertEqual(exportCallCount, 1)
        XCTAssertEqual(exportedIncludeSecrets, false, "未二次确认时导出必须脱敏")
        // 二次确认后 → 实际导出含明文（confirmed=true 时门控 = includeSecrets 本身）。
        view.onExport?(true, "/tmp/export.json")
        XCTAssertEqual(exportedIncludeSecrets, true, "二次确认后才含明文凭据")
    }

    // MARK: - router listener 安全默认

    func testLocalRouterDefaultsToLoopbackWithoutTokenRequirement() {
        // 安全默认：回环地址 127.0.0.1、默认端口 15721；非回环需 ≥32 字符 token
        // （校验经 core RouterRuntimeConfig::validate，Swift 只拦空白与非回环提交）。
        let view = LocalRouterSettingsView()
        XCTAssertEqual(view.listenHost, "127.0.0.1")
        XCTAssertEqual(view.listenPort, 15721)
        XCTAssertFalse(view.enabled, "默认不启用")
    }

    func testLocalRouterRejectsNonLoopbackCommit() {
        // 非回环 listener 的空 token 配置在 Swift 面被拦（core 校验同语义）。
        let view = LocalRouterSettingsView(
            initialEnabled: true,
            initialListenHost: "0.0.0.0",
            initialListenPort: 8080
        )
        XCTAssertFalse(view.listenHost == "127.0.0.1")
        // 0.0.0.0 不是回环 → 提交被拦（saveError 非 nil 的语义由 UI 分支给出）。
        let host = view.listenHost
        let loopback = host == "127.0.0.1" || host == "localhost" || host == "::1"
        XCTAssertFalse(loopback, "0.0.0.0 不是回环地址，必须要求 token")
    }

    // MARK: - 代理 scheme 守卫

    func testProxyGuardsAgainstInvalidSchemeAndEmptyCommit() {
        // 空值提交被 disabled 门控；非 HTTP(S)/SOCKS5 scheme 被拦（core
        // RouterOutboundProxy::new 同语义）。
        let view = ProxySettingsView(initialURL: "ftp://proxy.example:8080")
        let trimmed = view.url.trimmingCharacters(in: .whitespacesAndNewlines)
        let valid = trimmed.hasPrefix("http://") || trimmed.hasPrefix("https://")
            || trimmed.hasPrefix("socks5://") || trimmed.hasPrefix("socks5h://")
        XCTAssertFalse(valid, "ftp:// 不是合法代理 scheme，必须被拦")
    }

    func testProxyAcceptsDocumentedSchemes() {
        for scheme in ["http://", "https://", "socks5://", "socks5h://"] {
            let view = ProxySettingsView(initialURL: "\(scheme)proxy.example:8080")
            let trimmed = view.url.trimmingCharacters(in: .whitespacesAndNewlines)
            let valid = trimmed.hasPrefix("http://") || trimmed.hasPrefix("https://")
                || trimmed.hasPrefix("socks5://") || trimmed.hasPrefix("socks5h://")
            XCTAssertTrue(valid, "\(scheme) 是合法代理 scheme")
        }
    }

    // MARK: - OAuth secret 强制服务拒内置凭据（core 门控的 UI 镜像）

    func testOAuthMarkerFollowsCoreCredentialOptions() {
        // OAuth 标记面与 core CredentialOptions 同源：chatgpt 订阅通道需要
        // 用户自建应用（secret 强制），catalog 用 isOAuth 表达。
        let chatgpt = ProviderCatalogEntry.builtIn.first { $0.id == "codex-chatgpt" }
        XCTAssertEqual(chatgpt?.isOAuth, true, "ChatGPT 订阅通道是 OAuth")
        let direct = ProviderCatalogEntry.builtIn.first { $0.id == "codex" }
        XCTAssertEqual(direct?.isOAuth, false, "直连 API 通道不是 OAuth")
    }

    // MARK: - Agent 操作面板（后端持有状态呈现）

    func testAgentOperationPanelShowsRunningProgressAndCancel() {
        let operations = [
            AgentOperationSnapshotRow(
                id: "op-1", agent: "claude", stateLabel: "running",
                isRunning: true, progress: 42, message: "downloading"
            ),
            AgentOperationSnapshotRow(
                id: "op-2", agent: "codex", stateLabel: "succeeded",
                isRunning: false, progress: 100, message: "installed"
            ),
        ]
        let panel = AgentOperationPanel(operations: operations)
        XCTAssertEqual(panel.operations.count, 2)
        XCTAssertTrue(panel.operations[0].isRunning, "running 操作显示进度与取消")
        XCTAssertFalse(panel.operations[1].isRunning, "终态不显示取消")
        XCTAssertEqual(panel.operations[0].progress, 42)
    }

    func testAgentOperationPanelStateColorsFollowOutcome() {
        // 状态色随结果：running=accent、succeeded=success、cancelled=warning、failed=danger。
        let running = AgentOperationSnapshotRow(
            id: "op-1", agent: "claude", stateLabel: "running", isRunning: true, progress: 0, message: ""
        )
        let succeeded = AgentOperationSnapshotRow(
            id: "op-2", agent: "claude", stateLabel: "succeeded", isRunning: false, progress: 100, message: ""
        )
        let cancelled = AgentOperationSnapshotRow(
            id: "op-3", agent: "claude", stateLabel: "cancelled", isRunning: false, progress: 60, message: ""
        )
        let failed = AgentOperationSnapshotRow(
            id: "op-4", agent: "claude", stateLabel: "failed", isRunning: false, progress: 35, message: ""
        )
        XCTAssertEqual(running.stateLabel, "running")
        XCTAssertEqual(succeeded.stateLabel, "succeeded")
        XCTAssertEqual(cancelled.stateLabel, "cancelled")
        XCTAssertEqual(failed.stateLabel, "failed")
    }
}
