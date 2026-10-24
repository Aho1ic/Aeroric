import AeroricUI
import AppKit
import Darwin
import ImageIO
import XCTest

/// Run ONLY through pnpm native:ui-test, after the stage-01 process ownership
/// and exclusive-desktop safety preconditions have been satisfied. These
/// cases never launch against a fallback/default data directory.
@MainActor
final class NativeVisualFoundationUITests: XCTestCase {
    private enum ReviewTestError: Error {
        case unsafeRunnerEnvironment
        case missingElement(String)
        case screenshotGeometry(String)
        case reviewNotForeground
    }

    func testReviewControlsAreAccessibleAndIsolated() throws {
        let (app, directory) = try launchReview()
        defer { app.terminate() }
        try assertIsolated(app, directory: directory)

        let refresh = try element(app, "review.workspace.refresh")
        XCTAssertEqual(refresh.elementType, .button)
        XCTAssertEqual(refresh.label, "刷新工作区样本")
        XCTAssertGreaterThanOrEqual(refresh.frame.width, 28)
        XCTAssertGreaterThanOrEqual(refresh.frame.height, 28)
        let disabled = try element(app, "review.icon.disabled")
        XCTAssertEqual(disabled.elementType, .button)
        XCTAssertFalse(disabled.isEnabled)
        XCTAssertFalse(disabled.label.isEmpty)
        refresh.hover()
        try capture(app, name: "icon-hover-and-disabled")
        refresh.click()
        try expectValue(app, "review.action.last", "刷新工作区")
        try element(app, "review.icon.help").click()
        try expectValue(app, "review.action.last", "帮助")

        let single = try element(app, "review.input.singleLine")
        XCTAssertEqual(single.label, "单行输入样本")
        try activateReview(app)
        single.click()
        single.typeText("stage01")
        XCTAssertTrue((single.value as? String ?? "").contains("stage01"))

        // Traverse backwards from the single-line field: forwards enters
        // TextEditor, where Tab is editing input rather than focus traversal.
        // This is real keyboard navigation, not a programmatic focus setter
        // or a change to the user's global keyboard preferences.
        for _ in 0..<30 {
            if value(app, "review.focus.icon") == "on" { break }
            app.typeKey(.tab, modifierFlags: .shift)
        }
        try expectValue(app, "review.focus.icon", "on")
        try capture(app, name: "icon-native-keyboard-focus")
        app.typeKey(" ", modifierFlags: [])
        try expectValue(app, "review.action.last", "刷新工作区")

        let multiline = try element(app, "review.input.multiLine")
        XCTAssertFalse(multiline.label.isEmpty)
        try activateReview(app)
        multiline.click()
        multiline.typeKey("a", modifierFlags: .command)
        multiline.typeText("first line\nsecond line")
        XCTAssertTrue((multiline.value as? String ?? "").contains("second line"))

        _ = try element(app, "review.row.aeroric")
        // Click the system row, not its static text. Text can be selected
        // while the editor still owns first responder (as in the shell test).
        let row = app.outlineRows.containing(.staticText, identifier: "review.row.aeroric").firstMatch
        XCTAssertTrue(row.exists)
        try activateReview(app)
        row.click()
        try activateReview(app)
        app.typeKey(.downArrow, modifierFlags: [])
        try expectValue(app, "review.selection", "review.row.task")
        let lastRow = try element(app, "review.scroll.row.20")
        let scroll = app.scrollViews.containing(.staticText, identifier: "review.scroll.row.20").firstMatch
        XCTAssertTrue(scroll.exists)
        for _ in 0..<6 {
            if lastRow.isHittable { break }
            try activateReview(app)
            scroll.scroll(byDeltaX: 0, deltaY: -180)
        }
        XCTAssertTrue(lastRow.isHittable, "last sample row must be reachable by native scrolling")
        try capture(app, name: "inputs-selection-and-scroll")
        try assertIsolated(app, directory: directory)
    }

