#if DEBUG
import AeroricUI
import AppKit
import CryptoKit
import Darwin
import XCTest

/// Run ONLY through the authorized full native UI runner in this task's
/// isolated account/VM. These helpers never grant that permission themselves.
/// Missing external Skill evidence fails BEFORE launch, never skips/seeds it.
@MainActor
final class NativeSkillReviewUITests: XCTestCase {
    private static let bundleID = "com.aeroric.native.dev"
    private static let suite = "AeroricNativeUITests/NativeSkillReviewUITests"
    private static let evidenceKey = "AERORIC_SKILL_PREFERENCE_EVIDENCE"
    private let testAttemptID = UUID().uuidString
    private static let reviewKeys = [
        "AERORIC_NATIVE_SKILL_REVIEW", "AERORIC_NATIVE_KNOWLEDGE_REVIEW",
        "AERORIC_NATIVE_EVOLUTION_REVIEW", "AERORIC_NATIVE_VISUAL_REVIEW",
        "AERORIC_NATIVE_COLLABORATION_REVIEW",
    ]
    private static let methods = [
        "testDraftReviewEditsAndDiscardStayOffline",
        "testNewSkillConsentAndProjectScopeRemainExplicit",
        "testTaskSelectionRespectsAvailabilityAndSources",
        "testUpdatesKeepInFlightVersionsWithinSafetyLimits",
        "testNativeAppearanceKeyboardAndCompactLayout",
        "testInvalidSkillRequestsNeverReachProduct",
    ]

    func testDraftReviewEditsAndDiscardStayOffline() throws {
        try withLaunch(method: #function) { launch in
            let app = launch.app
            try expect(app, "detail.contentState", "草稿待审")
            try click(launch, "source.open")
            try expect(app, "source.conditions", "前提（示例）")
            try expect(app, "source.validation", "验证要求（示例）")
            try expect(app, "source.limitation", "知识摘录不自动")
            try scrollLongText(launch, "source.excerpt")
            try escapeSheet(launch, content: "source.excerpt")
            let original = text(try leaf(app, "detail.title"))
            try click(launch, "edit.open")
            try replace(launch, "edit.name", with: "已取消的虚构修改")
            try escapeSheet(launch, content: "edit.name")
            try expect(app, "detail.title", original)
            try click(launch, "edit.open")
            try replace(launch, "edit.name", with: "人工修订的虚构方法")
            try click(launch, "edit.apply")
            try expect(app, "detail.title", "人工修订的虚构方法")
            try expect(app, "detail.consent", "未答")
            try expect(app, "detail.install", "尚未载入")
            try click(launch, "content.confirm")
            try expect(app, "detail.contentState", "内容与当前项目范围已确认")
            try disabled(app, "install.success")
            try disabled(app, "version.enable")
            try selectItem(launch, id: "keyboard", title: "键盘路径复核")
            try click(launch, "content.discard")
            try expect(app, "detail.contentState", "已丢弃")
            try disabled(app, "consent.agree")
            try disabled(app, "edit.open")
            try expect(app, "work.empty", "尚未选用")
            try capture(launch, "S1-edited-confirmed-and-discarded")
        }
    }

    func testNewSkillConsentAndProjectScopeRemainExplicit() throws {
        try withLaunch(method: #function) { launch in
            let app = launch.app
            try scenario(launch, "S2 · 新技能人工同意")
            for permission in ["默认权限（示例）", "Full Access（示例，不代替同意）"] {
                try menu(launch, "permission", title: permission)
                try click(launch, "consent.defer")
                try expect(app, "detail.consent", "未答")
                try disabled(app, "install.success")
                try click(launch, "consent.refuse")
                try expect(app, "detail.consent", "已拒绝")
                try disabled(app, "install.success")
                try click(launch, "consent.agree")
                try expect(app, "detail.consent", "已同意所展示")
                try expect(app, "detail.install", "尚未载入")
                try click(launch, "install.failure")
                try expect(app, "detail.install", "安装失败")
                try expect(app, "detail.usability", "不可选用")
                try disabled(app, "version.enable")
                try disabled(app, "select.boundary")
                try click(launch, "install.success")
                try expect(app, "detail.install", "安装成功")
                try expect(app, "detail.enabled", "未启用")
                try click(launch, "version.enable")
                try expect(app, "detail.enabled", "启用 v1.0")
                try expect(app, "work.empty", "尚未选用")
                try click(launch, "select.boundary")
                try expect(app, "reference.boundary", "用户手选")
                XCTAssertEqual(app.sheets.count, 0, "Same scope must not ask for a second use approval.")
                try click(launch, "edit.open")
                try replace(launch, "edit.conditions", with: "\(permission) 的新前提不在先前同意内（示例）")
                try click(launch, "edit.apply")
                try expect(app, "detail.consent", "未答")
                try disabled(app, "install.success")
            }
            try scenario(launch, "S3 · 范围与权限分离")
            try click(launch, "scope.open")
            try expect(app, "scope.explanation", "仅星港排程获准")
            try escapeSheet(launch, content: "scope.explanation")
            try menu(launch, "project", title: "潮汐笔记（虚构项目）")
            try expect(app, "detail.enabled", "未获准、未启用")
            try disabled(app, "consent.agree")
            try disabled(app, "select.boundary")
            try disabled(app, "work.stop")
            try click(launch, "scope.open")
            try expect(app, "scope.explanation", "Full Access 不扩")
            try click(launch, "sheet.cancel")
            try expect(app, "detail.enabled", "未获准、未启用")
            try menu(launch, "project", title: "星港排程（虚构项目）")
            try expect(app, "detail.enabled", "启用 v1.0")
            try capture(launch, "S2-S3-explicit-consent-and-scope")
        }
    }

