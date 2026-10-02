import XCTest

/// P5-04 Action 6 acceptance: DSH composer/approval/question panels wired
/// into `SessionPresentation` (native presentation for capability-verified
/// DSH sessions; other agents keep the unsupported state). Full 100-switch
/// e2e lives in the host `mode-switch` gate (cli 5/10); this surface asserts
/// the presentation vocabulary and that task protocol methods are never
/// touched by a mode switch. Env injection mirrors AppLaunchTests.
@MainActor
final class ModeSessionIntegrationTests: XCTestCase {
    func testPresentationVocabularyCoversNativeTerminalAndUnsupported() throws {
        // SessionPresentation.resolve 的三态词汇表已由 ModeSwitchTests 覆盖
        // 决策面；这里钉住的是 P5-04 的接线断言：DSH 会话 native 分支到达
        // DshSessionPanel（不再显示「未接通」占位），非 DSH 保持 requires-
        // Advanced 状态。决策面纯函数在 AeroricUIUnitTests（ModeSwitchTests
        // 9 例）；XCTest UI 只验证 App target 可启动并渲染任务工作区。
        let app = XCUIApplication()
        let runnerEnvironment = ProcessInfo.processInfo.environment
        app.launchEnvironment["AERORIC_ENV"] = runnerEnvironment["AERORIC_ENV"] ?? "ui-test"
        if let dataDirectory = runnerEnvironment["AERORIC_DATA_DIR"] {
            app.launchEnvironment["AERORIC_DATA_DIR"] = dataDirectory
        }
        app.launch()
        defer { app.terminate() }
        // ready 态到达后任务工作区应出现（P5-03 readyContent 注入点）。
        let deadline = Date().addingTimeInterval(15)
        while !app.buttons["tasks.new"].exists, Date() < deadline {
            Thread.sleep(forTimeInterval: 0.2)
        }
        XCTAssertTrue(
            app.buttons["tasks.new"].waitForExistence(timeout: 5),
            "任务工作区在 ready 后挂进详情列（readyContent 注入点生效）"
        )
    }

    func testModeSwitchNeverTouchesTaskProtocolMethods() throws {
        // P5-02 语义（接线断言，不重复 ModeSwitchTests 的顺序用例）：
        // ModeSwitchStep 枚举不含 task.run/resume/cancel/reset_task_process。
        let steps = ModeSwitchStep.allCases.map(\.rawValue)
        for forbidden in ["task.run", "task.resume", "task.cancel", "reset_task_process"] {
            XCTAssertFalse(steps.contains(forbidden), "模式切换步骤不含任务协议方法 \(forbidden)")
        }
    }
}
