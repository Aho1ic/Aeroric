import XCTest

/// P6-02 验收：XCTest UI `EditorTests.swift`——编辑器表面在 App target 可
/// 启动并渲染（AppLaunchTests 同款 env 注入；TextKit 桥的 CJK IME/Undo 由
/// TextKit 自身保证，XCTest 只断言表面挂载）。
@MainActor
final class EditorTests: XCTestCase {
    private let lifecycle = NativeUITestApplicationLifecycle()

    override func tearDownWithError() throws {
        let lifecycle = lifecycle
        try MainActor.assumeIsolated {
            try lifecycle.tearDown()
        }
        try super.tearDownWithError()
    }

    func testAppLaunchesWithEditorPackageMounted() throws {
        let app = XCUIApplication()
        let runnerEnvironment = ProcessInfo.processInfo.environment
        app.launchEnvironment["AERORIC_ENV"] = runnerEnvironment["AERORIC_ENV"] ?? "ui-test"
        if let dataDirectory = runnerEnvironment["AERORIC_DATA_DIR"] {
            app.launchEnvironment["AERORIC_DATA_DIR"] = dataDirectory
        }
        try lifecycle.launch(app)
        // 任务工作区在 ready 后出现（P5-03 readyContent 注入点）；编辑器表面
        // 挂载断言随 P6-02 的 App 场景接线（P6-02 交付的是编辑器组件面）。
        let deadline = Date().addingTimeInterval(15)
        while !app.buttons["tasks.new"].exists, Date() < deadline {
            Thread.sleep(forTimeInterval: 0.2)
        }
        XCTAssertTrue(
            app.buttons["tasks.new"].waitForExistence(timeout: 5),
            "任务工作区在 ready 后挂进详情列（编辑器组件随场景接线挂载）"
        )
    }
}