    func testTaskSelectionRespectsAvailabilityAndSources() throws {
        try withLaunch(method: #function) { launch in
            let app = launch.app
            try scenario(launch, "S4 · 任务按需选用")
            try click(launch, "select.boundary")
            try expect(app, "reference.boundary", "v1.0")
            try click(launch, "reference.boundary.source")
            try expect(app, "source.version", "v1.0")
            try escapeSheet(launch, content: "source.excerpt")
            try click(launch, "reference.boundary.remove")
            try expect(app, "work.empty", "尚未选用")
            try click(launch, "work.agent")
            try expect(app, "reference.boundary", "演示 Agent 按需选用")
            XCTAssertFalse(query(app, "reference.keyboard").exists)
            XCTAssertFalse(query(app, "candidate.unrelated").exists)
            try disabled(app, "select.candidate")
            try disabled(app, "select.dependency")
            try disabled(app, "select.missingSource")
            try menu(launch, "work", title: "键盘可达性复核（示例工作包）")
            try expect(app, "work.empty", "尚未选用")
            try click(launch, "select.keyboard")
            try expect(app, "reference.keyboard", "用户手选")
            try click(launch, "reference.keyboard.remove")
            try click(launch, "work.agent")
            try expect(app, "reference.keyboard", "演示 Agent 按需选用")
            try disabled(app, "select.unknown")
            XCTAssertFalse(query(app, "reference.boundary").exists)
            XCTAssertEqual(app.sheets.count, 0)
            try scenario(launch, "S5 · 可用性有依据")
            for executor in ["Claude Code", "Codex", "omp", "dsh"] {
                try menu(launch, "executor", title: "\(executor)（预设，非实际兼容结论）")
                try selectItem(launch, id: "dependency", title: "附图核对")
                try expect(app, "detail.availability", "预设，非实际兼容结论")
                try expect(app, "detail.availability", executor == "dsh" ? "不支持" : "缺少")
                try disabled(app, "select.dependency")
                try selectItem(launch, id: "missingSource", title: "无来源候选")
                try expect(app, "detail.availability", "没有来源")
                try click(launch, "source.open")
                try expect(app, "source.excerpt", "没有来源")
                try escapeSheet(launch, content: "source.excerpt")
                try disabled(app, "select.missingSource")
                try selectItem(launch, id: "unknown", title: "摘要交叉复核")
                try expect(app, "detail.availability", "未知")
            }
            try menu(launch, "work", title: "键盘可达性复核（示例工作包）")
            try disabled(app, "select.unknown")
            try capture(launch, "S4-S5-relevance-and-evidence")
        }
    }

    func testUpdatesKeepInFlightVersionsWithinSafetyLimits() throws {
        try withLaunch(method: #function) { launch in
            let app = launch.app
            try scenario(launch, "S6 · 更新先审阅")
            let revisedName = "当前获准的人工修订版（示例）"
            let revisedContent = "当前获准旧版的独立比较依据（完全虚构），不是最早预置稿。"
            try click(launch, "edit.open")
            try replace(launch, "edit.name", with: revisedName)
            try replace(launch, "edit.content", with: revisedContent)
            try click(launch, "edit.apply")
            try click(launch, "consent.agree")
            try click(launch, "install.success")
            try click(launch, "version.enable")
            try click(launch, "update.stage")
            try disclosure(launch, "旧版与更新差异（示例）")
            try expect(app, "update.versions", "v1.0 → 候选 v1.1")
            try expect(app, "update.oldTarget", revisedName)
            try expect(app, "update.oldContent", revisedContent)
            try expect(app, "update.newSource", "新版本来源")
            try expect(app, "update.capabilities", "新增")
            try expect(app, "update.dependencies", "新增")
            try expect(app, "update.scope", "不申请该扩围")
            for action in ["consent.defer", "consent.refuse"] {
                try click(launch, action)
                try expect(app, "detail.enabled", "启用 v1.0")
                try disabled(app, "install.success")
            }
            try click(launch, "edit.open")
            try replace(launch, "edit.name", with: "尚未获准的新候选名称（示例）")
            try replace(launch, "edit.validation", with: "人工修订候选验证（示例）")
            try click(launch, "edit.apply")
            try expect(app, "detail.enabled", "启用 v1.0")
            try expect(app, "select.boundary", revisedName + " v1.0")
            try click(launch, "select.boundary")
            try expect(app, "reference.boundary", revisedName)
            try expect(app, "reference.boundary", "v1.0")
            try click(launch, "consent.agree")
            try expect(app, "detail.install", "尚未载入")
            try click(launch, "install.success")
            try expect(app, "detail.enabled", "启用 v1.0")
            try click(launch, "version.enable")
            try expect(app, "detail.enabled", "启用 v1.1")
            try menu(launch, "work", title: "更新后的新任务（示例）")
            try expect(app, "work.empty", "尚未选用")
            try click(launch, "work.agent")
            try expect(app, "reference.boundary", "v1.1")
            try menu(launch, "work", title: "在途接口工作（示例，已固定 v1.0）")
            try expect(app, "reference.boundary", "v1.0")
            try expect(app, "reference.boundary", "条件满足")
            try disabled(app, "select.boundary")
            try disabled(app, "reference.boundary.remove")
            try capture(launch, "S6-new-task-v11-inflight-v10")
            try scenario(launch, "S7 · 在途不是永久豁免")
            for (id, reason) in [("revoked", "授权已撤销"), ("unsafe", "不安全"), ("hostBlocked", "宿主硬拦截")] {
                try click(launch, "inFlight.load.\(id)")
                try expect(app, "reference.boundary", reason)
                try expect(app, "reference.boundary", "v1.0")
                try disabled(app, "work.agent")
            }
            try click(launch, "inFlight.load.allowed")
            try expect(app, "reference.boundary", "条件满足")
            try click(launch, "work.stop")
            try click(launch, "inFlight.load.allowed")
            try expect(app, "reference.boundary", "Stop 优先")
            try expect(app, "work.stopState", "已要求停止")
            try expect(app, "consumption", "无真实加载/消费证据")
            try capture(launch, "S7-stop-wins-with-history-preserved")
        }
    }

