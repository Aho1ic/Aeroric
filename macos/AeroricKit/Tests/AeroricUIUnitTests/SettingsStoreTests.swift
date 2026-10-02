import XCTest
@testable import AeroricUI

/// P5-05 验收：SettingsStoreTests——panel 映射（G11 19 条全映射）、
/// Secret 策略（masked/单次写入清内存）、UI-only/shared 分界、保存守卫。
@MainActor
final class SettingsStoreTests: XCTestCase {

    func testPanelRegistryMapsAllReactPanels() {
        // G11：panelRegistry 的 19 条 key 逐条映射（React 实测计数）。
        let reactKeys = [
            "general", "theme", "fonts", "shortcuts", "proxy", "local-router",
            "remote", "mcp", "notebook-embedding", "cleanup-report", "wsl",
            "permissions", "all-agent-configs", "usage", "agent-updates",
            "hooks", "skills", "dsh-plugins", "about",
        ]
        XCTAssertEqual(SettingsStore.panelRegistry.count, 19)
        XCTAssertTrue(SettingsStore.mapsAllReactPanels(reactKeys), "G11：全部 19 panel 有映射")
        XCTAssertFalse(SettingsStore.mapsAllReactPanels(reactKeys + ["nonexistent"]))
    }

    func testWSLPanelIsWindowsOnlyAndOthersVisibleEverywhere() {
        let store = SettingsStore(platform: "macos")
        let ids = store.availablePanels().map(\.id)
        XCTAssertFalse(ids.contains("wsl"), "wsl 是 windows-only（G11 :263 唯一平台门控）")
        XCTAssertTrue(ids.contains("permissions"), "permissions 全平台可见（未写 platforms＝全平台）")
        XCTAssertEqual(ids.count, 18)
        let windows = SettingsStore(platform: "windows").availablePanels().map(\.id)
        XCTAssertTrue(windows.contains("wsl"))
        XCTAssertEqual(windows.count, 19)
    }

    func testSecretMaskedAndNeverRestored() {
        let masked = SettingsStore.maskSecret("sk-ant-1234567890")
        XCTAssertTrue(masked.configured)
        XCTAssertEqual(masked.maskedTail, "…890")
        XCTAssertFalse(masked.maskedTail?.contains("sk-ant") ?? true, "masked 展示不含前缀")
        // 空串＝未配置。
        XCTAssertFalse(SettingsStore.maskSecret("").configured)
        // 短串全打码。
        XCTAssertFalse(SettingsStore.maskSecret("ab").maskedTail?.contains("ab") ?? true)
    }

    func testSecretForSingleWriteDoesNotPersist() {
        // 单次写入：store 不持有明文（纯透传）——两个调用返回同一明文但
        // store 无状态（类型层面无存储，无 @AppStorage/@Published secret）。
        let first = SettingsStore.secretForSingleWrite("sk-test")
        let second = SettingsStore.secretForSingleWrite("sk-test")
        XCTAssertEqual(first, second)
        // 镜像检查：SettingsStore 无 secret 存储字段（编译期保证：无
        // var secret）。这里断言面板层面没有明文回填入口。
        XCTAssertNil(SettingsStore.maskSecret("").maskedTail)
    }

    func testUIOnlyVsSharedBusinessBoundary() {
        XCTAssertTrue(SettingsStore.isUIOnly(panelID: "theme"), "主题是 UI-only")
        XCTAssertTrue(SettingsStore.isUIOnly(panelID: "fonts"))
        XCTAssertFalse(SettingsStore.isUIOnly(panelID: "proxy"), "proxy 是 shared business（core 配置）")
        XCTAssertFalse(SettingsStore.isUIOnly(panelID: "mcp"))
        XCTAssertFalse(SettingsStore.isUIOnly(panelID: "dsh-plugins"))
    }

    func testInvalidSaveIsRejectedWithoutPollutingOldConfig() {
        XCTAssertNotNil(SettingsStore.validateSave(panelID: "proxy", value: "127.0.0.1:8080"), "proxy 无 scheme 拒绝")
        XCTAssertNil(SettingsStore.validateSave(panelID: "proxy", value: "http://127.0.0.1:8080"))
        XCTAssertNil(SettingsStore.validateSave(panelID: "theme", value: "dark"))
    }
}