    func testReviewAppearanceAndGeometryEvidence() throws {
        let (app, directory) = try launchReview()
        defer { app.terminate() }
        for key in ["reduceTransparency", "increaseContrast", "reduceMotion"] {
            let actual = value(app, "review.system.\(key)")
            XCTAssertTrue(["on", "off"].contains(actual))
            try expectValue(app, "review.effective.\(key)", actual)
        }
        let systemReducedTransparency = value(app, "review.system.reduceTransparency") == "on"
        try capture(app, name: "system-default-no-injection")

        for (appearance, expected) in [("浅色", "light"), ("深色", "dark")] {
            try chooseSegment(app, title: appearance)
            try expectValue(app, "review.effective.appearance", expected)
            for (title, budget, width, height, columns) in [
                ("1260×860", "1260 → 320/586/320", 1260.0, 860.0, [320.0, 586.0, 320.0]),
                ("1000×640", "1000 → 284/409/273", 1000.0, 640.0, [284.0, 409.0, 273.0]),
                ("400×600", "400 → 382", 400.0, 600.0, [382.0]),
            ] {
                try chooseSegment(app, title: title)
                try expectValue(app, "review.sample.budget", budget)
                try assertMeasuredGeometry(app, width: width, height: height, columns: columns)
                try capture(app, name: "\(expected)-\(Int(width))-glass-preference")
            }
        }

        try chooseSegment(app, title: "1260×860")
        try chooseSegment(app, title: "classic")
        try expectValue(app, "review.effective.surface", "classic")
        try expectValue(app, "review.effective.controlMaterial", "系统按钮")
        try capture(app, name: "classic-opaque-foundation")
        try chooseSegment(app, title: "glass")
        try expectValue(app, "review.effective.surface", systemReducedTransparency ? "classic" : "glass")
        if #available(macOS 26.0, *), !systemReducedTransparency {
            try expectValue(app, "review.effective.controlMaterial", "Liquid Glass")
        } else {
            try expectValue(app, "review.effective.controlMaterial", "系统按钮")
        }
        try element(app, "review.action.text").click()
        try expectValue(app, "review.action.last", "操作")

