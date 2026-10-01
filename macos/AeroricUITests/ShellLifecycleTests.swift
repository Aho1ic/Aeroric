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
        let app = XCUIApplication()
        app.launchEnvironment["AERORIC_ENV"] = runnerEnvironment["AERORIC_ENV"] ?? "ui-test"
        if let dataDirectory = runnerEnvironment["AERORIC_DATA_DIR"] {
            app.launchEnvironment["AERORIC_DATA_DIR"] = dataDirectory
        }
        if !projects.isEmpty {
            app.launchEnvironment["AERORIC_TEST_OPEN_PROJECT"] =
                projects.map(\.path).joined(separator: ":")
        }
        app.launch()
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

    private func sidebarRows(_ app: XCUIApplication, titled title: String, selected: Bool) -> XCUIElementQuery {
        app.staticTexts.matching(
            NSPredicate(format: "label == %@ AND isSelected == %d", title, selected ? 1 : 0)
        )
    }

    private func selectedSidebarTitles(_ app: XCUIApplication) -> Set<String> {
        let predicate = NSPredicate(
            format: "identifier BEGINSWITH 'sidebar.item.' AND isSelected == YES"
        )
        return Set(app.staticTexts.matching(predicate).allElementsBoundByIndex.map(\.label))
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

        app.typeKey("w", modifierFlags: .command)
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
            sidebarRow(app, titled: "alpha-project").waitForExistence(timeout: 10),
            "alpha-project row missing from sidebar"
        )
        XCTAssertTrue(
            sidebarRow(app, titled: "beta-project").waitForExistence(timeout: 10),
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
        XCTAssertTrue(sidebarRow(app, titled: "alpha-project").waitForExistence(timeout: 10))

        // Window 2 via its keyboard shortcut: a real second WindowGroup
        // scene, still served by the single supervisor/connection.
        app.typeKey("n", modifierFlags: [.command, .shift])
        XCTAssertTrue(
            app.descendants(matching: .any)["sidebar.list"].firstMatch.waitForExistence(timeout: 8),
            "second window did not paint its own shell"
        )

        // The key window is window 2: selecting beta here must not disturb
        // window 1's alpha once we re-select it there.
        sidebarRow(app, titled: "beta-project").firstMatch.click()

        // Bring window 1 forward by clicking its (currently unselected) row.
        let unselectedAlpha = sidebarRows(app, titled: "alpha-project", selected: false)
        XCTAssertTrue(
            unselectedAlpha.firstMatch.waitForExistence(timeout: 5),
            "expected an unselected alpha-project row in the other window"
        )
        unselectedAlpha.firstMatch.click()

        // Two different rows selected at once = two independent selections.
        XCTAssertEqual(
            selectedSidebarTitles(app),
            ["alpha-project", "beta-project"],
            "expected one selected row per window, not a single shared selection"
        )
        app.terminate()
    }

    /// Acceptance "sidebar keyboard selection": after a click selects the
    /// first row, the arrow keys move the List(selection:) selection.
    func testSidebarSelectionMovesWithArrowKeys() throws {
        let projects = try makeProjectDirectories()
        let app = launchApp(withProjects: projects)
        waitForShell(app)
        let alpha = sidebarRow(app, titled: "alpha-project")
        XCTAssertTrue(alpha.waitForExistence(timeout: 10))

        alpha.firstMatch.click()
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