    func testNativeAppearanceKeyboardAndCompactLayout() throws {
        try withLaunch(method: #function) { launch in
            let app = launch.app
            try scenario(launch, "S8 · 离线与重置")
            // Exercise the native menu BEFORE creating the dirty state whose
            // reset is asserted below; scene switching itself reloads fixtures.
            try click(launch, "scenario")
            app.typeKey(.home, modifierFlags: [])
            app.typeKey(.downArrow, modifierFlags: [])
            app.typeKey(.return, modifierFlags: [])
            try require(wait { !self.text(self.query(app, "scenario")).contains("S8") },
                        "Arrow/Return did not change the native scene menu selection")
            try scenario(launch, "S8 · 离线与重置")
            try click(launch, "edit.open")
            let nameField = try leaf(app, "edit.name")
            try reveal(launch, nameField)
            nameField.click()
            app.typeKey("a", modifierFlags: .command)
            app.typeText("键盘应用的虚构方法")
            app.typeKey(.tab, modifierFlags: [])
            app.typeKey(.tab, modifierFlags: .shift)
            app.typeKey(.return, modifierFlags: [])
            try expect(app, "detail.title", "键盘应用的虚构方法")
            try click(launch, "edit.open")
            try replace(launch, "edit.name", with: "Escape 不应用的内容")
            try escapeSheet(launch, content: "edit.name")
            try expect(app, "detail.title", "键盘应用的虚构方法")
            try click(launch, "consent.agree")
            try click(launch, "install.success")
            try click(launch, "version.enable")
            try click(launch, "select.boundary")
            for (color, surface) in [("浅色预览", "classic"), ("深色预览", "glass")] {
                try click(launch, "appearance.open")
                try menu(launch, "appearance.color", title: color)
                try segment(launch, "appearance.surface", title: surface)
                try menu(launch, "appearance.interaction", title: "advanced 呈现（示例，无 PTY）")
                try menu(launch, "permission", title: "Full Access（示例，不代替同意）")
                for id in ["transparency", "contrast", "motion"] {
                    let toggle = try leaf(app, "appearance.\(id)")
                    try reveal(launch, toggle)
                    if isOn(toggle) { toggle.click() }
                }
                try capture(launch, "\(color)-\(surface)-system-preferences-only")
                for id in ["transparency", "contrast", "motion"] {
                    let toggle = try leaf(app, "appearance.\(id)")
                    try reveal(launch, toggle)
                    if !isOn(toggle) { toggle.click() }
                    try require(isOn(toggle), "Native accessibility toggle did not change: \(id)")
                }
                try expect(app, "appearance.effective", "材质=classic")
                try expect(app, "appearance.effective", "减少动效=true")
                try capture(launch, "\(color)-\(surface)-strengthened-accessibility")
                try click(launch, "sheet.cancel")
                try expect(app, "reference.boundary", "v1.0")
            }
            for (title, width, height) in [("1260×860", 1260, 860), ("1000×640", 1000, 640), ("400×600", 400, 600)] {
                try click(launch, "appearance.open")
                try menu(launch, "appearance.size", title: title)
                try click(launch, "sheet.cancel")
                try verifyGeometry(launch, width: width, height: height)
                try click(launch, "source.open")
                try scrollLongText(launch, "source.excerpt")
                try escapeSheet(launch, content: "source.excerpt")
                try capture(launch, "actual-\(title)")
            }
            try expect(app, "detail.title", "键盘应用的虚构方法")
            try expect(app, "detail.install", "成功记录")
            try expect(app, "reference.boundary", "键盘应用的虚构方法")
            try click(launch, "reset")
            try expect(app, "detail.contentState", "草稿待审")
            try expect(app, "detail.install", "尚未载入")
            try expect(app, "work.empty", "尚未选用")
            try click(launch, "appearance.open")
            try expect(app, "appearance.interaction", "native 呈现")
            try expect(app, "permission", "默认权限")
            for id in ["transparency", "contrast", "motion"] {
                try require(!isOn(leaf(app, "appearance.\(id)")), "Reset retained an injected accessibility preview")
            }
            try escapeSheet(launch, content: "appearance.color")
            try click(launch, "edit.open")
            try replace(launch, "edit.name", with: "关闭窗口前的内存修改")
            try click(launch, "edit.apply")
            try click(launch, "consent.agree")
            try click(launch, "install.success")
            try click(launch, "version.enable")
            try click(launch, "select.boundary")
            try expect(app, "reference.boundary", "关闭窗口前的内存修改")
            let instance = text(try leaf(app, "memory.instance"))
            try require(UUID(uuidString: instance) != nil, "Missing actual window memory identity")
            launch.window.buttons[XCUIIdentifierCloseWindow].click()
            try require(wait { !launch.window.exists }, "The owned window did not close")
            try inputOwner(launch)
            let windowMenu = app.menuBars.menuBarItems.matching(
                NSPredicate(format: "label IN %@ OR title IN %@", ["Window", "窗口"], ["Window", "窗口"])
            ).firstMatch
            try require(windowMenu.waitForExistence(timeout: 5), "Missing standard Window menu")
            windowMenu.click()
            let reopen = app.menuItems.matching(
                NSPredicate(format: "label == %@ OR title == %@", "技能审阅样例", "技能审阅样例")
            ).firstMatch
            try require(reopen.waitForExistence(timeout: 5) && reopen.isEnabled, "Missing own reopen action")
            reopen.click()
            try require(launch.window.waitForExistence(timeout: 10), "The own window did not reopen")
            try require(text(try leaf(app, "memory.instance")) != instance, "Reopen reused window memory")
            try expect(app, "detail.title", "边界差异三步复核")
            try expect(app, "detail.install", "尚未载入")
            try expect(app, "detail.consent", "未答")
            try expect(app, "work.empty", "尚未选用")
            try capture(launch, "reopened-fresh-memory")
            // Leave a distinct noninitial state before the process exits, so
            // the next launch checks restart rather than an already-clean root.
            try click(launch, "edit.open")
            try replace(launch, "edit.name", with: "退出进程前的内存修改")
            try click(launch, "edit.apply")
            try click(launch, "consent.refuse")
            try expect(app, "detail.title", "退出进程前的内存修改")
            try expect(app, "detail.consent", "已拒绝")
        }
        // The first owned instance has exited in withLaunch's defer.
        try withLaunch(method: #function) { launch in
            try expect(launch.app, "detail.title", "边界差异三步复核")
            try expect(launch.app, "detail.contentState", "草稿待审")
            try expect(launch.app, "detail.consent", "未答")
            try expect(launch.app, "work.empty", "尚未选用")
            try click(launch, "appearance.open")
            try expect(launch.app, "appearance.interaction", "native 呈现")
            try expect(launch.app, "permission", "默认权限")
            try escapeSheet(launch, content: "appearance.color")
        }
    }

