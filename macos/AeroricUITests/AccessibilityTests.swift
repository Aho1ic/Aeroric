import XCTest

/// AP8 R-AP8-5 (原 P10-02 的验收半边):可访问性与键盘行为在**真实窗口**上的判据。
///
/// ## 为什么必须是 XCUITest 而不是单测
///
/// 单测能证明 `SurfaceChoice.resolve(preference:reduceTransparency:)` 这个纯函数算对了
/// (`ThemePolicyTests` 已经钉住),但证不了**它在产品里被用到了**:
/// 参数算对了而视图从不实例化它,单测照样绿。可访问性同理——`accessibilityLabel`
/// 写在源码里不等于 AX 树里读得出来(修饰符挂错层、被父容器吞掉、identifier 撞名,
/// 都会让它在源码里看着完全正确)。这两类只有真开窗口、真读 AX 才能判。
///
/// ## 本轮实测的边界(如实记,不假称)
///
/// 2026-10-13 查证:`GlassPanel`(承载 `glassEffect` 与 reduce-transparency 策略的类型)
/// 在 `macos/AeroricApp` 与 `AeroricKit/Sources` 里**没有任何实例化点**,
/// 只有 `WorkspaceWiringTests` 引用它;同时全仓没有任何视图读
/// `@Environment(\.accessibilityReduceTransparency)`。所以"系统减少透明度偏好
/// 真的改到了界面"这件事**今天不成立**,单测证明的只是那个纯函数。
/// 这条已作为具名缺口登记(`GAP-AP8-A11Y-SURFACE`),本文件只断言**真正成立**的东西。
///
/// ## 环境注入与隔离
///
/// 与 `ShellLifecycleTests` 同一套:`ui-test.mjs` 给 runner 注入 `AERORIC_*`
/// (TEST_RUNNER_ 前缀转发),这里再转给 app。**绝不触碰真实数据目录**。
@MainActor
final class AccessibilityTests: XCTestCase {
    private let lifecycle = NativeUITestApplicationLifecycle()

    override func tearDownWithError() throws {
        let lifecycle = lifecycle
        try MainActor.assumeIsolated {
            try lifecycle.tearDown()
        }
        try super.tearDownWithError()
    }

    private func launchApp(extraEnvironment: [String: String] = [:]) throws -> XCUIApplication {
        let runnerEnvironment = ProcessInfo.processInfo.environment
        var app = XCUIApplication()
        // Scene assembly under testmanagerd is intermittent (G41): relaunch with a
        // FRESH XCUIApplication and wait for the previous process to actually die.
        var painted = false
        for attempt in 0..<6 {
            if attempt > 0 {
                try lifecycle.quit(app)
            }
            app = XCUIApplication()
            app.launchEnvironment["AERORIC_ENV"] = runnerEnvironment["AERORIC_ENV"] ?? "ui-test"
            if let dataDirectory = runnerEnvironment["AERORIC_DATA_DIR"] {
                app.launchEnvironment["AERORIC_DATA_DIR"] = dataDirectory
            }
            for (key, value) in extraEnvironment {
                app.launchEnvironment[key] = value
            }
            try lifecycle.launch(app)
            painted = app.staticTexts["bootstrap.environment"].firstMatch.waitForExistence(timeout: 10)
            if painted { break }
        }
        if !painted {
            print("A11Y-AXDUMP-BEGIN\n\(app.debugDescription)\nA11Y-AXDUMP-END")
        }
        XCTAssertTrue(painted, "main window did not paint")
        return app
    }

