import XCTest
@testable import AeroricUI

/// P5-05 验收：SettingsCapabilityTests——四族别名、OMP 不支持的 effort
/// 不发、升级渠道确认、离线模型目录提示。
final class SettingsCapabilityTests: XCTestCase {

    func testBuiltinAgentsCoverAllFourFamilies() {
        let families = Set(SettingsCapability.builtinAgents.map(\.family))
        XCTAssertEqual(families, ["claude", "codex", "dsh", "omp"], "四族记录一致")
    }

    func testOMPDoesNotReceiveUnsupportedEffort() {
        // Action 5：原生设置不发 OMP 不支持的 effort（无 ultra）。
        XCTAssertFalse(SettingsCapability.ompThinkingLevels.contains("ultra"), "OMP 无 ultra")
        XCTAssertTrue(SettingsCapability.supportsEffort("max", family: "omp"))
        XCTAssertFalse(SettingsCapability.supportsEffort("ultra", family: "omp"))
    }

    func testFamilyOfAgentID() {
        XCTAssertEqual(SettingsCapability.familyOf(agentID: "claude", declared: nil), "claude")
        XCTAssertEqual(SettingsCapability.familyOf(agentID: "omp", declared: nil), "omp")
        XCTAssertNil(SettingsCapability.familyOf(agentID: "custom-x", declared: nil), "未知 id 无声明＝nil")
        XCTAssertEqual(SettingsCapability.familyOf(agentID: "custom-x", declared: "claude"), "claude")
    }

    func testDshUsesBuiltinOffHighMaxVocabulary() {
        XCTAssertEqual(SettingsCapability.dshEffortLevels, ["off", "high", "max"])
        XCTAssertEqual(SettingsCapability.effortLevels(for: "dsh"), ["off", "high", "max"])
        for effort in ["off", "high", "max"] {
            XCTAssertTrue(SettingsCapability.supportsEffort(effort, family: "dsh"), effort)
        }
        for effort in ["", "low", "medium", "xhigh", "ultra"] {
            XCTAssertFalse(SettingsCapability.supportsEffort(effort, family: "dsh"), effort)
        }
    }

    func testDshVocabularyDoesNotChangeOtherFamilyChoicesOrPermitStaleSelections() {
        for family in ["claude", "codex"] {
            XCTAssertEqual(SettingsCapability.effortLevels(for: family), ["low", "medium", "high"])
            for effort in ["low", "medium", "high"] {
                XCTAssertTrue(SettingsCapability.supportsEffort(effort, family: family))
            }
            for effort in ["", "off", "max"] {
                XCTAssertFalse(SettingsCapability.supportsEffort(effort, family: family))
            }
        }
        XCTAssertTrue(SettingsCapability.supportsEffort("max", family: "omp"))
        XCTAssertFalse(SettingsCapability.supportsEffort("", family: "omp"))
        XCTAssertFalse(SettingsCapability.supportsEffort("high", family: "unknown"))
        XCTAssertEqual(SettingsCapability.effortLevels(for: "omp"), SettingsCapability.ompThinkingLevels)
        XCTAssertEqual(SettingsCapability.effortLevels(for: "unknown"), [])
    }

    func testUpdateRequiresExplicitUserConfirmation() {
        XCTAssertFalse(SettingsCapability.canStartUpdate(channel: "npm", confirmedByUser: false), "不因打开设置自动 upgrade")
        XCTAssertTrue(SettingsCapability.canStartUpdate(channel: "npm", confirmedByUser: true))
        XCTAssertFalse(SettingsCapability.canStartUpdate(channel: "unknown", confirmedByUser: true))
        XCTAssertEqual(SettingsCapability.updateChannels, ["npm", "homebrew", "native"])
    }

    func testOfflineModelCatalogShowsSourceHintNotFakeOnline() {
        XCTAssertNil(SettingsCapability.modelCatalogHint(lastCheckedAtMs: nil, isOnline: true))
        XCTAssertNotNil(
            SettingsCapability.modelCatalogHint(lastCheckedAtMs: nil, isOnline: false),
            "离线无时间＝来源与更新时间未知（不显示虚假在线结果）"
        )
        let hint = SettingsCapability.modelCatalogHint(lastCheckedAtMs: 1_700_000_000_000, isOnline: false)
        XCTAssertEqual(hint, "模型目录离线：最后更新 \(Date(timeIntervalSince1970: 1_700_000_000).formatted(date: .abbreviated, time: .shortened))")
    }
}
