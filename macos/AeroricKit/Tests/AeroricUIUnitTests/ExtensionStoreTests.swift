import XCTest
@testable import AeroricUI

/// P8-01 验收：ExtensionStoreTests——信任判定与 core 同语义、env 掩码不回明文、
/// 待重启提示、hook 扩展名守卫、安装提议清账。
@MainActor
final class ExtensionStoreTests: XCTestCase {

    func testTrustVerdictMatchesCoreSemantics() {
        // 越界写入：确认也拒绝。
        XCTAssertEqual(
            ExtensionStore.trustVerdict(kind: "github", revision: "abc", writeScope: ["../bin"], userConfirmed: true),
            "rejected")
        // 第三方＋范围内：确认前 requiresConfirmation。
        XCTAssertEqual(
            ExtensionStore.trustVerdict(kind: "github", revision: "abc", writeScope: ["skills/pdf"], userConfirmed: false),
            "requiresConfirmation")
        XCTAssertEqual(
            ExtensionStore.trustVerdict(kind: "github", revision: "abc", writeScope: ["skills/pdf"], userConfirmed: true),
            "trusted")
        // builtin＋revision：无需确认。
        XCTAssertEqual(
            ExtensionStore.trustVerdict(kind: "builtin", revision: "1", writeScope: ["skills/x"], userConfirmed: false),
            "trusted")
        // builtin 无 revision：仍需确认。
        XCTAssertEqual(
            ExtensionStore.trustVerdict(kind: "builtin", revision: nil, writeScope: ["skills/x"], userConfirmed: false),
            "requiresConfirmation")
    }

    func testMaskedEnvNeverCarriesPlaintextIntoTheStore() {
        let store = ExtensionStore()
        // 宿主 mask_env 的产出（掩码）原样落地；store 无明文字段可写。
        store.ingestMcp([
            ExtensionStore.McpServer(
                name: "server1", command: "uvx", args: ["mcp-server"],
                maskedEnv: ["API_KEY": "••••••••", "LOG_LEVEL": "debug"], enabled: true),
        ])
        XCTAssertEqual(store.mcpServers[0].maskedEnv["API_KEY"], "••••••••")
        XCTAssertEqual(store.mcpServers[0].maskedEnv["LOG_LEVEL"], "debug")
        let mirrored = Mirror(reflecting: store.mcpServers[0]).children.compactMap(\.label)
        XCTAssertFalse(mirrored.contains { $0.lowercased().contains("plaintext") })
    }

    func testRestartNoticeAppearsOnSaveAndClearsExplicitly() {
        let store = ExtensionStore()
        XCTAssertFalse(store.pendingRestartNotice)
        store.markNeedsRestart()
        XCTAssertTrue(store.pendingRestartNotice, "配置保存后必须提示「下次启动生效」")
        store.clearRestartNotice()
        XCTAssertFalse(store.pendingRestartNotice)
    }

    func testHookScriptGuardFlagsNonJsTsEntries() {
        XCTAssertTrue(ExtensionStore.isHookScript("hook.js"))
        XCTAssertTrue(ExtensionStore.isHookScript("HOOK.TS"))
        XCTAssertFalse(ExtensionStore.isHookScript("hook.py"))
        XCTAssertFalse(ExtensionStore.isHookScript("hook.js.bak"))
    }

    func testInstallProposalClearsOnDecision() {
        let store = ExtensionStore()
        store.proposeInstall(ExtensionStore.PendingInstall(
            name: "pdf", origin: "github.com/x/y", revision: "abc",
            writeScope: ["skills/pdf"], permissions: ["read-files"]))
        XCTAssertNotNil(store.pendingInstall)
        store.clearInstallProposal()
        XCTAssertNil(store.pendingInstall)
    }
}
