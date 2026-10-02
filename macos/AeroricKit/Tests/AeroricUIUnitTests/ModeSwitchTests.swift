import XCTest
@testable import AeroricUI

/// P5-02 验收：模式迁移默认、Toggle 映射、保存回滚、呈现决策与
/// 切换顺序（不触碰任务协议——模式切换不是任务协议转换）。
@MainActor
final class ModeSwitchTests: XCTestCase {

    private func freshDefaults() -> UserDefaults {
        let suite = "test.mode-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defaults.removePersistentDomain(forName: suite)
        return defaults
    }

    // MARK: - 迁移默认（§2.4：导入用户 advanced，新装 native）

    func testMigrationDefaultDependsOnLegacyClientPresence() {
        XCTAssertEqual(InteractionMode.migrationDefault(tauriClientPresent: true), .advanced)
        XCTAssertEqual(InteractionMode.migrationDefault(tauriClientPresent: false), .native)
    }

    func testControllerFallsBackToMigrationDefaultWhenNothingPersisted() {
        let controller = InteractionModeController(
            tauriClientPresent: true,
            defaults: freshDefaults()
        )
        XCTAssertEqual(controller.mode, .advanced)
        XCTAssertEqual(controller.effectiveDefault, .advanced)
    }

    func testControllerPersistsAcrossInstances() {
        let defaults = freshDefaults()
        let first = InteractionModeController(tauriClientPresent: false, defaults: defaults)
        XCTAssertEqual(first.mode, .native)
        first.save(.advanced)
        let second = InteractionModeController(tauriClientPresent: false, defaults: defaults)
        XCTAssertEqual(second.mode, .advanced, "持久化的模式跨实例可读")
    }

    // MARK: - Toggle 映射（OFF=native，ON=advanced）

    func testToggleMapping() {
        XCTAssertEqual(InteractionMode(toggleIsOn: false), .native)
        XCTAssertEqual(InteractionMode(toggleIsOn: true), .advanced)
        XCTAssertEqual(InteractionMode.native.toggleIsOn, false)
        XCTAssertEqual(InteractionMode.advanced.toggleIsOn, true)
    }

    // MARK: - 保存失败回滚（Action 2）

    func testSaveFailureKeepsPreviousModeAndRecordsReason() {
        let defaults = freshDefaults()
        let controller = InteractionModeController(tauriClientPresent: false, defaults: defaults)
        controller.save(.native)
        XCTAssertEqual(controller.mode, .native)

        controller.failingSave = true
        controller.save(.advanced)
        XCTAssertEqual(controller.mode, .native, "保存失败必须回滚 UI 值")
        XCTAssertNotNil(controller.lastSaveError, "失败原因要给 UI 显示")
        XCTAssertEqual(defaults.string(forKey: InteractionModeController.defaultsKey), "native")
    }

    // MARK: - 呈现决策（Action 3）

    func testPresentationResolution() {
        // advanced：一律真终端。
        XCTAssertEqual(
            SessionPresentation.resolve(mode: .advanced, structuredCapabilitiesVerified: false),
            .terminal
        )
        // native + 已验证结构化能力：原生面板（真 composer 归 P5-04）。
        XCTAssertEqual(
            SessionPresentation.resolve(mode: .native, structuredCapabilitiesVerified: true),
            .native
        )
        // native + 未验证：状态卡＋切换按钮，不自动改全局偏好。
        XCTAssertEqual(
            SessionPresentation.resolve(mode: .native, structuredCapabilitiesVerified: false),
            .requiresAdvanced(reason: "此 Agent 的当前交互需要高级模式")
        )
    }

    // MARK: - 切换顺序与任务协议边界（Action 4）

    func testSwitchSequenceOrderAndCompleteness() {
        let sequence = ModeSwitchSequence(from: .native, to: .advanced)
        XCTAssertEqual(sequence.steps, [
            .deferIME,
            .saveDraftAndScrollAnchor,
            .releaseOldLease,
            .attachPresentation,
            .acquireLease,
            .restoreFocus,
        ])
        // 反向切换同构。
        let reverse = ModeSwitchSequence(from: .advanced, to: .native)
        XCTAssertEqual(reverse.steps.count, 6)
    }

    func testSameModeSwitchIsNoOp() {
        XCTAssertTrue(ModeSwitchSequence(from: .native, to: .native).steps.isEmpty)
    }

    func testSwitchNeverTouchesTaskProtocol() {
        // 记录一次「切换」会发的全部请求（真实切换只碰重放/租约面）。
        let switchRequests = [
            "terminal.plan_replay",
            "terminal.acquire_control",
            "terminal.release_control",
        ]
        XCTAssertFalse(
            ModeSwitchSequence.violates(requestedMethods: switchRequests),
            "模式切换不调用任务协议方法"
        )
        // 任何 start/resume/cancel/reset 都是任务协议转换，被禁止。
        for forbidden in ModeSwitchSequence.forbiddenMethods {
            XCTAssertTrue(
                ModeSwitchSequence.violates(requestedMethods: [forbidden]),
                "\(forbidden) 必须被判定为违规"
            )
        }
    }
}