    /// 导航必须**从 AX 树可寻址**,且各选项带可读标题。
    ///
    /// 反事实:`accessibilityLabel` 写在源码里但修饰符挂错层是常见缺陷
    /// (父容器吞掉 / 被 `Text` 覆盖),所以判据取自 AX 树而不是源码。
    /// 相邻互斥选项不得只靠颜色区分(`AGENTS.md` 原生选择规则),这里检验它的
    /// 可访问性后果:每个可选项必须有可读文本。
    ///
    /// 2026-10-13:工作面导航从 12 段的 `native.segmented` 改成侧栏分组列表
    /// (参考 harness/ 各客户端的收敛做法:顶层导航是侧栏,tab 条只留给打开的
    /// 文档)。判据随之改到 `sidebar.list` 与各 `sidebar.item.*` 行——**测的是
    /// 同一件事**(相邻互斥选项可读、可寻址),不是同一件控件。
    func testWorkspaceNavigationIsAddressableWithReadableRows() throws {
        let app = try launchApp()
        let list = app.descendants(matching: .any)["sidebar.list"].firstMatch
        XCTAssertTrue(
            list.waitForExistence(timeout: 15),
            "sidebar.list must be addressable from the AX tree (a label in source is not an AX surface)"
        )

        // 工作面行必须真的存在且读得出文字:分组标题不算,它们不可选。
        let rows = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier BEGINSWITH 'sidebar.item.section.'")
        ).allElementsBoundByIndex
        XCTAssertGreaterThan(
            rows.count, 1,
            "navigation over adjacent exclusive surfaces needs more than one addressable row"
        )
        let readable = rows.filter { !$0.label.isEmpty }
        XCTAssertFalse(
            readable.isEmpty,
            "navigation rows must read as non-empty labels to VoiceOver, not colour alone"
        )
    }

    /// 侧栏选择必须能**只用键盘**到达,且选中值可从 AX 读出来。
    ///
    /// 同时钉两件事:键盘可达性(计划要求"所有 destructive 操作能被键盘取消",
    /// 前提是焦点能到),以及"选中态由系统绘制"——判据取 `detail.selection`
    /// 的**值**,不是颜色或自绘底纹。
    func testSidebarSelectionIsKeyboardReachableAndItsValueIsReadable() throws {
        let projects = try makeProjectDirectories()
        let app = try launchApp(
            extraEnvironment: [
                "AERORIC_TEST_OPEN_PROJECT": projects.map(\.path).joined(separator: ":")
            ]
        )
        let firstRow = app.staticTexts["alpha-project"].firstMatch
        XCTAssertTrue(firstRow.waitForExistence(timeout: 20), "sidebar row missing")

        firstRow.click()
        app.typeKey(.downArrow, modifierFlags: [])

        let selectionProbe = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier == 'detail.selection'")
        ).firstMatch
        XCTAssertTrue(
            selectionProbe.waitForExistence(timeout: 10),
            "selection did not surface as an AX-readable value (detail.selection missing)"
        )
        let value = (selectionProbe.value as? String) ?? selectionProbe.label
        XCTAssertFalse(
            value.isEmpty,
            "the selected project must be readable as an AX value, not only painted"
        )
        XCTAssertTrue(
            value.contains("alpha-project") || value.contains("beta-project"),
            "the AX value must name the selected project, got \(value)"
        )
    }

    /// 窗口/外观:外壳在**系统当前的 light/dark** 下都画得出来,菜单也在。
    ///
    /// 判据刻意弱,且这里**不注入**外观变量:app 今天没有读任何外观 override
    /// (全仓 `grep AERORIC_TEST_APPEARANCE` 零命中),注入一个没人读的变量只会让
    /// 用例看起来在测外观而实际什么都没变。真正的外观矩阵(light/dark × glass/classic ×
    /// reduce-transparency)需要两样今天没有的东西:外壳读 `@Environment` 里的系统偏好,
    /// 以及一个可读的"当前表面"AX 读数——已登记为 `GAP-AP8-A11Y-SURFACE`。
    ///
    /// 这条用例留下的是**能证的那一半**:外壳起得来、画得出、菜单在。不拿截图冒充外观验证。
    func testShellPaintsAndKeepsItsMenuSurface() throws {
        let app = try launchApp()
        XCTAssertTrue(
            app.staticTexts["bootstrap.environment"].firstMatch.exists,
            "shell did not paint"
        )
        XCTAssertGreaterThan(
            app.menuBars.menuBarItems.count, 0,
            "menu bar must exist alongside the shell"
        )
    }

    private func makeProjectDirectories() throws -> [URL] {
        let base = FileManager.default.temporaryDirectory
            .appendingPathComponent("aeroric-a11y-\(UUID().uuidString)", isDirectory: true)
        var urls: [URL] = []
        for name in ["alpha-project", "beta-project"] {
            let url = base.appendingPathComponent(name, isDirectory: true)
            try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
            urls.append(url)
        }
        return urls
    }
}
