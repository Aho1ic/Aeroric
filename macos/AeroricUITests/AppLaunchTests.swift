import XCTest

/// P4-01 acceptance surface (Action 7): cold launch, bootstrap identifiers,
/// close — deliberately nothing else. Business UI arrives with P4-02+; the
/// isolation mechanism (an isolated data dir injected to BOTH the runner and
/// the app) is exercised on every run.
@MainActor
final class AppLaunchTests: XCTestCase {
    func testColdLaunchShowsBootstrapIdentifiersThenCloses() throws {
        let runnerEnvironment = ProcessInfo.processInfo.environment
        let app = XCUIApplication()
        // ui-test.mjs exports TEST_RUNNER_-prefixed variables, so the runner
        // process sees AERORIC_*; the app receives the same values through
        // launchEnvironment. This is what keeps the app off any production
        // data during tests (P4-01 Action 7).
        app.launchEnvironment["AERORIC_ENV"] = runnerEnvironment["AERORIC_ENV"] ?? "ui-test"
        if let dataDirectory = runnerEnvironment["AERORIC_DATA_DIR"] {
            app.launchEnvironment["AERORIC_DATA_DIR"] = dataDirectory
        }

        app.launch()

        let environmentLabel = app.staticTexts["bootstrap.environment"]
        XCTAssertTrue(
            environmentLabel.waitForExistence(timeout: 15),
            "bootstrap.environment missing after cold launch"
        )
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
            app.staticTexts["bootstrap.protocol"].waitForExistence(timeout: 5),
            "bootstrap.protocol missing"
        )
        XCTAssertTrue(
            app.staticTexts["bootstrap.build"].waitForExistence(timeout: 5),
            "bootstrap.build missing"
        )

        app.terminate()
        // Cold start + identifiers + close — the whole P4-01 UI surface.
    }
}