    func testInvalidSkillRequestsNeverReachProduct() throws {
        // Never omit the skill key here: absence deliberately reaches the OLD
        // chain and belongs to pure policy/source-integration tests, not launch.
        let cases: [(String, String, [String])] = [
            ("0", "ui-test", ["0", "0", "0", "0"]),
            ("", "ui-test", ["0", "0", "0", "0"]),
            ("invalid", "ui-test", ["0", "0", "0", "0"]),
            ("1", "not-ui-test", ["0", "0", "0", "0"]),
        ] + (1..<16).map { mask in
            ("1", "ui-test", (0..<4).map { mask & (1 << $0) == 0 ? "0" : "1" })
        }
        for (flag, environment, others) in cases {
            try withLaunch(method: #function, flags: [flag] + others, environment: environment, refused: true) { launch in
                let app = launch.app
                try expect(app, "refusal.title", "技能审阅未启动")
                if flag != "1" { _ = try leaf(app, "refusal.reason.invalidFlag") }
                if environment != "ui-test" { _ = try leaf(app, "refusal.reason.invalidEnvironment") }
                if others.contains("1") { _ = try leaf(app, "refusal.reason.conflictingReview") }
                XCTAssertFalse(query(app, "notice").exists)
                XCTAssertFalse(launch.window.debugDescription.contains(launch.directory.child.path))
                try capture(launch, "refusal-\(launch.attempt)")
            }
        }
    }

    // MARK: - Only local UI-test metadata; no business DTO/controller

