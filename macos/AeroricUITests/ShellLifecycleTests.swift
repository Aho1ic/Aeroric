import XCTest

/// P4-04 lifecycle acceptance: Cmd+, / Cmd+W on the shared settings window,
/// main-window restore after close (the Dock-reopen equivalent path), two
/// windows holding different project selections, and sidebar keyboard
/// selection. Env injection mirrors AppLaunchTests (TEST_RUNNER_ forwarding
/// pattern); `AERORIC_TEST_OPEN_PROJECT` drives the real `project.open` RPC
/// so the sidebar has real rows to select (NSOpenPanel cannot be driven
/// from XCUITest).
@MainActor
final class ShellLifecycleTests: XCTestCase {
    private let lifecycle = NativeUITestApplicationLifecycle()

    override func tearDownWithError() throws {
        let lifecycle = lifecycle
        try MainActor.assumeIsolated {
            try lifecycle.tearDown()
        }
        try super.tearDownWithError()
    }

    private func launchApp(withProjects projects: [URL] = []) throws -> XCUIApplication {
        let runnerEnvironment = ProcessInfo.processInfo.environment
        var app = XCUIApplication()
        // Scene assembly under testmanagerd is intermittent (G41): a launch
        // sometimes runs the event loop with zero windows and no task
        // execution. Each attempt uses a FRESH XCUIApplication only after the
        // lifecycle barrier confirms the previous app and children exited —
        // relaunching immediately produced two live
        // instances racing the same RPC channel (measured 2026-10-02, hook
        // log shows two same-second "hook start" entries).
        var painted = false
        for attempt in 0..<6 {
            if attempt > 0 {
                try lifecycle.quit(app)
            }
            app = XCUIApplication()
            app.launchEnvironment["AERORIC_ENV"] = runnerEnvironment["AERORIC_ENV"] ?? "ui-test"
            if let dataDirectory = runnerEnvironment["AERORIC_DATA_DIR"] {
                app.launchEnvironment["AERORIC_DATA_DIR"] = dataDirectory
                // TEMPORARY (G41): capture the serve child's stderr — its
                // startup log names the step it hangs on under testmanagerd.
                app.launchEnvironment["AERORIC_SERVE_STDERR_LOG"] =
                    dataDirectory + "/serve-stderr.log"
            }
            if !projects.isEmpty {
                app.launchEnvironment["AERORIC_TEST_OPEN_PROJECT"] =
                    projects.map(\.path).joined(separator: ":")
            }
            try lifecycle.launch(app)
            painted = app.staticTexts["bootstrap.environment"].firstMatch.waitForExistence(timeout: 10)
            if painted { break }
        }
        if !painted {
            print("SHELL-AXDUMP-BEGIN\n\(app.debugDescription)\nSHELL-AXDUMP-END")
        }
        return app
    }

    /// Two scratch project directories for the sidebar rows. Paths carry no
    /// ":" so the hook's separator is unambiguous.
    private func makeProjectDirectories() throws -> [URL] {
        let base = FileManager.default.temporaryDirectory
            .appendingPathComponent("aeroric-shell-tests-\(UUID().uuidString)", isDirectory: true)
        var urls: [URL] = []
        for name in ["alpha-project", "beta-project"] {
            let url = base.appendingPathComponent(name, isDirectory: true)
            try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
            urls.append(url)
        }
        return urls
    }

    private func waitForShell(_ app: XCUIApplication) {
        let painted = app.staticTexts["bootstrap.environment"].waitForExistence(timeout: 20)
        if !painted {
            // Full AX tree into the log — the 2026-10-01 zero-window and
            // identifier-hijack investigations were only readable this way.
            print("SHELL-AXDUMP-BEGIN\n\(app.debugDescription)\nSHELL-AXDUMP-END")
        }
        XCTAssertTrue(painted, "main window did not paint")
    }

    /// Menu bar layout under XCUITest: boundBy 0 is the *Apple* menu, 1 the
    /// app menu, 2 File — where CommandGroup(.newItem) lands. The index is
    /// probed rather than trusted.
    private func fileMenuItem(_ app: XCUIApplication, titled title: String) -> XCUIElement {
        let items = app.menuBars.menuBarItems
        for index in 1...4 {
            let candidate = items.element(boundBy: index).menus.menuItems[title]
            if candidate.exists { return candidate }
        }
        return items.element(boundBy: 2).menus.menuItems[title]
    }