        try element(app, "review.inject.reduceTransparency").click()
        try expectValue(app, "review.effective.surface", "classic")
        try expectValue(app, "review.effective.controlMaterial", "系统按钮")
        try capture(app, name: "injected-reduce-transparency")
        try element(app, "review.inject.increaseContrast").click()
        try expectValue(app, "review.effective.outline", "1.0")
        try capture(app, name: "injected-increased-contrast")
        try element(app, "review.inject.reduceMotion").click()
        try expectValue(app, "review.effective.motionDuration", "0.000")
        try element(app, "review.motion.play").click()
        try capture(app, name: "injected-reduced-motion")
        // Injection must not silently change the user's system preferences.
        XCTAssertEqual(value(app, "review.system.reduceTransparency"),
                       systemReducedTransparency ? "on" : "off")
        try assertIsolated(app, directory: directory)
    }

    /// Assert measured layout, not a fixture string repeated by the view.
    /// The frame probes sit on the actual panels inside their fixed columns.
    private func assertMeasuredGeometry(
        _ app: XCUIApplication, width: Double, height: Double, columns: [Double]
    ) throws {
        let window = app.windows.firstMatch
        // Read one public CGRect per poll, without nested frame.width KVC.
        // Keep exactly the same 5-second wait and ±1pt width/height bounds.
        let settled = NSPredicate { _, _ in
            guard window.exists else { return false }
            let frame = window.frame
            return abs(frame.width - width) <= 1 && abs(frame.height - height) <= 1
        }
        XCTAssertEqual(XCTWaiter.wait(
            for: [XCTNSPredicateExpectation(predicate: settled, object: window)],
            timeout: 5
        ), .completed, "sample dimensions must describe the entire standard NSWindow")
        let actual = window.frame
        XCTAssertEqual(actual.width, width, accuracy: 1)
        XCTAssertEqual(actual.height, height, accuracy: 1)
        try expectValue(app, "review.window.frame", String(format: "%.1f×%.1f", width, height))
        // NSWindow notifications and SwiftUI layout preferences arrive on
        // separate turns. Wait for the entire current sample, not merely
        // four parseable numbers left over from the previous sample.
        let panelsSettled = NSPredicate { [self] _, _ in
            guard let band = frameValue(app, name: "band"),
                  abs(band.width - width) <= 0.5, band.height > 100 else { return false }
            let names = columns.count == 1 ? ["workspace"] : ["sidebar", "workspace", "inspector"]
            var nextX = 9.0
            for (name, expectedWidth) in zip(names, columns) {
                guard let frame = frameValue(app, name: name),
                      abs(frame.minX - nextX) <= 0.5, abs(frame.minY - 8) <= 0.5,
                      abs(frame.width - expectedWidth) <= 0.5,
                      abs(frame.height - (band.height - 16)) <= 0.5 else { return false }
                nextX += expectedWidth + 8
            }
            return columns.count != 1
                || (value(app, "review.measured.sidebar") == "absent"
                    && value(app, "review.measured.inspector") == "absent")
        }
        XCTAssertEqual(XCTWaiter.wait(
            for: [XCTNSPredicateExpectation(predicate: panelsSettled, object: app)],
            timeout: 10
        ), .completed, "actual panel layout did not settle to the current sample")
        let band = try measuredFrame(app, name: "band")
        XCTAssertEqual(band.width, width, accuracy: 0.5)
        XCTAssertGreaterThan(band.height, 100)
        let names = columns.count == 1 ? ["workspace"] : ["sidebar", "workspace", "inspector"]
        var nextX = 9.0 // 8pt inset + 1pt fixture outline allowance
        for (name, expectedWidth) in zip(names, columns) {
            let frame = try measuredFrame(app, name: name)
            XCTAssertEqual(frame.minX, nextX, accuracy: 0.5, name)
            XCTAssertEqual(frame.minY, 8, accuracy: 0.5, name)
            XCTAssertEqual(frame.width, expectedWidth, accuracy: 0.5, name)
            XCTAssertEqual(frame.height, band.height - 16, accuracy: 0.5, name)
            nextX += expectedWidth + 8
        }
        XCTAssertEqual(nextX - 8 + 9, width, accuracy: 0.5, "all widths, gaps and insets conserved")
        if columns.count == 1 {
            try expectValue(app, "review.measured.sidebar", "absent")
            try expectValue(app, "review.measured.inspector", "absent")
        }
        let sizes = value(app, "review.window.content").split(separator: "×").compactMap { Double($0) }
        XCTAssertEqual(sizes.count, 2)
        if sizes.count == 2 {
            XCTAssertEqual(sizes[0], width, accuracy: 0.5)
            XCTAssertLessThanOrEqual(sizes[1], height + 0.5)
        }
        // A standard NSWindow can use fullSizeContentView without becoming
        // borderless. contentLayoutRect, not contentView.bounds, expresses
        // the region unobscured by native titlebar/toolbar chrome.
        let layout = value(app, "review.window.contentLayout")
            .split(separator: "×").compactMap { Double($0) }
        XCTAssertEqual(layout.count, 2)
        if layout.count == 2 {
            XCTAssertEqual(layout[0], width, accuracy: 0.5)
            XCTAssertLessThan(layout[1], height)
            XCTAssertGreaterThan(layout[1], band.height)
            if sizes.count == 2 { XCTAssertGreaterThanOrEqual(sizes[1] + 0.5, layout[1]) }
        }
    }

    private func frameValue(_ app: XCUIApplication, name: String) -> CGRect? {
        let values = value(app, "review.measured.\(name)").split(separator: ",").compactMap { Double($0) }
        guard values.count == 4 else { return nil }
        return CGRect(x: values[0], y: values[1], width: values[2], height: values[3])
    }

    private func measuredFrame(_ app: XCUIApplication, name: String) throws -> CGRect {
        let identifier = "review.measured.\(name)"
        let leaf = try element(app, identifier)
        let predicate = NSPredicate { [self] _, _ in
            value(app, identifier).split(separator: ",").compactMap { Double($0) }.count == 4
        }
        XCTAssertEqual(XCTWaiter.wait(
            for: [XCTNSPredicateExpectation(predicate: predicate, object: leaf)], timeout: 5
        ), .completed, "missing real layout measurement for \(name)")
        let values = value(app, identifier).split(separator: ",").compactMap { Double($0) }
        guard values.count == 4 else { throw ReviewTestError.missingElement(identifier) }
        return CGRect(x: values[0], y: values[1], width: values[2], height: values[3])
    }

    private func launchReview() throws -> (XCUIApplication, URL) {
        let environment = ProcessInfo.processInfo.environment
        guard environment["AERORIC_ENV"] == "ui-test",
              let directory = environment["AERORIC_DATA_DIR"] else {
            throw ReviewTestError.unsafeRunnerEnvironment
        }
        // XCTest's runner is sandboxed: NSHomeDirectory() is its container,
        // not the account home used by the unsandboxed app and original
        // runner script. Resolve the account independently of HOME/env and
        // reuse the app's full path policy (including symlink rejection).
        let accountHome = try accountHomeDirectory()
        guard NativeVisualReviewEntry.isIsolatedDataDirectory(directory, home: accountHome) else {
            throw ReviewTestError.unsafeRunnerEnvironment
        }
        print("STAGE01-RUNNER-PATHS runnerHome=\(NSHomeDirectory()) accountHome=\(accountHome) dataDirectory=\(directory)")
        let root = URL(fileURLWithPath: directory).standardizedFileURL.resolvingSymlinksInPath()
        let scratch = root.appendingPathComponent("stage01-\(UUID().uuidString)", isDirectory: true)
        // The sandboxed runner may read, but must not gain write access to
        // the account's Caches. This fixture needs no directory at all.
        // Assert that the unique child remains absent, before and after use.
        try assertReviewDirectoryAbsent(scratch)
        let app = XCUIApplication()
        app.launchEnvironment["AERORIC_ENV"] = "ui-test"
        app.launchEnvironment["AERORIC_DATA_DIR"] = scratch.path
        app.launchEnvironment["AERORIC_NATIVE_VISUAL_REVIEW"] = "1"
        app.launch()
        app.activate()
        do {
            _ = try element(app, "review.isolated")
            try activateReview(app)
        } catch {
            // Preserve the original launch error even if screenshot creation
            // also fails. capture always attaches AX before requesting pixels.
            try? capture(app, name: "launch-failure")
            app.terminate()
            throw error
        }
        return (app, scratch)
    }

    private func accountHomeDirectory() throws -> String {
        // Reentrant lookup: do not retain getpwuid's shared static storage.
        // Lookup failure (including an unexpectedly large record) fails closed.
        var record = passwd()
        var result: UnsafeMutablePointer<passwd>?
        var buffer = [CChar](repeating: 0, count: 16_384)
        return try buffer.withUnsafeMutableBufferPointer { bytes in
            guard let base = bytes.baseAddress,
                  getpwuid_r(getuid(), &record, base, bytes.count, &result) == 0,
                  result != nil, let home = record.pw_dir else {
                throw ReviewTestError.unsafeRunnerEnvironment
            }
            return String(cString: home)
        }
    }

    private func assertIsolated(_ app: XCUIApplication, directory: URL) throws {
        XCTAssertFalse(app.staticTexts["bootstrap.environment"].exists)
        XCTAssertFalse(app.descendants(matching: .any)["sidebar.list"].exists)
        try assertReviewDirectoryAbsent(directory)
    }

    private func assertReviewDirectoryAbsent(_ directory: URL) throws {
        // Read the existing parent instead of treating fileExists == false
        // as proof: permission errors and dangling links must not look green.
        let entries = try FileManager.default.contentsOfDirectory(
            atPath: directory.deletingLastPathComponent().path
        )
        XCTAssertFalse(
            entries.contains(directory.lastPathComponent),
            "review must not create even its data directory, let alone runtime/host/task/session artifacts"
        )
    }

    private func element(_ app: XCUIApplication, _ identifier: String) throws -> XCUIElement {
        let element = app.descendants(matching: .any).matching(identifier: identifier).firstMatch
        guard element.waitForExistence(timeout: 15) else {
            XCTFail("Missing AX leaf: \(identifier)")
            throw ReviewTestError.missingElement(identifier)
        }
        return element
    }

    private func value(_ app: XCUIApplication, _ identifier: String) -> String {
        let element = app.descendants(matching: .any).matching(identifier: identifier).firstMatch
        return (element.value as? String) ?? element.label
    }

    private func expectValue(_ app: XCUIApplication, _ identifier: String, _ expected: String) throws {
        let element = try element(app, identifier)
        XCTAssertFalse(element.label.isEmpty, "\(identifier) must have a readable name")
        let predicate = NSPredicate(format: "value == %@ OR label == %@", expected, expected)
        XCTAssertEqual(XCTWaiter.wait(
            for: [XCTNSPredicateExpectation(predicate: predicate, object: element)], timeout: 5
        ), .completed, "\(identifier) expected \(expected), got \(value(app, identifier))")
    }

    private func chooseSegment(_ app: XCUIApplication, title: String) throws {
        try activateReview(app)
        let predicate = NSPredicate(format: "label == %@ OR title == %@", title, title)
        let segment = app.radioButtons.matching(predicate).firstMatch
        guard segment.waitForExistence(timeout: 5) else {
            XCTFail("System segment is not independently addressable: \(title)")
            throw ReviewTestError.missingElement(title)
        }
        segment.click()
    }

    private func capture(_ app: XCUIApplication, name: String) throws {
        do {
            try activateReview(app)
        } catch {
            _ = attachAX(app, name: "\(name)-foreground-failure-DIAGNOSTIC")
            throw error
        }
        let window = app.windows.firstMatch
        let snapshotFrame = attachAX(app, name: name)
        guard window.exists else { throw ReviewTestError.missingElement("review window for \(name)") }
        let expectedPID = Int32(value(app, "review.process.pid"))
        let png = try windowPNG(window)
        guard expectedPID != nil,
              NSWorkspace.shared.frontmostApplication?.processIdentifier == expectedPID else {
            throw ReviewTestError.reviewNotForeground
        }
        guard window.frame == snapshotFrame else {
            throw ReviewTestError.screenshotGeometry("Window moved between AX and PNG capture")
        }
        let image = XCTAttachment(data: png, uniformTypeIdentifier: "public.png")
        image.name = "stage01-\(name)"
        image.lifetime = .keepAlways
        add(image)
    }

    @discardableResult
    private func attachAX(_ app: XCUIApplication, name: String) -> CGRect? {
        let window = app.windows.firstMatch
        let actualFrame = window.exists ? window.frame : nil
        let frame = window.exists ? String(describing: window.frame) : "No window"
        let text = """
        \(ProcessInfo.processInfo.operatingSystemVersionString)
        Window frame: \(frame)
        Fixture-only review; appearance/accessibility injections are labelled in the window.
        \(app.debugDescription)
        """
        print("STAGE01-AX-BEGIN \(name)\n\(text)\nSTAGE01-AX-END \(name)")
        let ax = XCTAttachment(data: Data(text.utf8), uniformTypeIdentifier: "public.plain-text")
        ax.name = "stage01-\(name)-AX"
        ax.lifetime = .keepAlways
        add(ax)
        return actualFrame
    }

    /// A successful activation request or cached isActive is not proof that
    /// the review owns the foreground. Check the fixture's actual process ID.
    /// This never assigns control focus or changes global keyboard settings.
    private func activateReview(_ app: XCUIApplication) throws {
        guard let expectedPID = Int32(value(app, "review.process.pid")) else {
            throw ReviewTestError.missingElement("review.process.pid")
        }
        if NSWorkspace.shared.frontmostApplication?.processIdentifier != expectedPID {
            app.activate()
        }
        let foreground = NSPredicate { _, _ in
            app.state == .runningForeground
                && NSWorkspace.shared.frontmostApplication?.processIdentifier == expectedPID
        }
        guard XCTWaiter.wait(
            for: [XCTNSPredicateExpectation(predicate: foreground, object: app)], timeout: 5
        ) == .completed else {
            XCTFail("Review is not foreground: expected \(expectedPID), observed \(String(describing: NSWorkspace.shared.frontmostApplication?.processIdentifier))")
            throw ReviewTestError.reviewNotForeground
        }
    }

    /// Public per-screen capture avoids the observed negative-y element
    /// screenshot error. Retain the nominal window crop as lossless PNG.
    /// These geometry guards cannot prove absence of occlusion or compositor
    /// transforms: every exported image still requires visual review.
    private func windowPNG(_ window: XCUIElement) throws -> Data {
        let frame = window.frame
        let displays = NSScreen.screens
        func topology(_ screens: [NSScreen]) -> [String] {
            screens.map {
                "\($0.deviceDescription[NSDeviceDescriptionKey("NSScreenNumber")] ?? ""):\($0.frame):\($0.visibleFrame):\($0.backingScaleFactor)"
            }
        }
        let initialTopology = topology(displays)
        guard let primary = displays.first else {
            throw ReviewTestError.screenshotGeometry("No active display")
        }
        let bounds = displays.map {
            CGRect(x: $0.frame.minX, y: primary.frame.maxY - $0.frame.maxY,
                   width: $0.frame.width, height: $0.frame.height)
        }
        let containing = bounds.indices.filter { bounds[$0].contains(frame) }
        guard containing.count == 1, let index = containing.first else {
            throw ReviewTestError.screenshotGeometry("Window not wholly on one display: \(frame), \(bounds)")
        }
        let target = displays[index]
        let targetBounds = bounds[index]
        let visible = CGRect(
            x: target.visibleFrame.minX, y: primary.frame.maxY - target.visibleFrame.maxY,
            width: target.visibleFrame.width, height: target.visibleFrame.height
        )
        guard visible.contains(frame) else {
            throw ReviewTestError.screenshotGeometry("Window exceeds display's visible frame: \(frame), \(visible)")
        }
        func requireStableGeometry() throws {
            guard window.frame == frame, topology(NSScreen.screens) == initialTopology else {
                throw ReviewTestError.screenshotGeometry("Window/display moved during capture")
            }
        }
        if index == 0 {
            let png = window.screenshot().pngRepresentation
            try requireStableGeometry()
            return png
        }
        let expectedWidth = Int((target.frame.width * target.backingScaleFactor).rounded())
        let expectedHeight = Int((target.frame.height * target.backingScaleFactor).rounded())
        let samePixelSize = displays.dropFirst().filter {
            Int(($0.frame.width * $0.backingScaleFactor).rounded()) == expectedWidth
                && Int(($0.frame.height * $0.backingScaleFactor).rounded()) == expectedHeight
        }
        let screens = XCUIScreen.screens
        guard samePixelSize.count == 1, screens.count == displays.count else {
            throw ReviewTestError.screenshotGeometry("Cannot unambiguously match secondary display")
        }
        // Only the first (main) screen has a documented index. Match the
        // other public screenshots by their unique backing-pixel dimensions.
        let matchingImages = screens.dropFirst().compactMap { screen -> CGImage? in
            let png = screen.screenshot().pngRepresentation
            guard let source = CGImageSourceCreateWithData(png as CFData, nil),
                  let pixels = CGImageSourceCreateImageAtIndex(source, 0, nil),
                  pixels.width == expectedWidth, pixels.height == expectedHeight else { return nil }
            return pixels
        }
        guard matchingImages.count == 1, let pixels = matchingImages.first else {
            throw ReviewTestError.screenshotGeometry("No unique XCTest image for secondary display")
        }
        let scaleX = CGFloat(pixels.width) / targetBounds.width
        let scaleY = CGFloat(pixels.height) / targetBounds.height
        let crop = CGRect(
            x: (frame.minX - targetBounds.minX) * scaleX,
            y: (frame.minY - targetBounds.minY) * scaleY,
            width: frame.width * scaleX, height: frame.height * scaleY
        )
        let integral = crop.integral
        guard abs(crop.minX - integral.minX) < 0.001,
              abs(crop.minY - integral.minY) < 0.001,
              abs(crop.width - integral.width) < 0.001,
              abs(crop.height - integral.height) < 0.001 else {
            throw ReviewTestError.screenshotGeometry("Window crop is not backing-pixel aligned: \(crop)")
        }
        guard CGRect(x: 0, y: 0, width: pixels.width, height: pixels.height).contains(crop),
              let result = pixels.cropping(to: crop) else {
            throw ReviewTestError.screenshotGeometry("Window crop exceeds display image: \(crop)")
        }
        try requireStableGeometry()
        print("STAGE01-CAPTURE secondaryDisplay=\(targetBounds) pixels=\(pixels.width)×\(pixels.height) window=\(frame) crop=\(crop)")
        guard let png = NSBitmapImageRep(cgImage: result).representation(using: .png, properties: [:]) else {
            throw ReviewTestError.screenshotGeometry("Could not encode original window pixels as PNG")
        }
        return png
    }
}