    private struct Marker: Decodable {
        let methodID: String
        let markerKey: String
        let markerToken: String
    }
    private struct Evidence: Decodable {
        let version: Int
        let runID: String
        let accountUID: UInt32
        let accountHome: String
        let bundleID: String
        let domain: String
        let suite: String
        let methods: [Marker]
        let approvalReference: String
        let preparedAt: Double
        let expiresAt: Double
        let preSnapshotEvidenceID: String
    }
    private struct DirectoryWitness {
        let home: String
        let parent: URL
        let child: URL
        let before: [String]
    }
    @MainActor private final class Launch {
        let app: XCUIApplication
        let process: NSRunningApplication
        let pid: pid_t
        let started: Date
        let directory: DirectoryWitness
        let evidence: Evidence
        let marker: Marker
        let method: String
        let attempt = UUID().uuidString
        let began = Date()
        let refused: Bool
        var observedChildren = Set<pid_t>()
        var window: XCUIElement { app.windows[refused ? "技能审阅未启动" : "技能审阅样例"] }
        init(app: XCUIApplication, process: NSRunningApplication, started: Date,
             directory: DirectoryWitness, evidence: Evidence, marker: Marker, method: String, refused: Bool) {
            self.app = app; self.process = process; pid = process.processIdentifier
            self.started = started; self.directory = directory
            self.evidence = evidence; self.marker = marker; self.method = method; self.refused = refused
        }
    }
    private enum CheckError: Error { case failed(String) }
    private func require(_ value: Bool, _ message: String) throws {
        guard value else { XCTFail(message); throw CheckError.failed(message) }
    }
    private func inherited(_ key: String) throws -> String {
        let values = ProcessInfo.processInfo.environment
        if let direct = values[key], let forwarded = values["TEST_RUNNER_\(key)"] {
            try require(direct == forwarded, "Conflicting runner spelling for \(key)")
        }
        return try XCTUnwrap(values[key] ?? values["TEST_RUNNER_\(key)"], "Missing authorized runner metadata: \(key)")
    }
    private func accountHome() throws -> String {
        try require(getuid() == geteuid(), "Changed effective account is not authorized")
        var record = passwd()
        var result: UnsafeMutablePointer<passwd>?
        var buffer = [CChar](repeating: 0, count: 65_536)
        return try buffer.withUnsafeMutableBufferPointer { bytes in
            let base = try XCTUnwrap(bytes.baseAddress)
            try require(getpwuid_r(getuid(), &record, base, bytes.count, &result) == 0 && result != nil,
                        "Cannot resolve the actual test account")
            return String(cString: try XCTUnwrap(record.pw_dir))
        }
    }
    private func directoryWitness() throws -> DirectoryWitness {
        try require(try inherited("AERORIC_ENV") == "ui-test", "Use only the authorized full native runner")
        let home = try accountHome()
        let parent = URL(fileURLWithPath: try inherited("AERORIC_DATA_DIR"), isDirectory: true)
        try require(NativeVisualReviewEntry.isIsolatedDataDirectory(parent.path, home: home), "Unsafe runner parent")
        let before = try FileManager.default.contentsOfDirectory(atPath: parent.path).sorted()
        let child = parent.appendingPathComponent("skill-review-\(UUID().uuidString)", isDirectory: true)
        try require(!before.contains(child.lastPathComponent)
                    && NativeVisualReviewEntry.isIsolatedDataDirectory(child.path, home: home), "Unsafe review child")
        return .init(home: home, parent: parent, child: child, before: before)
    }
    private func evidence(home: String, method: String) throws -> (Evidence, Marker) {
        let raw = try inherited(Self.evidenceKey)
        try require(raw.utf8.count <= 32_768, "Oversized Skill evidence descriptor")
        let value = try JSONDecoder().decode(Evidence.self, from: Data(raw.utf8))
        let now = Date().timeIntervalSince1970
        try require(value.version == 1 && UUID(uuidString: value.runID) != nil
                    && value.accountUID == getuid() && value.accountHome == home
                    && value.bundleID == Self.bundleID && value.domain == Self.bundleID
                    && value.suite == Self.suite, "Skill evidence identity does not match this isolated account/suite/domain")
        try require(value.preparedAt.isFinite && value.expiresAt.isFinite
                    && value.preparedAt <= now && now <= value.expiresAt
                    && value.expiresAt - value.preparedAt <= 7_200
                    && !value.approvalReference.isEmpty && !value.preSnapshotEvidenceID.isEmpty,
                    "Missing/expired authorization or external pre-snapshot reference")
        let expected = Set(Self.methods.map { "\(Self.suite)/\($0)" })
        try require(value.methods.count == 6 && Set(value.methods.map(\.methodID)) == expected
                    && Set(value.methods.map(\.markerKey)).count == 6
                    && Set(value.methods.map(\.markerToken)).count == 6,
                    "Skill evidence needs six distinct exact method/marker identities")
        for marker in value.methods {
            let short = String(marker.methodID.dropFirst(Self.suite.count + 1))
            try require(marker.markerKey == "skillReviewUITestMarker_\(value.runID)_\(short)"
                        && marker.markerToken.hasPrefix("nonsensitive-skill-")
                        && UUID(uuidString: String(marker.markerToken.dropFirst("nonsensitive-skill-".count))) != nil,
                        "Invalid nonsensitive Skill-only marker")
        }
        let marker = try XCTUnwrap(value.methods.first { $0.methodID == "\(Self.suite)/\(method)" })
        return (value, marker)
    }
    private func preference(_ evidence: Evidence, _ marker: Marker, stage: String) throws {
        // A fixed system READ of one exact external test key. No domain dump,
        // controller invocation, write/delete/synchronize, reseed or cleanup.
        try require(Date().timeIntervalSince1970 <= evidence.expiresAt, "Skill evidence expired during this run")
        let process = Process()
        let output = Pipe()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/defaults")
        process.arguments = ["read", URL(fileURLWithPath: evidence.accountHome)
            .appendingPathComponent("Library/Preferences/\(Self.bundleID)").path, marker.markerKey]
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        try process.run()
        let data = output.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        let preserved = process.terminationStatus == 0
            && String(decoding: data, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines) == marker.markerToken
        attach("stage=\(stage); key=\(marker.markerKey); preserved=\(preserved); readExit=\(process.terminationStatus). Marker survival is NOT zero-write evidence.", "preference-\(stage)")
        try require(preserved, "The external test marker is missing/changed; never seed, repair or skip it")
    }
    private func withLaunch(
        method function: String, flags: [String] = ["1", "0", "0", "0", "0"],
        environment: String = "ui-test", refused: Bool = false,
        _ body: (Launch) throws -> Void
    ) throws {
        let method = function.replacingOccurrences(of: "()", with: "")
        try require(Self.methods.contains(method) && flags.count == 5, "Unknown Skill method/request")
        try require(name.hasSuffix(" \(method)]") || name.hasSuffix(".\(method)"),
                    "Skill evidence must belong to this actual XCTest method")
        attach("method=\(method); XCTestAttempt=\(testAttemptID); began=\(Date()); external authorization/resources still mandatory.", "test-attempt")
        let directory = try directoryWitness()
        let (evidence, marker) = try evidence(home: directory.home, method: method)
        try preference(evidence, marker, stage: "before-launch")
        try require(NSRunningApplication.runningApplications(withBundleIdentifier: Self.bundleID).isEmpty,
                    "An existing Aeroric instance belongs to another owner; do not launch/kill it")
        let app = XCUIApplication(bundleIdentifier: Self.bundleID)
        try require(app.state == .notRunning, "XCUIApplication must not replace another live instance")
        var owned: Launch?
        defer {
            if let owned { finish(owned) }
            else {
                do { try preference(evidence, marker, stage: "ownership-not-established") }
                catch { XCTFail("Marker check without owned process: \(error)") }
                do { try directoryUnchanged(directory) }
                catch { XCTFail("Directory check without owned process: \(error)") }
                // No PID+launchDate identity: never terminate an unknown app.
            }
        }
        var overrides = app.launchEnvironment
        let launchID = UUID().uuidString
        let explicit = Dictionary(uniqueKeysWithValues: zip(Self.reviewKeys, flags))
            .merging(["AERORIC_ENV": environment, "AERORIC_DATA_DIR": directory.child.path,
                      "AERORIC_SKILL_REVIEW_TEST_LAUNCH_ID": launchID]) { _, new in new }
        switch NativeSkillReviewEntry.decide(environment: explicit, home: directory.home) {
        case .review:
            try require(!refused, "Expected refusal but this request would start the sample")
        case .refused:
            try require(refused, "Expected sample but this request is invalid")
        case .inactive:
            try require(false, "UI tests must never launch a request that could reach the product chain")
        }
        for (key, value) in explicit {
            overrides[key] = value
            overrides["TEST_RUNNER_\(key)"] = value
        }
        for key in [Self.evidenceKey, "AERORIC_KNOWLEDGE_WITNESS_CONTROLLER", "AERORIC_KNOWLEDGE_PREFERENCE_WITNESS"] {
            overrides[key] = ""; overrides["TEST_RUNNER_\(key)"] = ""
        }
        app.launchArguments = []
        app.launchEnvironment = overrides
        let before = Date()
        // Recheck immediately before XCTest's potentially replacing launch.
        try require(NSRunningApplication.runningApplications(withBundleIdentifier: Self.bundleID).isEmpty,
                    "Concurrent app appeared before launch; return resources to its owner")
        app.launch()
        try require(wait { NSRunningApplication.runningApplications(withBundleIdentifier: Self.bundleID).count == 1 },
                    "Cannot establish exactly one test-launched process")
        let running = try XCTUnwrap(NSRunningApplication.runningApplications(withBundleIdentifier: Self.bundleID).first)
        let started = try XCTUnwrap(running.launchDate, "Missing owned launch identity")
        try require(started >= before.addingTimeInterval(-2), "Observed process predates this launch")
        let expectedWindow = app.windows[refused ? "技能审阅未启动" : "技能审阅样例"]
        try require(expectedWindow.waitForExistence(timeout: 15), "No correlated Skill-only launch window")
        try expect(app, "process.launchID", launchID)
        try require(text(try leaf(app, "process.launchID")) == launchID
                    && text(try leaf(app, "process.pid")) == String(running.processIdentifier),
                    "XCTest launch nonce/PID does not match the candidate; do not own/activate/terminate it")
        try require(running.executableURL?.path.hasSuffix("/Debug/AeroricNative.app/Contents/MacOS/AeroricNative") == true,
                    "The correlated process is not the authorized Debug product")
        let launch = Launch(app: app, process: running, started: started, directory: directory,
                            evidence: evidence, marker: marker, method: method, refused: refused)
        owned = launch
        attach("runID=\(evidence.runID); method=\(method); XCTestAttempt=\(testAttemptID); launchAttempt=\(launch.attempt); began=\(launch.began); PID=\(launch.pid); launchDate=\(started); approval=\(evidence.approvalReference); preSnapshot=\(evidence.preSnapshotEvidenceID); safeChild=\(directory.child.lastPathComponent)", "launch-identity")
        try assertOwner(launch)
        try inputOwner(launch) // Only the nonce+PID-correlated process.
        do {
            try require(launch.window.waitForExistence(timeout: 15), "Expected Skill-only window did not appear")
            try expect(app, "process.pid", String(launch.pid))
            try isolation(launch, stage: "after-launch")
            if !refused { try expect(app, "notice", "离线技能审阅 · 全部为示例") }
            try body(launch)
            try isolation(launch, stage: "after-actions")
        } catch {
            attach(app.debugDescription, "failure-AX")
            if launch.window.exists { try? capture(launch, "failure") }
            throw error
        }
    }
    private func assertOwner(_ launch: Launch) throws {
        let processes = NSRunningApplication.runningApplications(withBundleIdentifier: Self.bundleID)
        try require(processes.count == 1 && processes.first?.processIdentifier == launch.pid
                    && processes.first?.launchDate == launch.started, "Owned PID/launchDate changed; do not manipulate it")
    }
    private func inputOwner(_ launch: Launch) throws {
        try assertOwner(launch)
        if NSWorkspace.shared.frontmostApplication?.processIdentifier != launch.pid { launch.app.activate() }
        try require(wait { NSWorkspace.shared.frontmostApplication?.processIdentifier == launch.pid },
                    "The owned app is not foreground; do not type/click another application")
    }
    private func directoryUnchanged(_ directory: DirectoryWitness) throws {
        let after = try FileManager.default.contentsOfDirectory(atPath: directory.parent.path).sorted()
        try require(after == directory.before && !after.contains(directory.child.lastPathComponent),
                    "Review created a data child or changed its existing parent")
        attach("parent before count=\(directory.before.count); after=\(after.count); SHA256=\(digest(after.joined(separator: "\n"))); unique child absent. No mkdir/cleanup.", "directory")
    }
    private func observeChildren(_ launch: Launch, stage: String) throws {
        try assertOwner(launch)
        var buffer = [pid_t](repeating: 0, count: 256)
        let bytes = buffer.withUnsafeMutableBytes {
            proc_listpids(UInt32(PROC_PPID_ONLY), UInt32(launch.pid), $0.baseAddress, Int32($0.count))
        }
        try require(bytes >= 0 && Int(bytes) < buffer.count * MemoryLayout<pid_t>.stride
                    && Int(bytes) % MemoryLayout<pid_t>.stride == 0, "Incomplete owned child-process observation")
        let children = buffer.prefix(Int(bytes) / MemoryLayout<pid_t>.stride).filter { $0 > 0 }
        launch.observedChildren.formUnion(children)
        attach("stage=\(stage); owned PID=\(launch.pid); launchDate=\(launch.started); observed children=\(children). Point-in-time only; cannot rule out short-lived/reparented processes.", "process-\(stage)")
        try require(children.isEmpty, "The offline review/refusal has a child process; do not kill it")
    }
    private func isolation(_ launch: Launch, stage: String) throws {
        try assertOwner(launch)
        for id in ["bootstrap.environment", "sidebar.list", "statusbar.phase",
                   "knowledgeReview.notice", "evolutionReview.offlineBadge", "review.workspace.refresh"] {
            try require(!launch.app.descendants(matching: .any).matching(identifier: id).firstMatch.exists,
                        "Unexpected product/other-review AX leaf: \(id)")
        }
        try directoryUnchanged(launch.directory)
        try preference(launch.evidence, launch.marker, stage: stage)
        try observeChildren(launch, stage: stage)
    }
    private func finish(_ launch: Launch) {
        // Independent checks: one failure must not bypass owned cleanup or
        // other witnesses. Never kill children, global matches or replacements.
        if launch.app.state != .notRunning {
            do { try observeChildren(launch, stage: "before-terminate") }
            catch { XCTFail("Process witness: \(error)") }
        }
        do { try preference(launch.evidence, launch.marker, stage: "before-terminate") }
        catch { XCTFail("Preference witness: \(error)") }
        if launch.app.state != .notRunning {
            do { try assertOwner(launch); launch.app.terminate() }
            catch { XCTFail("Not terminating an unowned/replaced process: \(error)") }
        }
        do {
            try require(wait {
                launch.app.state == .notRunning
                    && !NSRunningApplication.runningApplications(withBundleIdentifier: Self.bundleID)
                        .contains { $0.processIdentifier == launch.pid && $0.launchDate == launch.started }
            }, "The owned App did not exit")
        } catch { XCTFail("Owned exit witness: \(error)") }
        do { try directoryUnchanged(launch.directory) }
        catch { XCTFail("Post-exit directory witness: \(error)") }
        do { try preference(launch.evidence, launch.marker, stage: "after-terminate") }
        catch { XCTFail("Post-exit preference witness: \(error)") }
        for pid in launch.observedChildren {
            var path = [CChar](repeating: 0, count: 4096)
            let count = path.withUnsafeMutableBytes { proc_pidpath(pid, $0.baseAddress, UInt32($0.count)) }
            if count > 0 || errno != ESRCH {
                XCTFail("Previously observed child PID \(pid) still exists or cannot be proven exited; could be reparented/reused. No signal/cleanup attempted; return to the external process owner.")
            }
        }
        attach("runID=\(launch.evidence.runID); method=\(launch.method); XCTestAttempt=\(testAttemptID); launchAttempt=\(launch.attempt); PID=\(launch.pid); ended=\(Date()). External operator must correlate domain/process snapshots; marker survival alone is insufficient.", "attempt-ended")
    }

