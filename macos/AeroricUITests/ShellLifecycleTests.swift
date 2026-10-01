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
    private func launchApp(withProjects projects: [URL] = []) -> XCUIApplication {
        let runnerEnvironment = ProcessInfo.processInfo.environment
        var app = XCUIApplication()
        // Scene assembly under testmanagerd is intermittent (G41): a launch
        // sometimes runs the event loop with zero windows and no task
        // execution. Relaunching absorbs it. terminate() is asynchronous, so
        // each attempt uses a FRESH XCUIApplication and waits for the previous
        // process to actually die — relaunching immediately produced two live
        // instances racing the same RPC channel (measured 2026-10-02, hook
        // log shows two same-second "hook start" entries).
        var painted = false
        for attempt in 0..<6 {
            if attempt > 0 {
                app.terminate()
                let deadline = Date().addingTimeInterval(5)
                while isRunning(app), Date() < deadline {
                    Thread.sleep(forTimeInterval: 0.2)
                }
                Thread.sleep(forTimeInterval: 1)
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
            app.launch()
            painted = app.staticTexts["bootstrap.environment"].firstMatch.waitForExistence(timeout: 10)
            if painted { break }
        }
        if !painted {
            print("SHELL-AXDUMP-BEGIN\n\(app.debugDescription)\nSHELL-AXDUMP-END")
        }
        return app
    }

    private func isRunning(_ app: XCUIApplication) -> Bool {
        app.state == .runningForeground || app.state == .runningBackground
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
    /// selected project's title into a value-backed element
    /// (`detail.selection`, one per window). Each window holds its own
    /// value — the set of values across the app IS the per-window state.
    private func selectedSidebarTitles(_ app: XCUIApplication) -> Set<String> {
        let query = app.descendants(matching: .any).matching(
            NSPredicate(format: "identifier == 'detail.selection'")
        )
        var titles: Set<String> = []
        for element in query.allElementsBoundByIndex {
            if let value = element.value as? String {
                // value reads "已选项目：<title>" — keep the title part
                if let range = value.range(of: "：") {
                    titles.insert(String(value[range.upperBound...]))
                }
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
    /// destroying it — pressing ⌘, again re-shows the same instance.
    func testSettingsWindowOpensWithCommandCommaAndClosesWithCommandW() throws {
        let app = launchApp()
        waitForShell(app)

        app.typeKey(",", modifierFlags: .command)
        let settingsRoot = app.descendants(matching: .any)["settings.root"]
        XCTAssertTrue(settingsRoot.waitForExistence(timeout: 5), "⌘, did not open the settings window")

        app.typeKey("w", modifierFlags: .command)
        XCTAssertFalse(
            settingsRoot.waitForExistence(timeout: 3),
            "⌘W did not close the settings window"
        )

        // The controller kept the window; the shortcut re-shows it instead
        // of constructing a second one (P4-04 Action 3).
        app.typeKey(",", modifierFlags: .command)
        XCTAssertTrue(settingsRoot.waitForExistence(timeout: 5), "second ⌘, did not re-show settings")

        app.terminate()
    }

    /// Acceptance "Dock reopen": XCUITest cannot click the Dock icon, so
    /// this drives the same code path the Dock drives (AppDelegate keeps the
    /// default reopen handling; the visible surface is 显示主窗口). After
    /// ⌘W closes the last main window the app stays alive and the shell
    /// comes back — including after every window was closed (openWindow
    /// bridge, not just ordering an existing window forward).
    func testShowMainWindowRestoresAfterClose() throws {
        let app = launchApp()
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

        app.terminate()
    }

    /// Shell identifiers at rest (AppLaunchTests surface now rendered by the
    /// shell's BootstrapInfoBlock) plus the hook-opened sidebar rows and the
    /// status bar.
    func testShellIdentifiersPresentAtRest() throws {
        let projects = try makeProjectDirectories()
        let app = launchApp(withProjects: projects)
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
        app.terminate()
    }

    /// Acceptance "two windows with different project selections": the
    /// second main window paints its own shell and each window's per-window
    /// selection state holds a different project at the same time — a shared
    /// selection could only ever hold one.
    func testTwoMainWindowSelectionsAreIndependent() throws {
        let projects = try makeProjectDirectories()
        let app = launchApp(withProjects: projects)
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
        app.terminate()
    }

    /// Acceptance "sidebar keyboard selection": after a click selects the
    /// first row, the arrow keys move the List(selection:) selection.
    func testSidebarSelectionMovesWithArrowKeys() throws {
        let projects = try makeProjectDirectories()
        let app = launchApp(withProjects: projects)
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
        app.terminate()
    }
}