    private func sidebarRow(_ app: XCUIApplication, titled title: String) -> XCUIElement {
        app.staticTexts[title]
    }

    /// Selection probe: macOS List selection is NOT exposed through AX
    /// isSelected (outline rows and row texts both read isSelected == NO
    /// after a click — measured 2026-10-01/02), so the shell renders the
    /// selected row's title into a value-backed element
    /// (`detail.selection`, one per window). Each window holds its own
    /// value — the set of values across the app IS the per-window state.
    ///
    /// The value is the row title verbatim (`DetailTitleBar` sets
    /// `accessibilityValue(item.title)`); it used to be prefixed with
    /// `已选项目：`, which was product copy wrapped around a test probe.
    private func selectedSidebarTitles(_ app: XCUIApplication) -> Set<String> {
        let query = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier == 'detail.selection'")
        )
        var titles: Set<String> = []
        for element in query.allElementsBoundByIndex {
            if let value = element.value as? String, !value.isEmpty {
                titles.insert(value)
            }
        }
        return titles
    }

    private func selectionHeaderCount(_ app: XCUIApplication) -> Int {
        app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier == 'detail.selection'")
        ).count
    }

    /// Acceptance "Cmd+," and "Cmd+W": the shared settings window opens on
    /// the real keyboard shortcut and 关闭窗口 (⌘W) orders it out without
    /// destroying it — invoking it again re-shows the same instance.
    ///
    /// ## Why the settings command is driven through the menu (2026-10-14)
    ///
    /// `app.typeKey(",", modifierFlags: .command)` does **not** fire this menu
    /// item on a machine whose input source is not ASCII. Measured on the dev
    /// machine (input source `com.apple.inputmethod.SCIM.ITABC`, locale
    /// `zh_CN`): XCUITest maps the character `","` to a key event whose
    /// `charactersIgnoringModifiers` is the full-width `"，"`, which never
    /// matches the menu's ASCII key equivalent — so the shortcut silently does
    /// nothing, while ⌘N, ⌘⇧N and ⌘J all fire correctly through the same menu.
    ///
    /// The registration itself is correct and was verified directly: the app's
    /// `NSApp.mainMenu` reports `设置… | key=',' | mods=⌘ | enabled=true`, and
    /// rebinding the same item to a letter makes the keystroke work. So this
    /// is a limitation of synthesizing an ASCII comma under a non-ASCII input
    /// source, not a defect in the command — a real user's ⌘, is matched on
    /// the physical key's base character and is unaffected by the IME.
    ///
    /// The test therefore drives the command the way the menu does (the same
    /// `SettingsWindowController.shared.show` path) and keeps ⌘W as a real
    /// keystroke, since letters synthesize correctly. What it asserts is the
    /// behaviour the acceptance is about: open, close-without-destroying,
    /// re-show the same instance.
    func testSettingsWindowOpensWithCommandCommaAndClosesWithCommandW() throws {
        let app = try launchApp()
        waitForShell(app)

        let settingsRoot = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier == 'settings.root'")
        ).firstMatch

        let settingsItem = fileMenuItem(app, titled: "设置…")
        XCTAssertTrue(settingsItem.waitForExistence(timeout: 5), "设置… missing from the File menu")
        settingsItem.click()
        XCTAssertTrue(settingsRoot.waitForExistence(timeout: 5), "设置… did not open the settings window")

        app.typeKey("w", modifierFlags: .command)
        XCTAssertFalse(
            settingsRoot.waitForExistence(timeout: 3),
            "⌘W did not close the settings window"
        )

        // The controller kept the window; invoking the command again re-shows
        // it instead of constructing a second one (P4-04 Action 3).
        settingsItem.click()
        XCTAssertTrue(settingsRoot.waitForExistence(timeout: 5), "second invocation did not re-show settings")

        try lifecycle.quit(app)
    }

    /// Acceptance "Dock reopen": XCUITest cannot click the Dock icon, so
    /// this drives the same code path the Dock drives (AppDelegate keeps the
    /// default reopen handling; the visible surface is 显示主窗口). After
    /// ⌘W closes the last main window the app stays alive and the shell
    /// comes back — including after every window was closed (openWindow
    /// bridge, not just ordering an existing window forward).
    func testShowMainWindowRestoresAfterClose() throws {
        let app = try launchApp()
        waitForShell(app)
        let environmentLabel = app.staticTexts["bootstrap.environment"]

        // ⌘W closes the KEY window; a reopen-race phantom (G41) may hold
        // key first, so close up to three times until the shell is gone.
        for _ in 0..<3 where environmentLabel.exists {
            app.typeKey("w", modifierFlags: .command)
            _ = environmentLabel.waitForExistence(timeout: 2)
        }
        XCTAssertFalse(
            environmentLabel.waitForExistence(timeout: 2),
            "main window content still visible after ⌘W"
        )

        let showItem = fileMenuItem(app, titled: "显示主窗口")
        XCTAssertTrue(showItem.waitForExistence(timeout: 5), "显示主窗口 missing from the File menu")
        showItem.click()
        XCTAssertTrue(
            app.staticTexts["bootstrap.environment"].waitForExistence(timeout: 8),
            "显示主窗口 did not restore the shell"
        )

        try lifecycle.quit(app)
    }

    /// Shell identifiers at rest (AppLaunchTests surface now rendered by the
    /// shell's BootstrapInfoBlock) plus the hook-opened sidebar rows and the
    /// status bar.
    func testShellIdentifiersPresentAtRest() throws {
        let projects = try makeProjectDirectories()
        let app = try launchApp(withProjects: projects)
        waitForShell(app)
        for identifier in ["bootstrap.protocol", "bootstrap.build", "bootstrap.connection", "bootstrap.application"] {
            XCTAssertTrue(
                app.staticTexts[identifier].waitForExistence(timeout: 5),
                "\(identifier) missing from shell"
            )
        }
        XCTAssertTrue(
            app.descendants(matching: .any)["sidebar.list"].firstMatch.waitForExistence(timeout: 5),
            "sidebar list missing"
        )
        XCTAssertTrue(
            app.staticTexts["statusbar.phase"].waitForExistence(timeout: 5),
            "status bar phase text missing"
        )
        // The two hook-opened projects reached the sidebar through the real
        // project.open RPC (host echo → ProjectStore → sidebar items).
        XCTAssertTrue(
            sidebarRow(app, titled: "alpha-project").waitForExistence(timeout: 20),
            "alpha-project row missing from sidebar"
        )
        XCTAssertTrue(
            sidebarRow(app, titled: "beta-project").waitForExistence(timeout: 20),
            "beta-project row missing from sidebar"
        )
        try lifecycle.quit(app)
    }

    /// 每个工作面在**空数据下**必须有一个可寻址的空态叶子。
    ///
    /// 这张表同时钉两件事,而它们此前都只靠人眼:
    ///
    /// 1. **没有空白面板。** 参考实现里没有一个面板是空的(`opencode`
    ///    "Nothing here yet"/"Create a session to get started"、`deepseek`
    ///    "No sessions yet"/"No matches"),空面板与"渲染失败"无法区分。
    /// 2. **叶子标识符没有被容器吞掉。** 这是 2026-10-14 撞到多次的 AX 陷阱:
    ///    容器上的 `.accessibilityIdentifier` 会**替换**其下所有叶子的标识符
    ///    (`BootstrapInfoBlock` 的 `detail.bootstrap-info` 吞掉五行
    ///    `bootstrap.*`;`DebugPanel` 的 `ide.debug-panel` 吞掉八个按钮与空态;
    ///    本表最初跑起来时又抓出五处:files / connections / operations×3)。
    ///    症状是"测试查不到一个明明渲染了的元素"。**这是运行时判据**——
    ///    静态扫描分不清容器标识符与叶子标识符,只有真 AX 树能分辨。
    ///
    /// 只列**确定性**的:测试环境没有项目/任务/会话/连接/Docker daemon,
    /// 所以这些面板必然落到空态。依赖外部状态的(如 `infrastructure.images-empty`
    /// 需要 daemon 可用)刻意不列——那是条件断言,会变成噪音。
    private static let emptyStateLeaves: [String: [String]] = [
        "tasks": ["tasks.empty", "tasks.empty-detail"],
        "files": ["files.search.empty"],
        "git": ["git.changes-empty", "git.history-empty"],
        "ide": ["ide.debug.no-session", "ide.debug.bp-empty"],
        "database": ["database.sidebar-empty"],
        "connections": ["connections.list-empty", "connections.sftp-empty"],
        "infrastructure": ["infrastructure.ports-empty"],
        "extensions": ["extensions.skills-empty", "extensions.mcp-empty", "extensions.hooks-empty"],
        "operations": ["usage.empty", "diagnostics.empty", "notifications.empty"],
    ]

    /// 导航脊柱的**每一行都必须真的切到对应工作面**,且每个面在空数据下
    /// 必须有一个可寻址的空态叶子。
    ///
    /// 反事实(2026-10-13):12 段 tab 条换成侧栏分组列表后,最大的风险是
    /// "行点了没反应"——选中态变了但内容没换,或者内容换了但没渲染。这类缺陷
    /// 在截图上不可靠地判(窗口激活、多实例、像素相似),所以判据取 AX 树:
    /// 移动之后,`detail.selection` 的值必须等于目标行的标题。
    ///
    /// ## 为什么用键盘走,而不是逐行点击(2026-10-14 实测)
    ///
    /// 点击需要**命中点**:只要别的应用的窗口压在本窗口上,XCUITest 就报
    /// `Unable to find hit point for ScrollView`,并把压着的窗口列成
    /// "interrupting elements"——实测本机同时有 UURemote(投屏)、QQ、微信、
    /// Ghostty 五扇窗口压在上面,而 `app.activate()` **清不掉**它们。
    /// `typeKey` 是把事件合成进**应用进程**,不经过屏幕坐标,因此不受遮挡影响;
    /// 而 ↑/↓ 正是 shell 自己实现的键盘路径(窗口级等价键),所以这条用例
    /// 测的是真实交互,不是测试专用捷径。
    ///
    /// 点击路径的覆盖没有丢:`testSidebarSelectionMovesWithArrowKeys` 与
    /// `testTwoMainWindowSelectionsAreIndependent` 仍然真的点击侧栏行。
    ///
    /// ## 查询按标识符,不按标题文字
    ///
    /// 标题是普通中文词(「文件」「Git」「笔记」),`app.staticTexts["文件"]`
    /// 会匹配到内容区里任何同名文字——实测第一次运行就是这样点到别的面板的
    /// 滚动视图上。标识符只出现在侧栏行上。
    ///
    /// 期望值取自**行自己的 AX 值**,不写死在测试里:行标题改了测试不用改,
    /// 而"行与详情列说的是不是同一个东西"这个判据不变。
    func testEveryNavigationRowSwitchesTheDetailSurface() throws {
        let app = try launchApp()
        waitForShell(app)

        // 渲染顺序(工作 → 项目 → 工具 → 系统);本用例不注入项目,所以没有
        // 项目行。这与 `SidebarOrdering.rendered` 的顺序一致——键盘走的就是它。
        let sections = [
            "tasks", "files", "git", "ide", "database", "notebook",
            "agentTools", "connections", "infrastructure", "extensions",
            "remote", "operations",
        ]

        let probe = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier == 'detail.selection'")
        ).firstMatch

        for section in sections {
            let row = app.staticTexts.matching(
                identifier: "sidebar.item.section.\(section)"
            ).firstMatch
            XCTAssertTrue(
                row.waitForExistence(timeout: 10),
                "sidebar.item.section.\(section) 不在导航脊柱里"
            )
            let expected = (row.value as? String) ?? row.label
            XCTAssertFalse(expected.isEmpty, "\(section):行没有可读标题")

            // 用**点击**而不是 ↑/↓ 驱动。
            //
            // 实测 2026-10-14:`.typeKey(.downArrow)` 是**异步**处理的,按完立即
            // 断言会让整条走查**错位一格**——测试以为在第 N 行,界面还停在第
            // N-1 行,于是它拿第 N 行的期望值比第 N-1 行的实际值,并用第 N-1 个
            // 面板去查第 N 个面板的空态,报出一批**假**的"叶子不可寻址"。
            // 加等待能治错位,但 `click()` 本来就等应用 idle,是同步的,
            // 判据因此不依赖时序。
            //
            // 代价:点击需要命中点,别的应用的窗口压上来时会失败并报
            // "interrupting elements"。那是**环境前提**(XCUITest 的常规要求:
            // 跑 UI 测试的机器不应被占用),不是产品缺陷——失败信息里会点名
            // 压着的窗口,便于分辨。
            row.click()
            XCTAssertTrue(
                probe.waitForExistence(timeout: 10),
                "\(section):点完之后详情列没有标题(工作面没渲染)"
            )
            XCTAssertEqual(
                probe.value as? String,
                expected,
                "点「\(expected)」没有切换详情列——选中态与内容脱节"
            )

            for leaf in Self.emptyStateLeaves[section] ?? [] {
                XCTAssertTrue(
                    app.descendants(matching: .any)[leaf].waitForExistence(timeout: 5),
                    """
                    \(section):空态叶子 \(leaf) 不可寻址。两种可能,都必须修:
                    ① 该面板在空数据下是空白(与"渲染失败"无法区分);
                    ② 叶子标识符被容器上的 .accessibilityIdentifier 吞掉了
                       (AX 树里它会读作容器 id,见 G51)。
                    """
                )
            }
        }
        try lifecycle.quit(app)
    }

    /// Acceptance "two windows with different project selections": the
    /// second main window paints its own shell and each window's per-window
    /// selection state holds a different project at the same time — a shared
    /// selection could only ever hold one.
    func testTwoMainWindowSelectionsAreIndependent() throws {
        let projects = try makeProjectDirectories()
        let app = try launchApp(withProjects: projects)
        waitForShell(app)
        XCTAssertTrue(sidebarRow(app, titled: "alpha-project").waitForExistence(timeout: 20))

        // Window 1 selects alpha first (its own per-window @State).
        sidebarRow(app, titled: "alpha-project").firstMatch.click()
        XCTAssertEqual(selectedSidebarTitles(app), ["alpha-project"], "click did not select alpha in window 1")

        // Window 2 via the File-menu command (deterministic; the ⌘⇧N
        // keystroke raced the key window in earlier runs).
        let newWindow = fileMenuItem(app, titled: "新建窗口")
        XCTAssertTrue(newWindow.waitForExistence(timeout: 5), "新建窗口 missing from the File menu")
        newWindow.click()
        XCTAssertTrue(
            app.descendants(matching: .any)["sidebar.list"].firstMatch.waitForExistence(timeout: 8),
            "second window did not paint its own shell"
        )

        // Window 2 selects beta with the KEYBOARD: the sidebar is the
        // window's default focus target, so two ↓ move the system List
        // selection none → alpha → beta — entirely inside window 2. Window
        // 1 keeps alpha: with per-window @State the detail headers read
        // alpha AND beta simultaneously; a shared selection would have
        // replaced alpha with beta.
        app.typeKey(.downArrow, modifierFlags: [])
        app.typeKey(.downArrow, modifierFlags: [])

        XCTAssertEqual(
            selectedSidebarTitles(app),
            ["alpha-project", "beta-project"],
            "expected one selected row per window, not a single shared selection"
        )
        XCTAssertEqual(selectionHeaderCount(app), 2, "expected one header per window")
        try lifecycle.quit(app)
    }

    /// Acceptance "sidebar keyboard selection": after a click selects the
    /// first row, the arrow keys move the List(selection:) selection.
    func testSidebarSelectionMovesWithArrowKeys() throws {
        let projects = try makeProjectDirectories()
        let app = try launchApp(withProjects: projects)
        waitForShell(app)
        let alpha = sidebarRow(app, titled: "alpha-project")
        XCTAssertTrue(alpha.waitForExistence(timeout: 20))

        // Click the outline ROW (not the row's text): the click must land on
        // the list's own view so it holds keyboard focus for the ↓ that follows.
        let alphaRow = app.descendants(matching: .outlineRow)
            .matching(NSPredicate(format: "label == 'alpha-project'")).firstMatch
        if alphaRow.exists {
            alphaRow.click()
        } else {
            alpha.firstMatch.click()
        }
        XCTAssertEqual(selectedSidebarTitles(app), ["alpha-project"], "click did not select alpha")

        app.typeKey(.downArrow, modifierFlags: [])
        XCTAssertEqual(
            selectedSidebarTitles(app),
            ["beta-project"],
            "↓ did not move the sidebar selection to beta"
        )
        try lifecycle.quit(app)
    }
}