    // MARK: - Real native controls/keys/scrolling, never private focus patches

    private func query(_ app: XCUIApplication, _ suffix: String) -> XCUIElement {
        if app.sheets.firstMatch.exists {
            let inSheet = app.sheets.firstMatch.descendants(matching: .any)
                .matching(identifier: "skillReview.\(suffix)").firstMatch
            if inSheet.exists { return inSheet }
        }
        return app.descendants(matching: .any).matching(identifier: "skillReview.\(suffix)").firstMatch
    }
    @discardableResult private func leaf(_ app: XCUIApplication, _ suffix: String) throws -> XCUIElement {
        let result = query(app, suffix)
        try require(result.waitForExistence(timeout: 8), "Missing Skill AX leaf: \(suffix)")
        try require((!result.label.isEmpty && result.label != result.identifier)
                    || (!result.title.isEmpty && result.title != result.identifier)
                    || (result.elementType == .staticText && result.value is String)
                    || ([XCUIElement.ElementType.textField, .textView].contains(result.elementType)
                        && !(result.placeholderValue ?? "").isEmpty),
                    "Missing readable name/value for \(suffix)")
        return result
    }
    private func text(_ element: XCUIElement) -> String {
        if let value = element.value as? String, !value.isEmpty { return value }
        return element.label.isEmpty ? element.title : element.label
    }
    private func expect(_ app: XCUIApplication, _ suffix: String, _ fragment: String) throws {
        _ = try leaf(app, suffix)
        try require(wait { self.text(self.query(app, suffix)).contains(fragment) },
                    "\(suffix): expected \(fragment), observed \(text(query(app, suffix)))")
    }
    private func disabled(_ app: XCUIApplication, _ suffix: String) throws {
        try require(!leaf(app, suffix).isEnabled, "Unsafe action is enabled: \(suffix)")
    }
    private func click(_ launch: Launch, _ suffix: String) throws {
        let target = try leaf(launch.app, suffix)
        try reveal(launch, target)
        try require(target.isEnabled, "Cannot click a disabled action: \(suffix)")
        try inputOwner(launch)
        target.click()
    }
    private func reveal(_ launch: Launch, _ target: XCUIElement, anchor: XCUIElement? = nil) throws {
        try inputOwner(launch)
        let surface = launch.app.sheets.firstMatch.exists ? launch.app.sheets.firstMatch : launch.window
        let locator = anchor ?? target
        let name = locator.identifier.isEmpty ? locator.label : locator.identifier
        try require(!name.isEmpty, "A native scroll target needs a unique readable leaf anchor")
        let owners = surface.scrollViews.containing(locator.elementType, identifier: name)
            .allElementsBoundByIndex.filter { !$0.frame.isEmpty }
            .sorted { $0.scrollViews.count > $1.scrollViews.count }
        func visible(_ rect: CGRect) -> Bool {
            target.exists && target.isHittable && !target.frame.isEmpty
                && rect.insetBy(dx: -1, dy: -1).contains(target.frame)
        }
        var viewport = surface.frame.intersection(launch.window.frame)
        if visible(viewport) && owners.allSatisfy({ $0.frame.insetBy(dx: -1, dy: -1).contains(target.frame) }) { return }
        for (index, owner) in owners.enumerated() {
            viewport = viewport.intersection(owner.frame)
            try require(!viewport.isNull && viewport.height > 8 && viewport.width > 8, "Offscreen native scroll owner")
            let next = index + 1 < owners.count ? owners[index + 1] : target
            for _ in 0..<35 {
                let overlap = viewport.intersection(next.frame)
                if next.isHittable && !overlap.isNull
                    && overlap.height >= min(viewport.height, next.frame.height) - 2 { break }
                let delta: CGFloat = next.frame.midY < viewport.midY ? 160 : -160
                owner.coordinate(withNormalizedOffset: CGVector(dx: 0, dy: 0))
                    .withOffset(CGVector(dx: viewport.minX - owner.frame.minX + 2,
                                         dy: viewport.midY - owner.frame.minY))
                    .scroll(byDeltaX: 0, deltaY: delta)
                viewport = owners.prefix(index + 1).reduce(surface.frame.intersection(launch.window.frame)) {
                    $0.intersection($1.frame)
                }
            }
        }
        let final = owners.reduce(surface.frame.intersection(launch.window.frame)) { $0.intersection($1.frame) }
        try require(visible(final), "Cannot fully reveal the own native control: \(target.identifier)")
    }
    private func menu(_ launch: Launch, _ suffix: String, title: String) throws {
        try click(launch, suffix)
        let item = launch.app.menuItems.matching(NSPredicate(format: "label == %@ OR title == %@", title, title)).firstMatch
        try require(item.waitForExistence(timeout: 5) && item.isEnabled, "Missing native menu choice: \(title)")
        item.click()
        try expect(launch.app, suffix, title)
    }
    private func scenario(_ launch: Launch, _ title: String) throws { try menu(launch, "scenario", title: title) }
    private func selectItem(_ launch: Launch, id: String, title: String) throws {
        if query(launch.app, "itemPicker").exists {
            try menu(launch, "itemPicker", title: "\(title) · 示例")
        } else { try click(launch, "item.\(id)") }
        try expect(launch.app, "detail.title", title)
    }
    private func disclosure(_ launch: Launch, _ title: String) throws {
        let predicate = NSPredicate(format: "label == %@ OR title == %@", title, title)
        let candidates = [launch.app.disclosureTriangles, launch.app.buttons]
            .map { $0.matching(predicate).firstMatch }
        let item = try XCTUnwrap(candidates.first(where: { $0.exists }), "Missing native disclosure: \(title)")
        try reveal(launch, item)
        item.click()
    }
    private func replace(_ launch: Launch, _ suffix: String, with value: String) throws {
        try click(launch, suffix)
        launch.app.typeKey("a", modifierFlags: .command)
        launch.app.typeText(value)
        try expect(launch.app, suffix, value)
    }
    private func escapeSheet(_ launch: Launch, content: String) throws {
        _ = try leaf(launch.app, content)
        try inputOwner(launch)
        launch.app.typeKey(.escape, modifierFlags: [])
        try require(wait { !self.query(launch.app, content).exists }, "Escape did not dismiss the actual sheet")
    }
    private func isOn(_ element: XCUIElement) -> Bool {
        (element.value as? String) == "1" || (element.value as? NSNumber)?.boolValue == true
    }
    private func segment(_ launch: Launch, _ suffix: String, title: String) throws {
        let group = try leaf(launch.app, suffix)
        let item = group.descendants(matching: .any).matching(
            NSPredicate(format: "label == %@ OR title == %@", title, title)
        ).firstMatch
        try require(item.exists, "Missing system segment: \(title)")
        try reveal(launch, group)
        item.click()
        try require(wait { self.isOn(item) }, "Native segment selection did not change")
    }
    private func scrollLongText(_ launch: Launch, _ suffix: String) throws {
        let content = try leaf(launch.app, suffix)
        try require(text(content).contains("第24段"), "Fixture is missing its long-text tail")
        let owners = launch.app.scrollViews.containing(.any, identifier: "skillReview.\(suffix)")
            .allElementsBoundByIndex
        let owner = try XCTUnwrap(owners.min { $0.frame.height < $1.frame.height })
        try reveal(launch, owner, anchor: content)
        let before = content.frame
        owner.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).scroll(byDeltaX: 0, deltaY: -240)
        try require(wait { content.frame.origin != before.origin }, "Real native long-text scrolling did not move content")
        attach("longText=\(suffix); before=\(before); after=\(content.frame); viewport=\(owner.frame)", "long-text-scroll")
    }
    private func verifyGeometry(_ launch: Launch, width: Int, height: Int) throws {
        try require(wait {
            let frame = launch.window.frame
            return self.text(self.query(launch.app, "geometry"))
                .hasPrefix("外框 \(Int(frame.width))×\(Int(frame.height))pt；")
        }, "AppKit measurement did not converge with the actual owned AX outer frame")
        let raw = text(try leaf(launch.app, "geometry"))
        let frame = launch.window.frame
        attach("requestedOuter=\(width)×\(height); AXOuter=\(frame); \(raw). Actual window capture only, no scaling.", "geometry")
        if raw.contains("屏幕容纳目标=true") {
            try require(wait {
                abs(launch.window.frame.width - CGFloat(width)) <= 2
                    && abs(launch.window.frame.height - CGFloat(height)) <= 2
            }, "Own outer frame did not reach its requested size")
        } else {
            attach("NOT VERIFIED full target \(width)×\(height): display cannot accommodate it. Retain actual outer/content/layout evidence.", "display-limitation")
        }
        try require(raw.contains("内容 ") && raw.contains("布局 ")
                    && !raw.contains(" 0×") && !raw.contains("×0pt"),
                    "Missing actual outer/content/layout measurements")
    }
    private func capture(_ launch: Launch, _ label: String) throws {
        try assertOwner(launch)
        try require(launch.window.exists, "Cannot capture a missing owned window")
        let capture = XCTAttachment(screenshot: launch.window.screenshot())
        capture.name = "skill-review-\(label)-unscaled-own-window"
        capture.lifetime = .keepAlways
        add(capture)
        let bundle = launch.process.bundleURL.flatMap(Bundle.init(url:))
        attach("\(ProcessInfo.processInfo.operatingSystemVersionString); SDK=\(String(describing: bundle?.object(forInfoDictionaryKey: "DTSDKName"))); machine=\(machine()); PID=\(launch.pid); launchDate=\(launch.started); frame=\(launch.window.frame). Other OS/architectures and Material pixels still require explicit evidence/human review.", "platform-and-frame")
    }
    private func machine() -> String {
        #if arch(arm64)
        "arm64"
        #elseif arch(x86_64)
        "x86_64"
        #else
        "other"
        #endif
    }
    private func attach(_ text: String, _ name: String) {
        let attachment = XCTAttachment(string: text)
        attachment.name = "skill-review-\(name)"
        attachment.lifetime = .keepAlways
        add(attachment)
    }
    private func digest(_ text: String) -> String {
        SHA256.hash(data: Data(text.utf8)).map { String(format: "%02x", $0) }.joined()
    }
    private func wait(_ condition: () -> Bool) -> Bool {
        let deadline = Date().addingTimeInterval(10)
        repeat {
            if condition() { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.05))
        } while Date() < deadline
        return condition()
    }
}
#endif
