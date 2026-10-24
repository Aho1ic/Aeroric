import XCTest

/// P4-01 acceptance surface (Action 7): cold launch, bootstrap identifiers,
/// close — deliberately nothing else. Business UI arrives with P4-02+; the
/// isolation mechanism (an isolated data dir injected to BOTH the runner and
/// the app) is exercised on every run.
@MainActor
final class AppLaunchTests: XCTestCase {
    private let lifecycle = NativeUITestApplicationLifecycle()

    override func tearDownWithError() throws {
        let lifecycle = lifecycle
        try MainActor.assumeIsolated {
            try lifecycle.tearDown()
        }
        try super.tearDownWithError()
    }

    func testColdLaunchShowsBootstrapIdentifiersThenCloses() throws {
        let runnerEnvironment = ProcessInfo.processInfo.environment
        var app = XCUIApplication()
        // ui-test.mjs exports TEST_RUNNER_-prefixed variables, so the runner
        // process sees AERORIC_*; the app receives the same values through
        // launchEnvironment. This is what keeps the app off any production
        // data during tests (P4-01 Action 7).
        app.launchEnvironment["AERORIC_ENV"] = runnerEnvironment["AERORIC_ENV"] ?? "ui-test"
        if let dataDirectory = runnerEnvironment["AERORIC_DATA_DIR"] {
            app.launchEnvironment["AERORIC_DATA_DIR"] = dataDirectory
        }

        // Scene assembly under testmanagerd is intermittent (G41): a launch
        // sometimes runs the event loop with zero windows. Retrying absorbs
        // it — same mitigation as ShellLifecycleTests.launchApp (fresh
        // XCUIApplication per attempt; the lifecycle barrier must succeed).
        var painted = false
        for attempt in 0..<6 {
            if attempt > 0 {
                try lifecycle.quit(app)
                app = XCUIApplication()
                app.launchEnvironment["AERORIC_ENV"] = runnerEnvironment["AERORIC_ENV"] ?? "ui-test"
                if let dataDirectory = runnerEnvironment["AERORIC_DATA_DIR"] {
                    app.launchEnvironment["AERORIC_DATA_DIR"] = dataDirectory
                }
            }
            try lifecycle.launch(app)
            painted = app.staticTexts["bootstrap.environment"].firstMatch.waitForExistence(timeout: 10)
            if painted { break }
        }
        XCTAssertTrue(painted, "bootstrap.environment missing after cold launch (3 launches)")
        // firstMatch: a reopen race can produce a second shell window; the
        // duplicate identifiers must not throw the whole query off.
        let environmentLabel = app.staticTexts["bootstrap.environment"].firstMatch
        if let expectedEnvironment = runnerEnvironment["AERORIC_ENV"] {
            // macOS SwiftUI static texts carry the visible text in `value`;
            // `label` is empty (verified via AX tree dump 2026-10-01).
            let actualEnvironment = (environmentLabel.value as? String) ?? environmentLabel.label
            XCTAssertEqual(
                actualEnvironment, expectedEnvironment,
                "injected AERORIC_ENV did not reach the app"
            )
        }
        XCTAssertTrue(
            app.staticTexts["bootstrap.protocol"].firstMatch.waitForExistence(timeout: 5),
            "bootstrap.protocol missing"
        )
        XCTAssertTrue(
            app.staticTexts["bootstrap.build"].firstMatch.waitForExistence(timeout: 5),
            "bootstrap.build missing"
        )
        // P4-02: the connection row exists from first paint (its text is the
        // supervisor phase, which may legitimately still be "launching").
        XCTAssertTrue(
            app.staticTexts["bootstrap.connection"].firstMatch.waitForExistence(timeout: 5),
            "bootstrap.connection missing"
        )

        try lifecycle.quit(app)
        // Cold start + identifiers + close — the whole P4-01 UI surface.
    }
}
